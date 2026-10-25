//! Lifecycle Hooks of a running App against `FakeDiscordHttp` and
//! `FakeGateway` (Story 13.5: R8, R10-R14, R20, R22 and the acceptance
//! criteria). Its own test binary, because it installs a global capturing
//! tracing subscriber.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::io;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use noctis::events::{EventCtx, MessageDelete};
use noctis::lifecycle::{
    AllShardsReady, AppStarted, HookCtx, ModuleToggled, PublishCompleted, PublishFailed,
    ShardDisconnected, ShardFatal, ShardReady, ShardResumed, ShutdownReason, ShutdownStarted,
};
use noctis::prelude::*;
use noctis::publish::PublishScope;
use noctis::twilight::model::id::Id;
use noctis::ui::{Components, TextDisplay, V2Message};
use noctis::{ErrorReport, ReportKind, Runtime, StartError, Token};
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, FakeGatewayConfig, FakeResponse, GatewayBot, IdentifyReply,
    Method, RecordedRequest, StatusCode,
};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

const APP: &str = "/applications/@me";
const GLOBAL: &str = "/applications/7/commands";

// ---- globals ----

/// Appends every log line to a shared buffer (keeps the test output
/// quiet).
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The process-wide capture, installed once.
fn capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing_subscriber::fmt()
            .with_max_level(LevelFilter::DEBUG)
            .with_ansi(false)
            .with_writer(capture.clone())
            .init();
        capture
    })
}

/// Serializes the tests: they share the log capture.
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    LOCK.lock().await
}

// ---- the hooks ----

/// What the hooks saw: `(NAME, Debug of the payload)`, in order.
#[derive(Default)]
struct Probe(Mutex<Vec<(&'static str, String)>>);

impl Probe {
    fn push(&self, name: &'static str, data: String) {
        self.0.lock().unwrap().push((name, data));
    }

    fn all(&self) -> Vec<(&'static str, String)> {
        self.0.lock().unwrap().clone()
    }

    fn names(&self) -> Vec<&'static str> {
        self.all().into_iter().map(|(name, _)| name).collect()
    }

    fn data(&self, name: &str) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, data)| data)
            .collect()
    }

    async fn wait_for(&self, name: &str) {
        let deadline = Instant::now() + WAIT;
        while !self.names().contains(&name) {
            assert!(Instant::now() < deadline, "no {name} in {:?}", self.all());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// One recording hook per event.
macro_rules! recorders {
    ($($hook:ident: $event:ident;)*) => {$(
        #[derive(Hook)]
        struct $hook;

        impl $hook {
            #[handler]
            async fn handle(self, ctx: HookCtx, ev: $event) -> HandlerResult<()> {
                let probe = ctx.state::<Arc<Probe>>().expect("registered");
                probe.push(<$event as noctis::lifecycle::HookEvent>::NAME, format!("{ev:?}"));
                Ok(())
            }
        }
    )*};
}

recorders! {
    OnStarted: AppStarted;
    OnPublished: PublishCompleted;
    OnPublishFailed: PublishFailed;
    OnReady: ShardReady;
    OnResumed: ShardResumed;
    OnDisconnected: ShardDisconnected;
    OnFatal: ShardFatal;
    OnAllReady: AllShardsReady;
    OnToggled: ModuleToggled;
    OnShutdown: ShutdownStarted;
}

/// A shard hook that fails.
#[derive(Hook)]
struct FailsOnReady;

impl FailsOnReady {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: ShardReady) -> HandlerResult<()> {
        Err(HandlerError::new(io::Error::other("hook broke")))
    }
}

/// A shard hook that panics.
#[derive(Hook)]
struct PanicsOnReady;

impl PanicsOnReady {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: ShardReady) -> HandlerResult<()> {
        tokio::task::yield_now().await;
        panic!("hook exploded");
    }
}

/// A shutdown hook that never ends.
#[derive(Hook)]
struct HangsOnShutdown;

impl HangsOnShutdown {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: ShutdownStarted) -> HandlerResult<()> {
        tokio::time::sleep(Duration::from_hours(1)).await;
        Ok(())
    }
}

/// An event handler proving the App keeps dispatching.
#[derive(EventHandler)]
struct Deleted;

impl Deleted {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MessageDelete) -> HandlerResult<()> {
        let probe = ctx.state::<Arc<Probe>>().expect("registered");
        probe.push("EVENT", format!("{}", ev.id));
        Ok(())
    }
}

#[derive(SlashCommand)]
#[command("ping", description = "Pong")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<V2Message> {
        Ok(V2Message::new(
            Components::new().push(TextDisplay::new("pong")),
        ))
    }
}

/// Every recorder, in event order.
fn recorders() -> Module {
    Module::new("ops")
        .add::<OnStarted>()
        .add::<OnPublished>()
        .add::<OnPublishFailed>()
        .add::<OnReady>()
        .add::<OnResumed>()
        .add::<OnDisconnected>()
        .add::<OnFatal>()
        .add::<OnAllReady>()
        .add::<OnToggled>()
        .add::<OnShutdown>()
}

/// Every report's `(origin, kind, module, handler, event_type)`.
#[derive(Default)]
struct Reports(Mutex<Vec<String>>);

impl Reports {
    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => format!("error {error}"),
            ReportKind::Panic(message) => format!("panic {message}"),
            ReportKind::Cancelled => "cancelled".to_owned(),
            _ => "other".to_owned(),
        };
        self.0.lock().unwrap().push(format!(
            "{} {kind} {:?} {:?} {:?} {:?}",
            report.origin, report.module, report.handler, report.event_type, report.shard
        ));
    }
}

// ---- the App ----

/// A fake answering the application and keeping the global command list.
async fn fake_http() -> FakeDiscordHttp {
    let http = FakeDiscordHttp::start_with(GatewayBot {
        shards: 1,
        ..GatewayBot::default()
    })
    .await
    .unwrap();
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(
            StatusCode::OK,
            &json!({"id": "7", "integration_types_config": {"0": {}}}),
        ),
    );
    let remote = Arc::new(Mutex::new(json!([])));
    let read = Arc::clone(&remote);
    http.route_with(Method::GET, GLOBAL, move |_| {
        FakeResponse::json(StatusCode::OK, &read.lock().unwrap())
    });
    http.route_with(Method::PUT, GLOBAL, move |request: &RecordedRequest| {
        let body = request.json().unwrap_or(Value::Null);
        *remote.lock().unwrap() = body.clone();
        FakeResponse::json(StatusCode::OK, &body)
    });
    http
}

/// A running App on both fakes, stopped through `stop`.
struct Running {
    gw: FakeGateway,
    probe: Arc<Probe>,
    reports: Arc<Reports>,
    modules: noctis::Modules,
    stop: Option<oneshot::Sender<()>>,
    run: JoinHandle<Result<(), noctis::Error>>,
}

impl Running {
    async fn start(
        http: &FakeDiscordHttp,
        config: FakeGatewayConfig,
        configure: impl FnOnce(&mut AppBuilder),
    ) -> Self {
        capture();
        let gw = FakeGateway::start_with(config).await.unwrap();
        let probe = Arc::new(Probe::default());
        let reports = Arc::new(Reports::default());
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(http.url())
            .gateway_url(gw.url())
            .state(Arc::clone(&probe))
            .error_sink(ErrorOrigin::Hook, Arc::clone(&reports))
            .module(recorders())
            .module(Module::new("bot").add::<Ping>().add::<Deleted>());
        configure(&mut builder);
        let app = builder.build().unwrap();
        let modules = app.modules();
        let (stop, stopped) = oneshot::channel::<()>();
        let run = tokio::spawn(app.run_until(async {
            let _ = stopped.await;
        }));
        Self {
            gw,
            probe,
            reports,
            modules,
            stop: Some(stop),
            run,
        }
    }

    fn stop(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
    }

    async fn finish(&mut self, within: Duration) -> Result<(), noctis::Error> {
        tokio::time::timeout(within, &mut self.run)
            .await
            .expect("run_until returned in time")
            .expect("run_until did not panic")
    }
}

/// Acceptance 1 and 2 (R8, R10, R12, R13, R15, R20): one run, every event
/// once, in occurrence order, with typed data.
#[tokio::test(flavor = "multi_thread")]
async fn one_run_in_order() {
    let _serial = serial().await;
    let http = fake_http().await;
    let mut running = Running::start(&http, FakeGatewayConfig::default(), |b| {
        b.publisher(true);
    })
    .await;
    running.probe.wait_for("ALL_SHARDS_READY").await;
    assert_eq!(running.gw.close(4000, "test").unwrap(), 1);
    running.probe.wait_for("SHARD_RESUMED").await;
    let guild = ModuleScope::Guild(Id::new(5));
    running
        .modules
        .set_enabled("bot", guild, false)
        .await
        .unwrap();
    // R16: unchanged, fires nothing.
    running
        .modules
        .set_enabled("bot", guild, false)
        .await
        .unwrap();
    running.probe.wait_for("MODULE_TOGGLED").await;
    running.stop();
    assert!(running.finish(WAIT).await.is_ok());
    let probe = &running.probe;
    assert_eq!(
        probe.names(),
        [
            "APP_STARTED",
            "PUBLISH_COMPLETED",
            "SHARD_READY",
            "ALL_SHARDS_READY",
            "SHARD_DISCONNECTED",
            "SHARD_RESUMED",
            "MODULE_TOGGLED",
            "SHUTDOWN_STARTED",
        ],
        "{:?}",
        probe.all()
    );
    assert_eq!(
        probe.data("APP_STARTED"),
        ["AppStarted { publisher: true }"]
    );
    let published = &probe.data("PUBLISH_COMPLETED")[0];
    assert!(
        published.starts_with("PublishCompleted { scope: Global, clear: false"),
        "{published}"
    );
    assert!(
        published.contains("+ create chat_input `ping`"),
        "{published}"
    );
    assert_eq!(probe.data("SHARD_READY"), ["ShardReady { shard: 0 }"]);
    assert_eq!(
        probe.data("ALL_SHARDS_READY"),
        ["AllShardsReady { shards: 0..1, total: 1 }"]
    );
    assert_eq!(
        probe.data("SHARD_DISCONNECTED"),
        ["ShardDisconnected { shard: 0, close_code: Some(4000) }"]
    );
    assert_eq!(probe.data("SHARD_RESUMED"), ["ShardResumed { shard: 0 }"]);
    assert_eq!(
        probe.data("MODULE_TOGGLED"),
        [
            "ModuleToggled { module: \"bot\", scope: Guild(Id<GuildMarker>(5)), enabled: false, reset: false }"
        ]
    );
    assert_eq!(
        probe.data("SHUTDOWN_STARTED"),
        [format!(
            "ShutdownStarted {{ reason: {:?} }}",
            ShutdownReason::Requested
        )]
    );
    assert!(
        running.reports.all().is_empty(),
        "{:?}",
        running.reports.all()
    );
}

/// Acceptance 3 (R12, R20): a fatal close after startup fires `ShardFatal`
/// with its code (after the `ShardDisconnected` of the closed connection),
/// then `ShutdownStarted { AllShardsEnded }`.
#[tokio::test(flavor = "multi_thread")]
async fn fatal_close_after_startup() {
    let _serial = serial().await;
    let http = fake_http().await;
    let mut running = Running::start(&http, FakeGatewayConfig::default(), |_| {}).await;
    running.probe.wait_for("ALL_SHARDS_READY").await;
    assert_eq!(running.gw.close(4014, "disallowed").unwrap(), 1);
    let outcome = running.finish(WAIT).await;
    assert!(
        matches!(
            outcome,
            Err(noctis::Error::Run(noctis::RunError::AllShardsEnded))
        ),
        "{outcome:?}"
    );
    let probe = &running.probe;
    assert_eq!(
        probe.names(),
        [
            "APP_STARTED",
            "SHARD_READY",
            "ALL_SHARDS_READY",
            "SHARD_DISCONNECTED",
            "SHARD_FATAL",
            "SHUTDOWN_STARTED"
        ],
        "{:?}",
        probe.all()
    );
    assert_eq!(
        probe.data("SHARD_FATAL"),
        ["ShardFatal { shard: 0, close_code: Some(4014) }"]
    );
    assert_eq!(
        probe.data("SHUTDOWN_STARTED"),
        ["ShutdownStarted { reason: AllShardsEnded }"]
    );
}

/// R11: a failed startup Publish fires `PublishFailed`; the App keeps
/// starting.
#[tokio::test(flavor = "multi_thread")]
async fn failed_startup_publish() {
    let _serial = serial().await;
    let http = fake_http().await;
    http.route(
        Method::GET,
        APP,
        FakeResponse::json(
            StatusCode::INTERNAL_SERVER_ERROR,
            &json!({"message": "down"}),
        ),
    );
    let mut running = Running::start(&http, FakeGatewayConfig::default(), |b| {
        b.publisher(true);
    })
    .await;
    running.probe.wait_for("ALL_SHARDS_READY").await;
    running.stop();
    assert!(running.finish(WAIT).await.is_ok());
    let probe = &running.probe;
    assert_eq!(
        &probe.names()[..2],
        ["APP_STARTED", "PUBLISH_FAILED"],
        "{:?}",
        probe.all()
    );
    let failed = &probe.data("PUBLISH_FAILED")[0];
    assert!(
        failed.starts_with(
            "PublishFailed { scope: Global, dry_run: false, clear: false, error: Rejected"
        ),
        "{failed}"
    );
    assert!(failed.contains("500"), "{failed}");
}

/// Acceptance 4 (R4, R5): an erroring and a panicking hook each reach the
/// `Hook` sink with their Module, handler and event; later hooks still run
/// and the App keeps dispatching.
#[tokio::test(flavor = "multi_thread")]
async fn hook_failures_are_reported() {
    let _serial = serial().await;
    let http = fake_http().await;
    let mut running = Running::start(&http, FakeGatewayConfig::default(), |b| {
        b.module(
            Module::new("alerts")
                .add::<FailsOnReady>()
                .add::<PanicsOnReady>(),
        );
    })
    .await;
    running.probe.wait_for("ALL_SHARDS_READY").await;
    let sent = running.gw.dispatch(
        "MESSAGE_DELETE",
        json!({ "id": "77", "channel_id": "2", "guild_id": "3" }),
    );
    assert_eq!(sent, 1);
    running.probe.wait_for("EVENT").await;
    running.stop();
    assert!(running.finish(WAIT).await.is_ok());
    assert_eq!(
        running.reports.all(),
        [
            "hook error hook broke Some(\"alerts\") Some(\"lifecycle::FailsOnReady\") Some(\"SHARD_READY\") Some(0)",
            "hook panic hook exploded Some(\"alerts\") Some(\"lifecycle::PanicsOnReady\") Some(\"SHARD_READY\") Some(0)",
        ]
    );
    let names = running.probe.names();
    assert!(names.contains(&"ALL_SHARDS_READY") && names.contains(&"SHUTDOWN_STARTED"));
    assert_eq!(running.probe.data("EVENT"), ["77"]);
}

/// R14: a fatal close before every shard is ready fires `ShardFatal`, then
/// `ShutdownStarted { StartupFailed }`, and `run` returns its start error
/// after the hooks drained.
#[tokio::test(flavor = "multi_thread")]
async fn fatal_close_during_startup() {
    let _serial = serial().await;
    let http = fake_http().await;
    let config = FakeGatewayConfig {
        identify: IdentifyReply::Close(4014),
        ..FakeGatewayConfig::default()
    };
    let mut running = Running::start(&http, config, |_| {}).await;
    let outcome = running.finish(WAIT).await;
    assert!(
        matches!(
            outcome,
            Err(noctis::Error::Start(StartError::DisallowedIntents {
                shard: 0
            }))
        ),
        "{outcome:?}"
    );
    let probe = &running.probe;
    assert_eq!(
        probe.names(),
        [
            "APP_STARTED",
            "SHARD_DISCONNECTED",
            "SHARD_FATAL",
            "SHUTDOWN_STARTED"
        ],
        "{:?}",
        probe.all()
    );
    assert_eq!(
        probe.data("SHUTDOWN_STARTED"),
        ["ShutdownStarted { reason: StartupFailed }"]
    );
}

/// R22: a `ShutdownStarted` hook that never ends is cut after 1 s with
/// grace 0, reported once as `Cancelled`.
#[tokio::test(flavor = "multi_thread")]
async fn slow_shutdown_hook_is_cut() {
    let _serial = serial().await;
    let http = fake_http().await;
    let mut running = Running::start(&http, FakeGatewayConfig::default(), |b| {
        b.shutdown_grace(Duration::ZERO)
            .module(Module::new("slow").add::<HangsOnShutdown>());
    })
    .await;
    running.probe.wait_for("ALL_SHARDS_READY").await;
    running.stop();
    let started = Instant::now();
    assert!(running.finish(WAIT).await.is_ok());
    assert!(
        started.elapsed() >= Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        running.reports.all(),
        [
            "hook cancelled Some(\"slow\") Some(\"lifecycle::HangsOnShutdown\") Some(\"SHUTDOWN_STARTED\") None"
        ]
    );
}

/// R24: outside a run, `App::publish` runs its hooks before it returns.
#[tokio::test(flavor = "multi_thread")]
async fn publish_outside_a_run() {
    let _serial = serial().await;
    let http = fake_http().await;
    let probe = Arc::new(Probe::default());
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .token(Token::new("fake-token").unwrap())
        .http_proxy(http.url())
        .state(Arc::clone(&probe))
        .module(recorders())
        .module(Module::new("bot").add::<Ping>());
    let app = builder.build().unwrap();
    let report = app
        .publish(PublishScope::Global)
        .dry_run(true)
        .await
        .unwrap();
    assert!(report.is_dry_run());
    let data = probe.data("PUBLISH_COMPLETED");
    assert_eq!(data.len(), 1, "{:?}", probe.all());
    assert!(data[0].contains("dry_run: true"), "{}", data[0]);
}
