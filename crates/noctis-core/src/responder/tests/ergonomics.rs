//! Story 8.1 Matrix 1-22 and 25-45: `ctx.respond`, `ctx.update` and
//! `ctx.reply_text` over a recording upstream.

use noctis_i18n::keys;
use noctis_rules::{Locale, MessageKey, Translate};
use noctis_ui::{Modal, ModalLabel, TextInput, TextInputStyle};

use super::*;
use crate::command::CtxInner;
use crate::locale::{LocaleCell, Locales};
use crate::test_support::{Capture, test_route};
use crate::{
    CommandCtx, ComponentCtx, ComponentResponder, ExpiredCtx, HandlerError, HandlerResult,
    ModalResponder, ModalSubmitCtx, NoticeText, ReplyText,
};

test_route!(Form, "form", noctis_codec::RouteShape::INLINE, 0);

const UPDATE_CB: &str = "/api/v10/interactions/100/tok/callback";
const DELETE_FAILS: &[u8] = br#"{"code":10008,"message":"Unknown Message"}"#;
const GENERIC: &str = "Something went wrong. Please try again later.";

/// Asserts that `$result` failed with the [`ResponderError`] `$pat`.
macro_rules! assert_responder_err {
    ($result:expr, $pat:pat) => {{
        let err: HandlerError = $result.unwrap_err();
        assert!(
            matches!(err.as_error().downcast_ref::<ResponderError>(), Some($pat)),
            "{err:?}"
        );
    }};
}

/// A Command context bound to `shared`.
fn cmd(shared: &Arc<Shared>) -> CommandCtx {
    CommandCtx::from_parts(CtxInner {
        shared: Some(Arc::clone(shared)),
        ..CtxInner::detached_parts(Ephemerality::Public)
    })
}

/// A Command context bound to `shared`, with the resolved user and guild
/// locales.
fn cmd_in(shared: &Arc<Shared>, user: Locale, guild: Locale) -> CommandCtx {
    CommandCtx::from_parts(CtxInner {
        shared: Some(Arc::clone(shared)),
        locales: LocaleCell::new(Locales { user, guild }),
        ..CtxInner::detached_parts(Ephemerality::Public)
    })
}

/// A component context bound to `shared`.
fn comp(shared: &Arc<Shared>) -> ComponentCtx {
    ComponentCtx::detached_bound(shared, Ephemerality::Public)
}

/// A modal submit context bound to `shared`.
fn submit_ctx(shared: &Arc<Shared>) -> ModalSubmitCtx {
    ModalSubmitCtx::detached_bound(shared, Ephemerality::Public)
}

/// The component message 77 in channel 55.
#[expect(
    clippy::unnecessary_wraps,
    reason = "`Shared::declared` takes an `Option`"
)]
fn source(format: MessageFormat, ephemerality: Ephemerality) -> Option<SourceMessage> {
    Some(SourceMessage {
        id: Id::new(77),
        channel_id: Id::new(55),
        format,
        ephemerality,
        nonces: NonceSet::EMPTY,
        files: KnownFiles::EMPTY,
    })
}

fn v2_source() -> Option<SourceMessage> {
    source(MessageFormat::V2, Ephemerality::Public)
}

/// The edit body (`PATCH @original`, type-7 `data`) of `v2(text)`.
fn edit_data(text: &str) -> Value {
    json!({
        "flags": 32768,
        "components": v2_components(text),
        "allowed_mentions": { "parse": [] },
        "attachments": [],
    })
}

/// The type-4 callback of `v2(text)` with `flags`.
fn reply_cb(text: &str, flags: u64) -> (Method, String, Value) {
    (
        Method::POST,
        CB_REPLY.to_owned(),
        json!({ "type": 4, "data": {
            "flags": flags,
            "components": v2_components(text),
            "allowed_mentions": { "parse": [] },
        } }),
    )
}

/// The follow-up of `v2(text)` with `flags`.
fn follow_up(text: &str, flags: u64) -> (Method, String, Value) {
    (
        Method::POST,
        FOLLOWUP.to_owned(),
        json!({
            "flags": flags,
            "components": v2_components(text),
            "allowed_mentions": { "parse": [] },
        }),
    )
}

/// No interaction request carries a bot token or `with_components`.
fn assert_interaction_requests(fx: &Fixture) {
    for seen in fx.server.seen() {
        assert!(seen.headers.get("authorization").is_none(), "{seen:?}");
        assert!(
            !seen
                .query
                .as_deref()
                .unwrap_or("")
                .contains("with_components"),
            "{seen:?}"
        );
    }
}

fn clear(fx: &Fixture) {
    fx.server.log.lock().unwrap().clear();
}

/// Discord's answers, with every `DELETE` failing.
fn discord_failing_delete() -> Answer {
    let base = discord();
    Arc::new(move |seen: &Seen| {
        if seen.method == Method::DELETE {
            (StatusCode::NOT_FOUND, Bytes::from_static(DELETE_FAILS))
        } else {
            base(seen)
        }
    })
}

/// The modal of [`Form`] with one text input.
fn form() -> Modal {
    noctis_ui::__macro::build_modal::<Form>(Form { item: 1 }, "Form").push(ModalLabel::new(
        "Bio",
        TextInput::new("bio", TextInputStyle::Short),
    ))
}

// ---- ctx.respond: rows 1-14 ----

/// Row 1: a public Command while Pending: one type-4 callback.
#[tokio::test]
async fn respond_while_pending_is_one_type_4() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _: Responded = cmd(&shared).respond(v2("hi")).await.unwrap();
    assert_eq!(fx.requests(), [reply_cb("hi", 32768)]);
    assert_eq!(shared.state(), REPLIED);
    assert!(shared.answered());
    assert_interaction_requests(&fx);
}

/// Row 2: an ephemeral Command: flags 64 | 32768.
#[tokio::test]
async fn respond_ephemeral_command() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    let _ = cmd(&shared).respond(v2("hi")).await.unwrap();
    assert_eq!(fx.requests(), [reply_cb("hi", 32832)]);
}

/// Row 3 (INT-12): after a Command's type-5 auto-defer, one `PATCH
/// @original`; the ephemerality stays the defer's.
#[tokio::test]
async fn int_12_respond_after_auto_defer_edits_original() {
    let fx = Fixture::start(discord()).await;
    for (e, defer) in [
        (Ephemerality::Public, json!({ "type": 5 })),
        (
            Ephemerality::Ephemeral,
            json!({ "type": 5, "data": { "flags": 64 } }),
        ),
    ] {
        clear(&fx);
        let shared = fx.interaction("tok", e);
        assert!(shared.auto_defer().await);
        assert!(!shared.answered());
        let _ = cmd(&shared).respond(v2("m")).await.unwrap();
        assert_eq!(
            fx.requests(),
            [
                (Method::POST, CB.to_owned(), defer),
                (Method::PATCH, ORIGINAL.to_owned(), edit_data("m")),
            ]
        );
        assert!(shared.answered());
        assert_eq!(shared.original().ephemerality, e);
    }
}

/// `ctx.respond` after a type-5 `auto_defer()` ≡ `defer_reply` +
/// `edit_reply` on the wire.
#[tokio::test]
async fn respond_after_type_5_equals_defer_reply_and_edit_reply() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    let _ = cmd(&shared).respond(v2("m")).await.unwrap();
    let ctx_wire = fx.requests();
    clear(&fx);
    let shared = fx.interaction("tok", Ephemerality::Public);
    let acked = responder(&shared).defer_reply().await.unwrap();
    acked.edit_reply(v2("m")).await.unwrap();
    let _ = acked.done();
    assert_eq!(ctx_wire, fx.requests());
}

/// Row 4: after the handler's own `defer_reply()`, the edit completes the
/// loading message.
#[tokio::test]
async fn respond_after_own_defer_reply_edits() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared).defer_reply().await.unwrap().done();
    assert!(shared.answered());
    let _ = cmd(&shared).respond(v2("m")).await.unwrap();
    assert_eq!(
        fx.requests()[1],
        (Method::PATCH, ORIGINAL.to_owned(), edit_data("m"))
    );
}

/// Row 5: after a type-6 defer (auto or own), a follow-up with the declared
/// visibility; the cap and the token expiry apply.
#[tokio::test]
async fn respond_after_type_6_follows_up() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    let _ = comp(&shared).respond(v2("r")).await.unwrap();
    assert_eq!(
        fx.requests(),
        [
            (Method::POST, UPDATE_CB.to_owned(), json!({ "type": 6 })),
            follow_up("r", 32768),
        ]
    );
    assert!(!shared.answered());

    clear(&fx);
    let shared = fx.component(Declared::Reply(Ephemerality::Ephemeral), v2_source());
    let _ = component_responder(&shared)
        .defer_update()
        .await
        .unwrap()
        .done();
    let _ = comp(&shared).respond(v2("r")).await.unwrap();
    assert_eq!(fx.requests()[1], follow_up("r", 32832));
}

fn component_responder(shared: &Arc<Shared>) -> ComponentResponder {
    Responder::new(Arc::clone(shared))
}

/// Row 5: the follow-up cap (INT-11) and the expired token (INT-02) send
/// nothing.
#[tokio::test]
async fn respond_follow_up_cap_and_expiry() {
    let fx = Fixture::start(discord()).await;
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(fx.app.shared()),
        application_id: Id::new(7),
        interaction_id: Id::new(100),
        token: "tok".to_owned(),
        received_at: Instant::now(),
        channel_id: None,
        file_limit: None,
    });
    let shared =
        Shared::with_followup_cap(transport, Ephemerality::Public, Limits::default(), Some(0));
    let _ = cmd(&shared).respond(v2("a")).await.unwrap();
    assert_responder_err!(
        cmd(&shared).respond(v2("b")).await,
        ResponderError::FollowupCap
    );
    assert_eq!(fx.requests().len(), 1);

    clear(&fx);
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = cmd(&shared).respond(v2("a")).await.unwrap();
    skip(Duration::from_mins(15)).await;
    assert_responder_err!(
        cmd(&shared).respond(v2("b")).await,
        ResponderError::TokenExpired
    );
    assert_responder_err!(
        cmd(&shared).reply_text("b").await,
        ResponderError::TokenExpired
    );
    assert_eq!(fx.requests().len(), 1);
}

/// Row 6: once answered, every `respond` is another follow-up.
#[tokio::test]
async fn respond_once_answered_follows_up() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    for text in ["a", "b", "c"] {
        let _ = cmd(&shared).respond(v2(text)).await.unwrap();
    }
    assert_eq!(
        fx.requests(),
        [
            reply_cb("a", 32768),
            follow_up("b", 32768),
            follow_up("c", 32768)
        ]
    );

    // DEFERRED with the original already edited.
    clear(&fx);
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    assert!(shared.auto_defer().await);
    let _ = cmd(&shared).respond(v2("a")).await.unwrap();
    let _ = cmd(&shared).respond(v2("b")).await.unwrap();
    assert_eq!(fx.requests()[2], follow_up("b", 32832));
}

/// Row 7: a component's `respond` while Pending is type 4 with the declared
/// visibility, never type 7.
#[tokio::test]
async fn respond_on_a_component_is_type_4() {
    let fx = Fixture::start(discord()).await;
    for (declared, flags) in [
        (Declared::Update, 32768),
        (Declared::Reply(Ephemerality::Ephemeral), 32832),
        (Declared::Reply(Ephemerality::Public), 32768),
    ] {
        clear(&fx);
        let shared = fx.component(declared, v2_source());
        let _ = comp(&shared).respond(v2("n")).await.unwrap();
        assert_eq!(fx.requests(), [reply_cb("n", flags)]);
    }
}

/// Row 8: a slash-opened submit answers with type 4 (public under
/// `update`).
#[tokio::test]
async fn respond_on_a_slash_opened_submit() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.submit(Declared::Update, None);
    let _ = submit_ctx(&shared).respond(v2("s")).await.unwrap();
    assert_eq!(fx.requests(), [reply_cb("s", 32768)]);
}

/// Rows 9, 22 and 30: after a modal or a stop nothing is sent.
#[tokio::test]
async fn ctx_methods_after_a_modal_or_a_stop() {
    let fx = Fixture::start(discord()).await;
    for state in [MODAL, STOPPED] {
        let shared = fx.component(Declared::Update, v2_source());
        shared.state.store(state, Ordering::Release);
        assert_responder_err!(
            comp(&shared).respond(v2("a")).await,
            ResponderError::AlreadyResponded
        );
        assert_responder_err!(
            comp(&shared).update(v2("a")).await,
            ResponderError::AlreadyResponded
        );
        assert_responder_err!(
            comp(&shared).reply_text("a").await,
            ResponderError::AlreadyResponded
        );
    }
    assert!(fx.requests().is_empty());
    assert_eq!(
        ResponderError::AlreadyResponded.to_string(),
        "this handler already sent its initial response; nothing was sent"
    );
}

/// Row 10: over the Dispatcher a message reply, or the post of a deferred
/// reply; an ephemeral declared visibility is unsupported.
#[tokio::test]
async fn respond_over_the_dispatcher() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.message(Ephemerality::Public);
    let _ = cmd(&shared).respond(v2("a")).await.unwrap();
    let deferred = fx.message(Ephemerality::Public);
    let _ = responder(&deferred).defer_reply().await.unwrap().done();
    let _ = cmd(&deferred).respond(v2("b")).await.unwrap();
    let routes: Vec<_> = fx.requests().into_iter().map(|(m, p, _)| (m, p)).collect();
    assert_eq!(
        routes,
        [
            (Method::POST, "/api/v10/channels/5/messages".to_owned()),
            (Method::POST, "/api/v10/channels/5/typing".to_owned()),
            (Method::POST, "/api/v10/channels/5/messages".to_owned()),
        ]
    );
    clear(&fx);
    let ephemeral = fx.message(Ephemerality::Ephemeral);
    assert_responder_err!(
        cmd(&ephemeral).respond(v2("a")).await,
        ResponderError::EphemeralUnsupported
    );
    assert!(fx.requests().is_empty());
}

/// Rows 11-13 and 36: Strict rejects (no request, not answered), Lenient
/// truncates, UTF-16 units are counted.
#[tokio::test]
async fn respond_limits() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    for msg in [texts(41), v2(&"a".repeat(4001)), v2(&"😀".repeat(2001))] {
        assert_responder_err!(cmd(&shared).respond(msg).await, ResponderError::Invalid(_));
    }
    assert_responder_err!(
        cmd(&shared).reply_text("a".repeat(4001)).await,
        ResponderError::Invalid(_)
    );
    assert!(fx.requests().is_empty());
    assert!(!shared.answered());
    assert_eq!(shared.state(), PENDING);
    let _ = cmd(&shared).respond(v2(&"😀".repeat(2000))).await.unwrap();
    let _ = cmd(&shared).reply_text("a".repeat(4000)).await.unwrap();
    assert_eq!(fx.requests().len(), 2);

    let mut limits = Limits::default();
    limits.mode = LimitMode::Lenient;
    let fx = Fixture::start_with(discord(), limits).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = cmd(&shared).respond(v2(&"a".repeat(4001))).await.unwrap();
    let _ = cmd(&shared).reply_text("b".repeat(4001)).await.unwrap();
    let requests = fx.requests();
    let first = requests[0].2["data"]["components"][0]["content"].clone();
    assert_eq!(first, format!("{}…", "a".repeat(3999)));
    let second = requests[1].2["components"][0]["content"].clone();
    assert_eq!(second, format!("{}…", "b".repeat(3999)));
}

/// Row 14: a legacy message is a legacy type-4 body while Pending and a
/// legacy edit of the deferred original.
#[tokio::test]
async fn respond_legacy_message() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = cmd(&shared)
        .respond(LegacyMessage::new("hi"))
        .await
        .unwrap();
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    let _ = cmd(&shared)
        .respond(LegacyMessage::new("ho"))
        .await
        .unwrap();
    let requests = fx.requests();
    assert_eq!(requests[0].2["type"], 4);
    assert_eq!(requests[0].2["data"]["content"], "hi");
    assert_ne!(requests[0].2["data"]["flags"], 32768);
    assert_eq!(
        (&requests[2].0, requests[2].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(requests[2].2["content"], "ho");
}

// ---- ctx.update: rows 15-22 ----

/// Row 15: a type-7 update while Pending, V2 flag, no `EPHEMERAL`.
#[tokio::test]
async fn update_while_pending_is_type_7() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let _ = comp(&shared).update(v2("u")).await.unwrap();
    assert_eq!(
        fx.requests(),
        [(
            Method::POST,
            UPDATE_CB.to_owned(),
            json!({ "type": 7, "data": edit_data("u") })
        )]
    );
    assert!(shared.answered());
    assert_interaction_requests(&fx);
}

/// Row 16 (CV2-02): a legacy source gets its legacy fields cleared.
#[tokio::test]
async fn update_of_a_legacy_source_clears_it() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(
        Declared::Update,
        source(MessageFormat::Legacy, Ephemerality::Public),
    );
    let _ = comp(&shared).update(v2("u")).await.unwrap();
    let data = &fx.requests()[0].2["data"];
    assert_eq!(data["content"], Value::Null);
    assert!(data.get("content").is_some());
    assert_eq!(data["embeds"], json!([]));
    assert_eq!(data["flags"], 32768);
}

/// Row 17: after the type-6 auto-defer one `PATCH @original`; ≡
/// `defer_update` + `edit_reply`.
#[tokio::test]
async fn update_after_type_6_equals_defer_update_and_edit_reply() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    let _ = comp(&shared).update(v2("u")).await.unwrap();
    assert!(shared.answered());
    let ctx_wire = fx.requests();
    assert_eq!(
        ctx_wire,
        [
            (Method::POST, UPDATE_CB.to_owned(), json!({ "type": 6 })),
            (Method::PATCH, ORIGINAL.to_owned(), edit_data("u")),
        ]
    );
    clear(&fx);
    let shared = fx.component(Declared::Update, v2_source());
    let acked = component_responder(&shared).defer_update().await.unwrap();
    acked.edit_reply(v2("u")).await.unwrap();
    let _ = acked.done();
    assert_eq!(ctx_wire, fx.requests());
}

/// Row 18: under `ephemeral | public` a lost race to the type-5 auto-defer
/// edits the deferred reply.
#[tokio::test]
async fn update_after_type_5_edits_the_deferred_reply() {
    let fx = Fixture::start(discord()).await;
    for e in [Ephemerality::Ephemeral, Ephemerality::Public] {
        clear(&fx);
        let shared = fx.component(Declared::Reply(e), v2_source());
        assert!(shared.auto_defer().await);
        let _ = comp(&shared).update(v2("u")).await.unwrap();
        assert_eq!(
            fx.requests()[1],
            (Method::PATCH, ORIGINAL.to_owned(), edit_data("u"))
        );
    }
}

/// Rows 19-20 (INT-04): a slash-opened submit has nothing to update, with
/// or without a prior auto-defer; no request, nothing answered.
#[tokio::test]
async fn int_04_update_unavailable_without_a_source_ctx() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.submit(Declared::Update, None);
    assert_responder_err!(
        submit_ctx(&shared).update(v2("u")).await,
        ResponderError::UpdateUnavailable
    );
    assert!(fx.requests().is_empty());
    assert_eq!(shared.state(), PENDING);
    assert!(!shared.answered());

    assert!(shared.auto_defer().await);
    assert_eq!(shared.state(), DEFERRED);
    assert_responder_err!(
        submit_ctx(&shared).update(v2("u")).await,
        ResponderError::UpdateUnavailable
    );
    assert_eq!(
        fx.requests(),
        [(Method::POST, UPDATE_CB.to_owned(), json!({ "type": 5 }))]
    );
}

/// Rows 21 and 39 (INT-03): once answered, by a ctx method or a typestate
/// Pending method, `ctx.update` sends nothing.
#[tokio::test]
async fn int_03_typestate_then_ctx_update_already_responded() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let _ = component_responder(&shared)
        .reply(v2("r"))
        .await
        .unwrap()
        .done();
    assert_responder_err!(
        comp(&shared).update(v2("u")).await,
        ResponderError::AlreadyResponded
    );
    let shared = fx.component(Declared::Update, v2_source());
    let _ = component_responder(&shared)
        .defer_update()
        .await
        .unwrap()
        .done();
    assert_responder_err!(
        comp(&shared).update(v2("u")).await,
        ResponderError::AlreadyResponded
    );
    for first in 0..3 {
        let shared = fx.component(Declared::Update, v2_source());
        let ctx = comp(&shared);
        match first {
            0 => drop(ctx.respond(v2("a")).await.unwrap()),
            1 => drop(ctx.update(v2("a")).await.unwrap()),
            _ => drop(ctx.reply_text("a").await.unwrap()),
        }
        assert_responder_err!(ctx.update(v2("u")).await, ResponderError::AlreadyResponded);
    }
    assert_eq!(fx.requests().len(), 5);
}

/// Row 38 (INT-03): after a ctx method took the initial response, every
/// typestate Pending method is `AlreadyResponded` without a request.
#[tokio::test]
async fn int_03_ctx_then_typestate_already_responded() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    let _ = comp(&shared).respond(v2("a")).await.unwrap();
    let r = || component_responder(&shared);
    let errors = [
        r().reply(v2("b")).await.map(drop).unwrap_err(),
        r().reply_as(v2("b"), Ephemerality::Ephemeral)
            .await
            .map(drop)
            .unwrap_err(),
        r().reply_with_response(v2("b"))
            .await
            .map(drop)
            .unwrap_err(),
        r().defer_reply().await.map(drop).unwrap_err(),
        r().defer_reply_as(Ephemerality::Ephemeral)
            .await
            .map(drop)
            .unwrap_err(),
        r().defer_reply_with_response().await.map(drop).unwrap_err(),
        r().update(v2("b")).await.map(drop).unwrap_err(),
        r().update_with_response(v2("b"))
            .await
            .map(drop)
            .unwrap_err(),
        r().defer_update().await.map(drop).unwrap_err(),
        r().defer_update_with_response()
            .await
            .map(drop)
            .unwrap_err(),
        Responder::<Component, Pending, Manual>::new(Arc::clone(&shared))
            .show_modal(form())
            .await
            .map(drop)
            .unwrap_err(),
    ];
    for err in errors {
        assert!(matches!(err, ResponderError::AlreadyResponded), "{err}");
    }
    // A Command answered through `reply_text`, a submit through `update`.
    let command = fx.interaction("tok", Ephemerality::Public);
    let _ = cmd(&command).reply_text("t").await.unwrap();
    let err = responder(&command)
        .reply(v2("b"))
        .await
        .map(drop)
        .unwrap_err();
    assert!(matches!(err, ResponderError::AlreadyResponded), "{err}");
    let submit = fx.submit(Declared::Update, v2_source());
    let _ = submit_ctx(&submit).update(v2("u")).await.unwrap();
    let modal: ModalResponder = Responder::new(Arc::clone(&submit));
    let err = modal.reply(v2("b")).await.map(drop).unwrap_err();
    assert!(matches!(err, ResponderError::AlreadyResponded), "{err}");
    assert_eq!(fx.requests().len(), 3);
}

/// Row 40: a typestate reply, then `respond` / `reply_text` follow up.
#[tokio::test]
async fn typestate_then_respond_follows_up() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = responder(&shared).reply(v2("a")).await.unwrap().done();
    let _ = cmd(&shared).respond(v2("b")).await.unwrap();
    let _ = cmd(&shared).reply_text("c").await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests[1], follow_up("b", 32768));
    assert_eq!(requests[2], follow_up("c", 32832));
}

/// Row 41: the auto-defer alone does not mark the handler answered, so the
/// typestate keeps its late behaviour.
#[tokio::test]
async fn auto_defer_alone_keeps_late_typestate() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    assert!(!shared.answered());
    let _ = responder(&shared).reply(v2("a")).await.unwrap().done();
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    let _ = component_responder(&shared)
        .update(v2("b"))
        .await
        .unwrap()
        .done();
    let routes: Vec<_> = fx.requests().into_iter().map(|(m, p, _)| (m, p)).collect();
    assert_eq!(
        routes,
        [
            (Method::POST, CB.to_owned()),
            (Method::PATCH, ORIGINAL.to_owned()),
            (Method::POST, CB.to_owned()),
            (Method::PATCH, ORIGINAL.to_owned()),
        ]
    );
}

/// Row 42: the auto-defer racing `ctx.respond` sends exactly one callback.
#[tokio::test]
async fn auto_defer_races_respond() {
    let fx = Fixture::start(discord()).await;
    for _ in 0..8 {
        clear(&fx);
        let shared = fx.interaction("tok", Ephemerality::Public);
        let ctx = cmd(&shared);
        let (deferred, responded) = tokio::join!(shared.auto_defer(), ctx.respond(v2("m")));
        assert!(deferred);
        let _ = responded.unwrap();
        let requests = fx.requests();
        let callbacks = requests
            .iter()
            .filter(|(_, p, _)| p.contains("/callback"))
            .count();
        assert_eq!(callbacks, 1, "{requests:?}");
        match requests.len() {
            1 => assert_eq!(requests[0].2["type"], 4),
            _ => assert_eq!(requests[1].0, Method::PATCH),
        }
    }
}

// ---- ctx.reply_text: rows 25-37 ----

/// Rows 25-26: one Text Display, ephemeral by default, `.public()` public.
#[tokio::test]
async fn reply_text_while_pending() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = cmd(&shared).reply_text("Saved.").await.unwrap();
    assert_eq!(fx.requests(), [reply_cb("Saved.", 32832)]);
    assert_eq!(
        fx.requests()[0].2["data"]["components"],
        json!([{ "type": 10, "id": 1, "content": "Saved." }])
    );
    assert!(shared.answered());
    clear(&fx);
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    let _ = cmd(&shared).reply_text("Saved.").public().await.unwrap();
    assert_eq!(fx.requests(), [reply_cb("Saved.", 32768)]);
}

/// Row 27 (INT-12): after a type-5 defer of the same visibility, the text
/// edits the loading message.
#[tokio::test]
async fn int_12_reply_text_after_defer_edits_original() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Ephemeral);
    assert!(shared.auto_defer().await);
    let _ = cmd(&shared).reply_text("t").await.unwrap();
    assert_eq!(
        fx.requests()[1],
        (Method::PATCH, ORIGINAL.to_owned(), edit_data("t"))
    );
    assert_eq!(fx.requests().len(), 2);
    assert!(shared.answered());
}

/// Row 28: after a type-5 defer of the other visibility, the loading
/// message is deleted, then the text follows up; a failed delete is one
/// `warn` and the follow-up is still sent.
#[tokio::test]
async fn reply_text_after_defer_of_other_visibility() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    let _ = cmd(&shared).reply_text("t").await.unwrap();
    let _ = cmd(&shared).respond(v2("after")).await.unwrap();
    let requests = fx.requests();
    assert_eq!(
        requests[1..],
        [
            (Method::DELETE, ORIGINAL.to_owned(), Value::Null),
            follow_up("t", 32832),
            follow_up("after", 32768),
        ]
    );

    let fx = Fixture::start(discord_failing_delete()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert!(shared.auto_defer().await);
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let _ = cmd(&shared).reply_text("t").await.unwrap();
    let lines = capture.lines_with(&["WARN", "noctis::dispatch", "reply_text"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert_eq!(fx.requests()[2], follow_up("t", 32832));
}

/// Row 29: after a type-6 defer or once answered, a follow-up.
#[tokio::test]
async fn reply_text_after_type_6_or_answered() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.component(Declared::Update, v2_source());
    assert!(shared.auto_defer().await);
    let _ = comp(&shared).reply_text("a").await.unwrap();
    let _ = comp(&shared).reply_text("b").public().await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests[1], follow_up("a", 32832));
    assert_eq!(requests[2], follow_up("b", 32768));
}

/// Row 31: over the Dispatcher the text is always public.
#[tokio::test]
async fn reply_text_over_the_dispatcher_is_public() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.message(Ephemerality::Public);
    let _ = cmd(&shared).reply_text("a").await.unwrap();
    let _ = cmd(&shared).reply_text("b").await.unwrap();
    let requests = fx.requests();
    assert_eq!(requests.len(), 2);
    for (method, path, body) in requests {
        assert_eq!(
            (method, path.as_str()),
            (Method::POST, "/api/v10/channels/5/messages")
        );
        assert_eq!(body["flags"], 32768);
    }
}

/// A catalog with `shop.saved` in every locale, as the locale's code.
struct Catalog;

impl Translate for Catalog {
    fn translate(
        &self,
        locale: Locale,
        key: MessageKey,
        _: &[(&str, &dyn fmt::Display)],
    ) -> Option<String> {
        match key.as_str() {
            "shop.saved" => Some(format!("{locale:?}")),
            "noctis.error.generic" => Some("generic".to_owned()),
            _ => None,
        }
    }
}

/// Row 32: a key resolves in the User Locale (ephemeral) or the Guild
/// Locale (public) of the run's resolved pair.
#[test]
fn reply_text_key_locale() {
    let key = || NoticeText::from(MessageKey::new("shop.saved"));
    let text = |locales, visibility| {
        super::super::ergonomics::text_in(&Catalog, locales, key(), visibility)
    };
    let (eph, public) = (Ephemerality::Ephemeral, Ephemerality::Public);
    let tr_de = Locales {
        user: Locale::Tr,
        guild: Locale::De,
    };
    assert_eq!(text(tr_de, eph), "Tr");
    assert_eq!(text(tr_de, public), "De");
    assert_eq!(text(Locales::both(Locale::EnUs), eph), "EnUs");
    assert_eq!(
        super::super::ergonomics::text_in(&Catalog, tr_de, "lit".into(), eph),
        "lit"
    );
}

/// Rows 32-33 on the wire: a key in the App's catalog; a missing key is
/// the generic text with one `warn` on `noctis::i18n`.
#[tokio::test]
async fn reply_text_keys_on_the_wire() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let _ = cmd_in(&shared, Locale::Tr, Locale::De)
        .reply_text(keys::GUARD_OWNER)
        .await
        .unwrap();
    let shared = fx.interaction("tok", Ephemerality::Public);
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let _ = cmd(&shared)
        .reply_text(MessageKey::new("shop.missing"))
        .await
        .unwrap();
    let lines = capture.lines_with(&["WARN", "noctis::i18n", "shop.missing"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    let requests = fx.requests();
    assert_eq!(
        requests[0].2["data"]["components"][0]["content"],
        "Only the person who started this can use it."
    );
    assert_eq!(requests[1].2["data"]["components"][0]["content"], GENERIC);
}

/// Row 34: `String`, `&'static str` and `MessageKey` all convert.
#[tokio::test]
async fn reply_text_inputs() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let ctx = cmd(&shared);
    let _ = ctx.reply_text(String::from("a")).await.unwrap();
    let _ = ctx.reply_text("b").await.unwrap();
    let _ = ctx.reply_text(keys::ERROR_GENERIC).await.unwrap();
    assert_eq!(fx.requests().len(), 3);
    let pending: ReplyText = ctx.reply_text("secret");
    let debug = format!("{pending:?}");
    assert!(!debug.contains("secret"), "{debug}");
    assert!(debug.contains("Ephemeral"), "{debug}");
    drop(pending);
}

/// Row 35: empty or blank text is sent as is; Discord's 4xx is surfaced.
#[tokio::test]
async fn reply_text_empty_and_rejected() {
    let fx = Fixture::start(discord()).await;
    for text in ["", "  "] {
        let shared = fx.interaction("tok", Ephemerality::Public);
        let _ = cmd(&shared).reply_text(text).await.unwrap();
    }
    let requests = fx.requests();
    assert_eq!(requests[0].2["data"]["components"][0]["content"], "");
    assert_eq!(requests[1].2["data"]["components"][0]["content"], "  ");

    let answer: Answer = Arc::new(|_: &Seen| {
        (
            StatusCode::BAD_REQUEST,
            Bytes::from_static(br#"{"code":50035,"message":"Invalid Form Body"}"#),
        )
    });
    let fx = Fixture::start(answer).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    assert_responder_err!(
        cmd(&shared).reply_text("").await,
        ResponderError::Rejected { status: 400, .. }
    );
}

/// Row 37 (MSG-05): mentions stay in the text and ping nobody.
#[tokio::test]
async fn msg_05_reply_text_pings_nobody() {
    let fx = Fixture::start(discord()).await;
    let shared = fx.interaction("tok", Ephemerality::Public);
    let text = "<@1> @everyone <@&2>";
    let _ = cmd(&shared).reply_text(text).await.unwrap();
    let data = &fx.requests()[0].2["data"];
    assert_eq!(data["components"][0]["content"], text);
    assert_eq!(data["allowed_mentions"], json!({ "parse": [] }));
}

// ---- seams: rows 43-45 ----

async fn ctx_respond(ctx: CommandCtx, _: CommandResponder) -> HandlerResult<Responded> {
    ctx.respond(v2("hi")).await
}

async fn ctx_invalid(ctx: CommandCtx, _: CommandResponder) -> HandlerResult<Responded> {
    ctx.respond(texts(41)).await
}

/// Row 43: the detached seams send nothing but still validate; `ctx.update`
/// succeeds on the placeholder source.
#[tokio::test]
async fn detached_seams() {
    let ok = crate::CommandHandler::__new_plain("m::ok", &["ok"], "d", ctx_respond);
    crate::__testing::call_command(&ok, "ok").await.unwrap();
    let bad = crate::CommandHandler::__new_plain("m::bad", &["bad"], "d", ctx_invalid);
    assert_responder_err!(
        crate::__testing::call_command(&bad, "bad").await,
        ResponderError::Invalid(_)
    );
    let shared = Shared::declared(
        Transport::Detached,
        Declared::Update,
        v2_source(),
        Limits::default(),
        None,
    );
    let _ = comp(&shared).update(v2("u")).await.unwrap();
    let _ = comp(&shared).reply_text("t").await.unwrap();

    // The component and modal seams bind their ctx to the placeholder
    // source: `update` and `respond` succeed, an over-limit body is `Invalid`.
    let component = |name: &'static str, n: usize| {
        crate::ComponentHandler::__new::<Form, _, _>(
            name,
            "form",
            crate::ComponentKind::Button,
            move |_: Form, ctx: ComponentCtx| async move {
                if n > 1 {
                    return ctx.update(texts(n)).await;
                }
                let _ = ctx.update(v2("u")).await?;
                ctx.respond(v2("r")).await
            },
        )
    };
    let _ = crate::__testing::call_component(&component("m::c", 1), Form { item: 1 })
        .await
        .unwrap();
    assert_responder_err!(
        crate::__testing::call_component(&component("m::c41", 41), Form { item: 1 }).await,
        ResponderError::Invalid(_)
    );
    let modal = |name: &'static str, n: usize| {
        crate::ModalHandler::__new::<Form, _, _>(
            name,
            "form",
            move |_: Form, ctx: ModalSubmitCtx| async move {
                if n > 1 {
                    return ctx.update(texts(n)).await;
                }
                let _ = ctx.update(v2("u")).await?;
                ctx.respond(v2("r")).await
            },
        )
    };
    let _ = crate::__testing::call_modal(&modal("m::m", 1), Form { item: 1 })
        .await
        .unwrap();
    assert_responder_err!(
        crate::__testing::call_modal(&modal("m::m41", 41), Form { item: 1 }).await,
        ResponderError::Invalid(_)
    );
}

/// Row 44: an unbound context answers `NoReply` without a request.
#[tokio::test]
async fn unbound_context_has_no_reply() {
    assert_responder_err!(
        CommandCtx::new("x").respond(v2("a")).await,
        ResponderError::NoReply
    );
    assert_responder_err!(
        CommandCtx::new("x").reply_text("a").await,
        ResponderError::NoReply
    );
    assert_responder_err!(
        ComponentCtx::detached().update(v2("a")).await,
        ResponderError::NoReply
    );
    assert_responder_err!(
        ExpiredCtx::detached().update(v2("a")).await,
        ResponderError::NoReply
    );
    assert_responder_err!(
        ModalSubmitCtx::detached().respond(v2("a")).await,
        ResponderError::NoReply
    );
}

/// Row 45: `response_locale` of the component contexts follows the
/// declared response and, under `update`, the source message.
#[tokio::test]
async fn component_response_locale() {
    let fx = Fixture::start(discord()).await;
    let ephemeral_source = source(MessageFormat::V2, Ephemerality::Ephemeral);
    for (declared, src, expected) in [
        (
            Declared::Reply(Ephemerality::Ephemeral),
            v2_source(),
            Locale::Tr,
        ),
        (
            Declared::Reply(Ephemerality::Public),
            ephemeral_source.clone(),
            Locale::De,
        ),
        (Declared::Update, ephemeral_source, Locale::Tr),
        (Declared::Update, v2_source(), Locale::De),
        (Declared::Update, None, Locale::De),
    ] {
        let shared = fx.component(declared, src);
        let parts = Arc::new(CtxInner {
            locales: LocaleCell::new(Locales {
                user: Locale::Tr,
                guild: Locale::De,
            }),
            ephemerality: shared.response_ephemerality(),
            shared: Some(Arc::clone(&shared)),
            ..CtxInner::detached_parts(Ephemerality::Public)
        });
        assert_eq!(
            ComponentCtx::from_inner(Arc::clone(&parts)).response_locale(),
            expected
        );
        assert_eq!(
            ModalSubmitCtx::from_inner(Arc::clone(&parts)).response_locale(),
            expected
        );
        assert_eq!(ExpiredCtx::from_inner(parts).response_locale(), expected);
    }
    assert_eq!(ComponentCtx::detached().response_locale(), Locale::EnUs);
}

const fn assert_send<T: Send>() {}
const _: () = assert_send::<ReplyText>();
const _: () = assert_send::<NoticeText>();
