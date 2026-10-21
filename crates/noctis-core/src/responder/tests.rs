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

    /// As [`Fixture::start`], with the payload key `key`.
    pub(crate) async fn start_keyed(answer: Answer, key: noctis_codec::PayloadKey) -> Self {
        let server = TestServer::start_with(false, answer).await;
        let app = App::builder()
            .runtime(&Runtime::for_test())
            .http_proxy(server.authority())
            .payload_key(key)
            .build()
            .unwrap();
        Self { server, app }
    }

    /// As [`Fixture::start`], with a bot token, so channel routes carry
    /// `authorization: Bot …` while interaction routes must not.
    pub(crate) async fn start_with_token(answer: Answer) -> Self {
        let server = TestServer::start_with(false, answer).await;
        let app = App::builder()
            .runtime(&Runtime::for_test())
            .http_proxy(server.authority())
            .token(crate::Token::new("abc").unwrap())
            .build()
            .unwrap();
        Self { server, app }
    }

    /// As [`Fixture::start`], with the `RefStore` `refs` and, if given,
    /// the App's default Ref TTL.
    pub(crate) async fn start_refs(
        answer: Answer,
        refs: Arc<crate::test_support::TestRefs>,
        ttl: Option<Duration>,
    ) -> Self {
        let server = TestServer::start_with(false, answer).await;
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .http_proxy(server.authority())
            .ref_store(refs);
        if let Some(ttl) = ttl {
            builder.ref_ttl(ttl);
        }
        let app = builder.build().unwrap();
        Self { server, app }
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
            channel_id: Some(Id::new(9)),
        });
        Shared::new(transport, ephemerality, self.app.limits().clone())
    }

    /// An interaction transport of a component or modal handler declaring
    /// `declared`, on the source message `source`.
    pub(crate) fn component(
        &self,
        declared: Declared,
        source: Option<SourceMessage>,
    ) -> Arc<Shared> {
        let transport = Transport::Interaction(InteractionTarget {
            app: Arc::clone(self.app.shared()),
            application_id: Id::new(7),
            interaction_id: Id::new(100),
            token: "tok".to_owned(),
            received_at: Instant::now(),
            channel_id: Some(Id::new(9)),
        });
        Shared::declared(transport, declared, source, self.app.limits().clone(), None)
    }

    /// As [`component`](Self::component), for a modal submit
    /// (`Shared::declared_submit`).
    pub(crate) fn submit(&self, declared: Declared, source: Option<SourceMessage>) -> Arc<Shared> {
        let transport = Transport::Interaction(InteractionTarget {
            app: Arc::clone(self.app.shared()),
            application_id: Id::new(7),
            interaction_id: Id::new(100),
            token: "tok".to_owned(),
            received_at: Instant::now(),
            channel_id: Some(Id::new(9)),
        });
        Shared::declared_submit(transport, declared, source, self.app.limits().clone(), None)
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
/// A type-4 callback asks for the message (`with_response`).
const CB_REPLY: &str = "/api/v10/interactions/100/tok/callback?with_response=true";
const ORIGINAL: &str = "/api/v10/webhooks/7/tok/messages/@original";
const FOLLOWUP: &str = "/api/v10/webhooks/7/tok?wait=true";

/// Forgets that the handler answered, so a test can play another handle
/// (the gate race of the auto-defer) on the same state.
pub(crate) fn unanswer(shared: &Shared) {
    shared.handler_answered.store(false, Ordering::Release);
}

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
            CB_REPLY.to_owned(),
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
        let _ = responder(&shared)
            .defer_reply_as(ephemerality)
            .await
            .unwrap()
            .done();
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
            json!({ "flags": 32768, "components": v2_components("b"), "allowed_mentions": { "parse": [] }, "attachments": [] })
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
    assert_eq!(requests[0].1, CB_REPLY);
    assert_eq!(requests[0].2["type"], 4);
}

/// Matrix 23: edit formats.
#[tokio::test]
async fn edit_formats() {
    let fx = Fixture::start(discord()).await;
    // V2 original, V2 edit: no clears.
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    acked.edit_reply(v2("b")).await.unwrap();
    assert!(fx.requests()[1].2.get("content").is_none());
    // V2 original, legacy edit: no HTTP.
    let err = acked.edit_reply(LegacyMessage::new("x")).await.unwrap_err();
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
    acked.edit_reply(v2("b")).await.unwrap();
    let edit = &fx.requests()[3].2;
    assert_eq!(
        *edit,
        json!({
            "flags": 32768, "content": null, "embeds": [], "poll": null, "sticker_ids": [],
            "components": v2_components("b"), "allowed_mentions": { "parse": [] }, "attachments": []
        })
    );
    // Now V2: a legacy edit fails.
    assert!(acked.edit_reply(LegacyMessage::new("y")).await.is_err());
    let _ = acked.done();

    // Deferred, legacy edit: the legacy body.
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap();
    acked.edit_reply(LegacyMessage::new("z")).await.unwrap();
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
        .defer_reply_as(Ephemerality::Ephemeral)
        .await
        .unwrap();
    let public = acked.follow_up(v2("p"), Visibility::Public).await.unwrap();
    let private = acked
        .follow_up(v2("e"), Visibility::Ephemeral)
        .await
        .unwrap();
    assert_eq!(public.message_id(), Id::new(55));
    private.edit(v2("e2")).await.unwrap();
    private.delete().await.unwrap();
    acked.delete_reply().await.unwrap();
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
        acked.edit_reply(v2("a")).await.unwrap_err(),
        acked
            .follow_up(v2("a"), Visibility::Public)
            .await
            .unwrap_err(),
        acked.delete_reply().await.unwrap_err(),
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
        channel_id: None,
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
        acked.edit_reply(v2("b")).await.unwrap_err(),
        acked
            .follow_up(v2("b"), Visibility::Public)
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
        channel_id: None,
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
        .defer_reply_as(Ephemerality::Public)
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
    acked.edit_reply(v2("b")).await.unwrap();
    acked.delete_reply().await.unwrap();
    let requests = fx.requests();
    assert_eq!(
        (requests[2].0.clone(), requests[2].1.as_str()),
        (Method::PATCH, "/api/v10/channels/5/messages/55")
    );
    assert_eq!(
        (requests[3].0.clone(), requests[3].1.as_str()),
        (Method::DELETE, "/api/v10/channels/5/messages/55")
    );
    let handle = acked.follow_up(v2("c"), Visibility::Public).await.unwrap();
    handle.edit(v2("d")).await.unwrap();
    handle.delete().await.unwrap();
    assert_eq!(fx.requests().len(), 7);
    let _ = acked.done();

    // Nothing to delete before a reply.
    fx.server.log.lock().unwrap().clear();
    let shared = fx.message(Ephemerality::Public);
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    acked.delete_reply().await.unwrap();
    let _ = acked.done();
    assert!(fx.requests().is_empty());

    // Ephemeral is unsupported, before any request.
    let shared = fx.message(Ephemerality::Ephemeral);
    let err = responder(&shared).reply(v2("a")).await.unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let shared = fx.message(Ephemerality::Public);
    let err = responder(&shared)
        .defer_reply_as(Ephemerality::Ephemeral)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    let err = acked
        .follow_up(v2("a"), Visibility::Ephemeral)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    let _ = acked.done();
    assert!(fx.requests().is_empty());

    // `reply_as` decides per call: Ephemeral fails before any request,
    // Public posts even on a declared-ephemeral Command.
    let shared = fx.message(Ephemerality::Public);
    let err = responder(&shared)
        .reply_as(v2("a"), Ephemerality::Ephemeral)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::EphemeralUnsupported), "{err}");
    assert!(fx.requests().is_empty());
    let shared = fx.message(Ephemerality::Ephemeral);
    let _ = responder(&shared)
        .reply_as(v2("a"), Ephemerality::Public)
        .await
        .unwrap()
        .done();
    let requests = fx.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        (requests[0].0.clone(), requests[0].1.as_str()),
        (Method::POST, "/api/v10/channels/5/messages")
    );
}

/// Matrix 48-50: a declared-ephemeral Command deferred publicly on the
/// message transport posts its first message through an edit or a
/// returned message.
#[tokio::test]
async fn message_transport_public_defer_of_ephemeral_command() {
    let fx = Fixture::start(discord()).await;
    for via_finish in [false, true] {
        fx.server.log.lock().unwrap().clear();
        let shared = fx.message(Ephemerality::Ephemeral);
        let acked = responder(&shared)
            .defer_reply_as(Ephemerality::Public)
            .await
            .unwrap();
        if via_finish {
            shared.finish(v2("a").into()).await.unwrap();
        } else {
            acked.edit_reply(v2("a")).await.unwrap();
        }
        let _ = acked.done();
        let requests = fx.requests();
        assert_eq!(requests.len(), 2, "via_finish = {via_finish}");
        assert_eq!(requests[0].1, "/api/v10/channels/5/typing");
        assert_eq!(
            (requests[1].0.clone(), requests[1].1.as_str()),
            (Method::POST, "/api/v10/channels/5/messages")
        );
    }
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
    acked.edit_reply(v2("b")).await.unwrap();
    let handle = acked.follow_up(v2("c"), Visibility::Public).await.unwrap();
    handle.edit(v2("d")).await.unwrap();
    handle.delete().await.unwrap();
    acked.delete_reply().await.unwrap();
    assert!(acked.edit_reply(LegacyMessage::new("x")).await.is_err());
    let _ = acked.done();
    unanswer(&shared);
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
    assert!(debug.contains("AutoDefer"), "{debug}");
}

/// Discord's answers, plus a `with_response` reply naming message 77 in
/// channel 9 (`resource.message`).
fn discord_with_message() -> Answer {
    let base = discord();
    Arc::new(move |seen: &Seen| {
        if seen.path.ends_with("/callback") && seen.query.as_deref() == Some("with_response=true") {
            let body = br#"{"interaction":{"id":"100","type":2},"resource":{"type":4,"message":{"id":"77","channel_id":"9"}}}"#;
            (StatusCode::OK, Bytes::from_static(body))
        } else {
            base(seen)
        }
    })
}

/// Moves the clock `by` forward (the token lifetime is measured on it),
/// then lets it run again so real I/O keeps working.
pub(crate) async fn skip(by: Duration) {
    tokio::time::pause();
    tokio::time::advance(by).await;
    tokio::time::resume();
}

/// The methods and paths of the requests after the first `from`.
fn routes(fx: &Fixture, from: usize) -> Vec<(Method, String)> {
    fx.requests()
        .into_iter()
        .skip(from)
        .map(|(m, p, _)| (m, p))
        .collect()
}

/// INT-02 (Story 6.6 Matrix 25-33): after 15 minutes public messages with
/// a known id fall back to the bot token; everything else is
/// `TokenExpired` with no request.
#[tokio::test]
async fn int_02_token_expired_without_http() {
    const T: Duration = Duration::from_mins(15);
    let fx = Fixture::start(discord_with_message()).await;

    // Matrix 25: just before the lifetime ends, the webhook route.
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    skip(Duration::from_secs(899)).await;
    acked.edit_reply(v2("b")).await.unwrap();
    assert_eq!(routes(&fx, 1), [(Method::PATCH, ORIGINAL.to_owned())]);
    let _ = acked.done();

    // Matrix 26, 31 and 32: a public original whose id is known.
    fx.server.log.lock().unwrap().clear();
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    skip(T).await;
    acked.edit_reply(v2("b")).await.unwrap();
    shared.finish(v2("c").into()).await.unwrap();
    acked.delete_reply().await.unwrap();
    let _ = acked.done();
    assert_eq!(
        routes(&fx, 1),
        [
            (Method::PATCH, "/api/v10/channels/9/messages/77".to_owned()),
            (Method::PATCH, "/api/v10/channels/9/messages/77".to_owned()),
            (Method::DELETE, "/api/v10/channels/9/messages/77".to_owned()),
        ]
    );
    assert_eq!(fx.requests()[1].2["components"], v2_components("b"));

    // Matrix 27-29: an ephemeral original, a deferred one never edited,
    // and follow-ups of either visibility.
    fx.server.log.lock().unwrap().clear();
    let ephemeral = fx.interaction("tok", Ephemerality::Ephemeral);
    let ephemeral = responder(&ephemeral).reply(v2("a")).await.unwrap();
    let deferred = fx.interaction("tok", Ephemerality::Public);
    let deferred = responder(&deferred)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap();
    skip(T).await;
    for err in [
        ephemeral.edit_reply(v2("b")).await.unwrap_err(),
        ephemeral.delete_reply().await.unwrap_err(),
        deferred.edit_reply(v2("b")).await.unwrap_err(),
        deferred.delete_reply().await.unwrap_err(),
        deferred
            .follow_up(v2("f"), Visibility::Public)
            .await
            .unwrap_err(),
        deferred
            .follow_up(v2("f"), Visibility::Ephemeral)
            .await
            .unwrap_err(),
    ] {
        assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    }
    assert_eq!(fx.requests().len(), 2);
    let _ = (ephemeral.done(), deferred.done());

    // Matrix 30: follow-up handles.
    fx.server.log.lock().unwrap().clear();
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap();
    let public = acked.follow_up(v2("p"), Visibility::Public).await.unwrap();
    let private = acked
        .follow_up(v2("e"), Visibility::Ephemeral)
        .await
        .unwrap();
    skip(T).await;
    public.edit(v2("p2")).await.unwrap();
    public.delete().await.unwrap();
    for err in [
        private.edit(v2("e2")).await.unwrap_err(),
        private.delete().await.unwrap_err(),
    ] {
        assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    }
    let _ = acked.done();
    assert_eq!(
        routes(&fx, 3),
        [
            (Method::PATCH, "/api/v10/channels/9/messages/55".to_owned()),
            (Method::DELETE, "/api/v10/channels/9/messages/55".to_owned()),
        ]
    );

    // Matrix 33: no channel is known.
    fx.server.log.lock().unwrap().clear();
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
        channel_id: None,
    });
    let shared = Shared::new(transport, Ephemerality::Public, Limits::default());
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    skip(T).await;
    let err = acked.edit_reply(v2("b")).await.unwrap_err();
    assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    let _ = acked.done();
    assert_eq!(fx.requests().len(), 1);
    assert_eq!(
        ResponderError::TokenExpired.to_string(),
        "the interaction token expired (15 minutes)"
    );
}

/// INT-02 (Matrix 28): a 40060 acknowledgement names no message, so the
/// original's id stays unknown.
#[tokio::test]
async fn already_acknowledged_reply_has_no_known_id() {
    let answer: Answer = Arc::new(|_: &Seen| {
        (
            StatusCode::BAD_REQUEST,
            Bytes::from_static(br#"{"code":40060,"message":"x"}"#),
        )
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    skip(Duration::from_mins(15)).await;
    let err = acked.edit_reply(v2("b")).await.unwrap_err();
    assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    let _ = acked.done();
    assert_eq!(fx.requests().len(), 1);
}

/// INT-02: a deferred original learns its id and channel from the
/// `PATCH @original` response, so after 15 minutes it falls back to the
/// bot token.
#[tokio::test]
async fn deferred_original_learns_its_id_from_the_edit() {
    let base = discord();
    let answer: Answer = Arc::new(move |seen: &Seen| {
        if seen.method == Method::PATCH && seen.path.ends_with("/@original") {
            (
                StatusCode::OK,
                Bytes::from_static(br#"{"id":"88","channel_id":"9"}"#),
            )
        } else {
            base(seen)
        }
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap();
    acked.edit_reply(v2("a")).await.unwrap();
    skip(Duration::from_mins(15)).await;
    acked.edit_reply(v2("b")).await.unwrap();
    acked.delete_reply().await.unwrap();
    let _ = acked.done();
    assert_eq!(
        routes(&fx, 1),
        [
            (Method::PATCH, ORIGINAL.to_owned()),
            (Method::PATCH, "/api/v10/channels/9/messages/88".to_owned()),
            (Method::DELETE, "/api/v10/channels/9/messages/88".to_owned()),
        ]
    );
}

/// INT-11 (Matrix 37 and 38): validation precedes the slot; a rejected
/// follow-up still uses one.
#[tokio::test]
async fn followup_cap_slots() {
    let posts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let answer: Answer = {
        let posts = Arc::clone(&posts);
        let base = discord();
        Arc::new(move |seen: &Seen| {
            if seen.method == Method::POST && seen.path == "/api/v10/webhooks/7/tok" {
                let n = posts.fetch_add(1, Ordering::SeqCst) + 1;
                if n == 5 {
                    return (
                        StatusCode::BAD_REQUEST,
                        Bytes::from_static(br#"{"code":50035,"message":"x"}"#),
                    );
                }
            }
            base(seen)
        })
    };
    let fx = Fixture::start(answer).await;
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
        channel_id: None,
    });
    let shared =
        Shared::with_followup_cap(transport, Ephemerality::Public, Limits::default(), Some(5));
    let acked = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap();
    let err = acked
        .follow_up(long_label(81), Visibility::Public)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::Invalid(_)), "{err}");
    for _ in 0..4 {
        let _ = acked.follow_up(v2("f"), Visibility::Public).await.unwrap();
    }
    let err = acked
        .follow_up(v2("f"), Visibility::Public)
        .await
        .unwrap_err();
    assert!(
        matches!(err, ResponderError::Rejected { status: 400, .. }),
        "{err}"
    );
    let err = acked
        .follow_up(v2("f"), Visibility::Ephemeral)
        .await
        .unwrap_err();
    assert!(matches!(err, ResponderError::FollowupCap), "{err}");
    let _ = acked.done();
    assert_eq!(posts.load(Ordering::SeqCst), 5);
    assert_eq!(
        ResponderError::FollowupCap.to_string(),
        "a user-installed app may send at most 5 follow-ups here"
    );
}

/// INT-03 at the gate: a defer after an acknowledgement sends nothing; the
/// auto-defer after a reply does nothing; a reply after the auto-defer
/// edits the original.
#[tokio::test]
async fn gate_orders_initial_responses() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    assert!(shared.auto_defer().await);
    assert_eq!(shared.state(), DEFERRED);
    let _ = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap()
        .done();
    unanswer(&shared);
    let _ = responder(&shared).reply(v2("a")).await.unwrap().done();
    assert!(shared.auto_defer().await);
    let requests = fx.requests();
    assert_eq!(
        requests[0],
        (
            Method::POST,
            CB.to_owned(),
            json!({ "type": 5, "data": { "flags": 64 } })
        )
    );
    assert_eq!(requests[1].0, Method::PATCH);
    assert_eq!(requests[1].1, ORIGINAL);
    assert_eq!(requests.len(), 2);

    // Off the interaction transport the auto-defer never sends.
    let shared = fx.message(Ephemerality::Public);
    assert!(shared.auto_defer().await);
    assert_eq!(shared.state(), PENDING);
    assert_eq!(fx.requests().len(), 2);
}

// ---- Story 6.8: `Shared::notice` in every state ----

const FOLLOWUP_PATH: &str = "/api/v10/webhooks/7/tok?wait=true";

/// The ephemeral type-4 notice of `text`, without `with_response`.
fn notice_callback(text: &str) -> (Method, String, Value) {
    (
        Method::POST,
        CB.to_owned(),
        json!({ "type": 4, "data": { "flags": 32832, "components": v2_components(text), "allowed_mentions": { "parse": [] } } }),
    )
}

/// Pending: one ephemeral type-4 callback, even for a public Command.
#[tokio::test]
async fn notice_while_pending_is_an_ephemeral_callback() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    shared.notice("oops".to_owned()).await.unwrap();
    assert_eq!(shared.state(), REPLIED);
    assert_eq!(fx.requests(), [notice_callback("oops")]);
}

/// Deferred: the original is deleted, then an ephemeral follow-up.
#[tokio::test]
async fn notice_after_defer_deletes_then_follows_up() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap()
        .done();
    shared.notice("oops".to_owned()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::DELETE, ORIGINAL)
    );
    assert_eq!(
        (&requests[2].0, requests[2].1.as_str()),
        (&Method::POST, FOLLOWUP_PATH)
    );
    assert_eq!(requests[2].2["flags"], 32832);
    assert_eq!(requests[2].2["components"], v2_components("oops"));
}

/// Deferred: a failed delete still sends the follow-up.
#[tokio::test]
async fn notice_after_defer_survives_a_failed_delete() {
    let base = discord();
    let answer: Answer = Arc::new(move |seen: &Seen| {
        if seen.method == Method::DELETE {
            (
                StatusCode::NOT_FOUND,
                Bytes::from_static(br#"{"code":10008,"message":"Unknown Message"}"#),
            )
        } else {
            base(seen)
        }
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap()
        .done();
    shared.notice("oops".to_owned()).await.unwrap();
    let routes: Vec<_> = fx.requests().into_iter().map(|(m, p, _)| (m, p)).collect();
    assert_eq!(
        routes,
        [
            (Method::POST, CB.to_owned()),
            (Method::DELETE, ORIGINAL.to_owned()),
            (Method::POST, FOLLOWUP_PATH.to_owned()),
        ]
    );
}

/// Replied: only an ephemeral follow-up.
#[tokio::test]
async fn notice_after_reply_is_a_follow_up() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared).reply(v2("a")).await.unwrap().done();
    shared.notice("oops".to_owned()).await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[1].1, FOLLOWUP_PATH);
    assert_eq!(requests[1].2["flags"], 32832);
}

/// Modal: nothing is sent.
#[tokio::test]
async fn notice_after_modal_sends_nothing() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    shared.state.store(MODAL, Ordering::Release);
    shared.notice("oops".to_owned()).await.unwrap();
    assert!(fx.requests().is_empty());
}

/// Message transport: a public reply while Pending, a public follow-up
/// after a reply or a defer.
#[tokio::test]
async fn notice_over_a_message_transport_is_public() {
    let fx = Fixture::start(discord()).await;
    let pending = fx.message(Ephemerality::Public);
    pending.notice("oops".to_owned()).await.unwrap();
    let replied = fx.message(Ephemerality::Public);
    let _ = responder(&replied).reply(v2("a")).await.unwrap().done();
    replied.notice("oops".to_owned()).await.unwrap();
    let deferred = fx.message(Ephemerality::Public);
    let _ = responder(&deferred)
        .defer_reply_as(Ephemerality::Public)
        .await
        .unwrap()
        .done();
    deferred.notice("oops".to_owned()).await.unwrap();
    let requests = fx.requests();
    let routes: Vec<_> = requests
        .iter()
        .map(|(m, p, _)| (m.clone(), p.as_str()))
        .collect();
    assert_eq!(
        routes,
        [
            (Method::POST, "/api/v10/channels/5/messages"),
            (Method::POST, "/api/v10/channels/5/messages"),
            (Method::POST, "/api/v10/channels/5/messages"),
            (Method::POST, "/api/v10/channels/5/typing"),
            (Method::POST, "/api/v10/channels/5/messages"),
        ]
    );
    // The Pending notice replies to the invoking message.
    assert_eq!(
        requests[0].2["message_reference"],
        json!({ "message_id": "6", "fail_if_not_exists": false })
    );
    for (_, _, body) in requests.iter().filter(|r| r.1.ends_with("/messages")) {
        assert_eq!(body["flags"], 32768);
    }
}

/// Detached: nothing is sent.
#[tokio::test]
async fn notice_detached_sends_nothing() {
    let shared = Shared::new(Transport::Detached, Ephemerality::Public, Limits::default());
    shared.notice("oops".to_owned()).await.unwrap();
}

/// The notice text is validated: Strict rejects text over 4000 chars.
#[tokio::test]
async fn notice_is_validated() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let err = shared.notice("a".repeat(4001)).await.unwrap_err();
    assert!(matches!(err, ResponderError::Invalid(_)), "{err:?}");
    assert!(fx.requests().is_empty());
}

/// Story 6.10 Matrix 22: `reply`/`defer_reply` use the declared response,
/// `reply_as`/`defer_reply_as` override it.
#[tokio::test]
async fn declared_response_and_overrides() {
    let fx = Fixture::start(discord()).await;
    let public = Ephemerality::Public;
    let ephemeral = Ephemerality::Ephemeral;
    let reply = |declared, explicit: Option<Ephemerality>| {
        let shared = fx.interaction("tok", declared);
        async move {
            let r = responder(&shared);
            let acked = match explicit {
                Some(e) => r.reply_as(v2("a"), e).await.unwrap(),
                None => r.reply(v2("a")).await.unwrap(),
            };
            let _ = acked.done();
        }
    };
    reply(public, None).await;
    reply(ephemeral, None).await;
    reply(public, Some(ephemeral)).await;
    reply(ephemeral, Some(public)).await;
    let flags: Vec<Value> = fx
        .requests()
        .iter()
        .map(|(_, path, body)| {
            assert_eq!(path, CB_REPLY);
            body["data"]["flags"].clone()
        })
        .collect();
    assert_eq!(
        flags,
        [json!(32768), json!(32832), json!(32832), json!(32768)]
    );

    fx.server.log.lock().unwrap().clear();
    for declared in [public, ephemeral] {
        let shared = fx.interaction("tok", declared);
        let _ = responder(&shared).defer_reply().await.unwrap().done();
    }
    let shared = fx.interaction("tok", public);
    let _ = responder(&shared)
        .defer_reply_as(ephemeral)
        .await
        .unwrap()
        .done();
    let ephemeral_defer = json!({ "type": 5, "data": { "flags": 64 } });
    assert_eq!(
        fx.requests(),
        [
            (Method::POST, CB.to_owned(), json!({ "type": 5 })),
            (Method::POST, CB.to_owned(), ephemeral_defer.clone()),
            (Method::POST, CB.to_owned(), ephemeral_defer),
        ]
    );
}

/// Story 6.10 Matrix 24 (INT-03, INT-10): after the auto-defer,
/// `reply_as` edits the reply and keeps the auto-defer's ephemerality, and
/// `defer_reply` sends nothing.
#[tokio::test]
async fn reply_as_after_auto_defer_edits() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    let acked = responder(&shared)
        .reply_as(v2("a"), Ephemerality::Ephemeral)
        .await
        .unwrap();
    assert_eq!(acked.shared.original().ephemerality, Ephemerality::Public);
    let _ = acked.done();
    let requests = fx.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].2, json!({ "type": 5 }));
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(requests[1].2["flags"], 32768);

    fx.server.log.lock().unwrap().clear();
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    fx.server.log.lock().unwrap().clear();
    let _ = responder(&shared).defer_reply().await.unwrap().done();
    assert!(fx.requests().is_empty());
}

/// A complete Discord message object `id` in channel `channel`.
fn message_json(id: &str, channel: &str) -> Bytes {
    Bytes::from(
        json!({
            "id": id,
            "channel_id": channel,
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
        })
        .to_string(),
    )
}

/// Discord's answers with a `with_response` reply naming message 77 in
/// channel 9, and message objects for `GET @original` and `GET` channel
/// messages.
fn discord_fetch() -> Answer {
    let base = discord_with_message();
    Arc::new(move |seen: &Seen| {
        if seen.method == Method::GET {
            let id = seen.path.rsplit('/').next().unwrap_or_default();
            let id = if id == "@original" { "77" } else { id };
            (StatusCode::OK, message_json(id, "9"))
        } else {
            base(seen)
        }
    })
}

/// Story 6.10 Matrix 26: `fetch_reply` over a valid token, after a reply
/// and after a defer.
#[tokio::test]
async fn fetch_reply_over_the_token() {
    let fx = Fixture::start(discord_fetch()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    let message = acked.fetch_reply().await.unwrap();
    assert_eq!((message.id.get(), message.channel_id.get()), (77, 9));
    let _ = acked.done();

    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    let acked = responder(&shared).defer_reply().await.unwrap();
    assert_eq!(acked.fetch_reply().await.unwrap().id.get(), 77);
    let _ = acked.done();

    let seen = fx.server.seen();
    let gets: Vec<_> = seen.iter().filter(|s| s.method == Method::GET).collect();
    assert_eq!(gets.len(), 2);
    for get in gets {
        assert_eq!(get.path, ORIGINAL);
        assert!(get.headers.get("authorization").is_none());
    }
}

/// Story 6.10 Matrix 26: an error status is `Rejected`, a body that is
/// not a message is `Transport`, a bad token is `InvalidToken`.
#[tokio::test]
async fn fetch_reply_errors() {
    let answer: Answer = Arc::new(|seen: &Seen| {
        if seen.method == Method::GET && seen.path.contains("/404/") {
            (
                StatusCode::NOT_FOUND,
                Bytes::from_static(br#"{"code":10008,"message":"Unknown Message"}"#),
            )
        } else {
            discord()(seen)
        }
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("404", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    let err = acked.fetch_reply().await.unwrap_err();
    assert!(
        matches!(
            err,
            ResponderError::Rejected {
                status: 404,
                code: Some(10008)
            }
        ),
        "{err}"
    );
    let _ = acked.done();

    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    let err = acked.fetch_reply().await.unwrap_err();
    assert!(matches!(err, ResponderError::Transport), "{err}");
    let _ = acked.done();

    fx.server.log.lock().unwrap().clear();
    let shared = fx.interaction("bad token", Ephemerality::Public);
    let acked: Responder<Command, Acked> = Responder::new(Arc::clone(&shared)).acked();
    let err = acked.fetch_reply().await.unwrap_err();
    assert!(matches!(err, ResponderError::InvalidToken), "{err}");
    let _ = acked.done();
    assert!(fx.requests().is_empty());
}

/// Story 6.10 Matrix 26 (INT-02): after 15 minutes a public reply whose id
/// and channel are known is fetched with the bot token; an ephemeral reply
/// or one whose id is unknown is `TokenExpired` with no request.
#[tokio::test]
async fn fetch_reply_after_the_token_expired() {
    const T: Duration = Duration::from_mins(15);
    let fx = Fixture::start(discord_fetch()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    skip(T).await;
    let message = acked.fetch_reply().await.unwrap();
    assert_eq!(message.id.get(), 77);
    let _ = acked.done();
    let seen = fx.server.seen();
    let get = seen.last().unwrap();
    assert_eq!(
        (&get.method, get.path.as_str()),
        (&Method::GET, "/api/v10/channels/9/messages/77")
    );

    fx.server.log.lock().unwrap().clear();
    let ephemeral = fx.interaction("tok", Ephemerality::Ephemeral);
    let ephemeral = responder(&ephemeral).reply(v2("a")).await.unwrap();
    let unknown = fx.interaction("tok", Ephemerality::Public);
    let unknown = responder(&unknown).defer_reply().await.unwrap();
    skip(T).await;
    for err in [
        ephemeral.fetch_reply().await.unwrap_err(),
        unknown.fetch_reply().await.unwrap_err(),
    ] {
        assert!(matches!(err, ResponderError::TokenExpired), "{err}");
    }
    assert_eq!(fx.requests().len(), 2);
    let _ = (ephemeral.done(), unknown.done());
}

/// Story 6.10 Matrix 26: a fetched public reply's id and channel are kept,
/// so it is reachable with the bot token after the token expired.
#[tokio::test]
async fn fetch_reply_learns_the_id() {
    let fx = Fixture::start(discord_fetch()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).defer_reply().await.unwrap();
    assert_eq!(shared.original().message_id, None);
    let _ = acked.fetch_reply().await.unwrap();
    assert_eq!(shared.original().message_id, Some(Id::new(77)));
    assert_eq!(shared.original().channel_id, Some(Id::new(9)));
    let _ = acked.done();
}

/// Story 6.10 Matrix 26: over a `Dispatcher` message transport the reply
/// is fetched with the bot token; before a reply, or detached, there is
/// none.
#[tokio::test]
async fn fetch_reply_off_the_interaction() {
    let fx = Fixture::start(discord_fetch()).await;
    let shared = fx.message(Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    assert_eq!(acked.fetch_reply().await.unwrap().id.get(), 55);
    let _ = acked.done();
    let get = fx.server.seen().pop().unwrap();
    assert_eq!(
        (&get.method, get.path.as_str()),
        (&Method::GET, "/api/v10/channels/5/messages/55")
    );

    fx.server.log.lock().unwrap().clear();
    let shared = fx.message(Ephemerality::Public);
    let acked = responder(&shared).defer_reply().await.unwrap();
    let err = acked.fetch_reply().await.unwrap_err();
    assert!(matches!(err, ResponderError::NoReply), "{err}");
    let _ = acked.done();
    assert_eq!(fx.requests().len(), 1, "only the typing indicator");

    let detached = Shared::new(Transport::Detached, Ephemerality::Public, Limits::default());
    let acked = responder(&detached).reply(v2("a")).await.unwrap();
    let err = acked.fetch_reply().await.unwrap_err();
    assert!(matches!(err, ResponderError::NoReply), "{err}");
    assert_eq!(err.to_string(), "there is no reply message to fetch");
    let _ = acked.done();
}

/// Story 6.10 Matrix 27: the renamed calls send exactly what the old ones
/// did.
#[tokio::test]
async fn renamed_calls_wire() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).reply(v2("a")).await.unwrap();
    acked.edit_reply(v2("b")).await.unwrap();
    let _ = acked.follow_up(v2("c"), Visibility::Public).await.unwrap();
    acked.delete_reply().await.unwrap();
    let _ = acked.done();
    assert_eq!(
        routes(&fx, 1),
        [
            (Method::PATCH, ORIGINAL.to_owned()),
            (Method::POST, FOLLOWUP.to_owned()),
            (Method::DELETE, ORIGINAL.to_owned()),
        ]
    );
}

mod components;
mod ergonomics;
mod refs;
