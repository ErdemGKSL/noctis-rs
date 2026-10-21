//! Story 7.5 Matrix 20-38: dispatch resolves Ref ids through the App's
//! namespaced `RefStore` (`get`, `take` for `one_shot`), counts
//! `noctis_ref_lookups_total{app, result}`, and deletes the Refs of a
//! returned `RefOps` after a successful send.

use std::future::Future;

use bytes::Bytes;
use hyper::StatusCode;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_codec::{RouteRuntime, RouteShape};
use noctis_ports::RefStore as _;

use super::components::{assert_generic_notice, click, submit};
use super::*;
use crate::component::{__ExpiredHook, __StaleHook};
use crate::kind::__Response;
use crate::pipeline::component::run_component;
use crate::test_support::{TestRefs, test_route};
use crate::{
    AutoDefer, ComponentCtx, ComponentHandler, ComponentKind, ComponentResponder, Expired,
    ModalHandler, ModalSubmitCtx, RefOps,
};

test_route!(Stored, "t.stored", RouteShape::BY_REF, 0);
test_route!(
    Once,
    "t.once",
    RouteShape::BY_REF,
    0,
    RouteRuntime::EMPTY.one_shot()
);
test_route!(
    Short,
    "t.short",
    RouteShape::BY_REF,
    0,
    RouteRuntime::EMPTY.with_ttl_secs(1)
);
test_route!(Cancel, "t.cancel", RouteShape::BY_REF, 0);
test_route!(DoneOps, "t.done", RouteShape::BY_REF, 0);
test_route!(FailOps, "t.fail", RouteShape::BY_REF, 0);
test_route!(PanicOps, "t.panic", RouteShape::BY_REF, 0);
test_route!(Inline, "t.inline", RouteShape::INLINE, 0);
test_route!(Form, "t.form", RouteShape::BY_REF, 0);
test_route!(
    OnceForm,
    "t.onceform",
    RouteShape::BY_REF,
    0,
    RouteRuntime::EMPTY.one_shot()
);

/// The Ref every test record is stored under.
const REF: &str = "0123456789A";

/// The App key of [`REF`] in the App `default`.
fn key(app: &str) -> String {
    format!("{app}:ref:{REF}")
}

/// A logging handler of `S` returning `R` from `f`.
fn handler<S, F, Fut, R>(name: &'static str, f: F) -> ComponentHandler
where
    S: noctis_codec::Route<Payload = S>
        + noctis_codec::DeclaredRuntime
        + crate::__Guards<ComponentCtx>
        + for<'de> noctis_codec::PayloadDecode<'de>
        + Send
        + 'static,
    F: Fn(S, ComponentCtx, ComponentResponder) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: crate::IntoResponse<crate::Component>,
{
    ComponentHandler::__new_with_responder::<S, AutoDefer, _, _, _>(
        name,
        <S as noctis_codec::Route>::ID.as_str(),
        ComponentKind::Button,
        __Response::Update,
        f,
    )
}

/// `name:item:<whether ctx.current_ref() is the id's Ref>`.
fn line(name: &str, item: u64, ctx: &ComponentCtx) -> String {
    let current = ctx.current_ref().map(|r| r.as_str().to_owned());
    format!("{name}:{item}:{}", current.as_deref() == Some(REF))
}

fn module(log: &Log) -> Module {
    let (l1, l2, l3, l4) = (log.clone(), log.clone(), log.clone(), log.clone());
    let (stale_log, expired_log) = (log.clone(), log.clone());
    let m = Module::new("t")
        .__add(
            handler::<Stored, _, _, _>("t::Stored", move |s: Stored, ctx, _| {
                l1.push(line("stored", s.item, &ctx));
                async { Ok(Responded::new()) }
            })
            .__with_on_stale(__StaleHook::__new::<AutoDefer, _, _, _>(
                move |_: ComponentCtx, _: ComponentResponder| {
                    stale_log.push("on_stale".to_owned());
                    async { Ok(Responded::new()) }
                },
            )),
        )
        .__add(
            handler::<Once, _, _, _>("t::Once", move |s: Once, ctx, _| {
                l2.push(line("once", s.item, &ctx));
                async { Ok(Responded::new()) }
            })
            .__with_on_expired(__ExpiredHook::__new::<AutoDefer, _, _, _>(
                move |ctx: ComponentCtx, expired: Expired, _: ComponentResponder| {
                    let current = ctx.current_ref().is_some();
                    expired_log.push(format!("on_expired:{expired:?}:{current}"));
                    async { Ok(Responded::new()) }
                },
            )),
        )
        .__add(handler::<Short, _, _, _>(
            "t::Short",
            move |s: Short, ctx, _| {
                l3.push(line("short", s.item, &ctx));
                async { Ok(Responded::new()) }
            },
        ))
        .__add(handler::<Cancel, _, _, _>(
            "t::Cancel",
            |_: Cancel, ctx: ComponentCtx, _| async move {
                let ops = RefOps::new().delete(ctx.current_ref().expect("a Ref id"));
                Ok((v2("cancelled"), ops))
            },
        ))
        .__add(handler::<DoneOps, _, _, _>(
            "t::DoneOps",
            |_: DoneOps, ctx: ComponentCtx, r: ComponentResponder| async move {
                let ops = RefOps::new().delete(ctx.current_ref().expect("a Ref id"));
                Ok((r.reply(v2("done")).await?.done(), ops))
            },
        ))
        .__add(handler::<FailOps, _, _, _>(
            "t::FailOps",
            |_: FailOps, _: ComponentCtx, _| async move {
                Err::<(V2Message, RefOps), _>(HandlerError::new(std::io::Error::other("boom")))
            },
        ))
        .__add(handler::<PanicOps, _, _, _>(
            "t::PanicOps",
            |_: PanicOps, _: ComponentCtx, _| async move { boom() },
        ))
        .__add(handler::<Inline, _, _, _>(
            "t::Inline",
            move |s: Inline, ctx, _| {
                l4.push(line("inline", s.item, &ctx));
                async { Ok(Responded::new()) }
            },
        ));
    with_modals(m, log)
}

/// Adds the `Form` and `OnceForm` modal handlers to `m`.
fn with_modals(m: Module, log: &Log) -> Module {
    let (form_log, form_expired) = (log.clone(), log.clone());
    let (once_form_log, once_form_expired) = (log.clone(), log.clone());
    m.__add(
        ModalHandler::__new::<Form, _, _>(
            "t::Form",
            "t.form",
            move |f: Form, ctx: ModalSubmitCtx| {
                let current = ctx.current_ref().map(|r| r.as_str().to_owned());
                form_log.push(format!(
                    "form:{}:{}",
                    f.item,
                    current.as_deref() == Some(REF)
                ));
                async { Ok(Responded::new()) }
            },
        )
        .__with_on_expired(__ExpiredHook::__new::<AutoDefer, _, _, _>(
            move |_: ModalSubmitCtx, expired: Expired, _: crate::ModalResponder| {
                form_expired.push(format!("form_expired:{expired:?}"));
                async { Ok(Responded::new()) }
            },
        )),
    )
    .__add(
        ModalHandler::__new::<OnceForm, _, _>(
            "t::OnceForm",
            "t.onceform",
            move |f: OnceForm, _: ModalSubmitCtx| {
                once_form_log.push(format!("once_form:{}", f.item));
                async { Ok(Responded::new()) }
            },
        )
        .__with_on_expired(__ExpiredHook::__new::<AutoDefer, _, _, _>(
            move |_: ModalSubmitCtx, expired: Expired, _: crate::ModalResponder| {
                once_form_expired.push(format!("once_form_expired:{expired:?}"));
                async { Ok(Responded::new()) }
            },
        )),
    )
}

/// A running App with [`module`] over the scripted store `refs`, a
/// recording Component sink and a local metrics recorder.
struct Fx {
    p: Pipeline,
    refs: Arc<TestRefs>,
    sink: Arc<Recording>,
    log: Log,
}

impl Fx {
    async fn start() -> Self {
        Self::start_as(TestRefs::new(), None, discord()).await
    }

    /// As [`Fx::start`], with the store `refs`, the App id `app` and the
    /// upstream answering `answer`.
    async fn start_as(refs: Arc<TestRefs>, app: Option<&'static str>, answer: Answer) -> Self {
        let log = Log::default();
        let sink = Arc::new(Recording::default());
        let component_sink = Arc::clone(&sink);
        let store = Arc::clone(&refs);
        let p = Pipeline::start_answering(module(&log), answer, move |b| {
            b.error_sink(ErrorOrigin::Component, component_sink)
                .ref_store(store);
            if let Some(app) = app {
                b.app_id(app);
            }
        })
        .await;
        Self { p, refs, sink, log }
    }

    async fn run(&self, raw: &Value) {
        run_component(
            Arc::clone(self.p.fx.app.registry()),
            Arc::clone(self.p.fx.app.shared()),
            inbound(raw),
            CancellationToken::new(),
        )
        .await;
    }

    /// Runs `raw` with a log capture; returns the captured text.
    async fn run_captured(&self, raw: &Value) -> String {
        let capture = Capture::default();
        {
            let _guard = tracing::subscriber::set_default(capture.subscriber());
            self.run(raw).await;
        }
        capture.text()
    }

    /// The requests so far, then forgets them.
    fn take(&self) -> Vec<(Method, String, Value)> {
        let requests = self.p.requests();
        self.p.fx.server.log.lock().unwrap().clear();
        requests
    }

    fn lines(&self) -> Vec<String> {
        self.log.0.lock().unwrap().drain(..).collect()
    }
}

/// A click on the button `route` carrying [`REF`].
fn ref_click(route: &str) -> Value {
    click(&format!("0{route}:~{REF}"), 2)
}

/// The `noctis_ref_lookups_total` counts by `result` since the last read;
/// asserts every one carries exactly the `app` and `result` labels.
fn lookups(snapshotter: &Snapshotter) -> Vec<(String, String, u64)> {
    let mut out = Vec::new();
    for (key, _, _, value) in snapshotter.snapshot().into_vec() {
        if key.key().name() != "noctis_ref_lookups_total" {
            continue;
        }
        let labels: Vec<(String, String)> = key
            .key()
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect();
        assert_eq!(labels.len(), 2, "{labels:?}");
        assert_eq!(labels[0].0, "app");
        assert_eq!(labels[1].0, "result");
        if let DebugValue::Counter(n) = value
            && n > 0
        {
            out.push((labels[0].1.clone(), labels[1].1.clone(), n));
        }
    }
    out.sort();
    out
}

fn count(app: &str, result: &str, n: u64) -> (String, String, u64) {
    (app.to_owned(), result.to_owned(), n)
}

/// Matrix 20: a click on a stored Ref runs the handler with the stored
/// Payload and `current_ref()`; the record stays; one `hit`.
#[tokio::test]
async fn resolve_hit_keeps_the_record() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"0t.stored:1");
    fx.run(&ref_click("t.stored")).await;
    assert_eq!(fx.lines(), ["stored:1:true"]);
    assert_eq!(fx.refs.calls_of("get").len(), 1);
    assert!(fx.refs.calls_of("take").is_empty());
    assert!(fx.refs.value(&key("default")).is_some());
    assert_eq!(lookups(&snapshotter), [count("default", "hit", 1)]);
    // An inline id has no Ref and no lookup.
    fx.run(&click("0t.inline:5", 2)).await;
    assert_eq!(fx.lines(), ["inline:5:false"]);
    assert!(lookups(&snapshotter).is_empty());
    assert!(fx.sink.seen().is_empty());
}

/// Matrix 21: a `one_shot` Ref is taken: the second click (and the loser
/// of two concurrent clicks) reaches `#[on_expired]` with `RefMissing`.
#[tokio::test]
async fn one_shot_is_consumed_once() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"0t.once:7");
    fx.run(&ref_click("t.once")).await;
    fx.run(&ref_click("t.once")).await;
    assert_eq!(fx.lines(), ["once:7:true", "on_expired:RefMissing:true"]);
    assert_eq!(fx.refs.calls_of("take").len(), 2);
    assert!(fx.refs.calls_of("get").is_empty());
    assert!(fx.refs.value(&key("default")).is_none());
    assert_eq!(
        lookups(&snapshotter),
        [count("default", "hit", 1), count("default", "miss", 1)]
    );

    fx.refs.insert(&key("default"), b"0t.once:8");
    let click = ref_click("t.once");
    tokio::join!(fx.run(&click), fx.run(&click));
    let mut lines = fx.lines();
    lines.sort();
    assert_eq!(lines, ["on_expired:RefMissing:true", "once:8:true"]);
    assert_eq!(
        lookups(&snapshotter),
        [count("default", "hit", 1), count("default", "miss", 1)]
    );
}

/// Matrix 22 and 23: an unknown Ref, and one whose TTL ran out, are
/// `miss`es answered by the Expired chain (here the default notice).
#[tokio::test]
async fn missing_and_expired_refs_are_misses() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;
    let text = fx.run_captured(&ref_click("t.stored")).await;
    assert!(
        text.contains("reason=\"ref_unresolved\"") && text.contains("hook=\"default\""),
        "{text}"
    );
    let requests = fx.take();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].2["data"]["flags"], 32832);
    assert!(fx.lines().is_empty());
    assert_eq!(lookups(&snapshotter), [count("default", "miss", 1)]);

    // A 1 s Route TTL, stored by the Responder, then the clock moves 2 s.
    tokio::time::pause();
    assert!(
        fx.refs
            .put_if_absent(
                &key("default"),
                Bytes::from_static(b"0t.short:1"),
                Some(Duration::from_secs(1))
            )
            .await
            .unwrap()
    );
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::time::resume();
    fx.run(&ref_click("t.short")).await;
    assert!(fx.lines().is_empty());
    assert_eq!(lookups(&snapshotter), [count("default", "miss", 1)]);
}

/// Matrix 24: App `b` sharing App `a`'s store does not see `a`'s Refs.
#[tokio::test]
async fn cid_05_refs_are_app_namespaced() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let refs = TestRefs::new();
    let a = Fx::start_as(Arc::clone(&refs), Some("a"), discord()).await;
    let b = Fx::start_as(Arc::clone(&refs), Some("b"), discord()).await;
    refs.insert(&key("a"), b"0t.stored:2");
    b.run(&ref_click("t.stored")).await;
    assert!(b.lines().is_empty());
    assert_eq!(refs.calls_of("get")[0].key, key("b"));
    a.run(&ref_click("t.stored")).await;
    assert_eq!(a.lines(), ["stored:2:true"]);
    assert_eq!(
        lookups(&snapshotter),
        [count("a", "hit", 1), count("b", "miss", 1)]
    );
}

/// Runs `click` against the record `record` and asserts one warn with
/// `reason`, the generic notice, no handler and no Ref in the logs
/// (Matrix 38).
async fn rejected(fx: &Fx, record: Option<&[u8]>, reason: &str) {
    if let Some(record) = record {
        fx.refs.insert(&key("default"), record);
    }
    let text = fx.run_captured(&ref_click("t.stored")).await;
    let warns: Vec<_> = text
        .lines()
        .filter(|l| l.contains("component interaction rejected"))
        .collect();
    assert_eq!(warns.len(), 1, "{text}");
    assert!(warns[0].contains(&format!("reason=\"{reason}\"")), "{text}");
    assert!(warns[0].contains("route=\"t.stored\""), "{text}");
    assert!(!text.contains(REF), "the Ref leaked: {text}");
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_generic_notice(&requests[0]);
    assert!(fx.lines().is_empty());
}

/// Matrix 25, 26, 28, 29 and 38: forged, garbage and undecodable records
/// and a failing store; each is one warn and the generic notice, and no
/// log line carries the Ref.
#[tokio::test]
async fn cid_05_record_route_mismatch_is_forged_at_dispatch() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;
    rejected(&fx, Some(b"0other:1"), "forged").await;
    rejected(&fx, Some(&[0xff]), "forged").await;
    rejected(&fx, Some(b""), "forged").await;
    assert_eq!(lookups(&snapshotter), [count("default", "forged", 3)]);
    rejected(&fx, Some(b"0t.stored:1,2"), "undecodable").await;
    assert_eq!(lookups(&snapshotter), [count("default", "hit", 1)]);
    fx.refs.fail("get");
    rejected(&fx, None, "ref_store").await;
    assert!(lookups(&snapshotter).is_empty());
    assert!(fx.sink.seen().is_empty());
}

/// Matrix 27: a record of another schema goes to the stale chain
/// (`#[on_stale]`); one `stale`.
#[tokio::test]
async fn stale_record_reaches_on_stale() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"1t.stored:11");
    let text = fx.run_captured(&ref_click("t.stored")).await;
    assert_eq!(fx.lines(), ["on_stale"]);
    assert!(text.contains("reason=\"stale\""), "{text}");
    assert!(!text.contains(REF), "{text}");
    assert_eq!(lookups(&snapshotter), [count("default", "stale", 1)]);
}

/// Matrix 30: a by-Ref modal submit decodes its Payload from the record,
/// or reaches `#[on_expired]` when the record is missing.
#[tokio::test]
async fn ref_modal_submit() {
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"0t.form:9");
    let id = format!("0t.form:~{REF}");
    fx.run(&submit(&id, true)).await;
    assert_eq!(fx.lines(), ["form:9:true"]);
    fx.refs.insert(&key("default"), b"0t.form:9");
    fx.run(&submit(&id, false)).await;
    assert!(fx.lines().is_empty());
    let requests = fx.take();
    assert_generic_notice(requests.last().unwrap());
    fx.run(&submit(&format!("0t.form:~{}", "B".repeat(11)), true))
        .await;
    assert_eq!(fx.lines(), ["form_expired:RefMissing"]);
}

/// A `one_shot` by-Ref modal takes its Ref: the first submit decodes the
/// record, the second reaches `#[on_expired]` with `RefMissing`.
#[tokio::test]
async fn one_shot_modal_takes_its_ref() {
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"0t.onceform:5");
    let id = format!("0t.onceform:~{REF}");
    fx.run(&submit(&id, true)).await;
    assert_eq!(fx.lines(), ["once_form:5"]);
    fx.run(&submit(&id, true)).await;
    assert_eq!(fx.lines(), ["once_form_expired:RefMissing"]);
    assert_eq!(fx.refs.calls_of("take").len(), 2);
    assert!(fx.refs.calls_of("get").is_empty());
}

/// Matrix 31 and 34: a returned `(msg, RefOps)` and `(Responded, RefOps)`
/// delete their Refs after the send.
#[tokio::test]
async fn ref_ops_delete_after_the_send() {
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"0t.cancel:1");
    fx.run(&ref_click("t.cancel")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].2["type"], 4);
    assert!(fx.refs.value(&key("default")).is_none());
    let ops: Vec<_> = fx.refs.calls().into_iter().map(|c| (c.op, c.key)).collect();
    assert_eq!(ops, [("get", key("default")), ("delete", key("default"))]);

    fx.refs.insert(&key("default"), b"0t.done:1");
    fx.run(&ref_click("t.done")).await;
    assert_eq!(fx.take().len(), 1);
    assert!(fx.refs.value(&key("default")).is_none());
    assert_eq!(fx.refs.calls_of("delete").len(), 2);
    assert!(fx.sink.seen().is_empty());
}

/// Matrix 32 and 33: a failed send, an error or a panic deletes nothing.
#[tokio::test]
async fn ref_ops_need_a_successful_run() {
    let failing: Answer = Arc::new(|_: &crate::edge::test_support::Seen| {
        (StatusCode::INTERNAL_SERVER_ERROR, Bytes::from_static(b"{}"))
    });
    let fx = Fx::start_as(TestRefs::new(), None, failing).await;
    fx.refs.insert(&key("default"), b"0t.cancel:1");
    fx.run(&ref_click("t.cancel")).await;
    assert!(fx.refs.calls_of("delete").is_empty());
    assert!(fx.refs.value(&key("default")).is_some());

    let fx = Fx::start().await;
    for (route, record) in [("t.fail", &b"0t.fail:1"[..]), ("t.panic", b"0t.panic:1")] {
        fx.refs.insert(&key("default"), record);
        fx.run(&ref_click(route)).await;
        assert!(fx.refs.calls_of("delete").is_empty(), "{route}");
    }
    assert_eq!(fx.sink.seen().len(), 2);
}

/// Matrix 35: duplicates collapse; an empty `RefOps` calls nothing.
#[test]
fn ref_ops_dedupe() {
    let r = noctis_codec::Ref::mint().unwrap();
    let ops = RefOps::new().delete(r).delete(r);
    assert_eq!(ops.len(), 1);
    assert_eq!(ops.refs(), &[r]);
    assert!(RefOps::new().is_empty());
    assert_eq!(RefOps::new(), RefOps::default());
}

/// Matrix 36: a failed delete is one warn on `noctis::refs` without the
/// Ref, and the interaction still succeeds.
#[tokio::test]
async fn a_failed_delete_is_one_warning() {
    let fx = Fx::start().await;
    fx.refs.insert(&key("default"), b"0t.cancel:1");
    fx.refs.fail("delete");
    let text = fx.run_captured(&ref_click("t.cancel")).await;
    let warns: Vec<_> = text
        .lines()
        .filter(|l| l.contains("noctis::refs") && l.contains("WARN"))
        .collect();
    assert_eq!(warns.len(), 1, "{text}");
    assert!(!text.contains(REF), "{text}");
    assert_eq!(fx.take().len(), 1);
    assert!(fx.sink.seen().is_empty());
}

/// A handler body that panics.
fn boom() -> HandlerResult<(V2Message, RefOps)> {
    panic!("handler")
}
