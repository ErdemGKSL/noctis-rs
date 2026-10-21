//! Story 7.3 Matrix 1-31, 44 and 47: stale and Ref ids reach the Expired
//! Handler chain (Route `#[on_stale]`, Route `#[on_expired]`, App
//! `expired_handler`, default `noctis.expired` notice) over a recording
//! upstream, through hand-built descriptors.

use std::future::Future;

use noctis_codec::{PayloadDecode, RouteShape};
use noctis_i18n::Builtin;
use noctis_ui::{Modal, ModalLabel, TextInput, TextInputStyle};

use super::components::{assert_generic_notice, click, submit};
use super::*;
use crate::component::{__ExpiredHook, __StaleHook};
use crate::kind::__Response;
use crate::pipeline::component::run_component;
use crate::test_support::test_route;
use crate::{
    AutoDefer, ComponentCtx, ComponentHandler, ComponentKind, ComponentResponder, Expired,
    ExpiredCtx, Manual, ModalHandler, ModalResponder, ModalSubmitCtx,
};

test_route!(Buy, "shop.buy", RouteShape::INLINE, 0);
test_route!(Fresh, "fresh", RouteShape::INLINE, 0);
test_route!(Slow, "slow", RouteShape::INLINE, 0);
test_route!(Late, "late", RouteShape::INLINE, 0);
test_route!(Eph, "eph", RouteShape::INLINE, 0);
test_route!(Man, "man", RouteShape::INLINE, 0);
test_route!(ByRef, "r", RouteShape::BY_REF, 0);
test_route!(ByRefHooks, "rh", RouteShape::BY_REF, 0);
test_route!(V61, "v61", RouteShape::INLINE, 61);
test_route!(Sig, "sig", RouteShape::INLINE.signed(), 0);
test_route!(Long, "abcdefghijklmnopqrstuvwx", RouteShape::INLINE, 0);
test_route!(Resp, "resp", RouteShape::INLINE, 0);
test_route!(UserErr, "user.err", RouteShape::INLINE, 0);
test_route!(Fails, "fails", RouteShape::INLINE, 0);
test_route!(Panics, "panics", RouteShape::INLINE, 0);
test_route!(SyncPanics, "sync.panics", RouteShape::INLINE, 0);
test_route!(Waits, "waits", RouteShape::INLINE, 0);
test_route!(ZzBuy, "zz.buy", RouteShape::INLINE, 0);
test_route!(ZzEph, "zz.eph", RouteShape::INLINE, 0);
test_route!(ZzMan, "zz.man", RouteShape::INLINE, 0);
test_route!(Bio, "bio", RouteShape::INLINE, 0);
test_route!(BioHooks, "bioh", RouteShape::INLINE, 0);
test_route!(BioLate, "biol", RouteShape::INLINE, 0);

/// The ack deadline of these tests.
const D: Duration = Duration::from_millis(50);

/// The default notice text (`Builtin` ships en-US only).
const EXPIRED_TEXT: &str = "This interaction has expired. Run the command again.";

fn sleep_ms(ms: u64) -> tokio::time::Sleep {
    tokio::time::sleep(Duration::from_millis(ms))
}

/// A component handler of `S` that logs `handler:<item>`, declared
/// `response`, of defer mode `M`.
fn handler<S, M>(log: &Log, name: &'static str, response: __Response) -> ComponentHandler
where
    S: noctis_codec::Route<Payload = S>
        + noctis_codec::DeclaredRuntime
        + crate::__Guards<ComponentCtx>
        + for<'de> PayloadDecode<'de>
        + Send
        + 'static,
    M: crate::DeferMode,
{
    let log = log.clone();
    ComponentHandler::__new_with_responder::<S, M, _, _, _>(
        name,
        <S as noctis_codec::Route>::ID.as_str(),
        ComponentKind::Button,
        response,
        move |_: S, _: ComponentCtx, _: crate::Responder<crate::Component, crate::Pending, M>| {
            log.push(format!("handler:{name}"));
            async { Ok(Responded::new()) }
        },
    )
}

/// An auto-deferred `#[on_stale]` hook that logs `on_stale:<text>`,
/// sleeps `ms` and returns `text`.
fn stale_hook(log: &Log, text: &'static str, ms: u64) -> __StaleHook<ComponentCtx> {
    let log = log.clone();
    __StaleHook::__new::<AutoDefer, _, _, _>(move |_: ComponentCtx, _: ComponentResponder| {
        log.push(format!("on_stale:{text}"));
        async move {
            sleep_ms(ms).await;
            Ok(v2(text))
        }
    })
}

/// An auto-deferred `#[on_expired]` hook that logs and returns
/// `expired-view {expired:?}`.
fn expired_hook(log: &Log) -> __ExpiredHook<ComponentCtx> {
    let log = log.clone();
    __ExpiredHook::__new::<AutoDefer, _, _, _>(
        move |_: ComponentCtx, expired: Expired, _: ComponentResponder| {
            log.push(format!("on_expired:{expired:?}"));
            async move { Ok(v2(&format!("expired-view {expired:?}"))) }
        },
    )
}

/// A failing `#[on_stale]` hook.
fn failing<F, Fut>(f: F) -> __StaleHook<ComponentCtx>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
{
    __StaleHook::__new::<AutoDefer, _, _, _>(move |_: ComponentCtx, _: ComponentResponder| f())
}

/// The modal `Man`'s hook opens.
fn form() -> Modal {
    noctis_ui::__macro::build_modal::<Bio>(Bio { item: 1 }, "Bio").push(ModalLabel::new(
        "Bio",
        TextInput::new("bio", TextInputStyle::Short),
    ))
}

/// Every Story 7.3 Route.
#[expect(clippy::too_many_lines, reason = "one Route per matrix fixture")]
fn module(log: &Log, waiting: &Arc<Notify>) -> Module {
    let started = Arc::clone(waiting);
    let bio_log = log.clone();
    let late_log = log.clone();
    let upd = __Response::Update;
    Module::new("x")
        .__add(handler::<Buy, AutoDefer>(log, "x::Buy", upd))
        .__add(
            handler::<Fresh, AutoDefer>(log, "x::Fresh", upd)
                .__with_on_stale(stale_hook(log, "stale-view", 0))
                .__with_on_expired(expired_hook(log)),
        )
        .__add(
            handler::<Slow, AutoDefer>(log, "x::Slow", upd).__with_on_stale(stale_hook(
                log,
                "slow-view",
                150,
            )),
        )
        .__add(handler::<Late, AutoDefer>(log, "x::Late", upd).__with_on_expired(expired_hook(log)))
        .__add(
            handler::<Eph, AutoDefer>(log, "x::Eph", __Response::Ephemeral)
                .__with_on_stale(stale_hook(log, "stale-view", 0)),
        )
        .__add(
            handler::<Man, Manual>(log, "x::Man", upd).__with_on_expired(__ExpiredHook::__new::<
                Manual,
                _,
                _,
                _,
            >(
                |_: ComponentCtx, _: Expired, r: ComponentResponder<Manual>| async move {
                    Ok(r.show_modal(form()).await?.done())
                },
            )),
        )
        .__add(handler::<ByRef, AutoDefer>(log, "x::ByRef", upd))
        .__add(
            handler::<ByRefHooks, AutoDefer>(log, "x::ByRefHooks", upd)
                .__with_on_stale(stale_hook(log, "stale-view", 0))
                .__with_on_expired(expired_hook(log)),
        )
        .__add(handler::<V61, AutoDefer>(log, "x::V61", upd))
        .__add(handler::<Sig, AutoDefer>(log, "x::Sig", upd))
        .__add(handler::<Long, AutoDefer>(log, "x::Long", upd))
        .__add(
            handler::<Resp, AutoDefer>(log, "x::Resp", upd).__with_on_expired(
                __ExpiredHook::__new::<AutoDefer, _, _, _>(
                    |_: ComponentCtx, _: Expired, r: ComponentResponder| async move {
                        Ok(r.reply_as(v2("resp"), Ephemerality::Ephemeral)
                            .await?
                            .done())
                    },
                ),
            ),
        )
        .__add(
            handler::<UserErr, AutoDefer>(log, "x::UserErr", upd).__with_on_stale(failing(
                || async { Err(HandlerError::user(MessageKey::new("shop.sold_out"))) },
            )),
        )
        .__add(
            handler::<Fails, AutoDefer>(log, "x::Fails", upd).__with_on_stale(failing(|| async {
                Err(HandlerError::new(std::io::Error::other("boom")))
            })),
        )
        .__add(
            handler::<Panics, AutoDefer>(log, "x::Panics", upd)
                .__with_on_stale(failing(|| async { panic!("hook") })),
        )
        .__add(
            handler::<SyncPanics, AutoDefer>(log, "x::SyncPanics", upd).__with_on_stale(
                __StaleHook::__new::<AutoDefer, _, _, _>(
                    |_: ComponentCtx,
                     _: ComponentResponder|
                     -> std::future::Ready<HandlerResult<V2Message>> {
                        panic!("sync hook")
                    },
                ),
            ),
        )
        .__add(
            handler::<Waits, AutoDefer>(log, "x::Waits", upd).__with_on_stale(failing(move || {
                let started = Arc::clone(&started);
                async move {
                    started.notify_one();
                    std::future::pending::<()>().await;
                    Ok(Responded::new())
                }
            })),
        )
        .__add(handler::<ZzBuy, AutoDefer>(log, "x::ZzBuy", upd))
        .__add(handler::<ZzEph, AutoDefer>(
            log,
            "x::ZzEph",
            __Response::Ephemeral,
        ))
        .__add(handler::<ZzMan, Manual>(log, "x::ZzMan", upd))
        .__add(ModalHandler::__new::<Bio, _, _>(
            "x::Bio",
            "bio",
            |_: Bio, _: ModalSubmitCtx| async { Ok(Responded::new()) },
        ))
        .__add(
            ModalHandler::__new::<BioHooks, _, _>(
                "x::BioHooks",
                "bioh",
                |_: BioHooks, _: ModalSubmitCtx| async { Ok(Responded::new()) },
            )
            .__with_on_stale(__StaleHook::__new::<AutoDefer, _, _, _>(
                move |ctx: ModalSubmitCtx, _: ModalResponder| {
                    bio_log.push(format!("modal_on_stale:{}", ctx.module()));
                    async { Ok(v2("modal-view")) }
                },
            )),
        )
        .__add(
            ModalHandler::__new::<BioLate, _, _>(
                "x::BioLate",
                "biol",
                |_: BioLate, _: ModalSubmitCtx| async { Ok(Responded::new()) },
            )
            .__with_on_expired(__ExpiredHook::__new::<AutoDefer, _, _, _>(
                move |_: ModalSubmitCtx, expired: Expired, _: ModalResponder| {
                    late_log.push(format!("modal_on_expired:{expired:?}"));
                    async move { Ok(v2(&format!("modal-expired {expired:?}"))) }
                },
            )),
        )
}

/// The App-level Expired Handler of these tests: it sleeps past `D` on a
/// `zz.*` Route, panics on `panics`, and returns `app <route> <why>`.
async fn app_handler(ctx: ExpiredCtx, expired: Expired) -> HandlerResult<V2Message> {
    if ctx.route().starts_with("zz.") {
        sleep_ms(150).await;
    }
    Ok(v2(&format!("app {} {}", ctx.route(), expired.as_str())))
}

/// The body of the panicking App handler.
fn explode() -> V2Message {
    panic!("app")
}

/// A running App with [`module`], `D` as the ack deadline, a payload key
/// (for `sig`) and a recording Component sink.
struct Fx {
    p: Pipeline,
    sink: Arc<Recording>,
    log: Log,
    waiting: Arc<Notify>,
}

impl Fx {
    async fn start() -> Self {
        Self::start_with(discord(), |_| {}).await
    }

    async fn start_with(answer: Answer, configure: impl FnOnce(&mut AppBuilder)) -> Self {
        let log = Log::default();
        let waiting = Arc::new(Notify::new());
        let sink = Arc::new(Recording::default());
        let component_sink = Arc::clone(&sink);
        let p = Pipeline::start_answering(module(&log, &waiting), answer, move |b| {
            b.error_sink(ErrorOrigin::Component, component_sink)
                .ack_deadline(D)
                .payload_key(noctis_codec::PayloadKey::from_bytes([0xab; 32]));
            configure(b);
        })
        .await;
        Self {
            p,
            sink,
            log,
            waiting,
        }
    }

    /// As [`Fx::start`], with the App-level Expired Handler.
    async fn with_app() -> Self {
        Self::start_with(discord(), |b| {
            b.expired_handler(app_handler);
        })
        .await
    }

    async fn run(&self, inbound: Inbound) {
        self.run_with(inbound, CancellationToken::new()).await;
    }

    async fn run_with(&self, inbound: Inbound, cancel: CancellationToken) {
        run_component(
            Arc::clone(self.p.fx.app.registry()),
            Arc::clone(self.p.fx.app.shared()),
            inbound,
            cancel,
        )
        .await;
    }

    /// The requests so far, then forgets them and the log; asserts no
    /// request carried a bot token (Matrix 47).
    fn take(&self) -> Vec<(Method, String, Value)> {
        let requests = self.p.requests();
        for seen in self.p.fx.server.seen() {
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
        self.p.fx.server.log.lock().unwrap().clear();
        requests
    }

    /// The hook log so far, then forgets it.
    fn hooks(&self) -> Vec<String> {
        self.log.0.lock().unwrap().drain(..).collect()
    }

    /// Runs `inbound` and asserts it got exactly the default expired
    /// notice, ran no handler or hook and logged one `debug` with
    /// `reason` and `hook="default"` and no warning.
    async fn expires(&self, inbound: Inbound, reason: &str) {
        let capture = Capture::default();
        {
            let _guard = tracing::subscriber::set_default(capture.subscriber());
            tracing::callsite::rebuild_interest_cache();
            self.run(inbound).await;
        }
        let requests = self.take();
        assert_eq!(requests.len(), 1, "{reason}: {requests:?}");
        assert_expired_notice(&requests[0], EXPIRED_TEXT);
        assert!(self.hooks().is_empty());
        let lines = capture.lines_with(&["noctis::dispatch", "component interaction expired"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        assert!(
            lines[0].contains("DEBUG")
                && lines[0].contains(&format!("reason=\"{reason}\""))
                && lines[0].contains("hook=\"default\""),
            "{}",
            lines[0]
        );
        assert!(!capture.text().contains("WARN"), "{}", capture.text());
        assert!(self.sink.seen().is_empty());
    }

    /// Runs `inbound` and asserts the 7.2 rejection `reason`: one warning
    /// and the generic notice, never the expired notice.
    async fn rejects(&self, inbound: Inbound, reason: &str) {
        let capture = Capture::default();
        {
            let _guard = tracing::subscriber::set_default(capture.subscriber());
            tracing::callsite::rebuild_interest_cache();
            self.run(inbound).await;
        }
        let lines = capture.lines_with(&["noctis::dispatch", "component interaction rejected"]);
        assert_eq!(lines.len(), 1, "{reason}: {}", capture.text());
        assert!(
            lines[0].contains(&format!("reason=\"{reason}\"")),
            "{}",
            lines[0]
        );
        assert!(capture.lines_with(&["interaction expired"]).is_empty());
        let requests = self.take();
        assert_eq!(requests.len(), 1, "{reason}: {requests:?}");
        assert_generic_notice(&requests[0]);
        assert_ne!(
            requests[0].2["data"]["components"][0]["content"],
            EXPIRED_TEXT
        );
        assert!(self.hooks().is_empty());
    }

    /// Runs `inbound` and returns the one `hook` field the chain logged.
    async fn hook_of(&self, inbound: Inbound) -> String {
        let capture = Capture::default();
        {
            let _guard = tracing::subscriber::set_default(capture.subscriber());
            tracing::callsite::rebuild_interest_cache();
            self.run(inbound).await;
        }
        let lines = capture.lines_with(&["component interaction expired"]);
        assert_eq!(lines.len(), 1, "{}", capture.text());
        let hook = lines[0]
            .split("hook=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap();
        hook.to_owned()
    }
}

/// Asserts `request` is the default Expired Handler's one type-4
/// ephemeral V2 callback with `text`.
fn assert_expired_notice(request: &(Method, String, Value), text: &str) {
    assert_eq!(
        request,
        &(
            Method::POST,
            CB.to_owned(),
            json!({
                "type": 4,
                "data": {
                    "flags": 32832,
                    "components": [{ "type": 10, "id": 1, "content": text }],
                    "allowed_mentions": { "parse": [] },
                },
            })
        ),
        "{request:?}"
    );
}

/// `parsed` on the V2 source message 77 in channel 55.
fn on_v2(parsed: &Value) -> Inbound {
    let mut raw = parsed.clone();
    raw["message"] = json!({ "id": "77", "channel_id": "55", "flags": 32768, "components": [] });
    inbound_raw(parsed, &raw)
}

/// A click with `custom_id` on a V2 message.
fn press(custom_id: &str) -> Inbound {
    on_v2(&click(custom_id, 2))
}

/// The update body of a one-text V2 View.
fn update_data(text: &str) -> Value {
    json!({
        "flags": 32768,
        "components": [{ "type": 10, "id": 1, "content": text }],
        "allowed_mentions": { "parse": [] },
        "attachments": [],
    })
}

/// The type-7 update of a one-text V2 View.
fn update(text: &str) -> (Method, String, Value) {
    (
        Method::POST,
        CB.to_owned(),
        json!({ "type": 7, "data": update_data(text) }),
    )
}

/// Matrix 1-8, 36 and 37: stale ids reach the default Expired Handler;
/// the checks before the decode keep their order and win.
#[tokio::test]
async fn stale_ids_reach_the_expired_handler() {
    let fx = Fx::start().await;
    // 1, 2: a stale schema, the highest schema digit.
    fx.expires(inbound(&click("1shop.buy:147", 2)), "stale")
        .await;
    fx.expires(inbound(&click("1shop.buy:z47", 2)), "stale")
        .await;
    // 3: against schema 61.
    fx.expires(inbound(&click("0v61:1", 2)), "stale").await;
    fx.expires(inbound(&click("1v61:11", 2)), "stale").await;
    // 4: a Ref body on an inline Route is stale, not a Ref.
    fx.expires(inbound(&click("0shop.buy:~0123456789A", 2)), "stale")
        .await;
    // 5: the `N` flag on a button, the `T` flag on an unsigned Route.
    fx.expires(inbound(&click("4shop.buy:0047", 2)), "stale")
        .await;
    fx.expires(inbound(&click("8shop.buy:47abcdef", 2)), "stale")
        .await;
    // 6: malformed, never stale.
    for id in ["1shop.buy:047", "2shop.buy:47", "Gshop.buy:47"] {
        fx.rejects(inbound(&click(id, 2)), "malformed").await;
    }
    // 7: unknown Route, kind mismatch, no author.
    fx.rejects(inbound(&click("1nope:147", 2)), "unknown_route")
        .await;
    fx.rejects(inbound(&click("1shop.buy:147", 3)), "kind_mismatch")
        .await;
    let mut anonymous = click("1shop.buy:147", 2);
    anonymous.as_object_mut().unwrap().remove("user");
    fx.rejects(inbound(&anonymous), "no_author").await;
    // 8: a signed Route's stale id is stale before its tag is checked.
    fx.expires(inbound(&click("9sig:11zzzzzz", 2)), "stale")
        .await;
    // 36: a 100-character stale id; one more is malformed.
    let exact = format!("1abcdefghijklmnopqrstuvwx:1{}", "1".repeat(73));
    assert_eq!(exact.len(), 100);
    fx.expires(inbound(&click(&exact, 2)), "stale").await;
    fx.rejects(inbound(&click(&format!("{exact}1"), 2)), "malformed")
        .await;
    // 37: non-ASCII.
    fx.rejects(inbound(&click("1shöp.buy:147", 2)), "malformed")
        .await;
    assert!(fx.log.lines().is_empty());
}

/// Matrix 9-12 and 44: `on_stale`, then `on_expired`, then the App
/// handler; the last `expired_handler` call wins.
#[tokio::test]
async fn hook_precedence() {
    let fx = Fx::start().await;
    // 9: `on_stale` wins and updates the clicked message.
    assert_eq!(fx.hook_of(press("1fresh:11")).await, "on_stale");
    assert_eq!(fx.take(), [update("stale-view")]);
    assert_eq!(fx.hooks(), ["on_stale:stale-view"]);
    // 10: `on_expired` with `Stale`.
    assert_eq!(fx.hook_of(press("1late:11")).await, "on_expired");
    assert_eq!(fx.take(), [update("expired-view Stale")]);
    assert_eq!(fx.hooks(), ["on_expired:Stale"]);

    // 11: the App handler for a Route without hooks.
    let fx = Fx::with_app().await;
    assert_eq!(fx.hook_of(press("1shop.buy:147")).await, "app");
    assert_eq!(fx.take(), [update("app shop.buy stale")]);
    // 12: a Route hook beats the App handler.
    assert_eq!(fx.hook_of(press("1late:11")).await, "on_expired");
    assert_eq!(fx.take(), [update("expired-view Stale")]);
    assert!(fx.sink.seen().is_empty());

    // 44: the last call wins.
    let fx = Fx::start_with(discord(), |b| {
        b.expired_handler(|_: ExpiredCtx, _: Expired| async { Ok(v2("a")) })
            .expired_handler(|_: ExpiredCtx, _: Expired| async { Ok(v2("b")) });
    })
    .await;
    fx.run(press("1shop.buy:147")).await;
    assert_eq!(fx.take(), [update("b")]);
}

/// Matrix 13-16: Ref ids go to `on_expired` (never `on_stale`), then the
/// App handler, then the default notice; a stale Ref id is stale.
#[tokio::test]
async fn refs_go_to_on_expired() {
    let fx = Fx::start().await;
    fx.expires(inbound(&click("0r:~0123456789A", 2)), "ref_unresolved")
        .await;
    assert_eq!(fx.hook_of(press("0rh:~0123456789A")).await, "on_expired");
    assert_eq!(fx.take(), [update("expired-view RefMissing")]);
    assert_eq!(fx.hooks(), ["on_expired:RefMissing"]);
    fx.expires(inbound(&click("1r:1~0123456789A", 2)), "stale")
        .await;

    let fx = Fx::with_app().await;
    assert_eq!(fx.hook_of(press("0r:~0123456789A")).await, "app");
    assert_eq!(fx.take(), [update("app r ref_missing")]);
}

/// Matrix 17-26: hooks and the App handler answer like handlers:
/// declared response, auto-defer, `Manual`, responders, errors, panics and
/// cancellation.
#[tokio::test]
#[expect(clippy::too_many_lines, reason = "one block per matrix row")]
async fn expired_hooks_answer_like_handlers() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let errors = || -> u64 {
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(k, ..)| {
                k.key().name() == "noctis_handler_errors_total"
                    && k.key().labels().any(|l| l.value() == "component")
            })
            .map(|(.., v)| match v {
                DebugValue::Counter(n) => n,
                _ => 0,
            })
            .sum()
    };
    let fx = Fx::with_app().await;
    // 17: an ephemeral Route's hook answers with a new ephemeral message.
    fx.run(press("1eph:11")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(
        (requests[0].1.as_str(), &requests[0].2["type"]),
        (CB_REPLY, &json!(4))
    );
    assert_eq!(requests[0].2["data"]["flags"], 32832);
    assert_eq!(
        requests[0].2["data"]["components"][0]["content"],
        "stale-view"
    );
    // 18: past the deadline the auto-defer sends type 6, then the edit.
    fx.run(press("1slow:11")).await;
    assert_eq!(
        fx.take(),
        [
            (Method::POST, CB.to_owned(), json!({ "type": 6 })),
            (Method::PATCH, ORIGINAL.to_owned(), update_data("slow-view")),
        ]
    );
    // 19: before the deadline, only the update.
    fx.run(press("1fresh:11")).await;
    assert_eq!(fx.take(), [update("stale-view")]);
    // 20: the App handler's auto-defer follows the declared response.
    fx.run(press("1zz.buy:11")).await;
    let requests = fx.take();
    assert_eq!(requests[0].2, json!({ "type": 6 }));
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::PATCH, ORIGINAL)
    );
    assert_eq!(requests[1].2, update_data("app zz.buy stale"));
    fx.run(press("1zz.eph:11")).await;
    let requests = fx.take();
    assert_eq!(requests[0].2, json!({ "type": 5, "data": { "flags": 64 } }));
    assert_eq!(requests[1].0, Method::PATCH);
    // 21: a `Manual` Route's hook may open a modal; no auto-defer.
    fx.run(press("1man:11")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].2["type"], 9);
    assert_eq!(requests[0].2["data"]["custom_id"], "0bio:1");
    // 22: the App handler on a `Manual` Route is still auto-deferred.
    fx.run(press("1zz.man:11")).await;
    let requests = fx.take();
    assert_eq!(requests[0].2, json!({ "type": 6 }));
    assert_eq!(requests[1].0, Method::PATCH);
    // 23: a hook with a responder.
    fx.run(press("1resp:11")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].2["type"], 4);
    assert_eq!(requests[0].2["data"]["flags"], 32832);
    assert!(fx.sink.seen().is_empty());
    assert_eq!(errors(), 0);

    // 24: a user error: its notice, a `debug`, no sink, no counter.
    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        tracing::callsite::rebuild_interest_cache();
        fx.run(press("1user.err:11")).await;
    }
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_generic_notice(&requests[0]);
    assert_eq!(
        capture
            .lines_with(&["DEBUG", "user condition", "shop.sold_out", "x::UserErr"])
            .len(),
        1,
        "{}",
        capture.text()
    );
    assert!(fx.sink.seen().is_empty());
    assert_eq!(errors(), 0);

    // 25: an error, a panic in a hook, a panic in the App handler.
    fx.run(press("1fails:11")).await;
    fx.run(press("1panics:11")).await;
    // A hook that panics before returning its future.
    fx.run(press("1sync.panics:11")).await;
    let fx_app_panic = Fx::start_with(discord(), |b| {
        b.expired_handler(|_: ExpiredCtx, _: Expired| async { Ok(explode()) });
    })
    .await;
    fx_app_panic.run(press("1shop.buy:147")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 3, "{requests:?}");
    for request in &requests {
        assert_generic_notice(request);
    }
    let requests = fx_app_panic.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_generic_notice(&requests[0]);
    let seen = fx.sink.seen();
    let kinds: Vec<_> = seen.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, ["error:boom", "panic:hook", "panic:sync hook"]);
    assert_eq!(
        (
            seen[0].origin,
            seen[0].module.as_deref(),
            seen[0].handler.as_deref()
        ),
        (ErrorOrigin::Component, Some("x"), Some("x::Fails"))
    );
    assert_eq!(
        (seen[2].module.as_deref(), seen[2].handler.as_deref()),
        (Some("x"), Some("x::SyncPanics"))
    );
    let seen = fx_app_panic.sink.seen();
    assert_eq!(
        (seen[0].kind.as_str(), seen[0].handler.as_deref()),
        ("panic:app", Some("x::Buy"))
    );
    assert_eq!(errors(), 4);
    // An App handler that panics before returning its future.
    let fx_sync_app = Fx::start_with(discord(), |b| {
        b.expired_handler(
            |_: ExpiredCtx, _: Expired| -> std::future::Ready<HandlerResult<V2Message>> {
                panic!("sync app")
            },
        );
    })
    .await;
    fx_sync_app.run(press("1shop.buy:147")).await;
    let requests = fx_sync_app.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_generic_notice(&requests[0]);
    let seen = fx_sync_app.sink.seen();
    assert_eq!(
        (
            seen[0].kind.as_str(),
            seen[0].module.as_deref(),
            seen[0].handler.as_deref()
        ),
        ("panic:sync app", Some("x"), Some("x::Buy"))
    );
    // Each snapshot counts the increments since the previous one.
    assert_eq!(errors(), 1);
    // The next click still works on both Apps.
    fx_sync_app.run(press("1late:11")).await;
    assert_eq!(fx_sync_app.take(), [update("expired-view Stale")]);
    fx.run(press("1fresh:11")).await;
    assert_eq!(fx.take(), [update("stale-view")]);
    fx.hooks();

    // 26: cancelled before and during the hook: no notice.
    let cancel = CancellationToken::new();
    cancel.cancel();
    fx.run_with(press("1fresh:11"), cancel).await;
    let cancel = CancellationToken::new();
    let run = fx.run_with(press("1waits:11"), cancel.clone());
    let trigger = async {
        fx.waiting.notified().await;
        cancel.cancel();
    };
    tokio::join!(run, trigger);
    assert!(fx.take().is_empty());
    let kinds: Vec<_> = fx.sink.seen().into_iter().map(|s| s.kind).collect();
    assert_eq!(kinds[3..], ["cancelled", "cancelled"]);
}

/// Matrix 27 and 28: the default notice's locale; a failed send is one
/// warning.
#[tokio::test]
async fn default_expired_notice_locale_and_failure() {
    let fx = Fx::start().await;
    let text = |locale: &str| notice::text(&Builtin, locale.parse().unwrap(), keys::EXPIRED);
    let cases = [
        (json!({ "locale": "tr" }), text("tr")),
        (json!({ "guild_locale": "de", "guild_id": "9" }), text("de")),
        (json!({}), EXPIRED_TEXT.to_owned()),
        (json!({ "locale": "xx-YY" }), EXPIRED_TEXT.to_owned()),
    ];
    for (extra, want) in cases {
        let mut d = click("1shop.buy:147", 2);
        d.as_object_mut().unwrap().remove("locale");
        d.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        fx.run(inbound(&d)).await;
        let requests = fx.take();
        assert_eq!(requests.len(), 1, "{extra}");
        assert_expired_notice(&requests[0], &want);
    }

    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    tracing::callsite::rebuild_interest_cache();
    let answer: Answer = Arc::new(|_: &crate::edge::test_support::Seen| {
        (
            hyper::StatusCode::INTERNAL_SERVER_ERROR,
            bytes::Bytes::new(),
        )
    });
    let fx = Fx::start_with(answer, |_| {}).await;
    fx.run(inbound(&click("1shop.buy:147", 2))).await;
    let lines = capture.lines_with(&["noctis::dispatch", "could not send the expired notice"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(
        lines[0].contains("WARN") && lines[0].contains("reason=\"stale\""),
        "{}",
        lines[0]
    );
    // The edge's hedged second attempt after a 5xx (Story 5.9) is the only
    // repeat: the pipeline sends the notice once and never retries it.
    let sent = fx.take();
    assert!((1..=2).contains(&sent.len()), "{sent:?}");
    for request in &sent {
        assert_expired_notice(request, EXPIRED_TEXT);
    }
    assert!(fx.sink.seen().is_empty());
}

/// Matrix 29-31: stale modal submits; a modal `#[on_expired]` alone.
#[tokio::test]
async fn modal_stale_hooks() {
    let fx = Fx::start().await;
    // 29: from a component message, `on_stale` updates it.
    fx.run(on_v2(&submit("1bioh:147", true))).await;
    assert_eq!(fx.take(), [update("modal-view")]);
    assert_eq!(fx.hooks(), ["modal_on_stale:x"]);
    // 30: slash-opened, the returned View is a new public message.
    fx.run(inbound(&submit("1bioh:147", true))).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(
        (requests[0].1.as_str(), &requests[0].2["type"]),
        (CB_REPLY, &json!(4))
    );
    assert_eq!(requests[0].2["data"]["flags"], 32768);
    fx.hooks();
    // 31: no hooks: the default notice.
    fx.expires(inbound(&submit("1bio:147", true)), "stale")
        .await;
    // Only `on_expired`: it runs with `Stale` and updates the message.
    fx.run(on_v2(&submit("1biol:147", true))).await;
    assert_eq!(fx.take(), [update("modal-expired Stale")]);
    assert_eq!(fx.hooks(), ["modal_on_expired:Stale"]);

    // The App handler of a modal converts with the modal kind.
    let fx = Fx::with_app().await;
    fx.run(on_v2(&submit("1bio:147", true))).await;
    assert_eq!(fx.take(), [update("app bio stale")]);
}
