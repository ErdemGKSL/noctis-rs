//! The shard task: reads raw gateway messages, reports [`ShardEvent`]s and
//! pushes parsed [`Inbound`] items (FR-2, AD-10, AD-21).

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use noctis_ports::SessionStore;
use serde::Deserialize;
use serde_json::value::RawValue;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::Instrument;
use twilight_gateway::error::{ReceiveMessageError, ReceiveMessageErrorType};
use twilight_gateway::{Config, ConfigBuilder, Event, EventTypeFlags, Message, Shard, ShardId};
use twilight_model::gateway::event::{GatewayEvent, GatewayEventDeserializer};

use crate::gateway::CloseMode;
use crate::inbound::{Inbound, InboundKind, InboundSink, RawJson, ShardEvent};
use crate::queue::PortQueue;
use crate::session::{self, ResumeState, STORE_TIMEOUT, Stale};

/// The dispatch opcode.
const DISPATCH: u8 = 0;
/// The invalid session opcode.
const INVALID_SESSION: u8 = 9;

/// What one text frame produced.
#[derive(Debug, Default)]
pub(crate) struct Classified {
    /// A status change to report.
    pub(crate) status: Option<ShardEvent>,
    /// An item for the sink.
    pub(crate) inbound: Option<Inbound>,
    /// The `d` of an op 9 `INVALID_SESSION` (whether it is resumable);
    /// `None` for any other frame, or when `d` is not a boolean.
    pub(crate) invalid_session: Option<bool>,
}

/// An op 9 payload's `d`.
#[derive(Deserialize)]
struct InvalidSession {
    #[serde(default)]
    d: Option<bool>,
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
    if op == INVALID_SESSION {
        let invalid_session = serde_json::from_str::<InvalidSession>(&text)
            .ok()
            .and_then(|payload| payload.d);
        return Classified {
            invalid_session,
            ..Classified::default()
        };
    }

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
                ..Classified::default()
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
            let fallback = raw.and_then(interaction_without_components);
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
        invalid_session: None,
    }
}

/// A component click (interaction type 3) or a modal submit (type 5) that
/// twilight cannot parse, parsed again from a copy with its components
/// emptied: `message.components` when `message` is an object (a source
/// message holding a component type twilight does not know, CV2-16) and,
/// for a submit, also `data.components` (twilight rejects some modal
/// components, such as a Checkbox's boolean `value`, FR-64). Noctis never
/// reads twilight's components of either: the component pipeline decodes
/// the raw bytes, which are kept as they are (`ModalSubmission`, MOD-09,
/// `ctx.unknown_components()`). `None` for any other type, or when the copy
/// still fails.
fn interaction_without_components(raw: Bytes) -> Option<InboundKind> {
    let mut value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let kind = value.get("type")?.as_u64()?;
    if kind != 3 && kind != 5 {
        return None;
    }
    let empty = || serde_json::Value::Array(Vec::new());
    if let Some(message) = value.get_mut("message").and_then(|m| m.as_object_mut()) {
        message.insert("components".to_owned(), empty());
    }
    if kind == 5
        && let Some(data) = value.get_mut("data").and_then(|d| d.as_object_mut())
    {
        data.insert("components".to_owned(), empty());
    }
    let interaction = serde_json::from_value(value).ok()?;
    Some(InboundKind::Interaction(
        Box::new(interaction),
        RawJson::new(raw),
    ))
}

/// Where a shard task sends what it reads.
pub(crate) struct Lane {
    /// The dispatch event types parsed into [`Inbound`] items.
    pub(crate) wanted: EventTypeFlags,
    /// Where parsed items go.
    pub(crate) sink: Arc<dyn InboundSink>,
    /// Where status changes go.
    pub(crate) events: UnboundedSender<ShardEvent>,
}

/// How a shard task uses the `SessionStore` at start.
pub(crate) struct SessionStart {
    /// The session port.
    pub(crate) store: Arc<dyn SessionStore>,
    /// `true` loads the stored session; `false` deletes it and identifies.
    pub(crate) resume: bool,
}

/// The shard's entry in the `SessionStore`.
struct Stored {
    store: Arc<dyn SessionStore>,
    key: String,
}

/// Runs shard `id` until its stream ends or a close is requested, in span
/// `noctis.shard`.
///
/// With `session.resume`, it first loads the shard's stored session: a
/// valid one makes twilight resume it (RESUME at HELLO, no IDENTIFY and no
/// identify lease); otherwise the shard identifies. Without it, the stored
/// entry is deleted instead (bounded by `STORE_TIMEOUT`) and the shard
/// identifies; the delete finishes before the shard connects, so it
/// precedes IDENTIFY. A close requested while loading or deleting stops the
/// task before it connects.
pub(crate) async fn run(
    id: ShardId,
    config: Config<PortQueue>,
    session: SessionStart,
    app: Arc<str>,
    lane: Lane,
    mut close: watch::Receiver<Option<CloseMode>>,
    close_timeout: Duration,
) {
    let span = tracing::info_span!("noctis.shard", app = &*app, shard = id.number());
    async move {
        let stored = Stored {
            key: session::key(id),
            store: session.store,
        };
        let resume = session.resume;
        let loaded = tokio::select! {
            biased;
            mode = close_requested(&mut close) => {
                tracing::debug!(
                    target: "noctis::gateway",
                    "close requested before the shard connected"
                );
                if mode == CloseMode::Fresh {
                    let deadline = Instant::now() + close_timeout;
                    session::clear(&*stored.store, &stored.key, deadline).await;
                }
                let _ = lane.events.send(ShardEvent::Closed { shard: id.number() });
                return;
            }
            loaded = async {
                if resume {
                    session::load(&*stored.store, &stored.key).await
                } else {
                    let deadline = Instant::now() + STORE_TIMEOUT;
                    session::clear(&*stored.store, &stored.key, deadline).await;
                    None
                }
            } => loaded,
        };
        let resume = ResumeState::new(loaded.is_some());
        let config = match loaded {
            Some((session, resume_url)) => ConfigBuilder::from(config)
                .session(session)
                .resume_url(resume_url)
                .build(),
            None => config,
        };
        let shard = Shard::with_config(id, config);
        drive(shard, lane, stored, resume, close, close_timeout).await;
    }
    .instrument(span)
    .await;
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
    stored: Stored,
    mut resume: ResumeState,
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
                let stale = resume.on_frame(classified.status.as_ref(), classified.invalid_session);
                if let Some(status) = classified.status {
                    let _ = lane.events.send(status);
                }
                if let Some(inbound) = classified.inbound {
                    lane.sink.push(inbound);
                }
                if let Some(stale) = stale {
                    match stale {
                        Stale::Rejected => tracing::info!(
                            target: "noctis::gateway",
                            "stored session rejected; identifying"
                        ),
                        Stale::Superseded => tracing::info!(
                            target: "noctis::gateway",
                            "stored session superseded by a new session"
                        ),
                    }
                    // Twilight reconnects only on the next poll, so the
                    // entry is gone before any later IDENTIFY or write.
                    let deadline = Instant::now() + STORE_TIMEOUT;
                    session::clear(&*stored.store, &stored.key, deadline).await;
                }
            }
        }
    };
    let event = match requested {
        Some(mode) => {
            close_shard(&mut shard, mode, close_timeout, &stored).await;
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
///
/// Then, within the same `close_timeout`: an acknowledged resumable close
/// stores the shard's session, a fresh close deletes the stored entry
/// (identified or not), anything else leaves the store alone.
async fn close_shard(
    shard: &mut Shard<PortQueue>,
    mode: CloseMode,
    close_timeout: Duration,
    stored: &Stored,
) {
    let deadline = Instant::now() + close_timeout;
    if !shard.state().is_identified() {
        tracing::debug!(
            target: "noctis::gateway",
            state = ?shard.state(),
            "shard not identified; stopped without a close frame"
        );
        if mode == CloseMode::Fresh {
            session::clear(&*stored.store, &stored.key, deadline).await;
        }
        return;
    }
    shard.close(mode.frame());
    let mut discarded = 0_usize;
    let acked = tokio::time::timeout_at(deadline, async {
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
    match mode {
        CloseMode::Resumable if !acked => tracing::debug!(
            target: "noctis::gateway",
            "close not acknowledged; session not stored"
        ),
        CloseMode::Resumable => {
            if let (Some(current), Some(resume_url)) = (shard.session(), shard.resume_url()) {
                session::save(&*stored.store, &stored.key, current, resume_url, deadline).await;
            } else {
                tracing::debug!(
                    target: "noctis::gateway",
                    "no session or resume URL; session not stored"
                );
            }
        }
        CloseMode::Fresh => session::clear(&*stored.store, &stored.key, deadline).await,
    }
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

    /// A message object holding `components`.
    fn message(components: &Value) -> Value {
        json!({
            "id": "88",
            "channel_id": "9",
            "author": { "id": "7", "username": "bot", "discriminator": "0", "avatar": null },
            "content": "",
            "timestamp": "2026-10-20T12:00:00.000000+00:00",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0,
            "flags": 32768,
            "components": components,
        })
    }

    /// A button click on a message holding `components`.
    fn click(components: &Value) -> Value {
        let mut d = interaction_d();
        d["type"] = json!(3);
        d["data"] = json!({ "custom_id": "0t:", "component_type": 2 });
        d["message"] = message(components);
        d
    }

    fn twilight_parses(d: &str) -> bool {
        serde_json::from_str::<twilight_model::application::interaction::Interaction>(d).is_ok()
    }

    /// The interaction and raw bytes `d` classifies to, if any.
    fn interaction_of(
        d: &str,
    ) -> Option<(
        Box<twilight_model::application::interaction::Interaction>,
        RawJson,
    )> {
        match run(&dispatch("INTERACTION_CREATE", d), EventTypeFlags::all()).inbound {
            Some(Inbound {
                kind: InboundKind::Interaction(interaction, raw),
                ..
            }) => Some((interaction, raw)),
            _ => None,
        }
    }

    /// Story 9.2 G1: a click on a message whose components twilight cannot
    /// parse still dispatches, with the raw bytes unchanged.
    #[test]
    fn cv2_16_click_unparsable_message_still_dispatches() {
        for bad in [json!({"type": 300}), json!({"type": 99, "content": 5})] {
            let d = click(&json!([{ "type": 10, "id": 1, "content": "a" }, bad])).to_string();
            let (interaction, raw) = interaction_of(&d).expect("the click dispatches");
            assert_eq!(interaction.id.get(), 5);
            assert_eq!(raw.as_bytes(), d.as_bytes());
        }
        assert!(!twilight_parses(
            &click(&json!([{ "type": 300 }])).to_string()
        ));
    }

    /// G2: a submit of a modal opened from such a message, with a Checkbox
    /// twilight rejects: both component lists are emptied in the copy.
    #[test]
    fn cv2_16_submit_from_unparsable_message_still_dispatches() {
        let mut d = interaction_d();
        d["type"] = json!(5);
        d["data"] = json!({ "custom_id": "0m:", "components": [
            { "type": 18, "id": 1, "component": { "type": 23, "id": 2, "custom_id": "agree", "value": true } }
        ] });
        d["message"] = message(&json!([{ "type": 300 }]));
        let d = d.to_string();
        assert!(!twilight_parses(&d));
        let (interaction, raw) = interaction_of(&d).expect("the submit dispatches");
        assert_eq!(interaction.id.get(), 5);
        assert_eq!(raw.as_bytes(), d.as_bytes());
    }

    /// G3: a component twilight parses needs no fallback.
    #[test]
    fn cv2_16_parsable_unknown_component_parses_normally() {
        let d = click(&json!([{ "type": 99 }])).to_string();
        assert!(twilight_parses(&d));
        let (_, raw) = interaction_of(&d).expect("the click dispatches");
        assert_eq!(raw.as_bytes(), d.as_bytes());
    }

    /// G4: other types, and a click whose `data` is broken, are still
    /// ignored.
    #[test]
    fn cv2_16_no_fallback_outside_components() {
        let mut command = click(&json!([{ "type": 300 }]));
        command["type"] = json!(2);
        assert!(interaction_of(&command.to_string()).is_none());
        let mut broken = click(&json!([{ "type": 300 }]));
        broken["data"] = json!({ "custom_id": "x", "component_type": true });
        assert!(interaction_of(&broken.to_string()).is_none());
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

    /// R6: op 9's `d` is read only when it is a boolean.
    #[test]
    fn invalid_session_flag() {
        for (text, expected) in [
            (r#"{"op":9,"d":false}"#, Some(false)),
            (r#"{"op":9,"d":true}"#, Some(true)),
            (r#"{"op":9,"d":null}"#, None),
            (r#"{"op":9}"#, None),
            (r#"{"op":7,"d":false}"#, None),
        ] {
            let out = run(text, EventTypeFlags::all());
            assert_eq!(out.invalid_session, expected, "{text}");
            assert!(out.status.is_none() && out.inbound.is_none(), "{text}");
        }
        let ready = dispatch("RESUMED", "null");
        assert_eq!(run(&ready, EventTypeFlags::all()).invalid_session, None);
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
