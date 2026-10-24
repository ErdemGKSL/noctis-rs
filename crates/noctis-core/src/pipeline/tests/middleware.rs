//! Middleware Matrix 21-31, 33, 34, 39, 44 and 45 (plus the run metadata
//! and start panics): Middleware in the
//! Command, component and event pipelines over a recording upstream.

use std::sync::atomic::{AtomicUsize, Ordering};

use twilight_model::gateway::event::EventType;

use super::components::{assert_generic_notice, click, submit};
use super::*;
use crate::kind::__ext;
use crate::test_support::{TBuy, message_delete};
use crate::{
    AfterCtx, AfterMiddleware, BeforeCtx, BeforeMiddleware, ComponentCtx, ComponentHandler,
    ComponentKind, ErrorReport, ErrorSink, Flow, Invocation, Middleware, ModalHandler,
    ModalSubmitCtx, ReportKind, RunOutcome,
};

/// The follow-up route (`wait=true`).
const FOLLOWUP: &str = "/api/v10/webhooks/7/tok?wait=true";

#[derive(Debug, PartialEq, Eq)]
struct A(u32);

/// What a report carried beyond [`Seen`]: its kind, the failing
/// Middleware and the Extension type names (`None`: no Extensions).
#[derive(Debug, Clone, PartialEq, Eq)]
struct MwSeen {
    origin: ErrorOrigin,
    kind: String,
    middleware: Option<String>,
    extensions: Option<Vec<String>>,
}

/// A sink recording [`MwSeen`]s.
#[derive(Default)]
struct MwSink(Mutex<Vec<MwSeen>>);

impl MwSink {
    fn seen(&self) -> Vec<MwSeen> {
        self.0.lock().unwrap().clone()
    }
}

impl ErrorSink for MwSink {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => format!("error:{error}"),
            ReportKind::Panic(message) => format!("panic:{message}"),
            ReportKind::Dropped => "dropped".to_owned(),
            ReportKind::Cancelled => "cancelled".to_owned(),
        };
        self.0.lock().unwrap().push(MwSeen {
            origin: report.origin,
            kind,
            middleware: report.middleware.map(str::to_owned),
            extensions: report
                .extensions
                .map(|e| e.type_names().map(str::to_owned).collect()),
        });
    }
}

/// A before-Middleware logging `label`, then answering `flow(ctx)` from
/// its future.
fn before(
    log: &Log,
    label: &'static str,
    flow: fn(&BeforeCtx) -> HandlerResult<Flow>,
) -> BeforeMiddleware {
    let log = log.clone();
    Middleware::before(move |ctx: BeforeCtx| {
        log.push(label);
        let log = log.clone();
        async move {
            let _ = &log;
            flow(&ctx)
        }
    })
}

/// A before-Middleware logging `label` and continuing.
fn pass(log: &Log, label: &'static str) -> BeforeMiddleware {
    before(log, label, |_| Ok(Flow::Continue))
}

/// An after-Middleware logging `label:<outcome>`.
fn after(log: &Log, label: &'static str) -> AfterMiddleware {
    let log = log.clone();
    Middleware::after(move |ctx: AfterCtx| {
        log.push(format!("{label}:{:?}", ctx.outcome()));
        async {}
    })
}

/// A Command at `path` logging `handler` and replying.
fn handler(log: &Log, name: &'static str, path: &'static [&'static str]) -> CommandHandler {
    let log = log.clone();
    CommandHandler::__new_plain(
        name,
        path,
        "d",
        move |_: CommandCtx, _: CommandResponder| {
            let log = log.clone();
            async move {
                log.push("handler");
                Ok(v2("ok"))
            }
        },
    )
}

fn slash_run(name: &str) -> Inbound {
    inbound(&interaction(slash(name, json!([])), json!({})))
}

/// The test tree: root `before(r1, r2)` `after(a1, a2)` with the Command
/// `root`, child `before(c1)` `after(ca1)` with the Command `child`.
fn tree(log: &Log, r2: BeforeMiddleware, a1: AfterMiddleware) -> Module {
    Module::new("root")
        .before(pass(log, "r1"))
        .before(r2)
        .after(a1)
        .after(after(log, "a2"))
        .__add(handler(log, "m::root", &["root"]))
        .module(
            Module::new("root/child")
                .before(pass(log, "c1"))
                .after(after(log, "ca1"))
                .__add(handler(log, "m::child", &["child"])),
        )
}

/// A running App of `module` whose Command, Component and Event sinks
/// record into the returned sink.
async fn start(module: Module, configure: impl FnOnce(&mut AppBuilder)) -> (Pipeline, Arc<MwSink>) {
    let sink = Arc::new(MwSink::default());
    let s = Arc::clone(&sink);
    let p = Pipeline::start_with(module, move |b| {
        b.error_sink(ErrorOrigin::Command, Arc::clone(&s))
            .error_sink(ErrorOrigin::Component, Arc::clone(&s))
            .error_sink(ErrorOrigin::Event, Arc::clone(&s))
            .error_sink(ErrorOrigin::Autocomplete, s);
        configure(b);
    })
    .await;
    (p, sink)
}

/// Matrix 21 and 22: the order around a child's handler, and a root
/// handler running no child Middleware.
#[tokio::test]
async fn order_and_scope() {
    let log = Log::default();
    let (p, sink) = start(tree(&log, pass(&log, "r2"), after(&log, "a1")), |_| {}).await;
    p.run(slash_run("child")).await;
    assert_eq!(
        log.lines(),
        [
            "r1",
            "r2",
            "c1",
            "handler",
            "ca1:Completed",
            "a2:Completed",
            "a1:Completed"
        ]
    );
    log.0.lock().unwrap().clear();
    p.run(slash_run("root")).await;
    assert_eq!(
        log.lines(),
        ["r1", "r2", "handler", "a2:Completed", "a1:Completed"]
    );
    assert!(sink.seen().is_empty());
    assert_eq!(p.requests().len(), 2);
}

/// Matrix 23 and 26 (pending): `Stop(None)` skips the rest and the
/// handler, sends nothing and runs the after chain with `Stopped`.
#[tokio::test]
async fn silent_stop_while_pending() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    tracing::callsite::rebuild_interest_cache();
    let log = Log::default();
    let r2 = before(&log, "r2", |_| Ok(Flow::Stop(None)));
    let (p, sink) = start(tree(&log, r2, after(&log, "a1")), |_| {}).await;
    p.run(slash_run("child")).await;
    assert_eq!(
        log.lines(),
        ["r1", "r2", "ca1:Stopped", "a2:Stopped", "a1:Stopped"]
    );
    assert_eq!(p.requests(), []);
    assert!(sink.seen().is_empty());
    let stopped = capture.lines_with(&["noctis::middleware", "stopped"]);
    assert_eq!(stopped.len(), 1, "{}", capture.text());
    assert!(
        stopped[0].contains("handler=\"m::child\"") && stopped[0].contains("middleware="),
        "{}",
        stopped[0]
    );
}

/// Matrix 24 (core): `Stop(Some(k))` before the deadline is one type-4
/// ephemeral notice of `k`.
#[tokio::test]
async fn stop_notice_while_pending() {
    let log = Log::default();
    let module = Module::new("m")
        .before(before(&log, "deny", |_| {
            Ok(Flow::Stop(Some(keys::ERROR_GENERIC)))
        }))
        .__add(handler(&log, "m::h", &["h"]));
    let (p, sink) = start(module, |_| {}).await;
    p.run(slash_run("h")).await;
    assert_eq!(log.lines(), ["deny"]);
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_notice(&requests[0], GENERIC);
    assert!(sink.seen().is_empty());
}

/// A Module whose before-Middleware sleeps past a 20 ms ack deadline, then
/// answers `flow`.
fn sleepy(log: &Log, flow: Flow) -> Module {
    let log = log.clone();
    Module::new("m")
        .before(Middleware::before(move |_: BeforeCtx| async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok(flow)
        }))
        .__add(handler(&log, "m::h", &["h"]))
}

/// Matrix 25 and 26 (deferred): a stop after the type-5 auto-defer
/// deletes the original, then follows up with `k`, or only deletes it.
#[tokio::test]
async fn stop_after_auto_defer() {
    let log = Log::default();
    let k = Flow::Stop(Some(keys::ERROR_GENERIC));
    let (p, _) = start(sleepy(&log, k), |b| {
        b.ack_deadline(Duration::from_millis(20));
    })
    .await;
    p.run(slash_run("h")).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert_eq!(
        requests[0],
        (Method::POST, CB.to_owned(), json!({ "type": 5 }))
    );
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::DELETE, ORIGINAL)
    );
    assert_eq!(requests[2].1, FOLLOWUP);
    assert_followup_notice(&requests[2], GENERIC);
    assert!(log.lines().is_empty(), "the handler never ran");

    let (p, _) = start(sleepy(&log, Flow::Stop(None)), |b| {
        b.ack_deadline(Duration::from_millis(20));
    })
    .await;
    p.run(slash_run("h")).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].2, json!({ "type": 5 }));
    assert_eq!(
        (&requests[1].0, requests[1].1.as_str()),
        (&Method::DELETE, ORIGINAL)
    );
}

/// Matrix 27-29: a before error, user error and panic (in the call and in
/// the future) skip the handler, notify, report (or `debug`) and run the
/// after chain.
#[tokio::test]
async fn before_failures() {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    fn fails(_: &BeforeCtx) -> HandlerResult<Flow> {
        Err(HandlerError::new(std::io::Error::other("db down")))
    }
    fn user(_: &BeforeCtx) -> HandlerResult<Flow> {
        Err(HandlerError::user(keys::ERROR_GENERIC))
    }
    fn panics(_: &BeforeCtx) -> HandlerResult<Flow> {
        panic!("in the future")
    }
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);

    let call_panics = || {
        Middleware::before(|_: BeforeCtx| -> std::future::Ready<HandlerResult<Flow>> {
            panic!("in the call")
        })
    };
    let cases: [(BeforeMiddleware, &str, Option<&str>); 4] = [
        (
            before(&Log::default(), "x", fails),
            "Failed",
            Some("error:db down"),
        ),
        (before(&Log::default(), "x", user), "UserError", None),
        (
            before(&Log::default(), "x", panics),
            "Panicked",
            Some("panic:in the future"),
        ),
        (call_panics(), "Panicked", Some("panic:in the call")),
    ];
    for (mw, outcome, reported) in cases {
        let log = Log::default();
        let name = mw.name();
        let module = Module::new("m")
            .before(mw)
            .after(after(&log, "a"))
            .__add(handler(&log, "m::h", &["h"]));
        let (p, sink) = start(module, |_| {}).await;
        p.run(slash_run("h")).await;
        assert_eq!(log.lines(), [format!("a:{outcome}")]);
        let requests = p.requests();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_notice(&requests[0], GENERIC);
        let seen = sink.seen();
        match reported {
            Some(kind) => {
                assert_eq!(seen.len(), 1, "{seen:?}");
                assert_eq!(seen[0].kind, kind);
                assert_eq!(seen[0].middleware.as_deref(), Some(name));
                assert_eq!(seen[0].extensions, Some(vec![]));
            }
            None => assert_eq!(seen, []),
        }
    }
    let errors: u64 = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(k, ..)| k.key().name() == "noctis_handler_errors_total")
        .map(|(.., v)| match v {
            DebugValue::Counter(n) => n,
            _ => 0,
        })
        .sum();
    assert_eq!(errors, 3);
}

/// Matrix 30: an after-Middleware panic is its own report; the others
/// still run, and the main outcome is unchanged.
#[tokio::test]
async fn after_panic_is_reported_alone() {
    use metrics_util::debugging::DebuggingRecorder;
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let log = Log::default();
    let a1 = Middleware::after(|_: AfterCtx| async { panic!("a1 boom") });
    let a1_name = a1.name();
    let (p, sink) = start(tree(&log, pass(&log, "r2"), a1), |_| {}).await;
    p.run(slash_run("child")).await;
    assert_eq!(
        log.lines(),
        ["r1", "r2", "c1", "handler", "ca1:Completed", "a2:Completed"]
    );
    assert_eq!(
        sink.seen(),
        [MwSeen {
            origin: ErrorOrigin::Command,
            kind: "panic:a1 boom".to_owned(),
            middleware: Some(a1_name.to_owned()),
            extensions: Some(vec![]),
        }]
    );
    assert_eq!(p.requests().len(), 1);
    assert_eq!(errors_total(snapshotter.snapshot()), 1);
}

/// The sum of every `noctis_handler_errors_total` counter in `snapshot`.
fn errors_total(snapshot: metrics_util::debugging::Snapshot) -> u64 {
    snapshot
        .into_vec()
        .into_iter()
        .filter(|(k, ..)| k.key().name() == "noctis_handler_errors_total")
        .map(|(.., v)| match v {
            metrics_util::debugging::DebugValue::Counter(n) => n,
            _ => 0,
        })
        .sum()
}

/// Matrix 31: a shutdown during the before chain runs no handler and no
/// after chain, and reports `Cancelled`.
#[tokio::test]
async fn cancel_during_before() {
    let log = Log::default();
    let started = Arc::new(Notify::new());
    let module = {
        let started = Arc::clone(&started);
        Module::new("m")
            .before(Middleware::before(move |_: BeforeCtx| {
                let started = Arc::clone(&started);
                async move {
                    started.notify_one();
                    std::future::pending::<()>().await;
                    Ok(Flow::Continue)
                }
            }))
            .after(after(&log, "a"))
            .__add(handler(&log, "m::h", &["h"]))
    };
    let (p, sink) = start(module, |_| {}).await;
    let cancel = CancellationToken::new();
    let p = Arc::new(p);
    let run = tokio::spawn({
        let p = Arc::clone(&p);
        let cancel = cancel.clone();
        async move { p.run_with(slash_run("h"), cancel).await }
    });
    started.notified().await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .unwrap()
        .unwrap();
    assert!(log.lines().is_empty(), "{:?}", log.lines());
    let seen = sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].kind, "cancelled");
}

/// Matrix 36 and 39 (pipeline): the inserted value reaches the handler and
/// the report; a declared provider that inserts nothing makes the handler
/// fail with `ExtensionMissing`.
#[tokio::test]
async fn extensions_reach_handler_and_report() {
    let seen_a = Log::default();
    let take_a = |log: &Log, name: &'static str, path: &'static [&'static str]| {
        let log = log.clone();
        CommandHandler::__new_plain(
            name,
            path,
            "d",
            move |ctx: CommandCtx, _: CommandResponder| {
                let log = log.clone();
                async move {
                    let a = __ext::<A, _>(&ctx, name)?;
                    log.push(format!("{a:?}"));
                    Err::<V2Message, _>(HandlerError::new(std::io::Error::other("after a")))
                }
            },
        )
    };
    let module = Module::new("m")
        .module(
            Module::new("m/ins")
                .before(
                    Middleware::before(|ctx: BeforeCtx| async move {
                        ctx.insert(A(7));
                        Ok(Flow::Continue)
                    })
                    .provides::<A>(),
                )
                .after(Middleware::after(|ctx: AfterCtx| async move {
                    assert_eq!(ctx.get::<A>().as_deref(), Some(&A(7)));
                    assert_eq!(ctx.outcome(), RunOutcome::Failed);
                }))
                .__add(take_a(&seen_a, "m::ins", &["ins"])),
        )
        .module(
            Module::new("m/none")
                .before(
                    Middleware::before(|_: BeforeCtx| async { Ok(Flow::Continue) }).provides::<A>(),
                )
                .__add(take_a(&seen_a, "m::none", &["none"])),
        );
    let (p, sink) = start(module, |_| {}).await;
    p.run(slash_run("ins")).await;
    assert_eq!(seen_a.lines(), ["Ext(A(7))"]);
    p.run(slash_run("none")).await;
    assert_eq!(seen_a.lines(), ["Ext(A(7))"]);
    let seen = sink.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0].kind, "error:after a");
    assert_eq!(seen[0].middleware, None);
    assert_eq!(
        seen[0].extensions,
        Some(vec![std::any::type_name::<A>().to_owned()])
    );
    assert_eq!(
        seen[1].kind,
        format!(
            "error:handler `m::none` takes `Ext<{}>`, but no Middleware inserted it in this run",
            std::any::type_name::<A>()
        )
    );
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_notice(&requests[1], GENERIC);
}

/// Matrix 44 and 45: without Middleware a handler report carries empty
/// Extensions; an overflow drop and an autocomplete report carry none.
#[tokio::test]
async fn report_extensions_without_middleware() {
    let module = Module::new("m").__add(CommandHandler::__new_plain(
        "m::fails",
        &["fails"],
        "d",
        |_: CommandCtx, _: CommandResponder| async {
            Err::<V2Message, _>(HandlerError::new(std::io::Error::other("x")))
        },
    ));
    let (p, sink) = start(module, |_| {}).await;
    p.run(slash_run("fails")).await;
    assert_eq!(sink.seen()[0].extensions, Some(vec![]));
    assert_eq!(sink.seen()[0].middleware, None);

    let drops = Arc::new(MwSink::default());
    let mut sinks = crate::sink::ErrorSinks::default();
    sinks.set(ErrorOrigin::Event, Arc::clone(&drops) as Arc<dyn ErrorSink>);
    let observer = crate::sink::EventDropObserver {
        app: Arc::from("default"),
        sinks,
    };
    noctis_gateway::DropObserver::overflow_dropped(&observer, 0, &message_delete(1, None));
    assert_eq!(
        drops.seen(),
        [MwSeen {
            origin: ErrorOrigin::Event,
            kind: "dropped".to_owned(),
            middleware: None,
            extensions: None,
        }]
    );
}

/// A counting before-Middleware (and its counter).
fn counting() -> (BeforeMiddleware, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    let mw = Middleware::before(move |_: BeforeCtx| {
        c.fetch_add(1, Ordering::SeqCst);
        async { Ok(Flow::Continue) }
    });
    (mw, calls)
}

/// Matrix 34: nothing runs for autocomplete, the Expired Handler chain, an
/// unknown Route or the detached seam (which has no Extensions).
#[tokio::test]
async fn middleware_not_run() {
    let (mw, calls) = counting();
    let buy = ComponentHandler::__new::<TBuy, _, _>(
        "m::Buy",
        "shop.buy",
        ComponentKind::Button,
        |_: TBuy, ctx: ComponentCtx| async move {
            let _ = __ext::<A, _>(&ctx, "m::Buy")?;
            Ok(Responded::new())
        },
    );
    let module = Module::new("m")
        .before(mw)
        .__add(autocomplete::config())
        .__add(buy.clone());
    let (p, sink) = start(module, |_| {}).await;
    autocomplete::run(&p, autocomplete::ac("err", "x")).await;
    let seen = sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].origin, ErrorOrigin::Autocomplete);
    assert_eq!(
        (seen[0].extensions.clone(), seen[0].middleware.clone()),
        (None, None)
    );
    // A stale id (another schema) and an unknown Route.
    for custom_id in ["0shop.buy:47:9", "0nope:1"] {
        run_component_raw(&p, &click(custom_id, 2)).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let err = crate::__testing::call_component(&buy, TBuy { item: 1 })
        .await
        .unwrap_err();
    assert!(
        err.as_error()
            .downcast_ref::<crate::ExtensionMissing>()
            .is_some(),
        "{err}"
    );
    // A real click runs it once.
    run_component_raw(&p, &click("0shop.buy:47", 2)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let _ = ModalSubmitCtx::detached();
}

async fn run_component_raw(p: &Pipeline, json: &Value) {
    component::run_component(
        Arc::clone(p.fx.app.registry()),
        Arc::clone(p.fx.app.shared()),
        inbound(json),
        CancellationToken::new(),
    )
    .await;
}

/// Matrix 21 (component) and 25 (component, `response = update`): a stop
/// after the type-6 auto-defer follows up and deletes nothing.
#[tokio::test]
async fn component_stop_after_defer_update() {
    let log = Log::default();
    let module = Module::new("m")
        .before(Middleware::before(|_: BeforeCtx| async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            Ok(Flow::Stop(Some(keys::ERROR_GENERIC)))
        }))
        .after(after(&log, "a"))
        .__add(ComponentHandler::__new::<TBuy, _, _>(
            "m::Buy",
            "shop.buy",
            ComponentKind::Button,
            |_: TBuy, _: ComponentCtx| async { Ok(Responded::new()) },
        ));
    let (p, _) = start(module, |b| {
        b.ack_deadline(Duration::from_millis(20));
    })
    .await;
    let mut json = click("0shop.buy:47", 2);
    json["message"] = json!({ "id": "8", "channel_id": "5", "flags": 32768 });
    let raw = inbound_raw(&click("0shop.buy:47", 2), &json);
    component::run_component(
        Arc::clone(p.fx.app.registry()),
        Arc::clone(p.fx.app.shared()),
        raw,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(log.lines(), ["a:Stopped"]);
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].2, json!({ "type": 6 }));
    assert_eq!(requests[1].1, FOLLOWUP);
    assert!(requests.iter().all(|r| r.0 != Method::DELETE));
}

/// Matrix 27 (component): a before error on a click is the generic notice
/// and a Component report naming the Middleware.
#[tokio::test]
async fn component_before_error() {
    let module = Module::new("m")
        .before(Middleware::before(|_: BeforeCtx| async {
            Err::<Flow, _>(HandlerError::new(std::io::Error::other("no")))
        }))
        .__add(ComponentHandler::__new::<TBuy, _, _>(
            "m::Buy",
            "shop.buy",
            ComponentKind::Button,
            |_: TBuy, _: ComponentCtx| async { Ok(Responded::new()) },
        ));
    let (p, sink) = start(module, |_| {}).await;
    run_component_raw(&p, &click("0shop.buy:47", 2)).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_generic_notice(&requests[0]);
    let seen = sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].origin, ErrorOrigin::Component);
    assert!(seen[0].middleware.is_some());
}

/// Matrix 32 and 33 (core): event Middleware run in order around a
/// handler, a stop sends nothing, and a filtered event runs none.
#[tokio::test]
async fn events_run_middleware_unless_filtered() {
    let log = Log::default();
    let handled = log.clone();
    let module = Module::new("root")
        .before(pass(&log, "r1"))
        .after(after(&log, "a1"))
        .module(
            Module::new("root/child")
                .before(pass(&log, "c1"))
                .after(after(&log, "ca1"))
                .__add(crate::test_support::handler::<
                    twilight_model::gateway::payload::incoming::MessageDelete,
                    _,
                    _,
                >("m::ev", false, move |_, _| {
                    let handled = handled.clone();
                    async move {
                        handled.push("handler");
                        Ok(())
                    }
                })),
        );
    let (p, sink) = start(module, |_| {}).await;
    let entry = Arc::clone(
        &p.fx
            .app
            .registry()
            .handlers(EventType::MessageDelete)
            .unwrap()
            .concurrent[0],
    );
    let cancel = CancellationToken::new();
    let app = Arc::clone(p.fx.app.shared());
    crate::worker::run_handler(&entry, 0, None, message_delete(1, None), &app, &cancel).await;
    assert_eq!(
        log.lines(),
        ["r1", "c1", "handler", "ca1:Completed", "a1:Completed"]
    );
    log.0.lock().unwrap().clear();
    // Filtered: another payload type.
    let create = Event::GatewayHeartbeatAck;
    crate::worker::run_handler(&entry, 0, None, create, &app, &cancel).await;
    assert!(log.lines().is_empty(), "{:?}", log.lines());
    assert!(sink.seen().is_empty());
    assert!(p.requests().is_empty());
}

/// The event `MESSAGE_DELETE` handler entry of `p`.
fn delete_entry(p: &Pipeline) -> Arc<crate::registry::Entry> {
    Arc::clone(
        &p.fx
            .app
            .registry()
            .handlers(EventType::MessageDelete)
            .unwrap()
            .concurrent[0],
    )
}

/// Matrix 30 (event): an after-Middleware panic is one Event-origin
/// `Panic` report naming it, and the remaining after-Middleware still run.
#[tokio::test]
async fn event_after_panic_is_reported() {
    let log = Log::default();
    let a1 = Middleware::after(|_: AfterCtx| async { panic!("a1 boom") });
    let a1_name = a1.name();
    let module = Module::new("root")
        .after(after(&log, "a0"))
        .after(a1)
        .module(Module::new("root/child").after(after(&log, "ca1")).__add(
            crate::test_support::handler::<
                twilight_model::gateway::payload::incoming::MessageDelete,
                _,
                _,
            >("m::ev", false, |_, _| async { Ok(()) }),
        ));
    let (p, sink) = start(module, |_| {}).await;
    let app = Arc::clone(p.fx.app.shared());
    let cancel = CancellationToken::new();
    crate::worker::run_handler(
        &delete_entry(&p),
        0,
        None,
        message_delete(1, None),
        &app,
        &cancel,
    )
    .await;
    assert_eq!(log.lines(), ["ca1:Completed", "a0:Completed"]);
    assert_eq!(
        sink.seen(),
        [MwSeen {
            origin: ErrorOrigin::Event,
            kind: "panic:a1 boom".to_owned(),
            middleware: Some(a1_name.to_owned()),
            extensions: Some(vec![]),
        }]
    );
}

/// An after-Middleware counting its calls (and its counter).
fn counting_after() -> (AfterMiddleware, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&calls);
    let mw = Middleware::after(move |_: AfterCtx| {
        c.fetch_add(1, Ordering::SeqCst);
        async {}
    });
    (mw, calls)
}

/// Run semantics 1: a start panic (the handler call itself panics) runs
/// no after-Middleware, over the Command and the event path; the panic is
/// still reported.
#[tokio::test]
async fn start_panic_runs_no_after() {
    let (before_mw, before_calls) = counting();
    let (after_mw, after_calls) = counting_after();
    let module = Module::new("m")
        .before(before_mw)
        .after(after_mw)
        .__add(CommandHandler::__new_plain(
            "m::starts",
            &["starts"],
            "d",
            |_: CommandCtx, _: CommandResponder| -> std::future::Ready<HandlerResult<V2Message>> {
                panic!("at the start")
            },
        ))
        .__add(crate::test_support::handler::<
            twilight_model::gateway::payload::incoming::MessageDelete,
            _,
            _,
        >(
            "m::ev",
            false,
            |_, _| -> std::future::Ready<HandlerResult<()>> { panic!("at the event start") },
        ));
    let (p, sink) = start(module, |_| {}).await;
    p.run(slash_run("starts")).await;
    let app = Arc::clone(p.fx.app.shared());
    let cancel = CancellationToken::new();
    crate::worker::run_handler(
        &delete_entry(&p),
        0,
        None,
        message_delete(1, None),
        &app,
        &cancel,
    )
    .await;
    assert_eq!(before_calls.load(Ordering::SeqCst), 0);
    assert_eq!(after_calls.load(Ordering::SeqCst), 0);
    let kinds: Vec<_> = sink
        .seen()
        .into_iter()
        .map(|s| (s.origin, s.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (ErrorOrigin::Command, "panic:at the start".to_owned()),
            (ErrorOrigin::Event, "panic:at the event start".to_owned()),
        ]
    );
}

/// What a recording before-Middleware saw of its run: the invocation,
/// the guild, channel and user ids, and the shard.
type Info = (Invocation, Option<u64>, Option<u64>, Option<u64>, u32);

/// A before-Middleware recording [`Info`] and inserting `A(9)` (and the
/// records).
fn recording() -> (BeforeMiddleware, Arc<Mutex<Vec<Info>>>) {
    let infos: Arc<Mutex<Vec<Info>>> = Arc::default();
    let i = Arc::clone(&infos);
    let mw = Middleware::before(move |ctx: BeforeCtx| {
        i.lock().unwrap().push((
            ctx.invocation(),
            ctx.guild_id().map(Id::get),
            ctx.channel_id().map(Id::get),
            ctx.user_id().map(Id::get),
            ctx.shard(),
        ));
        ctx.insert(A(9));
        async { Ok(Flow::Continue) }
    })
    .provides::<A>();
    (mw, infos)
}

/// Sets guild 4 and channel 5 on the interaction `json`.
fn in_guild(mut json: Value) -> Value {
    json["guild_id"] = json!("4");
    json["channel"] = json!({ "id": "5", "type": 0 });
    json
}

/// The run metadata a before-Middleware reads, from a guild slash Command,
/// a click, a modal submit, a user context menu (whose handler reads the
/// inserted `Ext<A>`) and an event.
#[tokio::test]
async fn before_ctx_run_metadata() {
    let (mw, infos) = recording();
    let log = Log::default();
    let menu = {
        let log = log.clone();
        ContextMenuHandler::__new_user(
            "m::show",
            "Show",
            CommandMetadata::new(),
            move |ctx: CommandCtx, _: ResolvedUser, _: CommandResponder| {
                let log = log.clone();
                async move {
                    let a = __ext::<A, _>(&ctx, "m::show")?;
                    log.push(format!("menu {a:?}"));
                    Ok(v2("u"))
                }
            },
        )
    };
    let module = Module::new("m")
        .before(mw)
        .__add(handler(&log, "m::probe", &["probe"]))
        .__add(menu)
        .__add(ComponentHandler::__new::<TBuy, _, _>(
            "m::Buy",
            "shop.buy",
            ComponentKind::Button,
            |_: TBuy, _: ComponentCtx| async { Ok(Responded::new()) },
        ))
        .__add(crate::test_support::handler::<
            twilight_model::gateway::payload::incoming::MessageDelete,
            _,
            _,
        >("m::ev", false, |_, _| async { Ok(()) }));
    let (p, sink) = start(module, |_| {}).await;
    p.run(inbound(&in_guild(interaction(
        slash("probe", json!([])),
        json!({}),
    ))))
    .await;
    run_component_raw(&p, &in_guild(click("0shop.buy:47", 2))).await;
    let data = json!({
        "id": "9", "name": "Show", "type": 2, "target_id": "11",
        "resolved": { "users": { "11": user("11") } }
    });
    p.run(inbound(&in_guild(interaction(data, json!({})))))
        .await;
    let app = Arc::clone(p.fx.app.shared());
    let cancel = CancellationToken::new();
    crate::worker::run_handler(
        &delete_entry(&p),
        3,
        Some(Id::new(4)),
        message_delete(1, Some(4)),
        &app,
        &cancel,
    )
    .await;

    let (modal_mw, modal_infos) = recording();
    let modal = Module::new("m")
        .before(modal_mw)
        .__add(ModalHandler::__new::<TBuy, _, _>(
            "m::Sub",
            "shop.buy",
            |_: TBuy, _: ModalSubmitCtx| async { Ok(Responded::new()) },
        ));
    let (mp, modal_sink) = start(modal, |_| {}).await;
    run_component_raw(&mp, &in_guild(submit("0shop.buy:47", true))).await;

    assert_eq!(log.lines(), ["handler", "menu Ext(A(9))"]);
    let guild = (Some(4), Some(5), Some(3), 0);
    let with = |invocation| (invocation, guild.0, guild.1, guild.2, guild.3);
    assert_eq!(
        *infos.lock().unwrap(),
        [
            with(Invocation::Command { path: "probe" }),
            with(Invocation::Component { route: "shop.buy" }),
            with(Invocation::ContextMenu { name: "Show" }),
            (
                Invocation::Event {
                    event_type: "MESSAGE_DELETE"
                },
                Some(4),
                None,
                None,
                3
            ),
        ]
    );
    assert_eq!(
        *modal_infos.lock().unwrap(),
        [with(Invocation::Modal { route: "shop.buy" })]
    );
    assert!(sink.seen().is_empty(), "{:?}", sink.seen());
    assert!(modal_sink.seen().is_empty(), "{:?}", modal_sink.seen());
}
