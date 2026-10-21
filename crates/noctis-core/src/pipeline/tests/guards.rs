//! Route Guard Matrix 23, 27-29, 32, 33, 35-37: stage 8 of
//! the component pipeline over a recording upstream, with hand-written
//! guarded Routes (what the derive emits).

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_codec::{RouteRuntime, RouteShape};
use twilight_model::id::Id;

use super::components::click;
use super::*;
use crate::guard::{__Denial, __check_custom, __check_owner, __check_permissions};
use crate::pipeline::component::run_component;
use crate::test_support::{TestRefs, test_route};
use crate::{
    Access, AfterCtx, BeforeCtx, ComponentCtx, ComponentHandler, ComponentKind, Flow, Middleware,
    Permissions, Responded,
};

/// `noctis.guard.owner` in en-US.
const OWNER: &str = "Only the person who started this can use it.";
/// `noctis.guard.permissions` in en-US.
const PERMISSIONS: &str = "You don't have permission to use this.";
/// `noctis.guard.failed` in en-US.
const FAILED: &str = "You can't use this.";
/// `noctis.expired` in en-US.
const EXPIRED: &str = "This interaction has expired. Run the command again.";
/// `noctis.stale` in en-US (the user-error key of the custom Guard).
const STALE: &str = "This message is out of date. Run the command again.";

/// The Ref every test record is stored under.
const REF: &str = "0123456789A";

/// A guarded hand-written Route `$name` whose Guard body is `$body`, with
/// `this` (the struct) and `ctx` in scope.
macro_rules! guarded {
    ($name:ident, $id:literal, $shape:expr, $runtime:expr, |$this:ident, $ctx:ident| $body:expr) => {
        test_route!(@unguarded $name, $id, $shape, 0, $runtime);

        impl crate::__Guards<ComponentCtx> for $name {
            const GUARDED: bool = true;

            fn __guard<'a>(
                &'a self,
                ctx: &'a ComponentCtx,
            ) -> impl Future<Output = HandlerResult<Option<__Denial>>> + Send + 'a {
                let ($this, $ctx) = (self, ctx);
                async move { $body }
            }
        }
    };
}

/// Calls of the custom Guard of [`Ordered`].
static ORDER_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Calls of the custom Guard of [`Counted`].
static COUNT_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Calls of the custom Guard of [`Unreached`] and [`UnreachedRef`].
static UNREACHED_CALLS: AtomicUsize = AtomicUsize::new(0);

/// A custom Guard that counts its call in `calls` and allows.
#[expect(clippy::unused_async, reason = "a custom Guard is an async fn")]
async fn count(calls: &AtomicUsize) -> HandlerResult<Access> {
    calls.fetch_add(1, Ordering::SeqCst);
    Ok(Access::Allow)
}

// `owner = item`, `permissions = MANAGE_GUILD`, `guard = t::count`.
guarded!(
    Ordered,
    "g.order",
    RouteShape::INLINE,
    RouteRuntime::EMPTY,
    |this, ctx| {
        if let Some(d) = __check_owner(ctx.user_id(), Id::new(this.item)) {
            return Ok(Some(d));
        }
        if let Some(d) = __check_permissions(ctx.member_permissions(), Permissions::MANAGE_GUILD) {
            return Ok(Some(d));
        }
        __check_custom("t::count", count(&ORDER_CALLS).await)
    }
);

// `guard = t::custom`: its answer depends on `item`.
guarded!(
    Custom,
    "g.custom",
    RouteShape::INLINE,
    RouteRuntime::EMPTY,
    |this, _ctx| {
        let result = match this.item {
            0 => Ok(Access::Allow),
            1 => Err(HandlerError::user(keys::STALE)),
            2 => Err(HandlerError::new(std::io::Error::other("guard db down"))),
            3 => panic!("guard panicked"),
            _ => Ok(Access::deny()),
        };
        __check_custom("t::custom", result)
    }
);

// `guard = t::count`, counting into `COUNT_CALLS`.
guarded!(
    Counted,
    "g.count",
    RouteShape::INLINE,
    RouteRuntime::EMPTY,
    |_this, _ctx| __check_custom("t::count", count(&COUNT_CALLS).await)
);

// Guards that must never run: a stale id and a missing Ref skip them.
guarded!(
    Unreached,
    "g.unreached",
    RouteShape::INLINE,
    RouteRuntime::EMPTY,
    |_this, _ctx| __check_custom("t::count", count(&UNREACHED_CALLS).await)
);
guarded!(
    UnreachedRef,
    "g.unref",
    RouteShape::BY_REF,
    RouteRuntime::EMPTY,
    |_this, _ctx| __check_custom("t::count", count(&UNREACHED_CALLS).await)
);

// `by_ref, one_shot, owner = item`.
guarded!(
    Once,
    "g.once",
    RouteShape::BY_REF,
    RouteRuntime::EMPTY.one_shot(),
    |this, ctx| Ok(__check_owner(ctx.user_id(), Id::new(this.item)))
);
// The same with `ttl = "15m"`.
guarded!(
    OnceTtl,
    "g.oncettl",
    RouteShape::BY_REF,
    RouteRuntime::EMPTY.one_shot().with_ttl_secs(900),
    |this, ctx| Ok(__check_owner(ctx.user_id(), Id::new(this.item)))
);

test_route!(Plain, "g.plain", RouteShape::INLINE, 0);

/// A handler of `S` logging `name:item`.
fn handler<S>(log: &Log, name: &'static str) -> ComponentHandler
where
    S: noctis_codec::Route<Payload = S>
        + noctis_codec::DeclaredRuntime
        + crate::__Guards<ComponentCtx>
        + for<'de> noctis_codec::PayloadDecode<'de>
        + Item
        + Send
        + 'static,
{
    let log = log.clone();
    ComponentHandler::__new::<S, _, _>(
        name,
        <S as noctis_codec::Route>::ID.as_str(),
        ComponentKind::Button,
        move |value: S, _: ComponentCtx| {
            log.push(format!("{name}:{}", value.item()));
            async { Ok(Responded::new()) }
        },
    )
}

/// The `item` field of a test Route.
trait Item {
    fn item(&self) -> u64;
}

macro_rules! item {
    ($($t:ty),*) => {
        $(impl Item for $t {
            fn item(&self) -> u64 {
                self.item
            }
        })*
    };
}

item!(
    Ordered,
    Custom,
    Counted,
    Unreached,
    UnreachedRef,
    Once,
    OnceTtl
);

/// An after-Middleware logging `after:<outcome>:<whether Extension A is
/// present>`.
fn after(log: &Log) -> crate::AfterMiddleware {
    let log = log.clone();
    Middleware::after(move |ctx: AfterCtx| {
        log.push(format!(
            "after:{:?}:{}",
            ctx.outcome(),
            ctx.get::<A>().is_some()
        ));
        async {}
    })
}

#[derive(Debug)]
struct A;

/// The test App: every guarded Route in Module `g` (with a before
/// inserting `A` and an after), plus `Counted` behind a stopping before in
/// Module `stop`.
fn module(log: &Log) -> Module {
    Module::new("root")
        .module(
            Module::new("g")
                .before(
                    Middleware::before(|ctx: BeforeCtx| async move {
                        ctx.insert(A);
                        Ok(Flow::Continue)
                    })
                    .provides::<A>(),
                )
                .after(after(log))
                .__add(handler::<Ordered>(log, "g::Ordered"))
                .__add(handler::<Custom>(log, "g::Custom"))
                .__add(handler::<Unreached>(log, "g::Unreached"))
                .__add(handler::<UnreachedRef>(log, "g::UnreachedRef"))
                .__add(handler::<Once>(log, "g::Once"))
                .__add(handler::<OnceTtl>(log, "g::OnceTtl")),
        )
        .module(
            Module::new("stop")
                .before(Middleware::before(|_: BeforeCtx| async {
                    Ok(Flow::Stop(Some(keys::GUARD_FAILED)))
                }))
                .__add(handler::<Counted>(log, "stop::Counted")),
        )
}

struct Fx {
    p: Pipeline,
    refs: Arc<TestRefs>,
    sink: Arc<Recording>,
    log: Log,
}

impl Fx {
    async fn start() -> Self {
        let log = Log::default();
        let refs = TestRefs::new();
        let sink = Arc::new(Recording::default());
        let (store, component_sink) = (Arc::clone(&refs), Arc::clone(&sink));
        let p = Pipeline::start_answering(module(&log), discord(), move |b| {
            b.error_sink(ErrorOrigin::Component, component_sink)
                .ref_store(store);
        })
        .await;
        Self { p, refs, sink, log }
    }

    /// Runs `raw`; returns the captured log text.
    async fn run(&self, raw: &Value) -> String {
        let capture = Capture::default();
        {
            let _guard = tracing::subscriber::set_default(capture.subscriber());
            tracing::callsite::rebuild_interest_cache();
            run_component(
                Arc::clone(self.p.fx.app.registry()),
                Arc::clone(self.p.fx.app.shared()),
                inbound(raw),
                CancellationToken::new(),
            )
            .await;
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

/// A click on `custom_id` by `user` in guild 9 with the member
/// `permissions`.
fn guild_click(custom_id: &str, user: &str, permissions: &str) -> Value {
    let mut d = click(custom_id, 2);
    let object = d.as_object_mut().unwrap();
    object.remove("user");
    object.insert("guild_id".to_owned(), json!("9"));
    object.insert(
        "member".to_owned(),
        json!({ "user": super::user(user), "roles": [],
                "joined_at": "2024-01-01T00:00:00.000000+00:00", "deaf": false, "mute": false,
                "flags": 0, "permissions": permissions }),
    );
    d
}

/// A DM click on `custom_id` by `user`.
fn dm_click(custom_id: &str, user: &str) -> Value {
    let mut d = click(custom_id, 2);
    d["user"] = super::user(user);
    d
}

/// The sum of every series of the metric `name`.
fn metric(snapshotter: &Snapshotter, name: &str) -> usize {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.key().name() == name)
        .map(|(.., value)| match value {
            DebugValue::Counter(n) => usize::try_from(n).unwrap(),
            DebugValue::Histogram(v) => v.len(),
            DebugValue::Gauge(_) => 0,
        })
        .sum()
}

/// Matrix 16-23 (core): owner first, then permissions, then the custom
/// Guard, which runs only when both passed; a denial is one notice, one
/// `debug` naming the Guard (no user id), no duration, counter or report,
/// and the after-Middleware sees `Denied` with the Extensions (Matrix 33).
#[tokio::test]
async fn order_and_denials() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;

    // Matrix 23: item 42 (`g`), clicked by 3 in a DM: owner and
    // permissions both fail; the owner notice wins.
    let logs = fx.run(&dm_click("0g.order:g", "3")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_notice(&requests[0], OWNER);
    assert_eq!(ORDER_CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(fx.lines(), ["after:Denied:true"]);
    let denied: Vec<&str> = logs
        .lines()
        .filter(|l| l.contains("noctis::guard") && l.contains("denied"))
        .collect();
    assert_eq!(denied.len(), 1, "{logs}");
    for field in [
        "route=\"g.order\"",
        "handler=\"g::Ordered\"",
        "guard=\"owner\"",
        "key=\"noctis.guard.owner\"",
    ] {
        assert!(denied[0].contains(field), "{field}: {}", denied[0]);
    }
    assert!(!denied[0].contains("user"), "{}", denied[0]);

    // The owner in a DM: the permissions Guard denies (Matrix 22).
    fx.run(&dm_click("0g.order:3", "3")).await;
    assert_notice(&fx.take()[0], PERMISSIONS);
    // The owner without MANAGE_GUILD (Matrix 21).
    fx.run(&guild_click("0g.order:3", "3", "4")).await;
    assert_notice(&fx.take()[0], PERMISSIONS);
    assert_eq!(ORDER_CALLS.load(Ordering::SeqCst), 0);
    assert_eq!(fx.lines(), ["after:Denied:true", "after:Denied:true"]);
    assert_eq!(metric(&snapshotter, "noctis_handler_duration_seconds"), 0);
    assert_eq!(metric(&snapshotter, "noctis_handler_errors_total"), 0);

    // Every Guard passes (Matrix 16, 18, 19, 20): the handler runs.
    for permissions in ["32", "48", "8"] {
        fx.run(&guild_click("0g.order:3", "3", permissions)).await;
    }
    assert_eq!(ORDER_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(
        fx.lines(),
        [
            "g::Ordered:3",
            "after:Completed:true",
            "g::Ordered:3",
            "after:Completed:true",
            "g::Ordered:3",
            "after:Completed:true"
        ]
    );
    assert_eq!(metric(&snapshotter, "noctis_handler_duration_seconds"), 3);
    assert!(fx.sink.seen().is_empty());
}

/// Matrix 24, 25, 27-29: the custom Guard's allow, default denial, user
/// error, error and panic.
#[tokio::test]
async fn custom_guard_outcomes() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start().await;

    // Matrix 24: allow.
    fx.run(&dm_click("0g.custom:0", "3")).await;
    assert_eq!(fx.lines(), ["g::Custom:0", "after:Completed:true"]);
    fx.take();

    // Matrix 25: `Access::deny()`.
    let logs = fx.run(&dm_click("0g.custom:4", "3")).await;
    assert_notice(&fx.take()[0], FAILED);
    assert_eq!(fx.lines(), ["after:Denied:true"]);
    assert!(
        logs.contains("guard=\"t::custom\"") && logs.contains("key=\"noctis.guard.failed\""),
        "{logs}"
    );

    // Matrix 27: a user error is a handler user error.
    let logs = fx.run(&dm_click("0g.custom:1", "3")).await;
    assert_notice(&fx.take()[0], STALE);
    assert_eq!(fx.lines(), ["after:UserError:true"]);
    assert!(logs.contains("user condition"), "{logs}");
    assert!(fx.sink.seen().is_empty());
    assert_eq!(metric(&snapshotter, "noctis_handler_errors_total"), 0);

    // Matrix 28: an error is reported with the handler's name.
    fx.run(&dm_click("0g.custom:2", "3")).await;
    assert_notice(&fx.take()[0], GENERIC);
    assert_eq!(fx.lines(), ["after:Failed:true"]);
    let seen = fx.sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].kind, "error:guard db down");
    assert_eq!(seen[0].handler.as_deref(), Some("g::Custom"));
    assert_eq!(metric(&snapshotter, "noctis_handler_errors_total"), 1);

    // Matrix 29: a panic is caught.
    fx.run(&dm_click("0g.custom:3", "3")).await;
    assert_notice(&fx.take()[0], GENERIC);
    assert_eq!(fx.lines(), ["after:Panicked:true"]);
    let seen = fx.sink.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[1].kind, "panic:guard panicked");
}

/// Matrix 32: a before-Middleware stop wins; the Guards never run.
#[tokio::test]
async fn middleware_stop_runs_no_guard() {
    let fx = Fx::start().await;
    fx.run(&dm_click("0g.count:1", "3")).await;
    let requests = fx.take();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_notice(&requests[0], FAILED);
    assert_eq!(COUNT_CALLS.load(Ordering::SeqCst), 0);
    assert!(fx.lines().is_empty());
}

/// Matrix 35 (core): a denied `one_shot` Ref is put back with the App's
/// `ref_ttl` (or the Route's `ttl`), so the owner's click reaches the
/// handler.
#[tokio::test]
async fn one_shot_ref_is_restored() {
    let fx = Fx::start().await;
    let key = format!("default:ref:{REF}");
    // Item 42 (`g`): only user 42 may click.
    fx.refs.insert(&key, b"0g.once:g");
    let logs = fx.run(&dm_click(&format!("0g.once:~{REF}"), "3")).await;
    assert_notice(&fx.take()[0], OWNER);
    assert_eq!(fx.lines(), ["after:Denied:true"]);
    assert_eq!(fx.refs.value(&key).as_deref(), Some(&b"0g.once:g"[..]));
    let puts = fx.refs.calls_of("put_if_absent");
    assert_eq!(puts.len(), 1, "{puts:?}");
    assert_eq!(puts[0].ttl, Some(Duration::from_hours(24)));
    assert!(!logs.contains(REF), "{logs}");

    fx.run(&dm_click(&format!("0g.once:~{REF}"), "42")).await;
    assert_eq!(fx.lines(), ["g::Once:42", "after:Completed:true"]);
    assert_eq!(fx.refs.value(&key), None);
    assert_eq!(fx.refs.calls_of("put_if_absent").len(), 1);

    // The Route's own `ttl`.
    fx.refs.insert(&key, b"0g.oncettl:g");
    fx.run(&dm_click(&format!("0g.oncettl:~{REF}"), "3")).await;
    let puts = fx.refs.calls_of("put_if_absent");
    assert_eq!(puts[1].ttl, Some(Duration::from_mins(15)));
    assert!(fx.refs.value(&key).is_some());
}

/// Matrix 36: a failed or refused restore still sends the denial, with one
/// `warn` on `noctis::refs` that carries no Ref.
#[tokio::test]
async fn failed_restore_is_one_warning() {
    let fx = Fx::start().await;
    let key = format!("default:ref:{REF}");
    for answer in [Some(false), None] {
        fx.refs.insert(&key, b"0g.once:g");
        fx.refs.script_puts([answer]);
        let logs = fx.run(&dm_click(&format!("0g.once:~{REF}"), "3")).await;
        let requests = fx.take();
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_notice(&requests[0], OWNER);
        let warns: Vec<&str> = logs
            .lines()
            .filter(|l| {
                l.contains("noctis::refs")
                    && l.contains("could not restore the one-shot Ref after a Guard denial")
            })
            .collect();
        assert_eq!(warns.len(), 1, "{answer:?}: {logs}");
        assert!(warns[0].contains("WARN"), "{}", warns[0]);
        assert!(!logs.contains(REF), "{logs}");
        assert_eq!(fx.lines(), ["after:Denied:true"]);
    }
}

/// Matrix 37: a stale id and a missing Ref on a guarded Route run the
/// Expired Handler chain with no Guard; the detached seam runs the handler
/// without its Guards.
#[tokio::test]
async fn expired_chain_and_detached_seam_run_no_guard() {
    let fx = Fx::start().await;
    // Schema 1 on a schema-0 Route: stale.
    fx.run(&dm_click("1g.unreached:3", "3")).await;
    assert_notice(&fx.take()[0], EXPIRED);
    fx.run(&dm_click(&format!("0g.unref:~{REF}"), "3")).await;
    assert_notice(&fx.take()[0], EXPIRED);
    assert_eq!(UNREACHED_CALLS.load(Ordering::SeqCst), 0);
    assert!(fx.lines().is_empty());

    let log = Log::default();
    let h = handler::<Unreached>(&log, "g::Unreached");
    let _: Responded = crate::__testing::call_component(&h, Unreached { item: 5 })
        .await
        .unwrap();
    assert_eq!(log.lines(), ["g::Unreached:5"]);
    assert_eq!(UNREACHED_CALLS.load(Ordering::SeqCst), 0);
}

/// Matrix 38 (core): `member_permissions()` is the member's resolved
/// permissions, `None` in a DM.
#[tokio::test]
async fn member_permissions_accessor() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = Arc::clone(&seen);
    let module = Module::new("m").__add(ComponentHandler::__new::<Plain, _, _>(
        "m::Seen",
        "g.plain",
        ComponentKind::Button,
        move |_: Plain, ctx: ComponentCtx| {
            s.lock().unwrap().push(ctx.member_permissions());
            async { Ok(Responded::new()) }
        },
    ));
    let p = Pipeline::start(module).await;
    let run = |raw: Value| {
        run_component(
            Arc::clone(p.fx.app.registry()),
            Arc::clone(p.fx.app.shared()),
            inbound(&raw),
            CancellationToken::new(),
        )
    };
    run(guild_click("0g.plain:1", "3", "32")).await;
    run(dm_click("0g.plain:1", "3")).await;
    assert_eq!(
        *seen.lock().unwrap(),
        [Some(Permissions::MANAGE_GUILD), None]
    );
}
