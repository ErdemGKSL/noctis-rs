//! The shard task: reads raw gateway messages, reports [`ShardEvent`]s and
//! pushes parsed [`Inbound`] items (FR-2, AD-10, AD-21).

use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::value::RawValue;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::Instant;
use tracing::Instrument;
use twilight_gateway::error::ReceiveMessageErrorType;
use twilight_gateway::{Event, EventTypeFlags, Message, Shard, ShardId};
use twilight_model::gateway::event::{GatewayEvent, GatewayEventDeserializer};

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
            tracing::warn!(
                target: "noctis::gateway",
                event_type = event_type.as_deref().unwrap_or("-"),
                "gateway payload could not be parsed; ignored"
            );
            None
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

/// Runs one shard until its stream ends, in span `noctis.shard`.
pub(crate) async fn run(
    shard: Shard<PortQueue>,
    app: Arc<str>,
    wanted: EventTypeFlags,
    sink: Arc<dyn InboundSink>,
    events: UnboundedSender<ShardEvent>,
) {
    let span = tracing::info_span!("noctis.shard", app = &*app, shard = shard.id().number());
    drive(shard, wanted, sink, events).instrument(span).await;
}

async fn drive(
    mut shard: Shard<PortQueue>,
    wanted: EventTypeFlags,
    sink: Arc<dyn InboundSink>,
    events: UnboundedSender<ShardEvent>,
) {
    let id = shard.id();
    let mut last_close = None;
    while let Some(item) = shard.next().await {
        match item {
            Err(error) => {
                // `Deserializing`'s `Display` carries the payload: never log it.
                if let ReceiveMessageErrorType::Deserializing { .. } = error.kind() {
                    tracing::warn!(
                        target: "noctis::gateway",
                        "gateway message could not be deserialized; ignored"
                    );
                } else {
                    tracing::warn!(target: "noctis::gateway", %error, "gateway receive failed");
                }
            }
            Ok(Message::Close(frame)) => {
                last_close = frame.map(|f| f.code);
                let _ = events.send(ShardEvent::Disconnected {
                    shard: id.number(),
                    close_code: last_close,
                });
            }
            Ok(Message::Text(text)) => {
                let received_at = Instant::now();
                let classified = classify(text, wanted, id, received_at);
                if let Some(status) = classified.status {
                    let _ = events.send(status);
                }
                if let Some(inbound) = classified.inbound {
                    sink.push(inbound);
                }
            }
        }
    }
    let _ = events.send(ShardEvent::Ended {
        shard: id.number(),
        close_code: last_close,
    });
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
