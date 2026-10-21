//! The shard task: reads raw gateway messages, reports [`ShardEvent`]s and
//! pushes parsed [`Inbound`] items (FR-2, AD-10, AD-21).

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::value::RawValue;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::Instrument;
use twilight_gateway::error::{ReceiveMessageError, ReceiveMessageErrorType};
use twilight_gateway::{Event, EventTypeFlags, Message, Shard, ShardId};
use twilight_model::gateway::event::{GatewayEvent, GatewayEventDeserializer};

use crate::gateway::CloseMode;
use crate::inbound::{Inbound, InboundKind, InboundSink, RawJson, ShardEvent};
use crate::queue::PortQueue;

/// The dispatch opcode.
const DISPATCH: u8 = 0;

/// What one text frame produced.
#[derive(Debug, Default)]
pub(crate) struct Classified {
    /// A status change to report.
    pub(crate) status: Option<ShardEvent>,
    /// An item for the sink.
    pub(crate) inbound: Option<Inbound>,
}

/// The `d` field of a gateway payload, borrowed as raw JSON.
#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow)]
    d: &'a RawValue,
}

/// Turns one text frame into a status change and/or an [`Inbound`].
///
/// Never logs the payload: parse failures name only the event type.
pub(crate) fn classify(
    text: String,
    wanted: EventTypeFlags,
    shard: ShardId,
    received_at: Instant,
) -> Classified {
    let Some(header) = GatewayEventDeserializer::from_json(&text) else {
        tracing::warn!(target: "noctis::gateway", "gateway message without an opcode; ignored");
        return Classified::default();
    };
    let op = header.op();
    let event_type = header.event_type().map(str::to_owned);
    drop(header);

    let status = match (op, event_type.as_deref()) {
        (DISPATCH, Some("READY")) => Some(ShardEvent::Ready {
            shard: shard.number(),
        }),
        (DISPATCH, Some("RESUMED")) => Some(ShardEvent::Resumed {
            shard: shard.number(),
        }),
        _ => None,
    };

    let raw = if event_type.as_deref() == Some("INTERACTION_CREATE")
        && wanted.contains(EventTypeFlags::INTERACTION_CREATE)
    {
        let Ok(envelope) = serde_json::from_str::<Envelope<'_>>(&text) else {
            tracing::warn!(
                target: "noctis::gateway",
                event_type = "INTERACTION_CREATE",
                "gateway payload without a readable `d`; ignored"
            );
            return Classified {
                status,
                inbound: None,
            };
        };
        Some(Bytes::copy_from_slice(envelope.d.get().as_bytes()))
    } else {
        None
    };

    let kind = match twilight_gateway::parse(text, wanted) {
        Ok(Some(GatewayEvent::Dispatch(_, event))) => match (Event::from(event), raw) {
            (Event::InteractionCreate(create), Some(raw)) => Some(InboundKind::Interaction(
                Box::new(create.0),
                RawJson::new(raw),
            )),
            (event, _) => Some(InboundKind::Event(Box::new(event))),
        },
        Ok(_) => None,
        Err(_) => {
            let fallback = raw.and_then(modal_submit_without_components);
            if fallback.is_none() {
                tracing::warn!(
                    target: "noctis::gateway",
                    event_type = event_type.as_deref().unwrap_or("-"),
                    "gateway payload could not be parsed; ignored"
                );
            }
            fallback
        }
    };
    Classified {
        status,
        inbound: kind.map(|kind| Inbound {
            shard,
            received_at,
            kind,
        }),
    }
}

/// A modal submit (interaction type 5) that twilight cannot parse, because
/// it rejects some modal components (a Checkbox's boolean `value`, FR-64),
/// parsed again with an empty `data.components`. Noctis never reads
/// twilight's components of a submit: the component pipeline decodes the
/// raw bytes, which are kept as they are (`ModalSubmission`, MOD-09).
/// `None` for anything else.
fn modal_submit_without_components(raw: Bytes) -> Option<InboundKind> {
    let mut value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    if value.get("type")?.as_u64()? != 5 {
        return None;
    }
    let data = value.get_mut("data")?.as_object_mut()?;
    data.insert(
        "components".to_owned(),
        serde_json::Value::Array(Vec::new()),
    );
    let interaction = serde_json::from_value(value).ok()?;
    Some(InboundKind::Interaction(
        Box::new(interaction),
        RawJson::new(raw),
    ))
}

/// Runs one shard until its stream ends or a close is requested, in span
/// `noctis.shard`.
pub(crate) async fn run(
    shard: Shard<PortQueue>,
    app: Arc<str>,
    wanted: EventTypeFlags,
    sink: Arc<dyn InboundSink>,
    events: UnboundedSender<ShardEvent>,
    close: watch::Receiver<Option<CloseMode>>,
    close_timeout: Duration,
) {
    let span = tracing::info_span!("noctis.shard", app = &*app, shard = shard.id().number());
    let lane = Lane {
        wanted,
        sink,
        events,
    };
    drive(shard, lane, close, close_timeout)
        .instrument(span)
        .await;
}

/// Where a shard task sends what it reads.
struct Lane {
    wanted: EventTypeFlags,
    sink: Arc<dyn InboundSink>,
    events: UnboundedSender<ShardEvent>,
}

/// Waits for a close request; a dropped sender counts as
/// [`CloseMode::Resumable`]. Cancel-safe.
async fn close_requested(close: &mut watch::Receiver<Option<CloseMode>>) -> CloseMode {
    match close.wait_for(Option::is_some).await {
        Ok(requested) => requested.unwrap_or_default(),
        Err(_) => CloseMode::Resumable,
    }
}

async fn drive(
    mut shard: Shard<PortQueue>,
    lane: Lane,
    mut close: watch::Receiver<Option<CloseMode>>,
    close_timeout: Duration,
) {
    let id = shard.id();
    let mut last_close = None;
    let requested = loop {
        let item = tokio::select! {
            biased;
            mode = close_requested(&mut close) => break Some(mode),
            item = shard.next() => item,
        };
        let Some(item) = item else { break None };
        match item {
            Err(error) => log_receive_error(&error),
            Ok(Message::Close(frame)) => {
                last_close = frame.map(|f| f.code);
                let _ = lane.events.send(ShardEvent::Disconnected {
                    shard: id.number(),
                    close_code: last_close,
                });
            }
            Ok(Message::Text(text)) => {
                let received_at = Instant::now();
                let classified = classify(text, lane.wanted, id, received_at);
                if let Some(status) = classified.status {
                    let _ = lane.events.send(status);
                }
                if let Some(inbound) = classified.inbound {
                    lane.sink.push(inbound);
                }
            }
        }
    };
    let event = match requested {
        Some(mode) => {
            close_shard(&mut shard, mode, close_timeout).await;
            ShardEvent::Closed { shard: id.number() }
        }
        None => ShardEvent::Ended {
            shard: id.number(),
            close_code: last_close,
        },
    };
    let _ = lane.events.send(event);
}

/// Closes a shard on request: an identified shard sends `mode`'s frame and
/// reads until the gateway's close frame, the end of the stream or
/// `close_timeout`; any other shard stops at once. Nothing read here reaches
/// the sink.
async fn close_shard(shard: &mut Shard<PortQueue>, mode: CloseMode, close_timeout: Duration) {
    if !shard.state().is_identified() {
        tracing::debug!(
            target: "noctis::gateway",
            state = ?shard.state(),
            "shard not identified; stopped without a close frame"
        );
        return;
    }
    shard.close(mode.frame());
    let mut discarded = 0_usize;
    let acked = tokio::time::timeout(close_timeout, async {
        while let Some(item) = shard.next().await {
            match item {
                // Never read past the close: twilight would reconnect.
                Ok(Message::Close(_)) => return true,
                Ok(Message::Text(text)) => {
                    if GatewayEventDeserializer::from_json(&text)
                        .is_some_and(|header| header.op() == DISPATCH)
                    {
                        discarded += 1;
                    }
                }
                Err(error) => log_receive_error(&error),
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    tracing::debug!(
        target: "noctis::gateway",
        code = mode.frame().code,
        acked,
        discarded,
        "shard closed on request; dispatches after the request discarded"
    );
}

/// Logs a receive error; `Deserializing`'s `Display` carries the payload,
/// so it is never logged.
fn log_receive_error(error: &ReceiveMessageError) {
    if let ReceiveMessageErrorType::Deserializing { .. } = error.kind() {
        tracing::warn!(
            target: "noctis::gateway",
            "gateway message could not be deserialized; ignored"
        );
    } else {
        tracing::warn!(target: "noctis::gateway", %error, "gateway receive failed");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use twilight_gateway::EventType;

    use super::*;

    /// An `INTERACTION_CREATE` `d` that twilight deserializes (a PING),
    /// with an extra non-ASCII field.
    fn interaction_d() -> Value {
        json!({
            "id": "5",
            "application_id": "1",
            "type": 1,
            "token": "interaction-token",
            "version": 1,
            "authorizing_integration_owners": {},
            "entitlements": [],
            "x": "ü🙂ü",
        })
    }

    fn dispatch(t: &str, d: &str) -> String {
        format!(r#"{{"op":0,"s":2,"t":"{t}","d":{d}}}"#)
    }

    fn run(text: &str, wanted: EventTypeFlags) -> Classified {
        classify(text.to_owned(), wanted, ShardId::new(2, 4), Instant::now())
    }

    #[test]
    fn fixture_deserializes() {
        let interaction: twilight_model::application::interaction::Interaction =
            serde_json::from_value(interaction_d()).unwrap();
        assert_eq!(interaction.id.get(), 5);
    }

    #[test]
    fn interaction_keeps_exact_raw_bytes() {
        // Pretty-printed, so the raw slice keeps spaces and newlines.
        let d = serde_json::to_string_pretty(&interaction_d()).unwrap();
        let text = dispatch("INTERACTION_CREATE", &d);
        let at = Instant::now();
        let out = classify(text, EventTypeFlags::INTERACTION_CREATE, ShardId::ONE, at);
        assert_eq!(out.status, None);
        let inbound = out.inbound.unwrap();
        let debug = format!("{inbound:?}");
        assert!(!debug.contains("interaction-token"), "{debug}");
        assert!(!debug.contains("ü🙂ü"), "{debug}");
        assert_eq!(inbound.shard, ShardId::ONE);
        assert_eq!(inbound.received_at, at);
        let InboundKind::Interaction(interaction, raw) = inbound.kind else {
            panic!("not an interaction");
        };
        assert_eq!(interaction.id.get(), 5);
        assert_eq!(raw.as_bytes(), d.as_bytes());
        assert!(!format!("{raw:?}").contains("interaction-token"));
    }

    /// Story 7.8: a modal submit twilight rejects (a Checkbox's boolean
    /// `value`) still reaches the sink, with its raw bytes unchanged; any
    /// other unparsable interaction is still ignored.
    #[test]
    fn modal_submit_twilight_rejects_keeps_raw_bytes() {
        let mut d = interaction_d();
        d["type"] = json!(5);
        d["data"] = json!({ "custom_id": "0m:", "components": [
            { "type": 18, "id": 1, "component": { "type": 23, "id": 2, "custom_id": "agree", "value": true } }
        ] });
        let d = serde_json::to_string(&d).unwrap();
        assert!(
            serde_json::from_str::<twilight_model::application::interaction::Interaction>(&d)
                .is_err()
        );
        let out = run(
            &dispatch("INTERACTION_CREATE", &d),
            EventTypeFlags::INTERACTION_CREATE,
        );
        let Some(Inbound {
            kind: InboundKind::Interaction(interaction, raw),
            ..
        }) = out.inbound
        else {
            panic!("no interaction");
        };
        assert_eq!(interaction.id.get(), 5);
        assert_eq!(raw.as_bytes(), d.as_bytes());

        // A component click with the same flaw is not a submit.
        let mut click = interaction_d();
        click["type"] = json!(3);
        click["data"] = json!({ "custom_id": "x", "component_type": true });
        let text = dispatch("INTERACTION_CREATE", &click.to_string());
        assert!(run(&text, EventTypeFlags::all()).inbound.is_none());
    }

    #[test]
    fn modal_submit_flaw_outside_components_is_dropped() {
        // A submit with valid components but a non-numeric `id` fails even
        // with `data.components` stripped, so it yields no inbound.
        let mut d = interaction_d();
        d["type"] = json!(5);
        d["id"] = json!("not-a-snowflake");
        d["data"] = json!({ "custom_id": "0m:", "components": [
            { "type": 18, "id": 1, "component": { "type": 23, "id": 2, "custom_id": "agree", "value": true } }
        ] });
        let text = dispatch("INTERACTION_CREATE", &d.to_string());
        assert!(run(&text, EventTypeFlags::all()).inbound.is_none());
    }

    #[test]
    fn other_event() {
        let text = dispatch("MESSAGE_DELETE", r#"{"id":"3","channel_id":"2"}"#);
        let out = run(&text, EventTypeFlags::all());
        let Some(Inbound {
            kind: InboundKind::Event(event),
            ..
        }) = out.inbound
        else {
            panic!("no event");
        };
        assert_eq!(event.kind(), EventType::MessageDelete);
        assert!(
            run(&text, EventTypeFlags::INTERACTION_CREATE)
                .inbound
                .is_none()
        );
    }

    #[test]
    fn status_events() {
        let ready = dispatch(
            "READY",
            r#"{"v":10,"user":{"id":"1","username":"b","discriminator":"0","avatar":null},"guilds":[],"session_id":"s","resume_gateway_url":"ws://x","application":{"id":"1","flags":0}}"#,
        );
        let out = run(&ready, EventTypeFlags::empty());
        assert_eq!(out.status, Some(ShardEvent::Ready { shard: 2 }));
        assert!(out.inbound.is_none());
        let out = run(&dispatch("RESUMED", "null"), EventTypeFlags::empty());
        assert_eq!(out.status, Some(ShardEvent::Resumed { shard: 2 }));
        assert!(out.inbound.is_none());
    }

    #[test]
    fn non_dispatch_yields_nothing() {
        for text in [
            r#"{"op":10,"d":{"heartbeat_interval":41250}}"#,
            r#"{"op":11,"d":null}"#,
            r#"{"op":9,"d":false}"#,
            r#"{"op":7,"d":null}"#,
        ] {
            let out = run(text, EventTypeFlags::all());
            assert!(out.status.is_none() && out.inbound.is_none(), "{text}");
        }
    }

    #[test]
    fn bad_input_yields_nothing() {
        let cases = [
            ("not json".to_owned(), EventTypeFlags::all()),
            (String::new(), EventTypeFlags::all()),
            (dispatch("INTERACTION_CREATE", "{}"), EventTypeFlags::all()),
            (
                dispatch("INTERACTION_CREATE", "null"),
                EventTypeFlags::all(),
            ),
            (dispatch("FOO_BAR", "{}"), EventTypeFlags::all()),
            (
                dispatch(
                    "INTERACTION_CREATE",
                    &serde_json::to_string(&interaction_d()).unwrap(),
                ),
                EventTypeFlags::MESSAGE_DELETE,
            ),
        ];
        for (text, wanted) in cases {
            let out = run(&text, wanted);
            assert!(out.status.is_none() && out.inbound.is_none(), "{text}");
        }
    }
}
