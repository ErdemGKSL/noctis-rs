//! Story 6.5 Matrix 18-34: the Responder over a recording upstream.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use hyper::{Method, StatusCode};
use noctis_rules::{LimitMode, RuleId};
use noctis_ui::{
    ActionRow, Attachment, AttachmentSet, Components, LegacyMessage, LinkButton, TextDisplay,
};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::*;
use crate::edge::test_support::{Answer, Seen, TestServer};
use crate::{App, Runtime};

/// One text.
pub(crate) fn v2(text: &str) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(text.to_owned())))
}

/// The JSON of [`v2`]'s components.
fn v2_components(text: &str) -> Value {
    json!([{ "type": 10, "id": 1, "content": text }])
}

/// Discord's answers: 204 for callbacks and deletes, a message with id 55
/// for follow-ups and channel messages, `{}` otherwise.
pub(crate) fn discord() -> Answer {
    Arc::new(|seen: &Seen| {
        if seen.path.ends_with("/callback") || seen.method == Method::DELETE {
            (StatusCode::NO_CONTENT, Bytes::new())
        } else if seen.method == Method::POST && !seen.path.ends_with("/typing") {
            (StatusCode::OK, Bytes::from_static(br#"{"id":"55"}"#))
        } else {
            (StatusCode::OK, Bytes::from_static(b"{}"))
        }
    })
}

/// An App sending through a recording upstream.
pub(crate) struct Fixture {
    pub(crate) server: TestServer,
    pub(crate) app: App,
}

impl Fixture {
    pub(crate) async fn start(answer: Answer) -> Self {
        Self::start_with(answer, Limits::default()).await
    }

    pub(crate) async fn start_with(answer: Answer, limits: Limits) -> Self {
        let server = TestServer::start_with(false, answer).await;
        let app = App::builder()
            .runtime(&Runtime::for_test())
            .http_proxy(server.authority())
            .limits(limits)
            .build()
            .unwrap();
        Self { server, app }
    }

    /// An interaction transport (application 7, interaction 100).
    pub(crate) fn interaction(&self, token: &str, ephemerality: Ephemerality) -> Arc<Shared> {
        self.interaction_at(token, ephemerality, Instant::now())
    }

    fn interaction_at(
        &self,
        token: &str,
        ephemerality: Ephemerality,
        received_at: Instant,
    ) -> Arc<Shared> {
        let transport = Transport::Interaction(InteractionTarget {
            app: Arc::clone(self.app.shared()),
            application_id: Id::new(7),
            interaction_id: Id::new(100),
            token: token.to_owned(),
            received_at,
        });
        Shared::new(transport, ephemerality, self.app.limits().clone())
    }

    /// A message transport (channel 5, message 6).
    fn message(&self, ephemerality: Ephemerality) -> Arc<Shared> {
        let transport = Transport::Message(MessageTarget {
            app: Arc::clone(self.app.shared()),
            channel_id: Id::new(5),
            message_id: Id::new(6),
        });
        Shared::new(transport, ephemerality, self.app.limits().clone())
    }

    /// Every request: method, path with query, JSON body (`Null` when
    /// empty).
    pub(crate) fn requests(&self) -> Vec<(Method, String, Value)> {
        self.server
            .seen()
            .into_iter()
            .map(|s| {
                let path = match &s.query {
                    Some(q) => format!("{}?{q}", s.path),
                    None => s.path.clone(),
                };
                let body = serde_json::from_slice(&s.body).unwrap_or(Value::Null);
                (s.method, path, body)
            })
            .collect()
    }
}

const CB: &str = "/api/v10/interactions/100/tok/callback";
const ORIGINAL: &str = "/api/v10/webhooks/7/tok/messages/@original";
const FOLLOWUP: &str = "/api/v10/webhooks/7/tok?wait=true&with_components=true";

fn responder(shared: &Arc<Shared>) -> CommandResponder {
    Responder::new(Arc::clone(shared))
}

/// Matrix 18: one reply, then `done`; nothing after.
#[tokio::test]
async fn reply_then_done() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("hi")).await.unwrap();
    let _: Responded = acked.done();
    assert_eq!(shared.state(), REPLIED);
    assert_eq!(
        fx.requests(),
        [(
            Method::POST,
            CB.to_owned(),
            json!({ "type": 4, "data": { "flags": 32768, "components": v2_components("hi"), "allowed_mentions": { "parse": [] } } })
        )]
    );
    let seen = fx.server.seen();
    assert!(seen[0].headers.get("authorization").is_none());
}

/// Matrix 2: a declared ephemeral reply.
#[tokio::test]
async fn ephemeral_reply_flags() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    let _ = responder(&shared).reply(v2("hi")).await.unwrap().done();
    assert_eq!(fx.requests()[0].2["data"]["flags"], 32832);
}

/// Matrix 19 (INT-08): defer bodies carry only `EPHEMERAL`.
#[tokio::test]
async fn defer_bodies() {
    let fx = Fixture::start(discord()).await;
    for (ephemerality, body) in [
        (Ephemerality::Public, json!({ "type": 5 })),
        (
            Ephemerality::Ephemeral,
            json!({ "type": 5, "data": { "flags": 64 } }),
        ),
    ] {
        fx.server.log.lock().unwrap().clear();
        let shared = fx.interaction("tok", Ephemerality::Public);
        let _ = responder(&shared).defer(ephemerality).await.unwrap().done();
        assert_eq!(shared.state(), DEFERRED);
        assert_eq!(fx.requests(), [(Method::POST, CB.to_owned(), body)]);
    }
}

/// Matrix 20 and 21: a returned message after a defer or a reply edits
/// the original.
#[tokio::test]
async fn finish_edits_after_an_ack() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    let _ = responder(&shared).reply(v2("a")).await.unwrap().done();
    shared.finish(v2("b").into()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].2["data"]["components"], v2_components("a"));
    assert_eq!(
        requests[1],
        (
            Method::PATCH,
            ORIGINAL.to_owned(),
            json!({ "flags": 32768, "components": v2_components("b"), "allowed_mentions": { "parse": [] } })
        )
    );
}

/// Matrix 22: a returned message while Pending is the reply.
#[tokio::test]
async fn finish_replies_while_pending() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    shared.finish(v2("a").into()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].1, CB);
    assert_eq!(requests[0].2["type"], 4);
}

/// Matrix 23: edit formats.
#[tokio::test]
async fn edit_formats() {
    let fx = Fixture::start(discord()).await;
    // V2 original, V2 edit: no clears.
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    acked.edit_original(v2("b")).await.unwrap();
    assert!(fx.requests()[1].2.get("content").is_none());
    // V2 original, legacy edit: no HTTP.
    let err = acked
        .edit_original(LegacyMessage::new("x"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResponderError::Invalid(UiError::V2ToLegacy)),
        "{err}"
    );
    assert_eq!(fx.requests().len(), 2);
    let _ = acked.done();

    // Legacy original, V2 edit: the legacy clears.
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .reply(LegacyMessage::new("x"))
        .await
        .unwrap();
    acked.edit_original(v2("b")).await.unwrap();
    let edit = &fx.requests()[3].2;
    assert_eq!(
        *edit,
        json!({
            "flags": 32768, "content": null, "embeds": [], "poll": null, "sticker_ids": [],
            "components": v2_components("b"), "allowed_mentions": { "parse": [] }
        })
    );
    // Now V2: a legacy edit fails.
    assert!(acked.edit_original(LegacyMessage::new("y")).await.is_err());
    let _ = acked.done();

    // Deferred, legacy edit: the legacy body.
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .defer(Ephemerality::Public)
        .await
        .unwrap();
    acked.edit_original(LegacyMessage::new("z")).await.unwrap();
    assert_eq!(fx.requests().last().unwrap().2["content"], "z");
    let _ = acked.done();
}

/// Matrix 24-26: follow-ups, their edits and deletes, and deleting the
/// original.
#[tokio::test]
async fn followups_and_deletes() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .defer(Ephemerality::Ephemeral)
        .await
        .unwrap();
    let public = acked.followup(v2("p"), Visibility::Public).await.unwrap();
    let private = acked
        .followup(v2("e"), Visibility::Ephemeral)
        .await
        .unwrap();
    assert_eq!(public.message_id(), Id::new(55));
    private.edit(v2("e2")).await.unwrap();
    private.delete().await.unwrap();
    acked.delete_original().await.unwrap();
    let _ = acked.done();
    let requests = fx.requests();
    assert_eq!(requests[1].0, Method::POST);
    assert_eq!(requests[1].1, FOLLOWUP);
    assert_eq!(requests[1].2["flags"], 32768);
    assert_eq!(requests[2].2["flags"], 32832);
    assert_eq!(
        (requests[3].0.clone(), requests[3].1.as_str()),
        (Method::PATCH, "/api/v10/webhooks/7/tok/messages/55")
    );
    assert_eq!(
        (requests[4].0.clone(), requests[4].1.as_str()),
        (Method::DELETE, "/api/v10/webhooks/7/tok/messages/55")
    );
    assert_eq!(
        (requests[5].0.clone(), requests[5].1.as_str()),
        (Method::DELETE, ORIGINAL)
    );
    assert!(format!("{private:?}").contains("message_id"));
}

fn long_label(len: usize) -> V2Message {
    V2Message::new(Components::new().push(ActionRow::buttons([
        LinkButton::new("https://x.y").label("a".repeat(len)),
    ])))
}

fn texts(n: usize) -> V2Message {
    let mut components = Components::new();
    for i in 0..n {
        components = components.push(TextDisplay::new(format!("t{i}")));
    }
    V2Message::new(components)
}

/// Matrix 27 and 28: Limits, Strict and Lenient.
#[tokio::test]
async fn limits_strict_and_lenient() {
    let fx = Fixture::start(discord()).await;
    for msg in [long_label(81), texts(41)] {
        let shared = fx.interaction("tok", Ephemerality::Public);
        let err = responder(&shared).reply(msg).await.unwrap_err();
        let ResponderError::Invalid(UiError::Limits { violations }) = &err else {
            panic!("{err}");
        };
        assert!(
            violations
                .iter()
                .any(|v| v.rule == RuleId::CV2_08 || v.rule == RuleId::CV2_03),
            "{violations:?}"
        );
    }
    assert!(fx.requests().is_empty());
    for msg in [long_label(80), texts(40)] {
        let shared = fx.interaction("tok", Ephemerality::Public);
        let _ = responder(&shared).reply(msg).await.unwrap().done();
    }
    assert_eq!(fx.requests().len(), 2);

    let mut limits = Limits::default();
    limits.mode = LimitMode::Lenient;
    let fx = Fixture::start_with(discord(), limits.clone()).await;
    assert_eq!(fx.app.limits(), &limits);
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared)
        .reply(long_label(81))
        .await
        .unwrap()
        .done();
    let label = fx.requests()[0].2["data"]["components"][0]["components"][0]["label"].clone();
    assert_eq!(label, format!("{}…", "a".repeat(79)));
}

/// Matrix 29 and 33: attachments and a bad token send nothing.
#[tokio::test]
async fn attachments_and_bad_token() {
    let fx = Fixture::start(discord()).await;
    let mut set = AttachmentSet::new();
    let _ = set.add(Attachment::new(&b"x"[..], "png").unwrap());
    let shared = fx.interaction("tok", Ephemerality::Public);
    let err = responder(&shared)
        .reply(v2("a").attachments(set))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResponderError::AttachmentsUnsupported),
        "{err}"
    );

    let shared = fx.interaction("a b", Ephemerality::Public);
    assert!(!format!("{shared:?}").contains("a b"));
    let err = responder(&shared).reply(v2("a")).await.unwrap_err();
    assert!(matches!(err, ResponderError::InvalidToken), "{err}");
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    for err in [
        acked.edit_original(v2("a")).await.unwrap_err(),
        acked
            .followup(v2("a"), Visibility::Public)
            .await
            .unwrap_err(),
        acked.delete_original().await.unwrap_err(),
    ] {
        assert!(matches!(err, ResponderError::InvalidToken), "{err}");
    }
    let _ = acked.done();
    assert!(fx.requests().is_empty());
}

/// Matrix 31: a rejected callback; 40060 counts as answered.
#[tokio::test]
async fn callback_rejected() {
    let answer: Answer = Arc::new(|seen: &Seen| {
        let code = if seen.path.contains("/200/") {
            40060
        } else {
            50035
        };
        (
            StatusCode::BAD_REQUEST,
            Bytes::from(format!(r#"{{"code":{code},"message":"x"}}"#)),
        )
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let err = responder(&shared).reply(v2("a")).await.unwrap_err();
    assert!(
        matches!(
            err,
            ResponderError::Rejected {
                status: 400,
                code: Some(50035)
            }
        ),
        "{err}"
    );
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(200),
        token: "tok".to_owned(),
        received_at: Instant::now(),
    });
    let shared = Shared::new(transport, Ephemerality::Public, Limits::default());
    let _ = responder(&shared).reply(v2("a")).await.unwrap().done();
}

/// Matrix 32: past the ack deadline nothing is sent.
#[tokio::test]
async fn callback_late() {
    let fx = Fixture::start(discord()).await;
    let late = Instant::now().checked_sub(Duration::from_secs(3)).unwrap();
    let shared = fx.interaction_at("tok", Ephemerality::Public, late);
    let err = responder(&shared).reply(v2("a")).await.unwrap_err();
    assert!(matches!(err, ResponderError::AckDeadlineMissed(_)), "{err}");
    assert!(fx.requests().is_empty());
}

/// Matrix 34: a rejected edit or follow-up; an unreachable upstream.
#[tokio::test]
async fn edit_and_followup_rejected() {
    let answer: Answer = Arc::new(|seen: &Seen| {
        if seen.path.ends_with("/callback") {
            (StatusCode::NO_CONTENT, Bytes::new())
        } else {
            (
                StatusCode::NOT_FOUND,
                Bytes::from_static(br#"{"code":10015,"message":"Unknown Webhook"}"#),
            )
        }
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    for err in [
        acked.edit_original(v2("b")).await.unwrap_err(),
        acked
            .followup(v2("b"), Visibility::Public)
            .await
            .unwrap_err(),
    ] {
        assert!(
            matches!(
                err,
                ResponderError::Rejected {
                    status: 404,
                    code: Some(10015)
                }
            ),
            "{err}"
        );
    }
    let _ = acked.done();
    assert_eq!(
        ResponderError::Rejected {
            status: 404,
            code: None
        }
        .to_string(),
        "Discord rejected the response with HTTP 404"
    );

    let app = App::builder()
        .runtime(&Runtime::for_test())
        .http_proxy("127.0.0.1:1")
        .build()
        .unwrap();
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
    });
    let shared = Shared::new(transport, Ephemerality::Public, Limits::default());
    let err = responder(&shared).reply(v2("a")).await.unwrap_err();
    assert!(matches!(err, ResponderError::Transport), "{err}");
}

/// Matrix 48-50: the message transport.
#[tokio::test]
async fn message_transport() {
    let fx = Fixture::start(discord()).await;
    // defer(Public), then a returned message: typing, then the reply.
    let shared = fx.message(Ephemerality::Public);
    let _ = responder(&shared)
        .defer(Ephemerality::Public)
        .await
        .unwrap()
        .done();
    shared.finish(v2("a").into()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(
        (requests[0].0.clone(), requests[0].1.as_str()),
        (Method::POST, "/api/v10/channels/5/typing")
    );
    assert_eq!(requests[1].1, "/api/v10/channels/5/messages");
    assert_eq!(
        requests[1].2,
        json!({
            "flags": 32768, "components": v2_components("a"), "allowed_mentions": { "parse": [] },
            "message_reference": { "message_id": "6", "fail_if_not_exists": false }
        })
    );
    // The reply is the original now: edits and deletes target it.
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    acked.edit_original(v2("b")).await.unwrap();
    acked.delete_original().await.unwrap();
    let requests = fx.requests();
    assert_eq!(
        (requests[2].0.clone(), requests[2].1.as_str()),
        (Method::PATCH, "/api/v10/channels/5/messages/55")
    );
    assert_eq!(
        (requests[3].0.clone(), requests[3].1.as_str()),
        (Method::DELETE, "/api/v10/channels/5/messages/55")
    );
    let handle = acked.followup(v2("c"), Visibility::Public).await.unwrap();
    handle.edit(v2("d")).await.unwrap();
    handle.delete().await.unwrap();
    assert_eq!(fx.requests().len(), 7);
    let _ = acked.done();

    // Nothing to delete before a reply.
    fx.server.log.lock().unwrap().clear();
    let shared = fx.message(Ephemerality::Public);
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    acked.delete_original().await.unwrap();
    let _ = acked.done();
    assert!(fx.requests().is_empty());

    // Ephemeral is unsupported, before any request.
    let shared = fx.message(Ephemerality::Ephemeral);
    let err = responder(&shared).reply(v2("a")).await.unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let shared = fx.message(Ephemerality::Public);
    let err = responder(&shared)
        .defer(Ephemerality::Ephemeral)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    let err = acked
        .followup(v2("a"), Visibility::Ephemeral)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let _ = acked.done();
    assert!(fx.requests().is_empty());

    // A declared-ephemeral Command deferred publicly cannot post its
    // first message through an edit or a returned message.
    let shared = fx.message(Ephemerality::Ephemeral);
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    let err = acked.edit_original(v2("a")).await.unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let _ = acked.done();
    let err = shared.finish(v2("a").into()).await.unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    assert!(fx.requests().is_empty());
}

/// Matrix 52: the detached transport sends nothing and still validates.
#[tokio::test]
async fn detached_transport() {
    let shared = Shared::new(
        Transport::Detached,
        Ephemerality::Ephemeral,
        Limits::default(),
    );
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    acked.edit_original(v2("b")).await.unwrap();
    let handle = acked.followup(v2("c"), Visibility::Public).await.unwrap();
    handle.edit(v2("d")).await.unwrap();
    handle.delete().await.unwrap();
    acked.delete_original().await.unwrap();
    assert!(acked.edit_original(LegacyMessage::new("x")).await.is_err());
    let _ = acked.done();
    let err = responder(&shared).reply(long_label(81)).await.unwrap_err();
    assert!(matches!(err, ResponderError::Invalid(_)), "{err}");
    assert!(format!("{shared:?}").contains("Detached"));
}

const fn assert_send<T: Send>() {}
const _: () = assert_send::<CommandResponder>();
const _: () = assert_send::<Responder<Command, Acked>>();
const _: () = assert_send::<FollowupHandle>();
const _: () = assert_send::<Responded>();

#[test]
fn values() {
    assert_eq!(Ephemerality::default(), Ephemerality::Public);
    let msg: OutgoingMessage = Components::new().into();
    assert!(matches!(msg, OutgoingMessage::V2(_)));
    let r = responder(&Shared::new(
        Transport::Detached,
        Ephemerality::Public,
        Limits::default(),
    ));
    let debug = format!("{r:?}");
    assert!(
        debug.contains("Pending") && debug.contains("Command"),
        "{debug}"
    );
    drop(r);
    assert_eq!(format!("{:?}", Command::<NoModal>(PhantomData)), "Command");
}
