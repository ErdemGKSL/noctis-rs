//! `App` and its mutable `AppBuilder` (FR-1, AD-24).

use std::any::TypeId;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::RangeInclusive;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use noctis_gateway::{
    CloseMode, DEFAULT_EVENT_CAPACITY, DEFAULT_OVERFLOW_CAPACITY, Gateway, GatewayOptions,
    LaneConfig, ShardEvent,
};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_ports::{
    AssetStore, CacheStore, DiscordCache, GuildAccessList, IdentifyQueue, LocaleSource,
    ModuleStateStore, RateLimitStore, RefStore, RenderCache, Renderer, SessionStore,
};
use tokio::sync::mpsc;
use tokio::time::Instant;
use twilight_model::gateway::Intents;

use crate::edge::{EDGE_BODY_PREFIX, Edge, HttpConfig, KeeperHandle, Upstream};
use crate::gateway_url::normalize_gateway_url;
use crate::handler::AppShared;
use crate::lifecycle::{Lifecycle, LifecycleEvent};
use crate::module::Module;
use crate::observe::{describe_metrics, set_shard_status};
use crate::plugin::PluginEntry;
use crate::ports::{PortOverrides, PortSlot, Ports};
use crate::proxy::normalize_http_proxy;
use crate::registry::Registry;
use crate::sharding::{GatewayInfo, plan};
use crate::sink::{ErrorSinks, EventDropObserver};
use crate::startup::{Signals, check_runtime, ensure_crypto_provider};
use crate::state::StateMap;
use crate::worker::{AbortOnDrop, Shutdown, Workers, drain};
use crate::{
    BuildError, BuildViolation, EnvError, Error, ErrorOrigin, ErrorSink, Plugin, RunError, Runtime,
    StartError, State, Token,
};

/// The environment variable [`App::from_env`] reads the token from.
const TOKEN_VAR: &str = "NOCTIS_TOKEN";
/// The environment variable [`App::from_env`] reads the HTTP proxy from.
const HTTP_PROXY_VAR: &str = "NOCTIS_HTTP_PROXY";
/// The environment variable [`App::from_env`] reads the gateway URL from.
const GATEWAY_URL_VAR: &str = "NOCTIS_GATEWAY_URL";
/// The environment variable [`App::from_env`] reads the fixed shard total from.
const SHARD_TOTAL_VAR: &str = "NOCTIS_SHARD_TOTAL";
/// The environment variable [`App::from_env`] reads the shutdown grace from.
const SHUTDOWN_GRACE_VAR: &str = "NOCTIS_SHUTDOWN_GRACE";
/// The shutdown grace period when `shutdown_grace` is not called.
const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
/// The allowed `shutdown_grace` range.
const SHUTDOWN_GRACE_RANGE: RangeInclusive<Duration> = Duration::ZERO..=Duration::from_mins(15);
/// How long shutdown waits for cancelled handlers after the grace period.
const CANCEL_WAIT: Duration = Duration::from_secs(1);
/// How long a closing shard waits for the gateway's close frame.
const SHARD_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
/// The HTTP request timeout when `http_timeout` is not called.
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// The allowed `http_timeout` range.
const HTTP_TIMEOUT_RANGE: RangeInclusive<Duration> =
    Duration::from_millis(1)..=Duration::from_mins(5);
/// The allowed `event_lane` capacity range.
const EVENT_CAPACITY_RANGE: RangeInclusive<usize> = 1..=1_048_576;
/// The allowed `event_lane` overflow range.
const EVENT_OVERFLOW_RANGE: RangeInclusive<usize> = 0..=4_194_304;
/// The handler concurrency when `handler_concurrency` is not called.
const DEFAULT_HANDLER_CONCURRENCY: usize = 256;
/// The allowed `handler_concurrency` range.
const HANDLER_CONCURRENCY_RANGE: RangeInclusive<usize> = 1..=65_536;

/// A configured bot, built by [`AppBuilder::build`].
///
/// It owns the App's one Discord HTTP client, its typed state and its ports.
/// [`run`](App::run) starts it.
///
/// ```no_run
/// use noctis_core::{App, AppBuilder, Error};
///
/// fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
///     builder.state(42_u32)
/// }
///
/// #[tokio::main]
/// async fn main() -> Result<(), Error> {
///     let app = configure(&mut App::from_env()?).build()?;
///     app.run().await
/// }
/// ```
pub struct App {
    runtime: Runtime,
    token: Option<Token>,
    /// The HTTP client and the state, shared with every `EventCtx`.
    shared: Arc<AppShared>,
    http_timeout: Duration,
    http_proxy: Option<String>,
    http_config: HttpConfig,
    gateway_url: Option<String>,
    shard_total: Option<NonZeroU32>,
    intents: Intents,
    ports: Ports,
    renderer: Option<Arc<dyn Renderer>>,
    registry: Arc<Registry>,
    event_capacity: NonZeroUsize,
    overflow_capacity: usize,
    handler_concurrency: usize,
    shutdown_grace: Duration,
    /// The crate-internal lifecycle broadcast (Lifecycle Hooks, Story 13.5).
    lifecycle: Lifecycle,
}

/// Why the supervised App stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The shutdown future resolved (a signal, for [`App::run`]).
    Requested,
    /// Every shard ended on its own.
    AllEnded,
}

impl App {
    /// An empty [`AppBuilder`] attached to [`Runtime::global`], with no token
    /// (enough for test Apps; [`run`](Self::run) needs one).
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.state(1_u8);
    /// ```
    pub fn builder() -> AppBuilder {
        AppBuilder::new(Runtime::global().clone())
    }

    /// [`App::builder`] plus the `NOCTIS_*` environment variables.
    ///
    /// Reads `NOCTIS_TOKEN`, `NOCTIS_HTTP_PROXY`, `NOCTIS_GATEWAY_URL`,
    /// `NOCTIS_SHARD_TOTAL` and then `NOCTIS_SHUTDOWN_GRACE`, each trimmed of
    /// ASCII whitespace; an empty value counts as unset. The proxy and the
    /// gateway URL are passed to [`AppBuilder::http_proxy`] and
    /// [`AppBuilder::gateway_url`] and validated by `build()`; the shard total
    /// (1-10 ASCII digits) is passed to [`AppBuilder::shard_total`]; the
    /// shutdown grace (1-4 ASCII digits of whole seconds) is passed to
    /// [`AppBuilder::shutdown_grace`], so 901-9999 fail at `build()`. Setters
    /// called afterwards win.
    ///
    /// # Errors
    ///
    /// [`Error::Env`] when a variable is not Unicode, holds an invalid token,
    /// an invalid shard total or an invalid shutdown grace; the message never
    /// shows the value.
    ///
    /// ```no_run
    /// let app = noctis_core::App::from_env()?;
    /// # Ok::<(), noctis_core::Error>(())
    /// ```
    pub fn from_env() -> Result<AppBuilder, Error> {
        Ok(AppBuilder::from_lookup(|var| std::env::var_os(var))?)
    }

    /// The Runtime this App is attached to.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.runtime(), noctis_core::Runtime::global());
    /// # }
    /// ```
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// The App's one Discord HTTP client; the same `Arc` on every call.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert!(std::sync::Arc::ptr_eq(app.http(), app.http()));
    /// # }
    /// ```
    pub fn http(&self) -> &Arc<twilight_http::Client> {
        &self.shared.http
    }

    /// The HTTP request timeout (default 10 s).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.http_timeout(), std::time::Duration::from_secs(10));
    /// # }
    /// ```
    pub fn http_timeout(&self) -> Duration {
        self.http_timeout
    }

    /// The normalized HTTP proxy authority (`host[:port]`), if one was set.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().http_proxy("http://proxy:3000").build().unwrap();
    /// assert_eq!(app.http_proxy(), Some("proxy:3000"));
    /// # }
    /// ```
    pub fn http_proxy(&self) -> Option<&str> {
        self.http_proxy.as_deref()
    }

    /// The Discord HTTP edge's tuning (default [`HttpConfig::default`]).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.http_config(), noctis_core::HttpConfig::default());
    /// # }
    /// ```
    pub fn http_config(&self) -> HttpConfig {
        self.http_config
    }

    /// The App's Discord HTTP edge.
    pub(crate) fn edge(&self) -> &Edge {
        &self.shared.edge
    }

    /// The normalized gateway URL (`ws://host[:port]` or `wss://host[:port]`),
    /// if one was set; shards connect there instead of Discord's gateway.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().gateway_url("WS://127.0.0.1:3000/").build().unwrap();
    /// assert_eq!(app.gateway_url(), Some("ws://127.0.0.1:3000"));
    /// # }
    /// ```
    pub fn gateway_url(&self) -> Option<&str> {
        self.gateway_url.as_deref()
    }

    /// The fixed shard total, if one was set; `None` uses Discord's
    /// recommendation.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().shard_total(16).build().unwrap();
    /// assert_eq!(app.shard_total(), Some(16));
    /// assert_eq!(noctis_core::App::builder().build().unwrap().shard_total(), None);
    /// # }
    /// ```
    pub fn shard_total(&self) -> Option<u32> {
        self.shard_total.map(NonZeroU32::get)
    }

    /// The gateway intents the App identifies with: the requested ones (at
    /// least `GUILDS`) plus the non-privileged intents its event handlers
    /// derive.
    ///
    /// ```
    /// use twilight_model::gateway::Intents;
    ///
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.intents(), Intents::GUILDS);
    /// # }
    /// ```
    pub fn intents(&self) -> Intents {
        self.intents
    }

    /// The shutdown grace period (default 10 s): how long the shutdown waits
    /// for queued and running handlers before it cancels them.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.shutdown_grace(), std::time::Duration::from_secs(10));
    /// # }
    /// ```
    pub fn shutdown_grace(&self) -> Duration {
        self.shutdown_grace
    }

    /// The state registered for `T`, if any.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().state(7_u32).build().unwrap();
    /// assert_eq!(*app.state::<u32>().unwrap(), 7);
    /// assert!(app.state::<u64>().is_none());
    /// # }
    /// ```
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        self.shared.states.get::<T>()
    }

    /// Whether a [`Renderer`] was set (by a plugin or `.renderer(..)`).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// assert!(!noctis_core::App::builder().build().unwrap().has_renderer());
    /// # }
    /// ```
    pub fn has_renderer(&self) -> bool {
        self.renderer.is_some()
    }

    /// The resolved ports.
    pub(crate) fn ports(&self) -> &Ports {
        &self.ports
    }

    /// The folded Registry.
    pub(crate) fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// The Error Sinks.
    #[cfg(test)]
    pub(crate) fn sinks(&self) -> &ErrorSinks {
        &self.shared.sinks
    }

    /// A receiver of the App's lifecycle broadcast.
    #[cfg(test)]
    pub(crate) fn subscribe_lifecycle(&self) -> tokio::sync::broadcast::Receiver<LifecycleEvent> {
        self.lifecycle.subscribe()
    }

    /// The handler concurrency.
    #[cfg(test)]
    pub(crate) fn handler_concurrency(&self) -> usize {
        self.handler_concurrency
    }

    /// The lane configuration `run` passes to the lanes.
    pub(crate) fn lane_config(&self) -> LaneConfig {
        LaneConfig {
            app: Arc::clone(&self.shared.app),
            event_capacity: self.event_capacity,
            overflow_capacity: self.overflow_capacity,
            droppable: self.registry.droppable(),
            drop_observer: Some(Arc::new(EventDropObserver {
                app: Arc::clone(&self.shared.app),
                sinks: self.shared.sinks.clone(),
            })),
        }
    }

    /// The worker set `run` drains into, controlled by `shutdown`.
    pub(crate) fn workers(&self, shutdown: Shutdown) -> Workers {
        Workers::new(
            Arc::clone(&self.registry),
            Arc::clone(&self.shared),
            self.handler_concurrency,
            shutdown,
        )
    }

    /// Starts the App and runs it until Ctrl-C or SIGTERM (Ctrl-C only on
    /// non-unix platforms), then shuts it down gracefully.
    ///
    /// Checks, in order, that a multi-thread tokio runtime is current, that a
    /// rustls crypto provider is installed (installing `ring`'s when none is
    /// set), installs the signal handlers and checks that a token is set.
    /// The rest is [`run_until`](Self::run_until) with the first signal as
    /// the shutdown trigger.
    ///
    /// # Errors
    ///
    /// [`StartError::SignalHandler`] when the signal handlers cannot be
    /// installed (a runtime without an IO driver), else the errors of
    /// [`run_until`](Self::run_until).
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::Error> {
    /// noctis_core::App::from_env()?.build()?.run().await?;
    /// # Ok(()) }
    /// ```
    pub async fn run(self) -> Result<(), Error> {
        check_runtime(true)?;
        ensure_crypto_provider().map_err(|_| StartError::CryptoProvider)?;
        let signals = Signals::install()?;
        self.run_with(signals.recv(), "signal").await
    }

    /// Starts the App and runs it until `shutdown` resolves, then shuts it
    /// down gracefully. [`run`](Self::run) is `run_until` on Ctrl-C/SIGTERM;
    /// use this one to stop the App from code (tests, embedders).
    ///
    /// Checks, in order, that a multi-thread tokio runtime is current, that a
    /// rustls crypto provider is installed (installing `ring`'s when none is
    /// set) and that a token is set. It then reads `GET /gateway/bot` with
    /// the App's HTTP client, plans the shard total (the fixed
    /// [`shard_total`](AppBuilder::shard_total) or Discord's recommendation)
    /// and the identify buckets (`max_concurrency`), and starts one task per
    /// shard. Each shard identifies through the `IdentifyQueue` port in
    /// bucket `shard_id % max_concurrency` (GW-01). Shard tasks push every
    /// parsed item into the lanes without waiting: interactions into the
    /// never-dropped Interaction Lane, other events into the bounded event
    /// lane (AD-10). The gateway parses only interactions and the event
    /// types the Module handlers take. One core task drains the lanes,
    /// interactions first; each event waits for one of
    /// [`handler_concurrency`](AppBuilder::handler_concurrency) worker
    /// permits (interactions never do) and runs its ordered handlers in
    /// per-guild order, then its concurrent handlers together. Startup
    /// finishes once every shard has received READY.
    ///
    /// A shard that closes fatally after startup is logged at `error`
    /// (`"shard closed fatally"`, with the typed cause) and its
    /// `noctis_shard_status` gauge goes to 0; the others keep running.
    ///
    /// When `shutdown` resolves (also during startup), or every shard has
    /// ended, the shutdown runs in this order (AD-21): log `shutdown
    /// started`; close every shard with code 4000 (resumable); drain both
    /// lanes and wait for the running handlers for at most
    /// [`shutdown_grace`](AppBuilder::shutdown_grace), with the HTTP client
    /// still working; cancel what is left (each cancelled handler is
    /// reported to the `Event` Error Sink as
    /// [`ReportKind::Cancelled`](crate::ReportKind::Cancelled), queued lane
    /// items are discarded with one warning) and wait up to 1 s more; wait
    /// up to 6 s for the shards to close; log `shutdown complete`. Dropping
    /// the returned future cancels every handler too.
    ///
    /// # Errors
    ///
    /// [`Error::Start`] with the [`StartError`] of the first failed step:
    /// the checks above, [`StartError::InvalidToken`] (HTTP 401 or close
    /// 4004), [`StartError::GatewayInfo`], [`StartError::InvalidGatewayInfo`],
    /// [`StartError::ShardTotalTooSmall`] (GW-03),
    /// [`StartError::SessionStartLimit`] (GW-02), or a fatal close during
    /// startup ([`StartError::InvalidIntents`],
    /// [`StartError::DisallowedIntents`], [`StartError::FatalClose`],
    /// [`StartError::ShardEnded`], [`StartError::GatewayStopped`]).
    /// [`Error::Run`] with [`RunError::AllShardsEnded`] when every shard
    /// ended on its own after startup (after the shutdown sequence ran).
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::Error> {
    /// let app = noctis_core::App::from_env()?.build()?;
    /// let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    /// let running = tokio::spawn(app.run_until(async {
    ///     let _ = stopped.await;
    /// }));
    /// // ... later:
    /// let _ = stop.send(());
    /// running.await.expect("not cancelled")?;
    /// # Ok(()) }
    /// ```
    pub async fn run_until(self, shutdown: impl Future<Output = ()> + Send) -> Result<(), Error> {
        self.run_with(shutdown, "requested").await
    }

    /// [`run_until`](Self::run_until), logging `reason` when `shutdown`
    /// resolves (`"signal"` from [`run`](Self::run), else `"requested"`).
    async fn run_with(
        self,
        shutdown: impl Future<Output = ()> + Send,
        reason: &'static str,
    ) -> Result<(), Error> {
        check_runtime(true)?;
        ensure_crypto_provider().map_err(|_| StartError::CryptoProvider)?;
        let Some(token) = &self.token else {
            return Err(StartError::MissingToken.into());
        };
        let controls = Shutdown::default();
        // Cancels every handler on every return path, including a dropped
        // `run_until` future, so no handler outlives it.
        let _cancel_guard = controls.cancel.clone().drop_guard();
        // The keeper warms the upstream connection now and keeps it warm
        // until the shutdown sequence has returned.
        let (_keeper, warmed) = self.edge().start_keeper();
        let keeper = self.edge().keeper();
        let mut shutdown = pin!(shutdown);
        let info = tokio::select! {
            biased;
            () = &mut shutdown => {
                // No gateway or drain exists yet: only the log lines and
                // the lifecycle broadcast of the shutdown sequence apply.
                let grace = self.shutdown_grace;
                tracing::info!(target: "noctis::app", reason, grace = ?grace, "shutdown started");
                self.lifecycle.send(LifecycleEvent::ShutdownStarted);
                tracing::info!(target: "noctis::app", "shutdown complete");
                return Ok(());
            }
            info = async {
                self.warm_up(warmed).await;
                self.gateway_info().await
            } => info?,
        };
        let plan = plan(info, self.shard_total)?;
        let total = plan.total.get();
        if plan.misaligned {
            tracing::warn!(
                target: "noctis::app",
                total,
                max_concurrency = plan.max_concurrency.get(),
                "shard total is not a multiple of max_concurrency; identify buckets are uneven"
            );
        }
        let identify_queue: Arc<dyn IdentifyQueue> =
            if self.ports().is_default(PortSlot::IdentifyQueue) {
                Arc::new(MemoryIdentifyQueue::new(plan.max_concurrency))
            } else {
                Arc::clone(&self.ports().identify_queue)
            };
        let options = GatewayOptions {
            app: Arc::clone(&self.shared.app),
            token: token.expose().to_owned(),
            intents: self.intents,
            gateway_url: self.gateway_url.clone(),
            event_types: self.registry.event_types(),
            identify_queue,
            total: plan.total,
            max_concurrency: plan.max_concurrency,
            close_timeout: SHARD_CLOSE_TIMEOUT,
        };
        describe_metrics();
        let (sender, receiver) = noctis_gateway::lanes(self.lane_config());
        let mut drain = AbortOnDrop(tokio::spawn(drain(
            receiver,
            self.workers(controls.clone()),
        )));
        let (gateway, mut events) = Gateway::start(options, Arc::new(sender)).map_err(|error| {
            tracing::error!(target: "noctis::app", %error, "the shards could not be configured");
            StartError::GatewayStopped
        })?;
        let status = ShardStatus {
            app: Arc::clone(&self.shared.app),
            total,
        };
        status.init();
        let started = tokio::select! {
            biased;
            () = &mut shutdown => None,
            result = await_startup(&mut events, &status, &self.lifecycle) => Some(result),
        };
        let stop = match started {
            Some(result) => {
                result?;
                tracing::info!(target: "noctis::app", "all {total} shards ready");
                supervise(
                    &mut events,
                    &status,
                    &self.lifecycle,
                    Some(&keeper),
                    &mut shutdown,
                )
                .await
            }
            None => Stop::Requested,
        };
        let watch = Watch {
            events: &mut events,
            status: &status,
            lifecycle: &self.lifecycle,
        };
        self.shut_down(stop, reason, gateway, watch, &controls, &mut drain)
            .await;
        match stop {
            Stop::Requested => Ok(()),
            Stop::AllEnded => Err(RunError::AllShardsEnded.into()),
        }
    }

    /// The AD-21 shutdown sequence; see [`run_until`](Self::run_until).
    async fn shut_down(
        &self,
        stop: Stop,
        requested: &'static str,
        mut gateway: Gateway,
        mut watch: Watch<'_>,
        controls: &Shutdown,
        drain: &mut AbortOnDrop,
    ) {
        let reason = match stop {
            Stop::Requested => requested,
            Stop::AllEnded => "all shards ended",
        };
        let grace = self.shutdown_grace;
        tracing::info!(target: "noctis::app", reason, grace = ?grace, "shutdown started");
        self.lifecycle.send(LifecycleEvent::ShutdownStarted);
        gateway.close_all(CloseMode::Resumable);
        wind_down(grace, controls, drain, &mut watch).await;
        let closed = watch
            .during(tokio::time::timeout(
                SHARD_CLOSE_TIMEOUT + Duration::from_secs(1),
                gateway.closed(),
            ))
            .await;
        if closed.is_err() {
            tracing::warn!(target: "noctis::app", "shards did not close in time; aborting");
        }
        drop(gateway);
        watch.flush();
        tracing::info!(target: "noctis::app", "shutdown complete");
    }

    /// Waits for the keeper's first warm-up dial, at most
    /// `connect_timeout + 6 s`; a failure only warns (the following
    /// `GET /gateway/bot` reports the error).
    async fn warm_up(
        &self,
        warmed: tokio::sync::oneshot::Receiver<Result<(), crate::edge::DialError>>,
    ) {
        let bound = self.http_config.connect_timeout + Duration::from_secs(6);
        match tokio::time::timeout(bound, warmed).await {
            Ok(Ok(Ok(()))) => {
                tracing::debug!(target: "noctis::http", "upstream connection warm");
            }
            Ok(Ok(Err(error))) => {
                tracing::warn!(target: "noctis::http", %error, "the upstream connection could not be warmed");
            }
            Ok(Err(_)) | Err(_) => {
                tracing::warn!(target: "noctis::http", "the upstream connection was not warm in time");
            }
        }
    }

    /// Reads `GET /gateway/bot`; every failure is logged with its `Display`
    /// (twilight never prints the token). The edge's own 502 (upstream
    /// unreachable) counts as no status.
    async fn gateway_info(&self) -> Result<GatewayInfo, StartError> {
        use twilight_http::error::ErrorType;

        let response = self.shared.http.gateway().authed().await.map_err(|error| {
            tracing::error!(target: "noctis::app", %error, "GET /gateway/bot failed");
            match error.kind() {
                ErrorType::Unauthorized => StartError::InvalidToken,
                ErrorType::Response { status, .. } if status.get() == 401 => {
                    StartError::InvalidToken
                }
                ErrorType::Response { status, body, .. }
                    if status.get() == 502 && body.starts_with(EDGE_BODY_PREFIX.as_bytes()) =>
                {
                    StartError::GatewayInfo { status: None }
                }
                ErrorType::Response { status, .. } => StartError::GatewayInfo {
                    status: Some(status.get()),
                },
                _ => StartError::GatewayInfo { status: None },
            }
        })?;
        let info = response.model().await.map_err(|error| {
            tracing::error!(target: "noctis::app", %error, "GET /gateway/bot body could not be read");
            StartError::GatewayInfo { status: None }
        })?;
        Ok(GatewayInfo::from(&info))
    }
}

/// The [`StartError`] for shard `shard` ending during startup with
/// `close_code`.
fn start_error(shard: u32, close_code: Option<u16>) -> StartError {
    match close_code {
        Some(4004) => StartError::InvalidToken,
        Some(4013) => StartError::InvalidIntents { shard },
        Some(4014) => StartError::DisallowedIntents { shard },
        Some(code) => StartError::FatalClose { shard, code },
        None => StartError::ShardEnded { shard },
    }
}

/// Keeps `noctis_shard_status{app, shard}` for shards `0..total`.
pub(crate) struct ShardStatus {
    pub(crate) app: Arc<str>,
    pub(crate) total: u32,
}

impl ShardStatus {
    /// Sets every shard's gauge to 0 (down).
    pub(crate) fn init(&self) {
        for shard in 0..self.total {
            set_shard_status(&self.app, shard, self.total, false);
        }
    }

    /// Updates the gauge of the event's shard: 1 on `Ready`/`Resumed`, 0
    /// on `Disconnected`/`Ended`/`Closed`.
    pub(crate) fn update(&self, event: &ShardEvent) {
        let (shard, up) = match *event {
            ShardEvent::Ready { shard } | ShardEvent::Resumed { shard } => (shard, true),
            ShardEvent::Disconnected { shard, .. }
            | ShardEvent::Ended { shard, .. }
            | ShardEvent::Closed { shard } => (shard, false),
            _ => return,
        };
        set_shard_status(&self.app, shard, self.total, up);
    }
}

/// Marks every shard down when `App::run` returns or its future is dropped,
/// so a stopped App never reports a shard as up.
impl Drop for ShardStatus {
    fn drop(&mut self) {
        self.init();
    }
}

/// Waits until every shard in `0..total` has sent `Ready` at least once,
/// broadcasting each shard event.
pub(crate) async fn await_startup(
    events: &mut mpsc::UnboundedReceiver<ShardEvent>,
    status: &ShardStatus,
    lifecycle: &Lifecycle,
) -> Result<(), StartError> {
    let mut ready = vec![false; usize::try_from(status.total).unwrap_or(usize::MAX)];
    let mut missing = ready.len();
    while missing > 0 {
        let Some(event) = events.recv().await else {
            return Err(StartError::GatewayStopped);
        };
        status.update(&event);
        lifecycle.shard(&event);
        match event {
            ShardEvent::Ready { shard } => {
                tracing::debug!(target: "noctis::app", shard, "shard ready");
                if let Some(seen) = slot(&mut ready, shard)
                    && !*seen
                {
                    *seen = true;
                    missing -= 1;
                }
            }
            ShardEvent::Ended { shard, close_code } => {
                let error = start_error(shard, close_code);
                tracing::error!(
                    target: "noctis::app",
                    shard,
                    ?close_code,
                    %error,
                    "shard ended during startup"
                );
                return Err(error);
            }
            ShardEvent::Disconnected { shard, close_code } => {
                tracing::warn!(
                    target: "noctis::app",
                    shard,
                    ?close_code,
                    "shard disconnected during startup"
                );
            }
            _ => {}
        }
    }
    Ok(())
}

/// The flag of `shard`, if it is in range.
fn slot(flags: &mut [bool], shard: u32) -> Option<&mut bool> {
    usize::try_from(shard)
        .ok()
        .and_then(|index| flags.get_mut(index))
}

/// Watches the shard events after startup until `shutdown` resolves
/// ([`Stop::Requested`]) or every shard has ended ([`Stop::AllEnded`]).
pub(crate) async fn supervise(
    events: &mut mpsc::UnboundedReceiver<ShardEvent>,
    status: &ShardStatus,
    lifecycle: &Lifecycle,
    keeper: Option<&KeeperHandle>,
    shutdown: impl Future<Output = ()>,
) -> Stop {
    let mut shutdown = pin!(shutdown);
    let mut ended = vec![false; usize::try_from(status.total).unwrap_or(usize::MAX)];
    let mut running = ended.len();
    loop {
        let event = tokio::select! {
            biased;
            () = &mut shutdown => return Stop::Requested,
            event = events.recv() => event,
        };
        let Some(event) = event else {
            return Stop::AllEnded;
        };
        on_shard_event(&event, status, lifecycle);
        if let (
            ShardEvent::Disconnected {
                close_code: None, ..
            },
            Some(keeper),
        ) = (event, keeper)
        {
            // No close frame: the network was lost, so the upstream HTTP
            // connection is suspect too.
            keeper.request_redial();
        }
        if let ShardEvent::Ended { shard, .. } = event
            && let Some(seen) = slot(&mut ended, shard)
            && !*seen
        {
            *seen = true;
            running -= 1;
            if running == 0 {
                return Stop::AllEnded;
            }
        }
    }
}

/// Handles one shard event after startup: the gauge, the lifecycle
/// broadcast and the log line.
fn on_shard_event(event: &ShardEvent, status: &ShardStatus, lifecycle: &Lifecycle) {
    status.update(event);
    lifecycle.shard(event);
    match *event {
        ShardEvent::Ended { shard, close_code } => {
            tracing::error!(
                target: "noctis::app",
                shard,
                close_code,
                error = %start_error(shard, close_code),
                "shard closed fatally"
            );
        }
        ShardEvent::Disconnected { shard, close_code } => {
            tracing::warn!(target: "noctis::app", shard, ?close_code, "shard disconnected");
        }
        ShardEvent::Ready { shard } => {
            tracing::debug!(target: "noctis::app", shard, "shard ready");
        }
        ShardEvent::Resumed { shard } => {
            tracing::debug!(target: "noctis::app", shard, "shard resumed");
        }
        ShardEvent::Closed { shard } => {
            tracing::debug!(target: "noctis::app", shard, "shard closed");
        }
        _ => {}
    }
}

/// Keeps consuming the shard events while the shutdown waits for something.
pub(crate) struct Watch<'a> {
    pub(crate) events: &'a mut mpsc::UnboundedReceiver<ShardEvent>,
    pub(crate) status: &'a ShardStatus,
    pub(crate) lifecycle: &'a Lifecycle,
}

impl Watch<'_> {
    /// Awaits `fut`, handling every shard event that arrives meanwhile.
    pub(crate) async fn during<F: Future>(&mut self, fut: F) -> F::Output {
        let mut fut = pin!(fut);
        let mut open = true;
        loop {
            tokio::select! {
                biased;
                output = &mut fut => return output,
                event = self.events.recv(), if open => match event {
                    Some(event) => on_shard_event(&event, self.status, self.lifecycle),
                    None => open = false,
                },
            }
        }
    }

    /// Handles the shard events already queued.
    pub(crate) fn flush(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            on_shard_event(&event, self.status, self.lifecycle);
        }
    }
}

/// Steps 3 and 4 of the shutdown: fires the drain's `shutdown` token and
/// waits for the drain and every tracked handler task until `grace` ends;
/// then fires `cancel` and waits up to [`CANCEL_WAIT`] more, warning about
/// tasks that still run. Shard events are handled meanwhile.
pub(crate) async fn wind_down(
    grace: Duration,
    controls: &Shutdown,
    drain: &mut AbortOnDrop,
    watch: &mut Watch<'_>,
) {
    let deadline = Instant::now() + grace;
    if grace.is_zero() {
        // Nothing may run: cancel before the drain sees the shutdown.
        controls.cancel.cancel();
    }
    controls.shutdown.cancel();
    let mut drained = false;
    let settled = watch
        .during(tokio::time::timeout_at(
            deadline,
            settle(drain, &mut drained, controls),
        ))
        .await;
    if settled.is_ok() {
        return;
    }
    controls.cancel.cancel();
    let _ = watch
        .during(tokio::time::timeout(
            CANCEL_WAIT,
            settle(drain, &mut drained, controls),
        ))
        .await;
    if !controls.tracker.is_empty() {
        tracing::warn!(
            target: "noctis::app",
            running = controls.tracker.len(),
            "handlers still running after cancellation"
        );
    }
}

/// Waits for the drain task (once), then for every tracked worker task.
async fn settle(drain: &mut AbortOnDrop, drained: &mut bool, controls: &Shutdown) {
    if !*drained {
        // A drain panic is logged by tokio's task machinery; nothing to add.
        let _ = (&mut drain.0).await;
        *drained = true;
    }
    controls.tracker.close();
    controls.tracker.wait().await;
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("runtime", &self.runtime)
            .field("token", &self.token)
            .field("http_timeout", &self.http_timeout)
            .field("http_proxy", &self.http_proxy)
            .field("http_config", &self.http_config)
            .field("gateway_url", &self.gateway_url)
            .field("shard_total", &self.shard_total)
            .field("intents", &self.intents)
            .field("states", &self.shared.states)
            .field("default_ports", self.ports.default_slots())
            .field("has_renderer", &self.has_renderer())
            .field("handlers", &self.registry.len())
            .field("event_capacity", &self.event_capacity)
            .field("overflow_capacity", &self.overflow_capacity)
            .field("handler_concurrency", &self.handler_concurrency)
            .field("shutdown_grace", &self.shutdown_grace)
            .field("error_sinks", &self.shared.sinks)
            .finish_non_exhaustive()
    }
}

/// Configures an [`App`]: a mutable builder whose setters return `&mut Self`.
///
/// [`build`](Self::build) moves the configuration out and leaves the builder
/// empty, still attached to its Runtime. Unset ports default to bounded
/// in-memory implementations; port setters take the last call.
///
/// ```no_run
/// use std::time::Duration;
///
/// use noctis_core::{App, AppBuilder, Error};
/// use twilight_model::gateway::Intents;
///
/// fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
///     builder
///         .intents(Intents::GUILD_MESSAGES)
///         .http_timeout(Duration::from_secs(5))
/// }
///
/// # #[tokio::main] async fn main() -> Result<(), Error> {
/// // Temporaries live to the end of the statement.
/// let app = configure(&mut App::from_env()?).build()?;
/// # Ok(()) }
/// ```
pub struct AppBuilder {
    runtime: Runtime,
    token: Option<Token>,
    intents: Intents,
    http_timeout: Duration,
    /// The raw, unvalidated proxy; `build` normalizes it.
    http_proxy: Option<String>,
    /// The raw edge tuning; `build` checks it.
    http_config: HttpConfig,
    /// The raw, unvalidated gateway URL; `build` normalizes it.
    gateway_url: Option<String>,
    /// The raw fixed shard total; `build` rejects 0.
    shard_total: Option<u32>,
    states: StateMap,
    ports: PortOverrides,
    renderer: Option<Arc<dyn Renderer>>,
    /// The mounted Module trees, in mount order.
    modules: Vec<Module>,
    /// The raw `(capacity, overflow)` of the event lane; `build` checks it.
    event_lane: (usize, usize),
    /// The raw handler concurrency; `build` checks it.
    handler_concurrency: usize,
    /// The raw shutdown grace; `build` checks it.
    shutdown_grace: Duration,
    /// The custom Error Sinks, by origin.
    error_sinks: ErrorSinks,
    /// Plugins waiting for `install`.
    plugins: Vec<PluginEntry>,
    /// Every plugin type registered since the last `build`.
    plugin_types: HashSet<TypeId>,
    violations: Vec<BuildViolation>,
}

/// Generates the port setters, each with its rustdoc and doctest.
macro_rules! port_setters {
    ($($method:ident($port:ident) = $import:literal, $example:literal;)*) => {$(
        #[doc = concat!(
            "Sets the [`", stringify!($port), "`] port (default: `", $example, "`); ",
            "the last call wins.\n\n",
            "```\n",
            "use noctis_ports::memory::", $import, ";\n\n",
            "let mut builder = noctis_core::App::builder();\n",
            "builder.", stringify!($method), "(", $example, ");\n",
            "```",
        )]
        pub fn $method(&mut self, port: impl $port) -> &mut Self {
            self.ports.$method = Some(Arc::new(port));
            self
        }
    )*};
}

impl AppBuilder {
    fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            token: None,
            intents: Intents::GUILDS,
            http_timeout: DEFAULT_HTTP_TIMEOUT,
            http_proxy: None,
            http_config: HttpConfig::default(),
            gateway_url: None,
            shard_total: None,
            states: StateMap::default(),
            ports: PortOverrides::default(),
            renderer: None,
            modules: Vec::new(),
            event_lane: (DEFAULT_EVENT_CAPACITY.get(), DEFAULT_OVERFLOW_CAPACITY),
            handler_concurrency: DEFAULT_HANDLER_CONCURRENCY,
            shutdown_grace: DEFAULT_SHUTDOWN_GRACE,
            error_sinks: ErrorSinks::default(),
            plugins: Vec::new(),
            plugin_types: HashSet::new(),
            violations: Vec::new(),
        }
    }

    /// [`App::from_env`] over `lookup` instead of the process environment.
    pub(crate) fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Result<Self, EnvError> {
        let mut builder = App::builder();
        if let Some(raw) = read_var(&lookup, TOKEN_VAR)? {
            let token = Token::new(&raw).map_err(|source| EnvError::InvalidToken {
                var: TOKEN_VAR,
                source,
            })?;
            builder.token(token);
        }
        if let Some(proxy) = read_var(&lookup, HTTP_PROXY_VAR)? {
            builder.http_proxy(proxy);
        }
        if let Some(url) = read_var(&lookup, GATEWAY_URL_VAR)? {
            builder.gateway_url(url);
        }
        if let Some(raw) = read_var(&lookup, SHARD_TOTAL_VAR)? {
            let total = parse_shard_total(&raw).ok_or(EnvError::InvalidShardTotal {
                var: SHARD_TOTAL_VAR,
            })?;
            builder.shard_total(total);
        }
        if let Some(raw) = read_var(&lookup, SHUTDOWN_GRACE_VAR)? {
            let secs = parse_grace_secs(&raw).ok_or(EnvError::InvalidShutdownGrace {
                var: SHUTDOWN_GRACE_VAR,
            })?;
            builder.shutdown_grace(Duration::from_secs(secs));
        }
        Ok(builder)
    }

    /// Sets the bot token.
    ///
    /// ```
    /// use noctis_core::{App, Token};
    ///
    /// let mut builder = App::builder();
    /// builder.token(Token::new("abc.def")?);
    /// # Ok::<(), noctis_core::TokenError>(())
    /// ```
    pub fn token(&mut self, token: Token) -> &mut Self {
        self.token = Some(token);
        self
    }

    /// Registers shared state, read as [`State<T>`]. One value per type: a
    /// second value of `T` is a [`BuildViolation::DuplicateState`] and the
    /// first is kept.
    ///
    /// ```
    /// struct Config {
    ///     greeting: String,
    /// }
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.state(Config { greeting: "hi".into() });
    /// ```
    pub fn state<T: Send + Sync + 'static>(&mut self, value: T) -> &mut Self {
        if !self.states.insert(value) {
            self.violations.push(BuildViolation::DuplicateState {
                type_name: std::any::type_name::<T>(),
            });
        }
        self
    }

    /// Adds gateway intents to the requested set; `GUILDS` is always in it.
    ///
    /// ```
    /// use twilight_model::gateway::Intents;
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.intents(Intents::GUILD_MESSAGES).intents(Intents::MESSAGE_CONTENT);
    /// ```
    pub fn intents(&mut self, intents: Intents) -> &mut Self {
        self.intents |= intents;
        self
    }

    /// Sets the Discord HTTP request timeout: 1 ms..=300 s, default 10 s. A
    /// value outside the range is a [`BuildViolation::InvalidHttpTimeout`].
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.http_timeout(std::time::Duration::from_secs(5));
    /// ```
    pub fn http_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.http_timeout = timeout;
        self
    }

    /// Tunes the Discord HTTP edge (see [`HttpConfig`]); the defaults are
    /// tuned for Discord. `build()` records one
    /// [`BuildViolation::InvalidHttpConfig`] per out-of-range field. The
    /// last call wins.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// let mut http = noctis_core::HttpConfig::default();
    /// http.hedge_after = Duration::from_millis(800);
    /// let mut builder = noctis_core::App::builder();
    /// builder.http(http);
    /// ```
    pub fn http(&mut self, config: HttpConfig) -> &mut Self {
        self.http_config = config;
        self
    }

    /// Sends every Discord HTTP request through a proxy (e.g. twilight's
    /// http-proxy, or the testkit's Discord HTTP fake) over plain HTTP and
    /// turns the local rate limiter off. The last call wins.
    ///
    /// Takes `host[:port]` or `http://host[:port]` (IPv6 hosts in brackets);
    /// `build()` validates it and records a
    /// [`BuildViolation::InvalidHttpProxy`] when it is anything else.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.http_proxy("http://proxy:3000");
    /// ```
    pub fn http_proxy(&mut self, proxy: impl Into<String>) -> &mut Self {
        self.http_proxy = Some(proxy.into());
        self
    }

    /// Connects every shard to `url` instead of `wss://gateway.discord.gg`
    /// (e.g. a gateway proxy, or the testkit's gateway fake), through
    /// twilight's gateway `proxy_url`. The last call wins.
    ///
    /// Takes `ws://host[:port]` or `wss://host[:port]` (IPv6 hosts in
    /// brackets, one trailing `/` allowed); `build()` validates it and records
    /// a [`BuildViolation::InvalidGatewayUrl`] when it is anything else.
    /// Resumes go to the `resume_gateway_url` that READY names, so a gateway
    /// proxy must rewrite it.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.gateway_url("ws://127.0.0.1:3000");
    /// ```
    pub fn gateway_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.gateway_url = Some(url.into());
        self
    }

    /// Fixes the shard total instead of using Discord's recommendation; the
    /// last call wins.
    ///
    /// `build()` records [`BuildViolation::InvalidShardTotal`] for 0. At
    /// [`App::run`], a total below ⌈recommended × 2 / 5⌉ is
    /// [`StartError::ShardTotalTooSmall`] (GW-03), and a total that is not a
    /// multiple of Discord's `max_concurrency` (when above 1) logs a warning.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.shard_total(16);
    /// ```
    pub fn shard_total(&mut self, total: u32) -> &mut Self {
        self.shard_total = Some(total);
        self
    }

    /// Mounts a [`Module`] tree. Trees fold in mount order (plugins
    /// included), each parent before its children; the handlers' event
    /// types become the gateway filter, and their non-privileged intents
    /// are added to the requested set. A handler needing a privileged
    /// intent that was not requested is a [`BuildViolation::MissingIntent`]
    /// (GW-05).
    ///
    /// ```
    /// use noctis_core::{App, Module};
    ///
    /// let mut builder = App::builder();
    /// builder.module(Module::new("profile")).module(Module::new("ops"));
    /// ```
    pub fn module(&mut self, module: Module) -> &mut Self {
        self.modules.push(module);
        self
    }

    /// Bounds the event lane: `capacity` queued events (1..=1048576,
    /// default 4096) and `overflow` non-droppable events behind a full
    /// lane (0..=4194304, default 16384). Out of range is a
    /// [`BuildViolation::InvalidEventLane`]. The last call wins.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.event_lane(8192, 32768);
    /// ```
    pub fn event_lane(&mut self, capacity: usize, overflow: usize) -> &mut Self {
        self.event_lane = (capacity, overflow);
        self
    }

    /// How many events run their handlers at once (1..=65536, default
    /// 256); interactions never wait for this bound. Out of range is a
    /// [`BuildViolation::InvalidHandlerConcurrency`]. The last call wins.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.handler_concurrency(64);
    /// ```
    pub fn handler_concurrency(&mut self, limit: usize) -> &mut Self {
        self.handler_concurrency = limit;
        self
    }

    /// How long the shutdown waits for queued and running handlers before it
    /// cancels them: 0 s..=900 s, default 10 s. Out of range is a
    /// [`BuildViolation::InvalidShutdownGrace`]. The last call wins.
    ///
    /// The whole shutdown takes at most the grace period plus 7 s (1 s for
    /// cancelled handlers, 6 s for the shards to close); keep it below your
    /// orchestrator's kill timeout (Kubernetes: 30 s by default).
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.shutdown_grace(std::time::Duration::from_secs(15));
    /// ```
    pub fn shutdown_grace(&mut self, grace: Duration) -> &mut Self {
        self.shutdown_grace = grace;
        self
    }

    /// Routes the failures of `origin` (handler errors and panics, and for
    /// [`ErrorOrigin::Event`] also handlers cancelled at shutdown and events
    /// dropped by a full overflow) to `sink` instead of
    /// [`LogSink`](crate::LogSink). The last call per origin wins; plugins
    /// may call it in `install`. Pass an `Arc` clone to share one sink
    /// between origins.
    ///
    /// ```
    /// use noctis_core::{App, ErrorOrigin, ErrorReport, ErrorSink, ReportKind};
    ///
    /// struct Alert;
    /// impl ErrorSink for Alert {
    ///     fn report(&self, report: ErrorReport<'_>) {
    ///         if let ReportKind::Panic(message) = report.kind {
    ///             tracing::error!(origin = %report.origin, message, "page on-call");
    ///         }
    ///     }
    /// }
    ///
    /// let mut builder = App::builder();
    /// builder.error_sink(ErrorOrigin::Event, Alert);
    /// ```
    pub fn error_sink(&mut self, origin: ErrorOrigin, sink: impl ErrorSink) -> &mut Self {
        self.error_sinks.set(origin, Arc::new(sink));
        self
    }

    /// Attaches the App to `runtime` instead of [`Runtime::global`].
    ///
    /// ```
    /// use noctis_core::{App, Runtime};
    ///
    /// let runtime = Runtime::for_test();
    /// let mut builder = App::builder();
    /// builder.runtime(&runtime);
    /// assert_eq!(builder.current_runtime(), &runtime);
    /// ```
    pub fn runtime(&mut self, runtime: &Runtime) -> &mut Self {
        self.runtime = runtime.clone();
        self
    }

    /// The Runtime the App will attach to; plugins request their process
    /// resources from it.
    ///
    /// ```
    /// let builder = noctis_core::App::builder();
    /// assert_eq!(builder.current_runtime(), noctis_core::Runtime::global());
    /// ```
    pub fn current_runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Adds a [`Plugin`]; `build` installs it. One per type: a second plugin
    /// of a type is a [`BuildViolation::DuplicatePlugin`] and the first is
    /// kept.
    ///
    /// ```
    /// use noctis_core::{App, AppBuilder, Plugin};
    ///
    /// struct Noop;
    /// impl Plugin for Noop {
    ///     fn install(&self, _: &mut AppBuilder) {}
    /// }
    ///
    /// let mut builder = App::builder();
    /// builder.plugin(Noop);
    /// ```
    pub fn plugin<P: Plugin>(&mut self, plugin: P) -> &mut Self {
        let entry = PluginEntry::new(plugin);
        if self.plugin_types.insert(entry.type_id) {
            self.plugins.push(entry);
        } else {
            self.violations.push(BuildViolation::DuplicatePlugin {
                type_name: entry.type_name,
            });
        }
        self
    }

    /// The added plugin of type `P`, to change before `build` installs it.
    ///
    /// A plugin whose install batch is running (inside `build`) is not
    /// visible.
    ///
    /// ```
    /// use noctis_core::{App, AppBuilder, Plugin};
    ///
    /// struct Prefix(&'static str);
    /// impl Plugin for Prefix {
    ///     fn install(&self, _: &mut AppBuilder) {}
    /// }
    ///
    /// let mut builder = App::builder();
    /// builder.plugin(Prefix("!"));
    /// builder.plugin_mut::<Prefix>().unwrap().0 = "?";
    /// ```
    pub fn plugin_mut<P: Plugin>(&mut self) -> Option<&mut P> {
        self.plugins
            .iter_mut()
            .find(|e| e.type_id == TypeId::of::<P>())
            .and_then(|e| e.plugin.as_any_mut().downcast_mut::<P>())
    }

    /// Records a violation; `build` returns it with every other one. Plugins
    /// report with [`BuildViolation::plugin`].
    ///
    /// ```
    /// use noctis_core::{App, AppBuilder, BuildViolation, Plugin};
    ///
    /// struct MyPlugin;
    ///
    /// impl Plugin for MyPlugin {
    ///     fn install(&self, _app: &mut AppBuilder) {}
    /// }
    ///
    /// let mut builder = App::builder();
    /// builder.report(BuildViolation::plugin::<MyPlugin>("greeting is empty: pass some text"));
    /// ```
    pub fn report(&mut self, violation: BuildViolation) -> &mut Self {
        self.violations.push(violation);
        self
    }

    /// Sets the [`Renderer`] (none by default; the image plugin sets it).
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// use noctis_ports::{BoxFuture, RenderError, RenderJob, RenderOutput, Renderer, TemplateDescriptor};
    ///
    /// struct Fake;
    /// impl Renderer for Fake {
    ///     fn render(&self, _: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
    ///         unimplemented!()
    ///     }
    ///     fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.renderer(Arc::new(Fake));
    /// ```
    pub fn renderer(&mut self, renderer: Arc<dyn Renderer>) -> &mut Self {
        self.renderer = Some(renderer);
        self
    }

    port_setters! {
        ref_store(RefStore) = "MemoryRefStore", "MemoryRefStore::new()";
        session_store(SessionStore) = "MemorySessionStore", "MemorySessionStore::new()";
        identify_queue(IdentifyQueue) = "MemoryIdentifyQueue", "MemoryIdentifyQueue::default()";
        module_state_store(ModuleStateStore) = "MemoryModuleStateStore", "MemoryModuleStateStore::new()";
        rate_limit_store(RateLimitStore) = "MemoryRateLimitStore", "MemoryRateLimitStore::new()";
        cache_store(CacheStore) = "MemoryCacheStore", "MemoryCacheStore::new()";
        render_cache(RenderCache) = "MemoryRenderCache", "MemoryRenderCache::new()";
        asset_store(AssetStore) = "MemoryAssetStore", "MemoryAssetStore::new()";
        locale_source(LocaleSource) = "NoLocaleOverlay", "NoLocaleOverlay";
        guild_access(GuildAccessList) = "AllowAllGuilds", "AllowAllGuilds";
        discord_cache_backend(DiscordCache) = "{DiscordCacheResources, MemoryDiscordCache}",
            "MemoryDiscordCache::new(DiscordCacheResources::empty())";
    }

    /// Installs the plugins, runs the App-build checks and builds the App.
    ///
    /// Installs every plugin once, in registration order (plugins added during
    /// an install afterwards, in the same build), then checks the HTTP
    /// timeout, the HTTP proxy, the gateway URL, the shard total, the event
    /// lane, handler concurrency, the shutdown grace and the [`HttpConfig`],
    /// binds the Discord HTTP edge's loopback listener, folds the mounted
    /// Modules into the Registry (deriving the gateway filter and intents, GW-05,
    /// and merging every Module's Commands and context menus: the command
    /// rules run once on the merged set, per scope, and an undescribed base or
    /// group takes a handler's description with a warning), the rustls
    /// crypto provider (installing `ring`'s when none is set) and the tokio
    /// runtime (any flavor). The edge then starts serving on the current
    /// runtime. The builder is left empty, attached to the same Runtime.
    ///
    /// # Errors
    ///
    /// One [`BuildError`] with every violation: setters', plugins' reports
    /// and the checks', in that order; among the checks, missing intents
    /// (GW-05), then every [`BuildViolation::Command`] (in the rule engine's
    /// order), then the crypto provider and the runtime.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::BuildError> {
    /// let app = noctis_core::App::builder().build()?;
    /// # Ok(()) }
    /// ```
    pub fn build(&mut self) -> Result<App, BuildError> {
        self.install_plugins();
        if !HTTP_TIMEOUT_RANGE.contains(&self.http_timeout) {
            self.violations.push(BuildViolation::InvalidHttpTimeout {
                actual: self.http_timeout,
            });
        }
        let http_proxy = match self.http_proxy.as_deref().map(normalize_http_proxy) {
            None => None,
            Some(Ok(proxy)) => Some(proxy),
            Some(Err(reason)) => {
                self.violations
                    .push(BuildViolation::InvalidHttpProxy { reason });
                None
            }
        };
        let gateway_url = match self.gateway_url.as_deref().map(normalize_gateway_url) {
            None => None,
            Some(Ok(url)) => Some(url),
            Some(Err(reason)) => {
                self.violations
                    .push(BuildViolation::InvalidGatewayUrl { reason });
                None
            }
        };
        let shard_total = match self.shard_total {
            None => None,
            Some(total) => {
                let total = NonZeroU32::new(total);
                if total.is_none() {
                    self.violations.push(BuildViolation::InvalidShardTotal);
                }
                total
            }
        };
        let (capacity, overflow) = self.event_lane;
        let event_capacity = NonZeroUsize::new(capacity)
            .filter(|_| EVENT_CAPACITY_RANGE.contains(&capacity))
            .filter(|_| EVENT_OVERFLOW_RANGE.contains(&overflow));
        if event_capacity.is_none() {
            self.violations
                .push(BuildViolation::InvalidEventLane { capacity, overflow });
        }
        if !HANDLER_CONCURRENCY_RANGE.contains(&self.handler_concurrency) {
            self.violations
                .push(BuildViolation::InvalidHandlerConcurrency {
                    actual: self.handler_concurrency,
                });
        }
        if !SHUTDOWN_GRACE_RANGE.contains(&self.shutdown_grace) {
            self.violations.push(BuildViolation::InvalidShutdownGrace {
                actual: self.shutdown_grace,
            });
        }
        let edge_listener = self.check_http();
        let (registry, missing) = Registry::fold(&self.modules, self.intents);
        self.violations.extend(missing);
        if ensure_crypto_provider().is_err() {
            self.violations.push(BuildViolation::CryptoProvider);
        }
        if check_runtime(false).is_err() {
            self.violations.push(BuildViolation::NoTokioRuntime);
        }
        let fresh = Self::new(self.runtime.clone());
        let config = std::mem::replace(self, fresh);
        let (Some(event_capacity), Some((listener, edge_addr))) = (
            event_capacity.filter(|_| config.violations.is_empty()),
            edge_listener,
        ) else {
            return Err(BuildError::new(config.violations));
        };
        let app: Arc<str> = Arc::from("default");
        let upstream = Upstream::for_proxy(http_proxy.as_deref(), config.http_config.upstream_h2c);
        let edge = Edge::start((listener, edge_addr), upstream, config.http_config, &app);
        let http = http_client(&config, &edge, http_proxy.is_some());
        Ok(App {
            runtime: config.runtime,
            token: config.token,
            shared: Arc::new(AppShared {
                // "default" until `AppId` exists (Story 7.4).
                app,
                sinks: config.error_sinks,
                http: Arc::new(http),
                states: config.states,
                edge,
            }),
            http_timeout: config.http_timeout,
            http_proxy,
            http_config: config.http_config,
            gateway_url,
            shard_total,
            intents: config.intents | registry.intents(),
            ports: Ports::resolve(config.ports),
            renderer: config.renderer,
            registry: Arc::new(registry),
            event_capacity,
            overflow_capacity: overflow,
            handler_concurrency: config.handler_concurrency,
            shutdown_grace: config.shutdown_grace,
            lifecycle: Lifecycle::new(),
        })
    }

    /// Records one violation per out-of-range [`HttpConfig`] field, then
    /// binds the edge's loopback listener (a failure is a violation too).
    fn check_http(&mut self) -> Option<(std::net::TcpListener, std::net::SocketAddr)> {
        for reason in self.http_config.violations() {
            self.violations
                .push(BuildViolation::InvalidHttpConfig { reason });
        }
        match Edge::bind() {
            Ok(bound) => Some(bound),
            Err(error) => {
                self.violations
                    .push(BuildViolation::HttpEdge { kind: error.kind() });
                None
            }
        }
    }

    /// Calls each pending plugin's `install` once, batch by batch, until an
    /// install batch adds no new plugin.
    fn install_plugins(&mut self) {
        loop {
            let batch = std::mem::take(&mut self.plugins);
            if batch.is_empty() {
                break;
            }
            for entry in &batch {
                entry.plugin.install(self);
            }
        }
    }
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "plugin_types duplicates `plugins`; port overrides are listed by slot; modules print as a count"
)]
impl fmt::Debug for AppBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plugins: Vec<&str> = self.plugins.iter().map(|e| e.type_name).collect();
        f.debug_struct("AppBuilder")
            .field("runtime", &self.runtime)
            .field("token", &self.token)
            .field("intents", &self.intents)
            .field("http_timeout", &self.http_timeout)
            .field("http_proxy", &self.http_proxy.is_some())
            .field("http_config", &self.http_config)
            .field("gateway_url", &self.gateway_url.is_some())
            .field("shard_total", &self.shard_total)
            .field("states", &self.states)
            .field("ports", &self.ports.set_slots())
            .field("has_renderer", &self.renderer.is_some())
            .field("modules", &self.modules.len())
            .field("event_lane", &self.event_lane)
            .field("handler_concurrency", &self.handler_concurrency)
            .field("shutdown_grace", &self.shutdown_grace)
            .field("error_sinks", &self.error_sinks)
            .field("plugins", &plugins)
            .field("violations", &self.violations)
            .finish()
    }
}

/// The App's one twilight client: every request goes to the loopback
/// `edge`; the local rate limiter is off only behind an `http_proxy`.
fn http_client(config: &AppBuilder, edge: &Edge, proxied: bool) -> twilight_http::Client {
    let mut http = twilight_http::Client::builder()
        .timeout(config.http_timeout)
        .proxy(edge.local_addr().to_string(), true);
    if let Some(token) = &config.token {
        http = http.token(token.expose().to_owned());
    }
    if proxied {
        http = http.ratelimiter(None);
    }
    http.build()
}

/// A shard total from 1-10 ASCII digits that fit a `u32` (leading zeros and
/// 0 accepted; `build()` rejects 0).
fn parse_shard_total(raw: &str) -> Option<u32> {
    let digits = (1..=10).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| raw.parse().ok()).flatten()
}

/// A shutdown grace in whole seconds from 1-4 ASCII digits (leading zeros
/// accepted; `build()` rejects more than 900).
fn parse_grace_secs(raw: &str) -> Option<u64> {
    let digits = (1..=4).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| raw.parse().ok()).flatten()
}

/// Reads `var` through `lookup`: trimmed of ASCII whitespace, empty means
/// unset.
fn read_var(
    lookup: &impl Fn(&str) -> Option<OsString>,
    var: &'static str,
) -> Result<Option<String>, EnvError> {
    let Some(raw) = lookup(var) else {
        return Ok(None);
    };
    let raw = raw
        .into_string()
        .map_err(|_| EnvError::NotUnicode { var })?;
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    Ok((!trimmed.is_empty()).then(|| trimmed.to_owned()))
}

#[cfg(test)]
mod tests;
