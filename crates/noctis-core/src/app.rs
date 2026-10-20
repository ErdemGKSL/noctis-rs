//! `App` and its mutable `AppBuilder` (FR-1, AD-24).

use std::any::TypeId;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::num::NonZeroU32;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use noctis_gateway::{
    EventTypeFlags, Gateway, GatewayOptions, Inbound, LaneConfig, LaneReceiver, ShardEvent,
};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_ports::{
    AssetStore, CacheStore, DiscordCache, GuildAccessList, IdentifyQueue, LocaleSource,
    ModuleStateStore, RateLimitStore, RefStore, RenderCache, Renderer, SessionStore,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use twilight_model::gateway::Intents;

use crate::gateway_url::normalize_gateway_url;
use crate::plugin::PluginEntry;
use crate::ports::{PortOverrides, PortSlot, Ports};
use crate::proxy::normalize_http_proxy;
use crate::sharding::{GatewayInfo, plan};
use crate::startup::{check_runtime, ensure_crypto_provider};
use crate::state::StateMap;
use crate::{
    BuildError, BuildViolation, EnvError, Error, Plugin, Runtime, StartError, State, Token,
};

/// The environment variable [`App::from_env`] reads the token from.
const TOKEN_VAR: &str = "NOCTIS_TOKEN";
/// The environment variable [`App::from_env`] reads the HTTP proxy from.
const HTTP_PROXY_VAR: &str = "NOCTIS_HTTP_PROXY";
/// The environment variable [`App::from_env`] reads the gateway URL from.
const GATEWAY_URL_VAR: &str = "NOCTIS_GATEWAY_URL";
/// The environment variable [`App::from_env`] reads the fixed shard total from.
const SHARD_TOTAL_VAR: &str = "NOCTIS_SHARD_TOTAL";
/// The HTTP request timeout when `http_timeout` is not called.
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// The allowed `http_timeout` range.
const HTTP_TIMEOUT_RANGE: RangeInclusive<Duration> =
    Duration::from_millis(1)..=Duration::from_mins(5);

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
    http: Arc<twilight_http::Client>,
    http_timeout: Duration,
    http_proxy: Option<String>,
    gateway_url: Option<String>,
    shard_total: Option<NonZeroU32>,
    intents: Intents,
    states: StateMap,
    ports: Ports,
    renderer: Option<Arc<dyn Renderer>>,
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
    /// Reads `NOCTIS_TOKEN`, `NOCTIS_HTTP_PROXY`, `NOCTIS_GATEWAY_URL` and then
    /// `NOCTIS_SHARD_TOTAL`, each trimmed of ASCII whitespace; an empty value
    /// counts as unset. The proxy and the gateway URL are passed to
    /// [`AppBuilder::http_proxy`] and [`AppBuilder::gateway_url`] and validated
    /// by `build()`; the shard total (1-10 ASCII digits) is passed to
    /// [`AppBuilder::shard_total`]. Setters called afterwards win.
    ///
    /// # Errors
    ///
    /// [`Error::Env`] when a variable is not Unicode, holds an invalid token or
    /// an invalid shard total; the message never shows the value.
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
        &self.http
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

    /// The requested gateway intents (at least `GUILDS`).
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
        self.states.get::<T>()
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

    /// Starts the App and runs its shards until every shard task has ended.
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
    /// lane (AD-10); one core task drains them, interactions first. Startup
    /// finishes once every shard has received READY; afterwards `run`
    /// returns `Ok(())` when every shard task has ended.
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
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::Error> {
    /// noctis_core::App::from_env()?.build()?.run().await?;
    /// # Ok(()) }
    /// ```
    pub async fn run(self) -> Result<(), Error> {
        check_runtime(true)?;
        ensure_crypto_provider().map_err(|_| StartError::CryptoProvider)?;
        let Some(token) = &self.token else {
            return Err(StartError::MissingToken.into());
        };
        let info = self.gateway_info().await?;
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
            token: token.expose().to_owned(),
            intents: self.intents,
            gateway_url: self.gateway_url.clone(),
            event_types: EventTypeFlags::INTERACTION_CREATE,
            identify_queue,
            total: plan.total,
            max_concurrency: plan.max_concurrency,
        };
        // `app` stays "default" until `AppId` exists (Story 7.4).
        let (sender, receiver) = noctis_gateway::lanes(LaneConfig::default());
        let _drain = AbortOnDrop(tokio::spawn(drain(receiver)));
        let (gateway, mut events) = Gateway::start(options, Arc::new(sender)).map_err(|error| {
            tracing::error!(target: "noctis::app", %error, "the shards could not be configured");
            StartError::GatewayStopped
        })?;
        await_startup(&mut events, total).await?;
        tracing::info!(target: "noctis::app", "all {total} shards ready");
        supervise(&mut events).await;
        drop(gateway);
        Ok(())
    }

    /// Reads `GET /gateway/bot`; every failure is logged with its `Display`
    /// (twilight never prints the token).
    async fn gateway_info(&self) -> Result<GatewayInfo, StartError> {
        use twilight_http::error::ErrorType;

        let response = self.http.gateway().authed().await.map_err(|error| {
            tracing::error!(target: "noctis::app", %error, "GET /gateway/bot failed");
            match error.kind() {
                ErrorType::Unauthorized => StartError::InvalidToken,
                ErrorType::Response { status, .. } if status.get() == 401 => {
                    StartError::InvalidToken
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

/// Waits until every shard in `0..total` has sent `Ready` at least once.
async fn await_startup(
    events: &mut mpsc::UnboundedReceiver<ShardEvent>,
    total: u32,
) -> Result<(), StartError> {
    let mut ready = vec![false; usize::try_from(total).unwrap_or(usize::MAX)];
    let mut missing = ready.len();
    while missing > 0 {
        let Some(event) = events.recv().await else {
            return Err(StartError::GatewayStopped);
        };
        match event {
            ShardEvent::Ready { shard } => {
                tracing::debug!(target: "noctis::app", shard, "shard ready");
                let slot = usize::try_from(shard)
                    .ok()
                    .and_then(|index| ready.get_mut(index));
                if let Some(seen) = slot
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

/// Aborts the task it holds when dropped, on every return path of `run`.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Takes every [`Inbound`] from the lanes, interactions first, until every
/// sender has dropped.
async fn drain(mut receiver: LaneReceiver) {
    while let Some(inbound) = receiver.recv().await {
        dispatch(inbound);
    }
}

/// Hands one [`Inbound`] to its handlers; never awaits. No handlers exist
/// before Story 5.6, so it traces the item and drops it.
fn dispatch(inbound: Inbound) {
    tracing::trace!(
        target: "noctis::app",
        shard = inbound.shard.number(),
        kind = ?inbound.kind,
        "no handler registered"
    );
    drop(inbound);
}

/// Logs shard events after startup until every shard task has ended.
async fn supervise(events: &mut mpsc::UnboundedReceiver<ShardEvent>) {
    while let Some(event) = events.recv().await {
        match event {
            ShardEvent::Ended { shard, close_code } => {
                tracing::error!(target: "noctis::app", shard, ?close_code, "shard ended");
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
            _ => {}
        }
    }
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("runtime", &self.runtime)
            .field("token", &self.token)
            .field("http_timeout", &self.http_timeout)
            .field("http_proxy", &self.http_proxy)
            .field("gateway_url", &self.gateway_url)
            .field("shard_total", &self.shard_total)
            .field("intents", &self.intents)
            .field("states", &self.states)
            .field("default_ports", self.ports.default_slots())
            .field("has_renderer", &self.has_renderer())
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
    /// The raw, unvalidated gateway URL; `build` normalizes it.
    gateway_url: Option<String>,
    /// The raw fixed shard total; `build` rejects 0.
    shard_total: Option<u32>,
    states: StateMap,
    ports: PortOverrides,
    renderer: Option<Arc<dyn Renderer>>,
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
            gateway_url: None,
            shard_total: None,
            states: StateMap::default(),
            ports: PortOverrides::default(),
            renderer: None,
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
    /// timeout, the HTTP proxy, the gateway URL, the shard total, the rustls
    /// crypto provider (installing `ring`'s when none is set) and the tokio
    /// runtime (any flavor). The builder is left empty,
    /// attached to the same Runtime.
    ///
    /// # Errors
    ///
    /// One [`BuildError`] with every violation: setters', plugins' reports
    /// and the checks', in that order.
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
        if ensure_crypto_provider().is_err() {
            self.violations.push(BuildViolation::CryptoProvider);
        }
        if check_runtime(false).is_err() {
            self.violations.push(BuildViolation::NoTokioRuntime);
        }
        let fresh = Self::new(self.runtime.clone());
        let config = std::mem::replace(self, fresh);
        if !config.violations.is_empty() {
            return Err(BuildError::new(config.violations));
        }
        let mut http = twilight_http::Client::builder().timeout(config.http_timeout);
        if let Some(token) = &config.token {
            http = http.token(token.expose().to_owned());
        }
        if let Some(proxy) = &http_proxy {
            http = http.proxy(proxy.clone(), true).ratelimiter(None);
        }
        Ok(App {
            runtime: config.runtime,
            token: config.token,
            http: Arc::new(http.build()),
            http_timeout: config.http_timeout,
            http_proxy,
            gateway_url,
            shard_total,
            intents: config.intents,
            states: config.states,
            ports: Ports::resolve(config.ports),
            renderer: config.renderer,
        })
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
    reason = "plugin_types duplicates `plugins`; port overrides are listed by slot"
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
            .field("gateway_url", &self.gateway_url.is_some())
            .field("shard_total", &self.shard_total)
            .field("states", &self.states)
            .field("ports", &self.ports.set_slots())
            .field("has_renderer", &self.renderer.is_some())
            .field("plugins", &plugins)
            .field("violations", &self.violations)
            .finish()
    }
}

/// A shard total from 1-10 ASCII digits that fit a `u32` (leading zeros and
/// 0 accepted; `build()` rejects 0).
fn parse_shard_total(raw: &str) -> Option<u32> {
    let digits = (1..=10).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_digit());
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
