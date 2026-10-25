//! Story 13.5: the Lifecycle Hook engine with hand-built descriptors (R1-R7,
//! R15-R18, R22-R25).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use twilight_model::id::Id;

use super::sealed::Sealed as _;
use super::*;
use crate::test_support::{Capture, Recording};
use crate::{App, AppBuilder, HandlerError, Module, Plugin, Runtime};

/// What the hooks of a test saw, in order.
#[derive(Default)]
struct Seq(Mutex<Vec<String>>);

impl Seq {
    fn push(&self, item: impl Into<String>) {
        self.0.lock().unwrap().push(item.into());
    }

    fn get(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// A hook of `E` that records `<label>` and returns `Ok`.
fn recording<E: HookEvent + fmt::Debug>(label: &'static str, seq: &Arc<Seq>) -> LifecycleHook {
    let seq = Arc::clone(seq);
    LifecycleHook::__new::<E, _, _>(label, move |ctx: HookCtx, event: E| {
        let seq = Arc::clone(&seq);
        async move {
            seq.push(format!("{label}@{}:{event:?}", ctx.module()));
            Ok(())
        }
    })
}

/// An App over `modules` whose `Hook` sink is `sink`.
fn app(modules: Vec<Module>, sink: &Arc<Recording>) -> App {
    app_with(modules, sink, |_| {})
}

fn app_with(
    modules: Vec<Module>,
    sink: &Arc<Recording>,
    configure: impl FnOnce(&mut AppBuilder),
) -> App {
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .error_sink(ErrorOrigin::Hook, Arc::clone(sink));
    for module in modules {
        builder.module(module);
    }
    configure(&mut builder);
    builder.build().unwrap()
}

#[test]
fn names_kinds_and_reasons() {
    let names = [
        AppStarted::NAME,
        ShardReady::NAME,
        ShardResumed::NAME,
        ShardDisconnected::NAME,
        ShardFatal::NAME,
        AllShardsReady::NAME,
        PublishCompleted::NAME,
        PublishFailed::NAME,
        ModuleToggled::NAME,
        ShutdownStarted::NAME,
    ];
    assert_eq!(
        names,
        [
            "APP_STARTED",
            "SHARD_READY",
            "SHARD_RESUMED",
            "SHARD_DISCONNECTED",
            "SHARD_FATAL",
            "ALL_SHARDS_READY",
            "PUBLISH_COMPLETED",
            "PUBLISH_FAILED",
            "MODULE_TOGGLED",
            "SHUTDOWN_STARTED",
        ]
    );
    let fired = ShardFatal {
        shard: 2,
        close_code: Some(4014),
    }
    .into_fired();
    assert_eq!(fired.kind(), HookKind::ShardFatal);
    assert_eq!(fired.name(), "SHARD_FATAL");
    assert_eq!(fired.shard(), Some(2));
    assert_eq!(
        AppStarted { publisher: true }.into_fired().shard(),
        None,
        "only shard events carry a shard"
    );
    for (reason, text) in [
        (ShutdownReason::Signal, "signal"),
        (ShutdownReason::Requested, "requested"),
        (ShutdownReason::AllShardsEnded, "all shards ended"),
        (ShutdownReason::StartupFailed, "startup failed"),
    ] {
        assert_eq!(reason.as_str(), text);
        assert_eq!(reason.to_string(), text);
    }
    // R12: `Closed` maps to no hook event.
    assert!(Fired::from_shard(&ShardEvent::Closed { shard: 0 }).is_none());
    assert_eq!(
        Fired::from_shard(&ShardEvent::Disconnected {
            shard: 1,
            close_code: None
        }),
        Some(Fired::ShardDisconnected(ShardDisconnected {
            shard: 1,
            close_code: None
        }))
    );
}

#[test]
fn descriptor_debug() {
    let seq = Arc::new(Seq::default());
    let hook = recording::<AppStarted>("bot::Up", &seq);
    let debug = format!("{hook:?}");
    assert!(
        debug.contains("bot::Up") && debug.contains("APP_STARTED"),
        "{debug}"
    );
    let module = Module::new("ops").__add(hook);
    assert!(format!("{module:?}").contains("hooks: [\"bot::Up\"]"));
}

/// R1: without hooks nothing is queued or built, and no runner starts.
#[tokio::test(flavor = "multi_thread")]
async fn no_hooks_no_work() {
    let sink = Arc::new(Recording::default());
    let app = app(vec![Module::new("ops")], &sink);
    assert!(app.shared().lifecycle.hooks().is_empty());
    let run = HookRun::start(app.shared());
    assert!(run.task.is_none());
    run.app_started(false);
    app.shared()
        .lifecycle
        .emit::<ShardReady>(|| unreachable!("no hook wants it"));
    fire::<AllShardsReady>(app.shared(), || unreachable!("no hook wants it")).await;
    run.finish(Duration::ZERO).await;
}

/// R2, R4-R7: fold order, errors and panics (on poll and at start) reach
/// the `Hook` sink, later hooks still run.
#[tokio::test(flavor = "multi_thread")]
async fn order_errors_and_panics() {
    struct Extra;
    impl Plugin for Extra {
        fn install(&self, app: &mut AppBuilder) {
            app.module(
                Module::new("plugin").__add(LifecycleHook::__new::<ShardReady, _, _>(
                    "p::Last",
                    |ctx: HookCtx, _: ShardReady| async move {
                        let seq = ctx.state::<Arc<Seq>>().unwrap();
                        seq.push("plugin");
                        Ok(())
                    },
                )),
            );
        }
    }
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let at_start = Arc::new(AtomicBool::new(true));
    let fails =
        LifecycleHook::__new::<ShardReady, _, _>("t::Fails", |ctx: HookCtx, _| async move {
            ctx.state::<Arc<Seq>>().unwrap().push("fails");
            Err(HandlerError::new(std::io::Error::other("disk full")))
        });
    let panics =
        LifecycleHook::__new::<ShardReady, _, _>("t::Panics", |ctx: HookCtx, _| async move {
            ctx.state::<Arc<Seq>>().unwrap().push("panics");
            tokio::task::yield_now().await;
            panic!("boom");
        });
    let flag = Arc::clone(&at_start);
    let start_panic = LifecycleHook::__new::<ShardReady, _, _>("t::AtStart", move |_, _| {
        let fut = async { Ok(()) };
        assert!(!flag.load(Ordering::SeqCst), "at start");
        fut
    });
    let ops = Module::new("ops")
        .__add(fails)
        .__add(recording::<AppStarted>("t::Other", &seq))
        .module(Module::new("ops/alerts").__add(panics).__add(start_panic));
    let app = app_with(vec![ops], &sink, |b| {
        b.state(Arc::clone(&seq)).plugin(Extra);
    });
    assert_eq!(app.shared().lifecycle.hooks().len(), 5);
    fire(app.shared(), || ShardReady { shard: 3 }).await;
    assert_eq!(seq.get(), ["fails", "panics", "plugin"]);
    let seen = sink.seen();
    let kinds: Vec<_> = seen.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, ["error:disk full", "panic:boom", "panic:at start"]);
    for (report, (module, handler)) in seen.iter().zip([
        ("ops", "t::Fails"),
        ("ops/alerts", "t::Panics"),
        ("ops/alerts", "t::AtStart"),
    ]) {
        assert_eq!(report.origin, ErrorOrigin::Hook);
        assert_eq!(report.shard, Some(3));
        assert_eq!(report.guild_id, None);
        assert_eq!(report.module.as_deref(), Some(module));
        assert_eq!(report.handler.as_deref(), Some(handler));
        assert_eq!(report.event_type.as_deref(), Some("SHARD_READY"));
    }
}

/// R7: without a custom sink, `LogSink` logs on `noctis::hook`.
#[tokio::test(flavor = "current_thread")]
async fn default_sink_logs() {
    let capture = Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .module(
            Module::new("ops").__add(LifecycleHook::__new::<AppStarted, _, _>(
                "t::Up",
                |_, _| async { Err(HandlerError::new(std::io::Error::other("nope"))) },
            )),
        );
    let app = builder.build().unwrap();
    fire(app.shared(), || AppStarted { publisher: false }).await;
    let lines = capture.lines_with(&["noctis::hook", "hook handler failed", "APP_STARTED"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
}

/// R3: an event's hooks start after the previous event's returned, in
/// arrival order, on the run's queue.
#[tokio::test(flavor = "multi_thread")]
async fn queue_keeps_order() {
    let sink = Arc::new(Recording::default());
    let done = Arc::new(AtomicUsize::new(0));
    let seq = Arc::new(Seq::default());
    let slow = |name: &'static str| {
        let done = Arc::clone(&done);
        LifecycleHook::__new::<ShardReady, _, _>(name, move |_, ev: ShardReady| {
            let done = Arc::clone(&done);
            async move {
                tokio::time::sleep(Duration::from_millis(30)).await;
                assert!(ev.shard < 2);
                done.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
    };
    let check = {
        let done = Arc::clone(&done);
        let seq = Arc::clone(&seq);
        LifecycleHook::__new::<AllShardsReady, _, _>("t::All", move |_, ev| {
            let done = done.load(Ordering::SeqCst);
            let seq = Arc::clone(&seq);
            async move {
                seq.push(format!("all {:?}/{} after {done}", ev.shards, ev.total));
                Ok(())
            }
        })
    };
    let module = Module::new("ops")
        .__add(slow("t::A"))
        .__add(slow("t::B"))
        .__add(check);
    let app = app(vec![module], &sink);
    let run = HookRun::start(app.shared());
    let lifecycle = &app.shared().lifecycle;
    lifecycle.shard(&ShardEvent::Ready { shard: 0 });
    lifecycle.shard(&ShardEvent::Ready { shard: 1 });
    lifecycle.emit(|| AllShardsReady {
        shards: 0..2,
        total: 2,
    });
    run.finish(Duration::from_secs(5)).await;
    assert_eq!(seq.get(), ["all 0..2/2 after 4"]);
    assert!(sink.seen().is_empty());
}

/// R8/R20-R21 at unit level: `ShutdownStarted` fires once and only after
/// `AppStarted`.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_pairs_with_start() {
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let module = Module::new("ops")
        .__add(recording::<AppStarted>("up", &seq))
        .__add(recording::<ShutdownStarted>("down", &seq));
    let app = app(vec![module], &sink);
    let run = HookRun::start(app.shared());
    run.shutdown_started(ShutdownReason::Requested);
    run.app_started(true);
    run.app_started(true);
    run.shutdown_started(ShutdownReason::Signal);
    run.shutdown_started(ShutdownReason::StartupFailed);
    run.finish(Duration::ZERO).await;
    assert_eq!(
        seq.get(),
        [
            "up@ops:AppStarted { publisher: true }",
            "down@ops:ShutdownStarted { reason: Signal }",
        ]
    );
}

/// R22: a hook still running at the end is cut after at least 1 s, reported
/// once as `Cancelled`; the queued rest is discarded with one warning.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn slow_hook_is_cut() {
    let capture = Capture::default();
    let _logs = tracing::subscriber::set_default(capture.subscriber());
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let slow = LifecycleHook::__new::<ShutdownStarted, _, _>("t::Slow", |_, _| async {
        tokio::time::sleep(Duration::from_hours(1)).await;
        Ok(())
    });
    let module = Module::new("ops")
        .__add(recording::<AppStarted>("up", &seq))
        .__add(slow)
        .__add(recording::<ShardReady>("never", &seq));
    let app = app(vec![module], &sink);
    let run = HookRun::start(app.shared());
    run.app_started(false);
    run.shutdown_started(ShutdownReason::Requested);
    app.shared()
        .lifecycle
        .shard(&ShardEvent::Ready { shard: 0 });
    let started = Instant::now();
    run.finish(Duration::ZERO).await;
    let waited = started.elapsed();
    assert!(
        waited >= DRAIN_AT_LEAST && waited < Duration::from_secs(3),
        "{waited:?}"
    );
    assert_eq!(seq.get(), ["up@ops:AppStarted { publisher: false }"]);
    let seen = sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].kind, "cancelled");
    assert_eq!(seen[0].event_type.as_deref(), Some("SHUTDOWN_STARTED"));
    let warned = capture.lines_with(&["lifecycle hooks discarded at shutdown", "events=1"]);
    assert_eq!(warned.len(), 1, "{}", capture.text());
}

/// R22: after `ShutdownStarted` the end-of-run drain waits up to
/// `shutdown_grace`, past the 1 s floor, for a hook that finishes in time.
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn drain_waits_for_shutdown_grace() {
    let sink = Arc::new(Recording::default());
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let slow = LifecycleHook::__new::<ShutdownStarted, _, _>("t::Slow", move |_, _| {
        let flag = Arc::clone(&flag);
        async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            flag.store(true, Ordering::SeqCst);
            Ok(())
        }
    });
    let app = app(vec![Module::new("ops").__add(slow)], &sink);
    let run = HookRun::start(app.shared());
    run.app_started(false);
    run.shutdown_started(ShutdownReason::Requested);
    let started = Instant::now();
    run.finish(Duration::from_secs(5)).await;
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_secs(2) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
    assert!(done.load(Ordering::SeqCst));
    let seen = sink.seen();
    assert!(seen.is_empty(), "{seen:?}");
}

/// R23: dropping the run aborts the runner and detaches the queue.
#[tokio::test(flavor = "multi_thread")]
async fn dropped_run_aborts() {
    struct Flag(Arc<AtomicBool>);
    impl Drop for Flag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(tokio::sync::Notify::new());
    let hook = {
        let dropped = Arc::clone(&dropped);
        let entered = Arc::clone(&entered);
        LifecycleHook::__new::<AppStarted, _, _>("t::Hang", move |_, _| {
            let flag = Flag(Arc::clone(&dropped));
            let entered = Arc::clone(&entered);
            async move {
                let _flag = flag;
                entered.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            }
        })
    };
    let sink = Arc::new(Recording::default());
    let app = app(vec![Module::new("ops").__add(hook)], &sink);
    let run = HookRun::start(app.shared());
    run.app_started(false);
    entered.notified().await;
    drop(run);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !dropped.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(app.shared().lifecycle.queue.lock().unwrap().is_none());
    assert!(sink.seen().is_empty());
}

/// The hooks of a toggle test: `ModuleToggled` recorded, and toggling
/// `b` off from the toggle of `a`.
fn toggle_app(sink: &Arc<Recording>, seq: &Arc<Seq>) -> App {
    let nested =
        LifecycleHook::__new::<ModuleToggled, _, _>("t::Nested", |ctx: HookCtx, ev| async move {
            if ev.module == "a" {
                ctx.modules()
                    .set_enabled("b", ModuleScope::Global, false)
                    .await?;
            }
            Ok(())
        });
    let ops = Module::new("ops")
        .__add(recording::<ModuleToggled>("seen", seq))
        .__add(nested);
    app(vec![ops, Module::new("a"), Module::new("b")], sink)
}

/// R15-R18, R24, R25 outside a run: inline, nested, once per change.
#[tokio::test(flavor = "multi_thread")]
async fn toggles_inline() {
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let app = toggle_app(&sink, &seq);
    let modules = app.modules();
    let guild = ModuleScope::Guild(Id::new(7));
    modules.set_enabled("a", guild, false).await.unwrap();
    assert_eq!(
        seq.get(),
        [
            "seen@ops:ModuleToggled { module: \"a\", scope: Guild(Id<GuildMarker>(7)), enabled: false, reset: false }",
            "seen@ops:ModuleToggled { module: \"b\", scope: Global, enabled: false, reset: false }",
        ]
    );
    // R16: unchanged; R18: unknown id.
    modules.set_enabled("a", guild, false).await.unwrap();
    assert!(modules.set_enabled("zzz", guild, true).await.is_err());
    assert_eq!(seq.get().len(), 2);
    // R17: a reset that removes the override, then one with nothing to do.
    modules.reset("a", guild).await.unwrap();
    modules.reset("a", guild).await.unwrap();
    let all = seq.get();
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(all[2].contains("module: \"a\"") && all[2].contains("enabled: true, reset: true"));
    assert!(sink.seen().is_empty());
    // A detached handle fires nothing.
    assert!(
        crate::Modules::of(None)
            .set_enabled("a", guild, true)
            .await
            .is_err()
    );
}

/// R25 in a run: the nested toggle is queued after the running hooks.
#[tokio::test(flavor = "multi_thread")]
async fn toggles_queued_in_a_run() {
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let app = toggle_app(&sink, &seq);
    let run = HookRun::start(app.shared());
    app.modules()
        .set_enabled("a", ModuleScope::Global, false)
        .await
        .unwrap();
    run.finish(Duration::from_secs(5)).await;
    let all = seq.get();
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(all[0].contains("module: \"a\"") && all[1].contains("module: \"b\""));
    // After the run, hooks run inline again.
    app.modules()
        .set_enabled("a", ModuleScope::Global, true)
        .await
        .unwrap();
    assert_eq!(seq.get().len(), 3);
}

/// R24: a Publish outside a run fires its hook before it returns.
#[tokio::test(flavor = "multi_thread")]
async fn publish_without_token_fires_failed() {
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let app = app(
        vec![Module::new("ops").__add(recording::<PublishFailed>("failed", &seq))],
        &sink,
    );
    let result = app.publish(PublishScope::Global).dry_run(true).await;
    assert_eq!(result, Err(PublishError::MissingToken));
    assert_eq!(
        seq.get(),
        [
            "failed@ops:PublishFailed { scope: Global, dry_run: true, clear: false, error: MissingToken }"
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hook_ctx_handles() {
    let sink = Arc::new(Recording::default());
    let checked = Arc::new(AtomicBool::new(false));
    let hook = {
        let checked = Arc::clone(&checked);
        LifecycleHook::__new::<AppStarted, _, _>("t::Ctx", move |ctx: HookCtx, _| {
            let checked = Arc::clone(&checked);
            async move {
                assert_eq!(ctx.module(), "ops");
                assert_eq!(ctx.app_id(), "default");
                assert_eq!(*ctx.state::<u8>().unwrap(), 7);
                assert!(ctx.modules().is_enabled("ops", ModuleScope::Global));
                let debug = format!("{ctx:?}");
                assert!(
                    debug.contains("module: \"ops\"") && !debug.contains("http"),
                    "{debug}"
                );
                let _ = (
                    ctx.http(),
                    ctx.gateway(),
                    ctx.kv(),
                    ctx.cache(),
                    ctx.webhooks(),
                );
                checked.store(true, Ordering::SeqCst);
                Ok(())
            }
        })
    };
    let app = app_with(vec![Module::new("ops").__add(hook)], &sink, |b| {
        b.state(7_u8);
    });
    fire(app.shared(), || AppStarted { publisher: false }).await;
    assert!(checked.load(Ordering::SeqCst));
    assert!(sink.seen().is_empty());
}

/// A test extension event.
#[derive(Debug, Clone, PartialEq)]
struct Leveled(u32);

impl ExtensionEvent for Leveled {
    const NAME: &'static str = "TEST_LEVELED";
}

/// Another test extension event, with no hook.
#[derive(Debug, Clone)]
struct Unheard;

impl ExtensionEvent for Unheard {
    const NAME: &'static str = "TEST_UNHEARD";
}

#[test]
fn extension_events_are_hook_events() {
    assert_eq!(<Leveled as HookEvent>::NAME, "TEST_LEVELED");
    assert_eq!(Leveled::KIND, HookKind::Extension);
    let fired = Leveled(3).into_fired();
    assert_eq!(fired.kind(), HookKind::Extension);
    assert_eq!(fired.name(), "TEST_LEVELED");
    assert_eq!(fired, fired.clone());
    assert_ne!(fired, Leveled(3).into_fired());
    assert_eq!(Leveled::from_fired(&fired), Some(&Leveled(3)));
    assert!(Unheard::from_fired(&fired).is_none());
    assert!(Leveled::from_fired(&AppStarted { publisher: false }.into_fired()).is_none());
    assert!(format!("{fired:?}").contains("TEST_LEVELED"));
    let hook = recording::<Leveled>("t::Lvl", &Arc::new(Seq::default()));
    assert!(format!("{hook:?}").contains("TEST_LEVELED"));
}

/// Extension events: in emit order on the hook queue, with the hook's
/// Module; another extension type never reaches them; errors and panics
/// are reported with `event_type = NAME` and later events still run.
#[tokio::test(flavor = "multi_thread")]
async fn extension_events_run_in_order() {
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let failing = LifecycleHook::__new::<Leveled, _, _>("t::Fails", |_, e: Leveled| async move {
        match e.0 {
            2 => Err(HandlerError::new(std::io::Error::other("bad level"))),
            3 => panic!("level boom"),
            _ => Ok(()),
        }
    });
    let module = Module::new("ops")
        .__add(recording::<Leveled>("t::Lvl", &seq))
        .module(Module::new("ops/xp").__add(failing))
        .__add(recording::<AppStarted>("t::Up", &seq));
    let app = app(vec![module], &sink);
    assert_eq!(app.shared().lifecycle.hooks().len(), 3);
    let emitter = app.emitter();
    assert!(format!("{emitter:?}").contains("attached: true"));
    // Outside a run: dropped.
    emitter.emit(Leveled(0));
    let run = HookRun::start(app.shared());
    run.app_started(false);
    for level in 1..=4 {
        emitter.emit(Leveled(level));
        emitter.emit(Unheard);
    }
    run.finish(Duration::from_secs(5)).await;
    assert_eq!(
        seq.get(),
        [
            "t::Up@ops:AppStarted { publisher: false }",
            "t::Lvl@ops:Leveled(1)",
            "t::Lvl@ops:Leveled(2)",
            "t::Lvl@ops:Leveled(3)",
            "t::Lvl@ops:Leveled(4)",
        ]
    );
    let seen = sink.seen();
    let kinds: Vec<_> = seen.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, ["error:bad level", "panic:level boom"]);
    for report in &seen {
        assert_eq!(report.origin, ErrorOrigin::Hook);
        assert_eq!(report.module.as_deref(), Some("ops/xp"));
        assert_eq!(report.handler.as_deref(), Some("t::Fails"));
        assert_eq!(report.event_type.as_deref(), Some("TEST_LEVELED"));
    }
}

/// Without a hook for the type nothing is queued; an `Emitter` outliving
/// its App does nothing; `HookCtx::emitter` reaches the same queue.
#[tokio::test(flavor = "multi_thread")]
async fn extension_events_without_hooks_or_app() {
    let sink = Arc::new(Recording::default());
    let seq = Arc::new(Seq::default());
    let relay =
        LifecycleHook::__new::<AppStarted, _, _>("t::Relay", |ctx: HookCtx, _| async move {
            ctx.emitter().emit(Leveled(9));
            Ok(())
        });
    let module = Module::new("ops")
        .__add(relay)
        .__add(recording::<Leveled>("t::Lvl", &seq));
    let app = app(vec![module], &sink);
    assert!(
        !app.shared()
            .lifecycle
            .hooks()
            .wants_extension(TypeId::of::<Unheard>())
    );
    let run = HookRun::start(app.shared());
    run.app_started(false);
    // `finish` detaches the queue first: wait for the relayed event.
    tokio::time::timeout(Duration::from_secs(5), async {
        while seq.get().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    run.finish(Duration::from_secs(5)).await;
    assert_eq!(seq.get(), ["t::Lvl@ops:Leveled(9)"]);
    let emitter = app.emitter();
    drop(app);
    assert!(format!("{emitter:?}").contains("attached: false"));
    emitter.emit(Leveled(1));
    assert!(sink.seen().is_empty());
}
