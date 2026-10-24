//! Story 15.2: AD-23 stage 3, the App's `GuildAccessList`, for Commands,
//! autocomplete, components, modals and the Dispatcher.

use std::sync::atomic::{AtomicUsize, Ordering};

use metrics_util::debugging::DebuggingRecorder;
use noctis_codec::{RouteRuntime, RouteShape};
use noctis_ports::GuildAccessList;
use noctis_ports::memory::GuildAccess;

use super::autocomplete;
use super::components::click;
use super::*;
use crate::kind::__Response;
use crate::pipeline::component::run_component;
use crate::test_support::{TestRefs, test_route};
use crate::{
    AutoDefer, BeforeCtx, ComponentCtx, ComponentHandler, ComponentKind, ComponentResponder, Flow,
    LocaleRequest, Middleware,
};

test_route!(
    Once,
    "a.once",
    RouteShape::BY_REF,
    0,
    RouteRuntime::EMPTY.one_shot()
);
test_route!(Plain, "a.plain", RouteShape::INLINE, 0);

/// `noctis.module.disabled` in en-US.
const DISABLED: &str = "This feature is disabled here.";
/// The guild every refused test interaction comes from.
const DENIED: u64 = 9;
/// The Ref of the `one_shot` click.
const REF: &str = "0123456789A";

/// A list that panics on every guild.
struct Panicking;

impl GuildAccessList for Panicking {
    fn allows(&self, _: Id<GuildMarker>) -> bool {
        panic!("access list")
    }
}

/// A Module with a logging `/ping`, a logging before-Middleware and the
/// component handlers of `a.once` and `a.plain`.
fn module(log: &Log) -> Module {
    let (mw, once, plain) = (log.clone(), log.clone(), log.clone());
    Module::new("m")
        .before(Middleware::before(move |_: BeforeCtx| {
            mw.push("middleware");
            async { Ok(Flow::Continue) }
        }))
        .__add(logging(log, "m::ping", &["ping"]))
        .__add(ComponentHandler::__new_with_responder::<
            Once,
            AutoDefer,
            _,
            _,
            _,
        >(
            "m::Once",
            "a.once",
            ComponentKind::Button,
            __Response::Update,
            move |o: Once, _: ComponentCtx, _: ComponentResponder| {
                once.push(format!("once:{}", o.item));
                async { Ok(Responded::new()) }
            },
        ))
        .__add(ComponentHandler::__new_with_responder::<
            Plain,
            AutoDefer,
            _,
            _,
            _,
        >(
            "m::Plain",
            "a.plain",
            ComponentKind::Button,
            __Response::Update,
            move |p: Plain, _: ComponentCtx, _: ComponentResponder| {
                plain.push(format!("plain:{}", p.item));
                async { Ok(Responded::new()) }
            },
        ))
}

/// The App over `access` with a Ref store and a `LocaleResolver` that
/// counts its calls.
struct Fx {
    p: Pipeline,
    log: Log,
    refs: Arc<TestRefs>,
    resolver_calls: Arc<AtomicUsize>,
    component_sink: Arc<Recording>,
}

impl Fx {
    async fn start(access: impl GuildAccessList) -> Self {
        let log = Log::default();
        let refs = TestRefs::new();
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let component_sink = Arc::new(Recording::default());
        let (store, calls, sink) = (
            Arc::clone(&refs),
            Arc::clone(&resolver_calls),
            Arc::clone(&component_sink),
        );
        let p = Pipeline::start_with(module(&log), move |b| {
            b.guild_access(access)
                .ref_store(store)
                .error_sink(ErrorOrigin::Component, sink)
                .locale_resolver(move |r: &LocaleRequest| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    r.resolved
                });
        })
        .await;
        Self {
            p,
            log,
            refs,
            resolver_calls,
            component_sink,
        }
    }

    async fn component(&self, raw: &Value) {
        run_component(
            Arc::clone(self.p.fx.app.registry()),
            Arc::clone(self.p.fx.app.shared()),
            inbound(raw),
            CancellationToken::new(),
        )
        .await;
    }
}

/// `/ping` from `guild` (none: a DM).
fn ping(guild: Option<u64>) -> Inbound {
    let extra = guild.map_or_else(|| json!({}), |g| json!({ "guild_id": g.to_string() }));
    inbound(&interaction(slash("ping", json!([])), extra))
}

/// `raw` sent from `guild`.
fn in_guild(mut raw: Value, guild: u64) -> Value {
    raw["guild_id"] = json!(guild.to_string());
    raw
}

/// A denied command gets one ephemeral `noctis.module.disabled` notice and one
/// `debug`, and runs no Middleware, handler, resolver, report or duration
/// sample; a DM and an allowed guild are served.
#[tokio::test]
async fn denied_command_gets_the_notice() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _metrics = metrics::set_default_local_recorder(&recorder);
    let fx = Fx::start(GuildAccess::deny([Id::new(DENIED)])).await;
    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        fx.p.run(ping(Some(DENIED))).await;
    }
    let requests = fx.p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_notice(&requests[0], DISABLED);
    assert!(fx.log.lines().is_empty());
    assert!(fx.p.reports().is_empty());
    assert_eq!(fx.resolver_calls.load(Ordering::SeqCst), 0);
    let lines = capture.lines_with(&["noctis::dispatch", "guild not served"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(lines[0].contains("DEBUG"), "{}", lines[0]);
    assert!(
        lines[0].contains("handler=\"m::ping\"") && lines[0].contains("kind=\"command\""),
        "{}",
        lines[0]
    );
    assert!(!lines[0].contains("user"), "{}", lines[0]);
    let durations = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.key().name() == "noctis_handler_duration_seconds")
        .count();
    assert_eq!(durations, 0);

    // A DM is served: the list is never consulted for it.
    fx.p.run(ping(None)).await;
    // Another guild is served.
    fx.p.run(ping(Some(10))).await;
    assert_eq!(
        fx.log.lines(),
        ["m::ping:ping", "middleware", "m::ping:ping", "middleware"]
    );
    assert_eq!(fx.p.requests().len(), 3);
}

/// Allow lists: a hit is served, a miss and an empty list are refused,
/// a DM is served; runtime updates apply to the next interaction.
#[tokio::test]
async fn allow_list_and_runtime_updates() {
    let access = Arc::new(GuildAccess::allow([Id::new(1)]));
    let fx = Fx::start(Arc::clone(&access)).await;
    fx.p.run(ping(Some(1))).await;
    fx.p.run(ping(Some(2))).await;
    assert_eq!(fx.log.lines(), ["m::ping:ping", "middleware"]);
    assert!(access.insert(Id::new(2)));
    fx.p.run(ping(Some(2))).await;
    assert!(access.remove(Id::new(2)));
    assert!(!access.remove(Id::new(2)));
    fx.p.run(ping(Some(2))).await;
    let requests = fx.p.requests();
    assert_eq!(requests.len(), 4, "{requests:?}");
    assert_notice(&requests[1], DISABLED);
    assert_notice(&requests[3], DISABLED);
    assert_eq!(
        fx.log.lines(),
        ["m::ping:ping", "middleware", "m::ping:ping", "middleware"]
    );

    let empty = Fx::start(GuildAccess::allow([])).await;
    empty.p.run(ping(Some(1))).await;
    empty.p.run(ping(None)).await;
    assert_eq!(empty.log.lines(), ["m::ping:ping", "middleware"]);
    assert_notice(&empty.p.requests()[0], DISABLED);
}

/// A panicking list is caught: one `error`, the guild is refused (fail
/// closed) with the notice, and nothing is reported.
#[tokio::test]
async fn panicking_list_fails_closed() {
    let fx = Fx::start(Panicking).await;
    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        fx.p.run(ping(Some(1))).await;
    }
    let requests = fx.p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_notice(&requests[0], DISABLED);
    assert!(fx.log.lines().is_empty());
    assert!(fx.p.reports().is_empty());
    let errors = capture.lines_with(&["ERROR", "noctis::dispatch", "GuildAccessList panicked"]);
    assert_eq!(errors.len(), 1, "{}", capture.text());
    assert!(errors[0].contains("handler=\"m::ping\""), "{}", errors[0]);
    // A DM never asks the list.
    fx.p.run(ping(None)).await;
    assert_eq!(fx.log.lines(), ["m::ping:ping", "middleware"]);
}

/// A denied click on a `one_shot` Ref gets the notice and never takes
/// the Ref; a stale id from a denied guild gets the notice, not the
/// Expired Handler chain.
#[tokio::test]
async fn denied_component_keeps_its_ref() {
    let fx = Fx::start(GuildAccess::deny([Id::new(DENIED)])).await;
    let key = format!("default:ref:{REF}");
    fx.refs.insert(&key, b"0a.once:1");
    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        // Parallel tests may cache `Interest::never` for a callsite; rebuild it.
        tracing::callsite::rebuild_interest_cache();
        fx.component(&in_guild(click(&format!("0a.once:~{REF}"), 2), DENIED))
            .await;
    }
    assert!(fx.refs.calls_of("take").is_empty());
    assert!(fx.refs.calls_of("get").is_empty());
    assert!(fx.refs.value(&key).is_some());
    let lines = capture.lines_with(&["noctis::dispatch", "guild not served"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(
        lines[0].contains("route=\"a.once\"") && lines[0].contains("kind=\"component\""),
        "{}",
        lines[0]
    );
    // Stale (schema 1 of a schema-0 Route) from the denied guild.
    fx.component(&in_guild(click("1a.plain:147", 2), DENIED))
        .await;
    let requests = fx.p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    for request in &requests {
        assert_notice(request, DISABLED);
    }
    assert!(fx.log.lines().is_empty());
    assert!(fx.component_sink.seen().is_empty());
    assert_eq!(fx.resolver_calls.load(Ordering::SeqCst), 0);

    // From a served guild the same click takes the Ref and runs.
    fx.component(&in_guild(click(&format!("0a.once:~{REF}"), 2), 10))
        .await;
    assert_eq!(fx.refs.calls_of("take").len(), 1);
    assert_eq!(fx.log.lines(), ["once:1", "middleware"]);
}

/// A denied `run_command` is `GuildNotServed` and sends nothing; a DM
/// invocation and another guild run.
#[tokio::test]
async fn denied_dispatcher_is_an_error() {
    let fx = Fx::start(GuildAccess::deny([Id::new(DENIED)])).await;
    let dispatcher = fx.p.dispatcher();
    let invocation = || {
        CommandInvocation::new(
            CommandInput::new("ping"),
            Id::new(5),
            Id::new(6),
            Id::new(3),
        )
    };
    let err = dispatcher
        .run_command(invocation().guild_id(Some(Id::new(DENIED))))
        .await
        .unwrap_err();
    assert!(
        matches!(err, DispatchError::GuildNotServed { guild_id } if guild_id.get() == DENIED),
        "{err:?}"
    );
    assert_eq!(err.to_string(), "guild `9` is not served by this App");
    assert!(fx.p.requests().is_empty());
    assert!(fx.log.lines().is_empty());
    dispatcher.run_command(invocation()).await.unwrap();
    dispatcher
        .run_command(invocation().guild_id(Some(Id::new(10))))
        .await
        .unwrap();
    assert_eq!(fx.p.requests().len(), 2);
    assert_eq!(
        fx.log.lines(),
        ["m::ping:ping", "middleware", "m::ping:ping", "middleware"]
    );
}

/// A denied autocomplete gets one type-8 callback with no choices; its
/// method (which would suggest the guild id) never runs.
#[tokio::test]
async fn denied_autocomplete_is_empty() {
    let sink = Arc::new(Recording::default());
    let ac_sink = Arc::clone(&sink);
    let p = Pipeline::start_with(Module::new("m").__add(autocomplete::config()), move |b| {
        b.guild_access(GuildAccess::deny([Id::new(DENIED)]))
            .error_sink(ErrorOrigin::Autocomplete, ac_sink);
    })
    .await;
    let raw = |guild: u64| {
        let data = json!({
            "id": "9", "name": "config", "type": 1,
            "options": [{ "type": 2, "name": "set", "options": [
                { "type": 1, "name": "prefix", "options": [
                    { "type": 3, "name": "guild", "value": "x", "focused": true },
                    { "type": 4, "name": "limit", "value": 3 }
                ] }
            ]}]
        });
        interaction(data, json!({ "type": 4, "guild_id": guild.to_string() }))
    };
    autocomplete::run(&p, inbound(&raw(DENIED))).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].1, CB);
    assert_eq!(
        requests[0].2,
        json!({ "type": 8, "data": { "choices": [] } })
    );
    assert!(sink.seen().is_empty());
    // A served guild runs the method.
    autocomplete::run(&p, inbound(&raw(10))).await;
    let requests = p.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(
        requests[1].2["data"]["choices"][0]["value"], "10",
        "{requests:?}"
    );
}
