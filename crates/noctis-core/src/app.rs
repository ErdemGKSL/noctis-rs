//! `App` and its mutable `AppBuilder` (FR-1, AD-24).

use std::any::TypeId;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::{Range, RangeInclusive};
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use noctis_codec::{PayloadKey, PayloadKeys};
use noctis_gateway::{
    CloseMode, DEFAULT_EVENT_CAPACITY, DEFAULT_OVERFLOW_CAPACITY, EventTypeFlags, Gateway,
    GatewayHandle, GatewayOptions, LaneConfig, LargeThreshold, Presence, PresenceHandle,
    ShardEvent,
};
use noctis_ports::memory::{MemoryDiscordCache, MemoryIdentifyQueue};
use noctis_ports::{
    AssetStore, CacheStore, DiscordCache, GuildAccessList, IdentifyQueue, LocaleSource,
    ModuleStateStore, RateLimitStore, RefStore, RenderCache, Renderer, RendererPorts, SessionStore,
};
use noctis_rules::Limits;
use tokio::sync::mpsc;
use tokio::time::Instant;
use twilight_model::gateway::Intents;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::cli::{CliError, CliMode, NoctisArgs, parse_guild_id};
use crate::component::{AppExpiredFn, Expired, ExpiredCtx, app_expired};
use crate::edge::{Edge, HttpConfig, KeeperHandle, Upstream, is_edge_failure};
use crate::gateway_url::normalize_gateway_url;
use crate::handler::AppShared;
use crate::lifecycle::{Lifecycle, LifecycleEvent};
use crate::module::Module;
use crate::namespace::{AppId, IntoPort, fallback_app_id};
use crate::observe::{describe_metrics, set_shard_status};
use crate::plugin::PluginEntry;
use crate::ports::{PortOverrides, PortSlot, Ports};
use crate::proxy::normalize_http_proxy;
use crate::publish::{Publish, PublishScope};
use crate::registry::Registry;
use crate::sharding::{GatewayInfo, ShardPlan, plan};
use crate::sink::{ErrorSinks, EventDropObserver};
use crate::startup::{Signals, check_runtime, ensure_crypto_provider};
use crate::state::StateMap;
use crate::worker::{AbortOnDrop, Shutdown, Workers, drain};
use crate::{
    BuildError, BuildViolation, Component, DiscordCacheConfig, EnvError, Error, ErrorOrigin,
    ErrorSink, HandlerResult, IntoResponse, LocaleOverlayError, LocaleOwner, ModalSubmit, Plugin,
    RateLimited, RunError, Runtime, StartError, State, Token,
};

/// The environment variable [`App::from_env`] reads the token from.
const TOKEN_VAR: &str = "NOCTIS_TOKEN";
/// The environment variable [`App::from_env`] reads the HTTP proxy from.
const HTTP_PROXY_VAR: &str = "NOCTIS_HTTP_PROXY";
/// The environment variable [`App::from_env`] reads the gateway URL from.
const GATEWAY_URL_VAR: &str = "NOCTIS_GATEWAY_URL";
/// The environment variable [`App::from_env`] reads the fixed shard total from.
const SHARD_TOTAL_VAR: &str = "NOCTIS_SHARD_TOTAL";
/// The environment variable [`App::from_env`] reads the Shard Range from.
const SHARD_RANGE_VAR: &str = "NOCTIS_SHARD_RANGE";
/// The environment variable [`App::from_env`] reads the shutdown grace from.
const SHUTDOWN_GRACE_VAR: &str = "NOCTIS_SHUTDOWN_GRACE";
/// The environment variable [`App::from_env`] reads the publisher switch from.
const PUBLISHER_VAR: &str = "NOCTIS_PUBLISHER";
/// The environment variable [`App::from_env`] reads the dev guild from.
const DEV_GUILD_VAR: &str = "NOCTIS_DEV_GUILD";
/// The environment variable [`App::from_env`] reads the payload key from.
const PAYLOAD_KEY_VAR: &str = "NOCTIS_PAYLOAD_KEY";
/// The environment variable [`App::from_env`] reads the previous payload key
/// (rotation) from.
const PAYLOAD_KEY_PREVIOUS_VAR: &str = "NOCTIS_PAYLOAD_KEY_PREVIOUS";
/// The environment variable [`App::from_env`] reads the App id from.
const APP_ID_VAR: &str = "NOCTIS_APP_ID";
/// The shutdown grace period when `shutdown_grace` is not called.
const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
/// The allowed `shutdown_grace` range.
const SHUTDOWN_GRACE_RANGE: RangeInclusive<Duration> = Duration::ZERO..=Duration::from_mins(15);
/// The auto-defer deadline when `ack_deadline` is not called (INT-01).
const DEFAULT_ACK_DEADLINE: Duration = Duration::from_secs(2);
/// The allowed `ack_deadline` range: above 2500 ms a hedged type-5
/// callback no longer fits Discord's 3 s.
const ACK_DEADLINE_RANGE: RangeInclusive<Duration> = Duration::ZERO..=Duration::from_millis(2500);
/// The Ref TTL when `ref_ttl` is not called (FR-17).
const DEFAULT_REF_TTL: Duration = Duration::from_hours(24);
/// The allowed `ref_ttl` range, the same as a Route's `ttl = ".."`.
const REF_TTL_RANGE: RangeInclusive<Duration> =
    Duration::from_secs(1)..=Duration::from_hours(365 * 24);
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
    /// The App id: the `app` label and the port namespace.
    id: AppId,
    /// The HTTP client and the state, shared with every `EventCtx`.
    shared: Arc<AppShared>,
    /// The merged strings (`shared.strings` as its concrete type), for the
    /// startup overlay install.
    strings: Arc<noctis_i18n::AppStrings>,
    http_timeout: Duration,
    http_proxy: Option<String>,
    http_config: HttpConfig,
    gateway_url: Option<String>,
    shard_total: Option<NonZeroU32>,
    /// The Shard Range, validated against `shard_total` by `build`.
    shard_range: Option<Range<u32>>,
    intents: Intents,
    ports: Ports,
    registry: Arc<Registry>,
    event_capacity: NonZeroUsize,
    overflow_capacity: usize,
    handler_concurrency: usize,
    shutdown_grace: Duration,
    /// Whether a normal run publishes globally at startup.
    publisher: bool,
    /// Whether shards resume stored sessions and shut down resumably.
    resume_sessions: bool,
    /// The presence every shard identifies with.
    presence: Option<Presence>,
    /// The IDENTIFY `large_threshold`.
    large_threshold: LargeThreshold,
    /// The default guild of `--publish-guild` without an id.
    dev_guild: Option<Id<GuildMarker>>,
    /// The crate-internal lifecycle broadcast (Lifecycle Hooks, Story 13.5).
    lifecycle: Lifecycle,
}

/// Keeps the App's [`GatewayHandle`] attached to a running [`Gateway`];
/// detaches it on drop, on every return path of `run_with` (after the
/// shutdown sequence waited for the shards to close).
struct Attached<'a>(&'a GatewayHandle);

impl<'a> Attached<'a> {
    fn new(handle: &'a GatewayHandle, gateway: &Gateway) -> Self {
        handle.attach(&gateway.handle());
        Self(handle)
    }
}

impl Drop for Attached<'_> {
    fn drop(&mut self) {
        self.0.detach();
    }
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
    /// `NOCTIS_SHARD_TOTAL`, `NOCTIS_SHARD_RANGE`, `NOCTIS_SHUTDOWN_GRACE`,
    /// `NOCTIS_PUBLISHER`,
    /// `NOCTIS_DEV_GUILD`, `NOCTIS_PAYLOAD_KEY`, `NOCTIS_PAYLOAD_KEY_PREVIOUS`
    /// and then `NOCTIS_APP_ID`, each trimmed of ASCII whitespace; an empty
    /// value counts as unset. The proxy and the gateway URL are passed to
    /// [`AppBuilder::http_proxy`] and [`AppBuilder::gateway_url`] and
    /// validated by `build()`; the shard total (1-10 ASCII digits) is passed
    /// to [`AppBuilder::shard_total`]; the Shard Range (`start..end`, each
    /// side 1-10 ASCII digits) to [`AppBuilder::shard_range`], whose bounds
    /// `build()` checks; the shutdown grace (1-4 ASCII digits
    /// of whole seconds) is passed to [`AppBuilder::shutdown_grace`], so
    /// 901-9999 fail at `build()`; the publisher switch (`true`, `false`,
    /// `1` or `0`, any ASCII case) is passed to [`AppBuilder::publisher`];
    /// the dev guild (1-20 ASCII digits, not 0) to [`AppBuilder::dev_guild`];
    /// the payload keys (64 hex digits each, [`PayloadKey::from_hex`]) to
    /// [`AppBuilder::payload_key`] and [`AppBuilder::previous_payload_key`];
    /// the App id, raw, to [`AppBuilder::app_id`], validated by `build()`.
    /// Setters called afterwards win.
    ///
    /// # Errors
    ///
    /// [`Error::Env`] when a variable is not Unicode, holds an invalid token,
    /// an invalid shard total, an invalid Shard Range
    /// ([`EnvError::InvalidShardRange`]), an invalid shutdown grace, an invalid
    /// publisher switch, an invalid dev guild or an invalid payload key
    /// ([`EnvError::InvalidPayloadKey`]); the message never shows the
    /// value.
    ///
    /// ```no_run
    /// let app = noctis_core::App::from_env()?;
    /// # Ok::<(), noctis_core::Error>(())
    /// ```
    pub fn from_env() -> Result<AppBuilder, Error> {
        Ok(AppBuilder::from_lookup(|var| std::env::var_os(var))?)
    }

    /// The App's id ([`AppBuilder::app_id`] or `NOCTIS_APP_ID`; by default
    /// the bot user id read from the token, in base36, else
    /// `default`): the `app` label of every span, metric and Error Sink
    /// report, and the namespace of every port key.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().app_id("my-bot").build().unwrap();
    /// assert_eq!(app.app_id().as_str(), "my-bot");
    /// assert_eq!(noctis_core::App::builder().build().unwrap().app_id().as_str(), "default");
    /// # }
    /// ```
    pub fn app_id(&self) -> &AppId {
        &self.id
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

    /// The App's webhook factory, for sends outside a handler (a startup
    /// log, a scheduled job).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// use twilight_model::id::Id;
    ///
    /// let app = noctis_core::App::builder().build().unwrap();
    /// let hook = app.webhooks().owned(Id::new(1), "token").unwrap();
    /// assert_eq!(hook.token(), "token");
    /// # }
    /// ```
    pub fn webhooks(&self) -> crate::Webhooks {
        crate::Webhooks::of(Some(&self.shared))
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

    /// The Limits every Responder validates outgoing messages against
    /// ([`AppBuilder::limits`]; [`Limits::default`], Strict, when unset).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// use noctis_rules::Limits;
    ///
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.limits(), &Limits::default());
    /// # }
    /// ```
    pub fn limits(&self) -> &Limits {
        &self.shared.limits
    }

    /// How long after an interaction arrives the auto-defer acknowledges
    /// it when its handler has not answered yet
    /// ([`AppBuilder::ack_deadline`]; 2000 ms when unset, INT-01).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.ack_deadline(), std::time::Duration::from_millis(2000));
    /// # }
    /// ```
    pub fn ack_deadline(&self) -> Duration {
        self.shared.ack_deadline
    }

    /// How long a Ref lives when its Route declares no `ttl`
    /// ([`AppBuilder::ref_ttl`]; 24 h when unset, FR-17).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.ref_ttl(), std::time::Duration::from_secs(24 * 3600));
    /// # }
    /// ```
    pub fn ref_ttl(&self) -> Duration {
        self.shared.ref_ttl
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

    /// Whether a normal run publishes globally at startup
    /// ([`AppBuilder::publisher`]; `false` when unset).
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().publisher(true).build().unwrap();
    /// assert!(app.is_publisher());
    /// # }
    /// ```
    pub fn is_publisher(&self) -> bool {
        self.publisher
    }

    /// Whether shards resume stored sessions at start and shut down
    /// resumably: the [`AppBuilder::resume_sessions`] value, `true` when
    /// unset. `--no-resume` overrides it inside
    /// [`run_with_args`](Self::run_with_args) for that run only, so this
    /// getter never reflects the flag.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().resume_sessions(false).build().unwrap();
    /// assert!(!app.resume_sessions());
    /// # }
    /// ```
    pub fn resume_sessions(&self) -> bool {
        self.resume_sessions
    }

    /// The Discord cache configuration ([`AppBuilder::discord_cache`];
    /// [`DiscordCacheConfig::default`] when unset).
    ///
    /// ```
    /// use noctis_core::DiscordCacheConfig;
    ///
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert_eq!(app.discord_cache(), DiscordCacheConfig::default());
    /// # }
    /// ```
    pub fn discord_cache(&self) -> DiscordCacheConfig {
        self.shared.discord_cache.config()
    }

    /// The event types the Discord cache configuration feeds on.
    fn cache_event_types(&self) -> EventTypeFlags {
        crate::discord_cache::event_types(self.discord_cache().resources())
    }

    /// The gateway event filter: the handlers' event types (interactions
    /// included) and the Discord cache's (FR-6).
    pub(crate) fn event_types(&self) -> EventTypeFlags {
        self.registry.event_types() | self.cache_event_types()
    }

    /// The App's gateway command handle (cheap to clone; the one
    /// `ctx.gateway()` returns): presence updates and Request Guild Members
    /// sent to the owning shard. Live while [`run`](Self::run) runs the
    /// shards (from right after they start until they closed); before, after
    /// and in a publish run every call is
    /// [`GatewayError::NotRunning`](noctis_gateway::GatewayError::NotRunning).
    /// Take it before `run` consumes the App.
    ///
    /// ```
    /// use noctis_gateway::{GatewayError, Presence};
    ///
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// let gateway = app.gateway();
    /// assert_eq!(gateway.presence().set(Presence::idle()), Err(GatewayError::NotRunning));
    /// # }
    /// ```
    pub fn gateway(&self) -> GatewayHandle {
        self.shared.gateway.clone()
    }

    /// The App's presence handle (`app.gateway().presence()`): set the
    /// presence of every shard or of one at run time.
    ///
    /// ```
    /// use noctis_gateway::{GatewayError, Presence};
    ///
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// let presence = app.presence();
    /// assert_eq!(presence.set(Presence::watching("/help")), Err(GatewayError::NotRunning));
    /// # }
    /// ```
    pub fn presence(&self) -> PresenceHandle {
        self.shared.gateway.presence()
    }

    /// The default guild of `--publish-guild` without an id
    /// ([`AppBuilder::dev_guild`]), if one was set.
    ///
    /// ```
    /// use twilight_model::id::Id;
    ///
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().dev_guild(Id::new(42)).build().unwrap();
    /// assert_eq!(app.dev_guild(), Some(Id::new(42)));
    /// # }
    /// ```
    pub fn dev_guild(&self) -> Option<Id<GuildMarker>> {
        self.dev_guild
    }

    /// A Publish of the Registry's commands to `scope`; `.await` it. See
    /// [`crate::publish`].
    ///
    /// Publish reads `GET /applications/@me`, builds the command JSON from
    /// the Registry alone, runs every check (META-04, and REG-03/REG-02 on
    /// the `Guild` dev mirror) before any write, then per target reads the
    /// remote commands and sends one bulk-overwrite `PUT` only when they
    /// differ. It never runs on its own on shard READY.
    ///
    /// ```no_run
    /// use noctis_core::publish::PublishScope;
    /// use twilight_model::id::Id;
    ///
    /// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let app = noctis_core::App::from_env()?.build()?;
    /// let report = app.publish(PublishScope::Guild(Id::new(42))).dry_run(true).await?;
    /// println!("{report}");
    /// # Ok(()) }
    /// ```
    pub fn publish(&self, scope: PublishScope) -> Publish<'_> {
        Publish::new(self, scope)
    }

    /// Whether a token is set.
    pub(crate) fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// The `app` span field.
    pub(crate) fn app_label(&self) -> &str {
        &self.shared.app
    }

    /// What every handler invocation shares with this App.
    pub(crate) fn shared(&self) -> &Arc<AppShared> {
        &self.shared
    }

    /// Replaces the App's strings; only before the App is shared.
    #[cfg(test)]
    pub(crate) fn set_strings(&mut self, strings: Arc<dyn noctis_rules::Translate>) {
        Arc::get_mut(&mut self.shared)
            .expect("the App is not shared yet")
            .strings = strings;
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

    /// The Shard Range this process runs ([`AppBuilder::shard_range`] or
    /// `NOCTIS_SHARD_RANGE`), if one was set; `None` runs every shard
    /// `0..total`.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder()
    ///     .shard_total(128)
    ///     .shard_range(32..64)
    ///     .build()
    ///     .unwrap();
    /// assert_eq!(app.shard_range(), Some(32..64));
    /// assert_eq!(noctis_core::App::builder().build().unwrap().shard_range(), None);
    /// # }
    /// ```
    pub fn shard_range(&self) -> Option<Range<u32>> {
        self.shard_range.clone()
    }

    /// The gateway intents the App identifies with: the requested ones (at
    /// least `GUILDS`) plus the non-privileged intents its event handlers
    /// and its [Discord cache resources](AppBuilder::discord_cache) derive.
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
        self.shared.renderer.is_some()
    }

    /// The App's [`Renderer`], if one was set (by a plugin or
    /// `.renderer(..)`); `ctx.render` renders through it.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().build().unwrap();
    /// assert!(app.renderer().is_none());
    /// # }
    /// ```
    pub fn renderer(&self) -> Option<&Arc<dyn Renderer>> {
        self.shared.renderer.as_ref()
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
            // A cache-fed event type is never dropped.
            droppable: self
                .registry
                .droppable()
                .difference(self.cache_event_types()),
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

    /// Runs the App as the process command line asks:
    /// [`run_with_args`](Self::run_with_args) with
    /// `std::env::args_os().skip(1)`.
    ///
    /// Without a Noctis flag it starts the bot and runs it until Ctrl-C or
    /// SIGTERM (Ctrl-C only on non-unix platforms), then shuts it down
    /// gracefully. Under `cargo test` the harness passes its own flags:
    /// tests call `run_with_args(std::iter::empty::<&str>())` instead.
    ///
    /// # Errors
    ///
    /// The errors of [`run_with_args`](Self::run_with_args); an unknown
    /// argument is [`Error::Cli`] ([`CliError::UnknownFlag`]).
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::Error> {
    /// noctis_core::App::from_env()?.build()?.run().await?;
    /// # Ok(()) }
    /// ```
    pub fn run(self) -> impl Future<Output = Result<(), Error>> + Send {
        self.run_with_args(std::env::args_os().skip(1))
    }

    /// Runs the App with an explicit command line (without the program
    /// name; see [`NoctisArgs`]).
    ///
    /// In order: parses `args` ([`Error::Cli`] before anything else); checks
    /// that a multi-thread tokio runtime is current and that a rustls crypto
    /// provider is installed (installing `ring`'s when none is set). With a
    /// mode (`--publish`, `--publish-guild [<id>]`, `--publish-clear [--guild
    /// <id>]`, each with optional `--dry-run`) it runs that Publish
    /// (`--publish-guild` without an id uses [`AppBuilder::dev_guild`], else
    /// [`CliError::MissingDevGuild`]), writes the
    /// [`PublishReport`](crate::publish::PublishReport) to stdout and
    /// returns, without installing signal handlers or connecting a shard.
    /// Without a mode, `--no-resume` sets
    /// [`resume_sessions`](AppBuilder::resume_sessions) to `false` for this
    /// run (logged at `info`), and it installs the signal handlers and runs
    /// [`run_until`](Self::run_until) with the first signal as the shutdown
    /// trigger.
    ///
    /// # Errors
    ///
    /// [`Error::Cli`] for a rejected command line, [`Error::Publish`] when
    /// the Publish of a mode fails, [`StartError::SignalHandler`] when the
    /// signal handlers cannot be installed (a runtime without an IO
    /// driver), else the errors of [`run_until`](Self::run_until).
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::Error> {
    /// let app = noctis_core::App::from_env()?.build()?;
    /// app.run_with_args(["--publish-guild", "123", "--dry-run"]).await?;
    /// # Ok(()) }
    /// ```
    pub fn run_with_args<I, A>(self, args: I) -> impl Future<Output = Result<(), Error>> + Send
    where
        I: IntoIterator<Item = A>,
        A: Into<OsString>,
    {
        // Parsed before the future exists: the iterator need not be `Send`.
        let parsed = NoctisArgs::parse(args);
        let mut app = self;
        async move {
            let args = parsed?;
            check_runtime(true)?;
            ensure_crypto_provider().map_err(|_| StartError::CryptoProvider)?;
            if let Some(mode) = args.mode() {
                return app.run_mode(mode, args.dry_run()).await;
            }
            if args.no_resume() {
                app.resume_sessions = false;
                tracing::info!(target: "noctis::app", "session resume disabled (--no-resume)");
            }
            let signals = Signals::install()?;
            app.run_with(signals.recv(), "signal").await
        }
    }

    /// Runs one CLI mode's Publish and prints its report.
    async fn run_mode(&self, mode: CliMode, dry_run: bool) -> Result<(), Error> {
        // `Stdout::write_fmt` holds the stdout lock for the whole report;
        // a `StdoutLock` itself cannot be held across the await.
        self.run_mode_to(mode, dry_run, &mut std::io::stdout())
            .await
    }

    /// [`run_mode`](Self::run_mode) writing the report to `out`.
    async fn run_mode_to(
        &self,
        mode: CliMode,
        dry_run: bool,
        out: &mut (impl std::io::Write + Send),
    ) -> Result<(), Error> {
        let (scope, clear) = match mode {
            CliMode::Publish => (PublishScope::Global, false),
            CliMode::PublishGuild(id) => {
                let guild = id.or(self.dev_guild).ok_or(CliError::MissingDevGuild)?;
                (PublishScope::Guild(guild), false)
            }
            CliMode::PublishClear(id) => {
                (id.map_or(PublishScope::Global, PublishScope::Guild), true)
            }
        };
        let report = self.publish(scope).dry_run(dry_run).clear(clear).await?;
        // A closed stdout (a broken pipe) does not undo the Publish.
        let _ = writeln!(out, "{report}");
        Ok(())
    }

    /// Starts the App and runs it until `shutdown` resolves, then shuts it
    /// down gracefully. [`run`](Self::run) is `run_until` on Ctrl-C/SIGTERM;
    /// use this one to stop the App from code (tests, embedders).
    ///
    /// Checks, in order, that a multi-thread tokio runtime is current, that a
    /// rustls crypto provider is installed (installing `ring`'s when none is
    /// set) and that a token is set. It then loads the runtime locale
    /// overlays: with merged locale catalogs, one
    /// [`LocaleSource::load`](noctis_ports::LocaleSource::load) per locale
    /// that has a file (in `Locale::ALL` order, with codes such as
    /// `"en-US"`), validated against the compiled catalogs and installed
    /// process-wide, so `Text::Key`s, `reply_text` and `Strings::of(..)`
    /// render them (Publish keeps the compiled command texts). Then, with
    /// [`publisher`](AppBuilder::publisher), it runs the startup Publish,
    /// reads `GET /gateway/bot` with the App's HTTP client, plans the shard
    /// total (the fixed [`shard_total`](AppBuilder::shard_total) or Discord's
    /// recommendation) and the identify buckets (`max_concurrency`), and
    /// starts one task per shard of the
    /// [`shard_range`](AppBuilder::shard_range) (every shard `0..total`
    /// without one; a range not aligned to `max_concurrency` logs a
    /// warning). Each shard identifies through the `IdentifyQueue` port in
    /// bucket `shard_id % max_concurrency` (GW-01). Shard tasks push every
    /// parsed item into the lanes without waiting: interactions into the
    /// never-dropped Interaction Lane, other events into the bounded event
    /// lane (AD-10). The gateway parses only interactions and the event
    /// types the Module handlers take. One core task drains the lanes,
    /// interactions first; each event waits for one of
    /// [`handler_concurrency`](AppBuilder::handler_concurrency) worker
    /// permits (interactions never do) and runs its ordered handlers in
    /// per-guild order, then its concurrent handlers together. A shard
    /// whose session is stored in the `SessionStore` (left by a graceful,
    /// resumable shutdown of a previous run sharing that store) resumes it
    /// instead of identifying; a rejected stored session falls back to
    /// IDENTIFY. With [`resume_sessions`](AppBuilder::resume_sessions)
    /// `false` (`--no-resume`) each shard deletes its stored entry instead
    /// and identifies. Startup finishes once every shard of this process has
    /// received READY or RESUMED.
    ///
    /// A shard that closes fatally after startup is logged at `error`
    /// (`"shard closed fatally"`, with the typed cause) and its
    /// `noctis_shard_status` gauge goes to 0; the others keep running.
    ///
    /// When `shutdown` resolves (also during startup), or every shard has
    /// ended, the shutdown runs in this order (AD-21): log `shutdown
    /// started`; close every shard with code 4000 (resumable: once the
    /// gateway acknowledges the close, its session id, last received
    /// sequence and resume URL are written to the `SessionStore`), or with
    /// code 1000 and the stored entry deleted when
    /// [`resume_sessions`](AppBuilder::resume_sessions) is `false`; drain both
    /// lanes and wait for the running handlers for at most
    /// [`shutdown_grace`](AppBuilder::shutdown_grace), with the HTTP client
    /// still working; cancel what is left (each cancelled handler is
    /// reported to the `Event` Error Sink as
    /// [`ReportKind::Cancelled`](crate::ReportKind::Cancelled), queued lane
    /// items are discarded with one warning) and wait up to 1 s more; wait
    /// up to 6 s for the shards to close; log `shutdown complete`. Every
    /// session write has finished when this returns. A shutdown that
    /// resolves before `GET /gateway/bot` answered has no shard plan, so it
    /// writes and clears nothing. Dropping the returned future cancels every
    /// handler too.
    ///
    /// Delivery is at-most-once: the stored sequence is the last one
    /// received, so events the event lane dropped under back-pressure (or
    /// discarded at shutdown) count as seen and a resume never replays
    /// them.
    ///
    /// # Errors
    ///
    /// [`Error::Start`] with the [`StartError`] of the first failed step:
    /// the checks above, [`StartError::InvalidToken`] (HTTP 401 or close
    /// 4004), [`StartError::GatewayInfo`], [`StartError::InvalidGatewayInfo`],
    /// [`StartError::ShardTotalTooSmall`] (GW-03),
    /// [`StartError::SessionStartLimit`] (GW-02: `remaining` below the
    /// number of shards this process identifies), or a fatal close during
    /// startup ([`StartError::InvalidIntents`],
    /// [`StartError::DisallowedIntents`], [`StartError::FatalClose`],
    /// [`StartError::ShardEnded`], [`StartError::GatewayStopped`]).
    /// [`Error::LocaleOverlay`] when the `LocaleSource` fails
    /// ([`LocaleOverlayError::Load`]) or a loaded overlay breaks a rule
    /// ([`LocaleOverlayError::Invalid`]: a reserved `cmd.*`, `cmdword.*`,
    /// `ctxcmd.*` or `noctis.*` key, an unknown or duplicate key, a
    /// placeholder mismatch, or other overlays already installed in this
    /// process), before any Discord request.
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
        self.warn_cluster_without_proxy();
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
                // Overlays first: a bad one stops startup before any
                // Discord request.
                self.load_locale_overlays().await?;
                self.publish_at_startup().await;
                self.warm_up(warmed).await;
                Ok::<_, Error>(self.gateway_info().await?)
            } => info?,
        };
        let plan = plan(info, self.shard_total, self.shard_range.clone())?;
        let total = plan.total.get();
        let shards = plan.shards.clone();
        log_plan(&plan);
        let options = self.gateway_options(token, &plan);
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
        let _attached = Attached::new(&self.shared.gateway, &gateway);
        let status = ShardStatus {
            app: Arc::clone(&self.shared.app),
            shards,
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
                tracing::info!(target: "noctis::app", "all {} shards ready", status.count());
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

    /// The [`GatewayOptions`] of this App's shards under `plan`.
    fn gateway_options(&self, token: &Token, plan: &ShardPlan) -> GatewayOptions {
        let identify_queue: Arc<dyn IdentifyQueue> =
            if self.ports().is_default(PortSlot::IdentifyQueue) {
                Arc::new(MemoryIdentifyQueue::new(plan.max_concurrency))
            } else {
                Arc::clone(&self.ports().identify_queue)
            };
        GatewayOptions {
            app: Arc::clone(&self.shared.app),
            token: token.expose().to_owned(),
            intents: self.intents,
            gateway_url: self.gateway_url.clone(),
            event_types: self.event_types(),
            identify_queue,
            session_store: Arc::clone(&self.ports().session_store) as Arc<dyn SessionStore>,
            resume: self.resume_sessions,
            presence: self.presence.clone(),
            large_threshold: self.large_threshold,
            shards: plan.shards.clone(),
            total: plan.total,
            max_concurrency: plan.max_concurrency,
            close_timeout: SHARD_CLOSE_TIMEOUT,
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
        gateway.close_all(if self.resume_sessions {
            CloseMode::Resumable
        } else {
            CloseMode::Fresh
        });
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

    /// Loads the runtime locale overlays through the `LocaleSource` port,
    /// one `load` per available locale in [`Locale::ALL`] order (none
    /// without catalogs), validates them against the merged strings and
    /// installs them process-wide (Story 11.5).
    ///
    /// [`Locale::ALL`]: noctis_rules::Locale::ALL
    pub(crate) async fn load_locale_overlays(&self) -> Result<(), LocaleOverlayError> {
        if self.strings.catalogs().is_empty() {
            return Ok(());
        }
        let available = self.shared.locales.available();
        let source = &self.ports().locale_source;
        let mut overlays = Vec::new();
        for locale in noctis_rules::Locale::ALL
            .into_iter()
            .filter(|l| available.contains(*l))
        {
            let code = locale.to_string();
            let pairs = source
                .load(&code)
                .await
                .map_err(|source| LocaleOverlayError::Load { locale, source })?;
            overlays.push((locale, pairs));
        }
        let values: usize = overlays.iter().map(|(_, pairs)| pairs.len()).sum();
        self.strings
            .install_overlays(&overlays)
            .map_err(LocaleOverlayError::Invalid)?;
        if values > 0 {
            let catalogs: HashSet<&str> = overlays
                .iter()
                .flat_map(|(_, pairs)| pairs.iter())
                .filter_map(|(key, _)| self.strings.owner(key))
                .map(noctis_i18n::Catalog::origin)
                .collect();
            tracing::info!(
                target: "noctis::i18n",
                values,
                catalogs = catalogs.len(),
                "locale overlays installed"
            );
        }
        Ok(())
    }

    /// The publisher's global Publish (only with `publisher`), before any
    /// shard connects; a failure is logged and the App keeps starting.
    async fn publish_at_startup(&self) {
        if !self.publisher {
            return;
        }
        if let Err(error) = self.publish(PublishScope::Global).await {
            tracing::error!(target: "noctis::publish", %error, "startup Publish failed");
        }
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

    /// Logs the RL-02 warning once when this process runs part of a
    /// Cluster without a shared rate-limit proxy.
    fn warn_cluster_without_proxy(&self) {
        let range = self.shard_range.as_ref();
        if cluster_without_proxy(range, self.shard_total(), self.http_proxy()) {
            warn_cluster_without_proxy(range, self.shard_total());
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
                ErrorType::Response { status, body, .. } if is_edge_failure(status.get(), body) => {
                    StartError::GatewayInfo { status: None }
                }
                ErrorType::Response { .. } if RateLimited::from_http_error(&error).is_some() => {
                    StartError::GatewayInfo { status: Some(429) }
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

/// Whether this process runs part of a Cluster (a Shard Range smaller than
/// the shard total) without a shared rate-limit proxy (RL-02).
pub(crate) fn cluster_without_proxy(
    range: Option<&Range<u32>>,
    total: Option<u32>,
    proxy: Option<&str>,
) -> bool {
    match (range, total, proxy) {
        (Some(range), Some(total), None) => {
            range.len() < usize::try_from(total).unwrap_or(usize::MAX)
        }
        _ => false,
    }
}

/// The RL-02 warning: processes sharing one bot token without one shared
/// rate-limit proxy can exceed Discord's global limit.
pub(crate) fn warn_cluster_without_proxy(range: Option<&Range<u32>>, total: Option<u32>) {
    let (start, end) = range.map_or((0, 0), |r| (r.start, r.end));
    tracing::warn!(
        target: "noctis::app",
        start,
        end,
        total = total.unwrap_or(0),
        "this process runs a Shard Range of a Cluster without an HTTP proxy: processes sharing one \
         bot token can exceed Discord's global limit of 50 requests/s per token (RL-02); point every \
         process at one shared rate-limit proxy with NOCTIS_HTTP_PROXY or AppBuilder::http_proxy"
    );
}

/// Logs the plan's alignment warnings and the shards this process starts.
fn log_plan(plan: &ShardPlan) {
    let total = plan.total.get();
    let shards = &plan.shards;
    if plan.total_misaligned {
        tracing::warn!(
            target: "noctis::app",
            total,
            max_concurrency = plan.max_concurrency.get(),
            "shard total is not a multiple of max_concurrency; identify buckets are uneven"
        );
    }
    if plan.range_misaligned {
        tracing::warn!(
            target: "noctis::app",
            start = shards.start,
            end = shards.end,
            max_concurrency = plan.max_concurrency.get(),
            "shard range is not aligned to max_concurrency; identify buckets are split across processes"
        );
    }
    tracing::info!(
        target: "noctis::app",
        "starting shards {}..{} of {total}",
        shards.start,
        shards.end
    );
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

/// Keeps `noctis_shard_status{app, shard}` for this process's shards.
pub(crate) struct ShardStatus {
    pub(crate) app: Arc<str>,
    /// The shards this process runs (its Shard Range, or `0..total`).
    pub(crate) shards: Range<u32>,
    pub(crate) total: u32,
}

impl ShardStatus {
    /// The number of shards this process runs.
    pub(crate) fn count(&self) -> usize {
        usize::try_from(self.shards.end.saturating_sub(self.shards.start)).unwrap_or(usize::MAX)
    }

    /// The flag of `shard` in `flags` (one per shard of the range), if
    /// `shard` is in the range.
    fn slot<'a>(&self, flags: &'a mut [bool], shard: u32) -> Option<&'a mut bool> {
        if !self.shards.contains(&shard) {
            return None;
        }
        usize::try_from(shard - self.shards.start)
            .ok()
            .and_then(|index| flags.get_mut(index))
    }

    /// Sets every shard's gauge to 0 (down).
    pub(crate) fn init(&self) {
        for shard in self.shards.clone() {
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
        if self.shards.contains(&shard) {
            set_shard_status(&self.app, shard, self.total, up);
        }
    }
}

/// Marks every shard down when `App::run` returns or its future is dropped,
/// so a stopped App never reports a shard as up.
impl Drop for ShardStatus {
    fn drop(&mut self) {
        self.init();
    }
}

/// Waits until every shard of `status.shards` has sent `Ready` or
/// `Resumed` at least once, broadcasting each shard event; one for a shard
/// outside the range counts for nothing.
pub(crate) async fn await_startup(
    events: &mut mpsc::UnboundedReceiver<ShardEvent>,
    status: &ShardStatus,
    lifecycle: &Lifecycle,
) -> Result<(), StartError> {
    let mut ready = vec![false; status.count()];
    let mut missing = ready.len();
    while missing > 0 {
        let Some(event) = events.recv().await else {
            return Err(StartError::GatewayStopped);
        };
        status.update(&event);
        lifecycle.shard(&event);
        match event {
            ShardEvent::Ready { shard } | ShardEvent::Resumed { shard } => {
                let message = match event {
                    ShardEvent::Resumed { .. } => "shard resumed",
                    _ => "shard ready",
                };
                tracing::debug!(target: "noctis::app", shard, "{message}");
                if let Some(seen) = status.slot(&mut ready, shard)
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
    let mut ended = vec![false; status.count()];
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
            && let Some(seen) = status.slot(&mut ended, shard)
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
            .field("app_id", &self.id)
            .field("http_timeout", &self.http_timeout)
            .field("http_proxy", &self.http_proxy)
            .field("http_config", &self.http_config)
            .field("gateway_url", &self.gateway_url)
            .field("shard_total", &self.shard_total)
            .field("shard_range", &self.shard_range)
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
            .field("limits", &self.shared.limits)
            .field("ack_deadline", &self.shared.ack_deadline)
            .field("ref_ttl", &self.shared.ref_ttl)
            .field("payload_key", &self.shared.payload_keys.is_some())
            .field("publisher", &self.publisher)
            .field("resume_sessions", &self.resume_sessions)
            .field("discord_cache", &self.discord_cache())
            .field("presence", &self.presence)
            .field("large_threshold", &self.large_threshold)
            .field("dev_guild", &self.dev_guild)
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
    /// The raw, unvalidated App id; `build` checks it.
    app_id: Option<String>,
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
    /// The raw Shard Range; `build` checks it against `shard_total`.
    shard_range: Option<Range<u32>>,
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
    /// The Limits every Responder validates against.
    limits: Limits,
    /// The raw auto-defer deadline; `build` checks it.
    ack_deadline: Duration,
    /// The raw default Ref TTL; `build` checks it.
    ref_ttl: Duration,
    /// Whether a normal run publishes globally at startup.
    publisher: bool,
    /// Whether shards resume stored sessions and shut down resumably.
    resume_sessions: bool,
    /// The Discord cache configuration (`discord_cache`).
    discord_cache: DiscordCacheConfig,
    /// The presence every shard identifies with.
    presence: Option<Presence>,
    /// The IDENTIFY `large_threshold`.
    large_threshold: LargeThreshold,
    /// The default guild of `--publish-guild` without an id.
    dev_guild: Option<Id<GuildMarker>>,
    /// The current payload key (never printed).
    payload_key: Option<PayloadKey>,
    /// The previous payload key of a rotation (never printed).
    previous_payload_key: Option<PayloadKey>,
    /// The App-level Expired Handler (`expired_handler`).
    expired_handler: Option<Arc<AppExpiredFn>>,
    /// The App's locale override (`locale_resolver`).
    locale_resolver: Option<Arc<dyn crate::LocaleResolver>>,
    /// The hidden run-end observer (`__testing::observe_runs`).
    run_observer: Option<crate::handler::RunObserver>,
    /// The locale catalogs, in registration order (`locales`).
    catalogs: Vec<noctis_i18n::Catalog>,
    /// Plugins waiting for `install`.
    plugins: Vec<PluginEntry>,
    /// Every plugin type registered since the last `build`.
    plugin_types: HashSet<TypeId>,
    violations: Vec<BuildViolation>,
}

/// Generates the port setters, each with its rustdoc and doctest: the
/// byte-store setters take [`IntoPort`] (a port or a `Shared` one), the
/// plain ones `impl Port`.
/// The default text of a plain port setter: its explicit `default`, else
/// its example in backticks.
macro_rules! port_default {
    ($example:literal) => {
        concat!("`", $example, "`")
    };
    ($example:literal, $default:literal) => {
        $default
    };
}

macro_rules! port_setters {
    (
        namespaced { $($ns_method:ident($ns_port:ident) = $ns_import:literal, $ns_example:literal $(, $ns_note:literal)?;)* }
        plain { $($method:ident($port:ident) = $import:literal, $example:literal $(, default = $default:literal)? $(, $note:literal)?;)* }
    ) => {
        $(
            #[doc = concat!(
                "Sets the [`", stringify!($ns_port), "`] port (default: `", $ns_example, "`); ",
                "the last call wins.\n\n",
                "Its keys are prefixed with the App's namespace, `<app_id>:<port>:`. ",
                "Pass `Shared::new(adapter)` to share the entries with every other App ",
                "that does the same: the prefix is then `_shared:<port>:`. One instance can ",
                "be handed to several Apps as an `Arc` (`Arc::clone(&store)`).\n\n",
                $($ns_note, "\n\n",)?
                "```\n",
                "use noctis_core::Shared;\n",
                "use noctis_ports::memory::", $ns_import, ";\n\n",
                "let mut builder = noctis_core::App::builder();\n",
                "builder.", stringify!($ns_method), "(", $ns_example, ");\n",
                "builder.", stringify!($ns_method), "(Shared::new(", $ns_example, "));\n",
                "```",
            )]
            pub fn $ns_method(&mut self, port: impl IntoPort<dyn $ns_port>) -> &mut Self {
                self.ports.$ns_method = Some(port.into_port());
                self
            }
        )*
        $(
            #[doc = concat!(
                "Sets the [`", stringify!($port), "`] port (default: ",
                port_default!($example $(, $default)?), "); ",
                "the last call wins.\n\n",
                $($note, "\n\n",)?
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
        )*
    };
}

impl AppBuilder {
    fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            token: None,
            app_id: None,
            intents: Intents::GUILDS,
            http_timeout: DEFAULT_HTTP_TIMEOUT,
            http_proxy: None,
            http_config: HttpConfig::default(),
            gateway_url: None,
            shard_total: None,
            shard_range: None,
            states: StateMap::default(),
            ports: PortOverrides::default(),
            renderer: None,
            modules: Vec::new(),
            event_lane: (DEFAULT_EVENT_CAPACITY.get(), DEFAULT_OVERFLOW_CAPACITY),
            handler_concurrency: DEFAULT_HANDLER_CONCURRENCY,
            shutdown_grace: DEFAULT_SHUTDOWN_GRACE,
            error_sinks: ErrorSinks::default(),
            limits: Limits::default(),
            ack_deadline: DEFAULT_ACK_DEADLINE,
            ref_ttl: DEFAULT_REF_TTL,
            publisher: false,
            resume_sessions: true,
            discord_cache: DiscordCacheConfig::default(),
            presence: None,
            large_threshold: LargeThreshold::default(),
            dev_guild: None,
            payload_key: None,
            previous_payload_key: None,
            expired_handler: None,
            locale_resolver: None,
            run_observer: None,
            catalogs: Vec::new(),
            plugins: Vec::new(),
            plugin_types: HashSet::new(),
            violations: Vec::new(),
        }
    }

    /// The key ring of the configured payload keys (CID-06): a previous key
    /// without a current one is a [`BuildViolation::PreviousKeyWithoutKey`],
    /// and without a current key every mounted `signed` Route of `registry`
    /// is a [`BuildViolation::MissingPayloadKey`], in fold order.
    fn key_ring(&mut self, registry: &Registry) -> Option<Arc<PayloadKeys>> {
        let Some(current) = self.payload_key.clone() else {
            if self.previous_payload_key.is_some() {
                self.violations.push(BuildViolation::PreviousKeyWithoutKey);
            }
            self.violations.extend(
                registry
                    .signed_routes()
                    .iter()
                    .map(|&route| BuildViolation::MissingPayloadKey { route }),
            );
            return None;
        };
        let ring = PayloadKeys::new(current);
        Some(Arc::new(match self.previous_payload_key.clone() {
            Some(previous) => ring.with_previous(previous),
            None => ring,
        }))
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
        if let Some(raw) = read_var(&lookup, SHARD_RANGE_VAR)? {
            let range = parse_shard_range(&raw).ok_or(EnvError::InvalidShardRange {
                var: SHARD_RANGE_VAR,
            })?;
            builder.shard_range(range);
        }
        if let Some(raw) = read_var(&lookup, SHUTDOWN_GRACE_VAR)? {
            let secs = parse_grace_secs(&raw).ok_or(EnvError::InvalidShutdownGrace {
                var: SHUTDOWN_GRACE_VAR,
            })?;
            builder.shutdown_grace(Duration::from_secs(secs));
        }
        if let Some(raw) = read_var(&lookup, PUBLISHER_VAR)? {
            let publisher =
                parse_bool(&raw).ok_or(EnvError::InvalidPublisher { var: PUBLISHER_VAR })?;
            builder.publisher(publisher);
        }
        if let Some(raw) = read_var(&lookup, DEV_GUILD_VAR)? {
            let guild =
                parse_guild_id(&raw).ok_or(EnvError::InvalidDevGuild { var: DEV_GUILD_VAR })?;
            builder.dev_guild(guild);
        }
        for (var, previous) in [(PAYLOAD_KEY_VAR, false), (PAYLOAD_KEY_PREVIOUS_VAR, true)] {
            if let Some(raw) = read_var(&lookup, var)? {
                let key = PayloadKey::from_hex(&raw)
                    .map_err(|source| EnvError::InvalidPayloadKey { var, source })?;
                if previous {
                    builder.previous_payload_key(key);
                } else {
                    builder.payload_key(key);
                }
            }
        }
        if let Some(raw) = read_var(&lookup, APP_ID_VAR)? {
            builder.app_id(raw);
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

    /// Sets the App id: 1-32 characters of `a-z`, `0-9` and `-`
    /// ([`AppId`]). It is the `app` label of every span, metric and Error
    /// Sink report, and every byte-store port key is prefixed with
    /// `<app_id>:<port>:`, so two Apps sharing one Adapter never see each
    /// other's data. [`App::from_env`] reads it from `NOCTIS_APP_ID`.
    ///
    /// Without one, the id is the bot user id read from the token, in
    /// base36 ([`AppId::from_application_id`]); that equals the application
    /// id for most bots, so call this explicitly when it differs. When the
    /// token does not decode, it is `default` and `build()` logs a warning.
    /// An invalid id is a [`BuildViolation::InvalidAppId`]. The last call
    /// wins.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let app = noctis_core::App::builder().app_id("my-bot").build().unwrap();
    /// assert_eq!(app.app_id().as_str(), "my-bot");
    /// # }
    /// ```
    pub fn app_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.app_id = Some(id.into());
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

    /// Adds the locale catalog of an `i18n!` invocation (its `catalog()`):
    /// every `Text::Key`, `noctis.*` notice and `HandlerError` message key
    /// then resolves from its files (the response locale, its sibling, then
    /// the catalog's default locale), and keys no catalog has fall back to
    /// the built-in `noctis.*` texts.
    ///
    /// These catalogs merge with every mounted Module's
    /// ([`Module::locales`]) at `build`: the App's first, in call order
    /// (whatever the order of `locales` and [`module`](Self::module) calls),
    /// then each Module's in fold order. The merged strings are App-wide:
    /// every Module resolves every key, and disabling a Module never
    /// removes its strings. Catalogs with one `origin()` count once. A key
    /// that two catalogs define is a [`BuildViolation::DuplicateLocaleKey`]
    /// at `build`, naming both catalogs and their owners.
    ///
    /// The merged catalogs also drive locale resolution (FR-43): a locale
    /// "has a file" when any merged catalog ships one, and the App's
    /// default locale is the `default_locale()` of the first catalog in
    /// merge order (`en-US` without catalogs). Every run's
    /// `ctx.user_locale()` and `ctx.guild_locale()` resolve against them
    /// (see [`locale_resolver`](Self::locale_resolver)).
    ///
    /// ```
    /// use noctis_i18n::Catalog;
    /// use noctis_rules::Locale;
    ///
    /// // What `i18n!` generates as `catalog()`.
    /// static CATALOG: Catalog = Catalog::__new(
    ///     "my-bot/locales",
    ///     Locale::EnUs,
    ///     &[(Locale::EnUs, &[("shop.buy", "Buy")])],
    ///     &[("shop.buy", &[])],
    /// );
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.locales(CATALOG);
    /// ```
    pub fn locales(&mut self, catalog: noctis_i18n::Catalog) -> &mut Self {
        self.catalogs.push(catalog);
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

    /// Sets the Limits every Responder validates outgoing messages against
    /// before any HTTP request (default: [`Limits::default`], Strict). In
    /// Lenient mode an over-long text is truncated with `…` instead of
    /// failing with `ResponderError::Invalid`; the last call wins.
    ///
    /// ```
    /// use noctis_rules::{LimitMode, Limits};
    ///
    /// let mut limits = Limits::default();
    /// limits.mode = LimitMode::Lenient;
    /// let mut builder = noctis_core::App::builder();
    /// builder.limits(limits);
    /// ```
    pub fn limits(&mut self, limits: Limits) -> &mut Self {
        self.limits = limits;
        self
    }

    /// Sets when the auto-defer acknowledges an interaction whose handler
    /// has not answered yet, measured from the moment the gateway received
    /// it: 0 ms..=2500 ms, default 2000 ms (INT-01). 0 always defers. Out
    /// of range is a [`BuildViolation::InvalidAckDeadline`]. The last call
    /// wins.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.ack_deadline(std::time::Duration::from_millis(1500));
    /// ```
    pub fn ack_deadline(&mut self, deadline: Duration) -> &mut Self {
        self.ack_deadline = deadline;
        self
    }

    /// Sets how long a Ref lives when its Route declares no `ttl = ".."`:
    /// 1 s..=365 d, default 24 h (FR-17). The Responder stores every minted
    /// Ref with it; a Route's own `ttl` wins. Out of range is a
    /// [`BuildViolation::InvalidRefTtl`]. The last call wins.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.ref_ttl(std::time::Duration::from_secs(3600));
    /// ```
    pub fn ref_ttl(&mut self, ttl: Duration) -> &mut Self {
        self.ref_ttl = ttl;
        self
    }

    /// Sets the payload key that signs the ids of `signed` Routes and
    /// verifies them (AD-5, CID-06); [`App::from_env`] reads it from
    /// `NOCTIS_PAYLOAD_KEY` (64 hex digits, such as the output of
    /// `openssl rand -hex 32`). A mounted `signed` Route without a key is a
    /// [`BuildViolation::MissingPayloadKey`]. The key is never logged nor
    /// handed to a port or adapter. The last call wins.
    ///
    /// ```
    /// use noctis_codec::PayloadKey;
    ///
    /// let key = PayloadKey::from_hex(&"ab".repeat(32))?;
    /// let mut builder = noctis_core::App::builder();
    /// builder.payload_key(key);
    /// # Ok::<(), noctis_codec::KeyError>(())
    /// ```
    pub fn payload_key(&mut self, key: PayloadKey) -> &mut Self {
        self.payload_key = Some(key);
        self
    }

    /// Sets the previous payload key of a rotation: ids signed with it
    /// still verify, new ids are signed with the current
    /// [`payload_key`](Self::payload_key); [`App::from_env`] reads it from
    /// `NOCTIS_PAYLOAD_KEY_PREVIOUS`. Without a current key it is a
    /// [`BuildViolation::PreviousKeyWithoutKey`]. The last call wins.
    ///
    /// ```
    /// use noctis_codec::PayloadKey;
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder
    ///     .payload_key(PayloadKey::from_hex(&"cd".repeat(32))?)
    ///     .previous_payload_key(PayloadKey::from_hex(&"ab".repeat(32))?);
    /// # Ok::<(), noctis_codec::KeyError>(())
    /// ```
    pub fn previous_payload_key(&mut self, key: PayloadKey) -> &mut Self {
        self.previous_payload_key = Some(key);
        self
    }

    /// Sets the App-level Expired Handler (FR-18, AD-23 stage 2): it answers
    /// a click or a modal submit whose `custom_id` is stale (another
    /// `schema`, body kind or flag set of a mounted Route; [`Expired::Stale`])
    /// or carries a missing Ref ([`Expired::RefMissing`]) when the Route has
    /// no `#[on_stale]`/`#[on_expired]` method for it. Without one, Noctis
    /// sends the ephemeral `noctis.expired` notice. The last call wins.
    ///
    /// It takes no responder and is always auto-deferred: the auto-defer and
    /// a returned message follow the Route's declared `response` (an
    /// in-place update under `response = update`), even on an
    /// `auto_defer = false` Route. An error or a panic shows the user the
    /// error notice and goes to the Component Error Sink.
    ///
    /// ```
    /// use noctis_core::{Expired, ExpiredCtx, HandlerResult};
    /// use noctis_ui::{Components, TextDisplay, V2Message};
    ///
    /// async fn expired(ctx: ExpiredCtx, why: Expired) -> HandlerResult<V2Message> {
    ///     let text = format!("`{}` is {}: run the command again", ctx.route(), why.as_str());
    ///     Ok(V2Message::new(Components::new().push(TextDisplay::new(text))))
    /// }
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.expired_handler(expired);
    /// ```
    pub fn expired_handler<F, Fut, R>(&mut self, f: F) -> &mut Self
    where
        F: Fn(ExpiredCtx, Expired) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Component> + IntoResponse<ModalSubmit>,
    {
        self.expired_handler = Some(app_expired(f));
        self
    }

    /// Sets the App's [`LocaleResolver`](crate::LocaleResolver) (FR-43):
    /// stage 4 of every run calls it exactly once, once the handler is
    /// known and before its context is built (slash Commands, context
    /// menus, components, modal submits, the Expired Handler chain,
    /// autocomplete, events and `Dispatcher::run_command`), with the
    /// built-in resolution in [`LocaleRequest::resolved`](crate::LocaleRequest::resolved).
    /// Its return value is used verbatim; a before-Middleware may still
    /// override it (`BeforeCtx::set_locale`). A panic is caught: one `error`
    /// on `noctis::i18n` naming the handler, and the run keeps the built-in
    /// resolution. Notices sent before stage 4 (a malformed or unknown
    /// `custom_id`, a kind mismatch, a missing author) never call it. The
    /// last call wins.
    ///
    /// ```
    /// use noctis_core::{LocaleRequest, Locales};
    /// use noctis_rules::Locale;
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.locale_resolver(|request: &LocaleRequest| match request.guild_id {
    ///     Some(id) if id.get() == 7 => Locales { guild: Locale::PtBr, ..request.resolved },
    ///     _ => request.resolved,
    /// });
    /// ```
    pub fn locale_resolver(&mut self, resolver: impl crate::LocaleResolver) -> &mut Self {
        self.locale_resolver = Some(Arc::new(resolver));
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

    /// Runs only the shards `shards` of the fixed [`shard_total`](Self::shard_total)
    /// in this process: one process's Shard Range in a Cluster. The last
    /// call wins; without it the process runs every shard `0..total`.
    ///
    /// The range is half-open (`32..64` runs shards 32 to 63) and shard ids
    /// are global: each shard identifies as `[id, total]` in bucket
    /// `id % max_concurrency` (GW-01), so every process must set the same
    /// total and share one `IdentifyQueue` ([`identify_queue`](Self::identify_queue),
    /// an `Arc` for Apps in one process). `NOCTIS_SHARD_RANGE=start..end`
    /// sets it from the environment.
    ///
    /// `build()` records [`BuildViolation::EmptyShardRange`] when `start`
    /// is not below `end`, [`BuildViolation::ShardRangeWithoutTotal`]
    /// without a fixed total, and [`BuildViolation::ShardRangeOutOfBounds`]
    /// when `end` exceeds the total. At [`App::run`], the session start
    /// limit must cover the range's shards (GW-02), and a range whose start,
    /// or whose end below the total, is not a multiple of Discord's
    /// `max_concurrency` logs a warning (its buckets are split across
    /// processes).
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.shard_total(128).shard_range(32..64);
    /// ```
    pub fn shard_range(&mut self, shards: Range<u32>) -> &mut Self {
        self.shard_range = Some(shards);
        self
    }

    /// Mounts a [`Module`] tree. Trees fold in mount order (plugins
    /// included), each parent before its children; the handlers' event
    /// types become the gateway filter, and their non-privileged intents
    /// are added to the requested set. A handler needing a privileged
    /// intent that was not requested is a [`BuildViolation::MissingIntent`]
    /// (GW-05). The trees' locale catalogs ([`Module::locales`]) merge
    /// after the App's own ([`locales`](Self::locales)).
    ///
    /// The root's id is one segment and each nested id its parent's plus
    /// one (`"profile"` → `"profile/badges"`); ids are unique per App across
    /// every tree. `build()` reports a broken id as
    /// [`BuildViolation::InvalidModuleId`] and a repeated one as
    /// [`BuildViolation::DuplicateModule`], so a tree is mounted at most
    /// once per App (it may be mounted on several Apps).
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

    /// Makes a normal run publish globally once at startup, before any shard
    /// connects (never on READY), like `--publish`; set it on exactly one
    /// process. A failed Publish is logged at `error` (target
    /// `noctis::publish`) and the App keeps starting. Default `false`; env
    /// `NOCTIS_PUBLISHER`; the last call wins.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.publisher(true);
    /// ```
    pub fn publisher(&mut self, publisher: bool) -> &mut Self {
        self.publisher = publisher;
        self
    }

    /// Whether shards resume the sessions stored in the `SessionStore`.
    /// Default `true`: each shard resumes its stored session at start and a
    /// graceful shutdown closes it with code 4000 and stores it again.
    /// `false` starts fresh: each shard deletes its stored entry before it
    /// identifies, and the shutdown closes with code 1000 and deletes the
    /// entry. A run with `--no-resume` sets it to `false`; the last call
    /// wins.
    ///
    /// ```
    /// let mut builder = noctis_core::App::builder();
    /// builder.resume_sessions(false);
    /// ```
    pub fn resume_sessions(&mut self, resume: bool) -> &mut Self {
        self.resume_sessions = resume;
        self
    }

    /// Sets which Discord resources the App caches (default
    /// [`DiscordCacheConfig::default`]: `GUILD | CHANNEL | ROLE`); the last
    /// call wins. [`DiscordCacheConfig::none`] turns the cache off.
    ///
    /// Core feeds the backend ([`discord_cache_backend`](Self::discord_cache_backend),
    /// by default a `MemoryDiscordCache` of these resources and message cap)
    /// before handlers see an event: the event types that keep each resource
    /// current join the gateway filter, are never dropped by the event lane,
    /// and their non-privileged intents are added. A resource needing a
    /// privileged intent that [`intents`](Self::intents) did not request is a
    /// [`BuildViolation::MissingCacheIntent`] (GW-05; `MEMBER` needs
    /// `GUILD_MEMBERS`). Handlers read it with `ctx.cache()`.
    ///
    /// ```
    /// use noctis_core::{DiscordCacheConfig, DiscordCacheResources};
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.discord_cache(DiscordCacheConfig::default().with_resources(DiscordCacheResources::USER));
    /// ```
    pub fn discord_cache(&mut self, config: DiscordCacheConfig) -> &mut Self {
        self.discord_cache = config;
        self
    }

    /// The presence every shard identifies with (default: none, which
    /// Discord shows as online without an activity); the last call wins.
    /// Change it at run time with [`App::presence`] or `ctx.gateway()`.
    ///
    /// ```
    /// use noctis_gateway::Presence;
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.presence(Presence::watching("/help"));
    /// ```
    pub fn presence(&mut self, presence: Presence) -> &mut Self {
        self.presence = Some(presence);
        self
    }

    /// The IDENTIFY `large_threshold` (GW-06; default 50): the member count
    /// above which `GUILD_CREATE` leaves out offline members.
    /// [`LargeThreshold::new`] only compiles for 50..=250. The last call
    /// wins.
    ///
    /// ```
    /// use noctis_gateway::LargeThreshold;
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.large_threshold(LargeThreshold::new::<100>());
    /// ```
    pub fn large_threshold(&mut self, threshold: LargeThreshold) -> &mut Self {
        self.large_threshold = threshold;
        self
    }

    /// The guild `--publish-guild` without an id publishes to (your test
    /// guild). Env `NOCTIS_DEV_GUILD`; the last call wins.
    ///
    /// ```
    /// use twilight_model::id::Id;
    ///
    /// let mut builder = noctis_core::App::builder();
    /// builder.dev_guild(Id::new(42));
    /// ```
    pub fn dev_guild(&mut self, guild: Id<GuildMarker>) -> &mut Self {
        self.dev_guild = Some(guild);
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
        namespaced {
            ref_store(RefStore) = "MemoryRefStore", "MemoryRefStore::new()";
            session_store(SessionStore) = "MemorySessionStore", "MemorySessionStore::new()",
                "Only the gateway reads and writes it: each shard's session is read at shard \
                start and written after a graceful, resumable shutdown. To resume across \
                restarts, the store must persist across restarts (the memory default only \
                resumes when the same instance reaches the next App).";
            module_state_store(ModuleStateStore) = "MemoryModuleStateStore", "MemoryModuleStateStore::new()";
            rate_limit_store(RateLimitStore) = "MemoryRateLimitStore", "MemoryRateLimitStore::new()";
            cache_store(CacheStore) = "MemoryCacheStore", "MemoryCacheStore::new()";
            render_cache(RenderCache) = "MemoryRenderCache", "MemoryRenderCache::new()";
            asset_store(AssetStore) = "MemoryAssetStore", "MemoryAssetStore::new()";
        }
        plain {
            identify_queue(IdentifyQueue) = "MemoryIdentifyQueue", "MemoryIdentifyQueue::default()";
            locale_source(LocaleSource) = "NoLocaleOverlay", "NoLocaleOverlay";
            guild_access(GuildAccessList) = "AllowAllGuilds", "AllowAllGuilds";
            discord_cache_backend(DiscordCache) = "{DiscordCacheResources, MemoryDiscordCache}",
                "MemoryDiscordCache::new(DiscordCacheResources::all())",
                default = "a `MemoryDiscordCache` of the [`discord_cache`](Self::discord_cache) \
                    configuration's resources and message cap",
                "The configuration, not the backend, decides what is fed and read: core feeds \
                only the configured resources' event types and every `ctx.cache()` getter of an \
                unconfigured resource returns `CacheError::Disabled`. With \
                `DiscordCacheConfig::none()` the backend is never fed nor read. The backend is \
                never namespaced; one instance can serve several Apps of a Cluster as an `Arc` \
                (`Arc::clone(&cache)`), and an out-of-process backend serializes the values \
                itself.";
        }
    }

    /// Installs the plugins, runs the App-build checks and builds the App.
    ///
    /// Installs every plugin once, in registration order (plugins added during
    /// an install afterwards, in the same build), then checks the HTTP
    /// timeout, the HTTP proxy, the gateway URL, the shard total, the event
    /// lane, handler concurrency, the shutdown grace, the ack deadline, the Ref TTL, the
    /// [`app_id`](Self::app_id) and the [`HttpConfig`],
    /// binds the Discord HTTP edge's loopback listener, folds the mounted
    /// Modules into the Registry (deriving the gateway filter and intents, GW-05,
    /// and merging every Module's Commands and context menus: the command
    /// rules run once on the merged set, per scope, and an undescribed base or
    /// group takes `cmd.<path>.description` from its catalog's default-locale
    /// file, else a handler's description with a warning; every command text
    /// is localized from the merged catalogs and checked), the rustls
    /// crypto provider (installing `ring`'s when none is set) and the tokio
    /// runtime (any flavor). The edge then starts serving on the current
    /// runtime, and every byte-store port is namespaced under the App id.
    /// The App's locale catalogs and then every mounted Module's merge (before
    /// the fold) into one App-wide string set that no Module state filters.
    /// The builder is left empty, attached to the same Runtime.
    ///
    /// # Errors
    ///
    /// One [`BuildError`] with every violation: setters', plugins' reports
    /// and the checks', in that order; among the checks, every
    /// [`BuildViolation::InvalidModuleId`] and
    /// [`BuildViolation::DuplicateModule`], then missing intents (GW-05),
    /// then every [`BuildViolation::Command`] (in the rule engine's
    /// order, then each declared or fallback description that differs from
    /// the value sent for its owner catalog's default locale, CMD-04, sorted
    /// by key), then every [`BuildViolation::CommandLocalization`] (sorted by
    /// key, locale and rule; skipped when the catalogs do not merge), then
    /// every [`BuildViolation::MissingCacheIntent`] (GW-05), then
    /// every [`BuildViolation::DuplicateLocaleKey`], then the
    /// crypto provider and the runtime.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::BuildError> {
    /// let app = noctis_core::App::builder().build()?;
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::too_many_lines,
        reason = "one linear pass: every check, then the App's parts"
    )]
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
        let shard_range = self.shard_range.clone().filter(|range| {
            let violation = if range.is_empty() {
                BuildViolation::EmptyShardRange {
                    start: range.start,
                    end: range.end,
                }
            } else {
                match self.shard_total {
                    None => BuildViolation::ShardRangeWithoutTotal {
                        start: range.start,
                        end: range.end,
                    },
                    // A total of 0 is already an InvalidShardTotal.
                    Some(0) => return false,
                    Some(total) if range.end > total => BuildViolation::ShardRangeOutOfBounds {
                        start: range.start,
                        end: range.end,
                        total,
                    },
                    Some(_) => return true,
                }
            };
            self.violations.push(violation);
            false
        });
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
        if !ACK_DEADLINE_RANGE.contains(&self.ack_deadline) {
            self.violations.push(BuildViolation::InvalidAckDeadline {
                actual: self.ack_deadline,
            });
        }
        if !REF_TTL_RANGE.contains(&self.ref_ttl) {
            self.violations.push(BuildViolation::InvalidRefTtl {
                actual: self.ref_ttl,
            });
        }
        let explicit_app_id = match self.app_id.as_deref().map(AppId::new) {
            None => None,
            Some(Ok(id)) => Some(id),
            Some(Err(reason)) => {
                self.violations
                    .push(BuildViolation::InvalidAppId { reason });
                None
            }
        };
        let edge_listener = self.check_http();
        // The catalogs merge before the fold, which localizes the commands
        // with them; their violations follow the fold's.
        let sources = locale_sources(&self.catalogs, &self.modules);
        let catalogs: Vec<noctis_i18n::Catalog> = sources.iter().map(|(c, _)| *c).collect();
        let (strings, duplicate_keys) = match noctis_i18n::AppStrings::new(&catalogs) {
            Ok(strings) => (Some(strings), Vec::new()),
            Err(duplicates) => {
                // `AppStrings` keeps the first catalog of an origin, so the
                // first source with that origin owns it.
                let owner = |origin: &str| {
                    sources
                        .iter()
                        .find(|(c, _)| c.origin() == origin)
                        .map_or(LocaleOwner::App, |(_, o)| *o)
                };
                let violations = duplicates
                    .into_iter()
                    .map(|d| BuildViolation::DuplicateLocaleKey {
                        key: d.key,
                        first: d.first,
                        second: d.second,
                        first_owner: owner(d.first),
                        second_owner: owner(d.second),
                    })
                    .collect();
                (None, violations)
            }
        };
        let (registry, missing) = Registry::fold(&self.modules, self.intents, strings.as_ref());
        let registry = Arc::new(registry);
        self.violations.extend(missing);
        self.violations.extend(
            crate::discord_cache::missing_intents(self.discord_cache, self.intents)
                .into_iter()
                .map(|(resource, intent)| BuildViolation::MissingCacheIntent { resource, intent }),
        );
        self.register_templates(&registry);
        let payload_keys = self.key_ring(&registry);
        self.violations.extend(duplicate_keys);
        if ensure_crypto_provider().is_err() {
            self.violations.push(BuildViolation::CryptoProvider);
        }
        if check_runtime(false).is_err() {
            self.violations.push(BuildViolation::NoTokioRuntime);
        }
        let fresh = Self::new(self.runtime.clone());
        let config = std::mem::replace(self, fresh);
        let (Some(event_capacity), Some((listener, edge_addr)), Some(strings)) = (
            event_capacity.filter(|_| config.violations.is_empty()),
            edge_listener,
            strings,
        ) else {
            return Err(BuildError::new(config.violations));
        };
        let strings = Arc::new(strings);
        let app_id = explicit_app_id.unwrap_or_else(|| default_app_id(config.token.as_ref()));
        let app: Arc<str> = Arc::from(app_id.as_str());
        let upstream = Upstream::for_proxy(http_proxy.as_deref(), config.http_config.upstream_h2c);
        let edge = Edge::start(
            (listener, edge_addr),
            upstream,
            config.http_config,
            config.http_timeout,
            &app,
        );
        let http = http_client(&config, &edge, http_proxy.is_some());
        let mut ports = Ports::resolve(config.ports, &app_id);
        let discord_cache = discord_cache_feed(&mut ports, config.discord_cache);
        if let Some(renderer) = &config.renderer {
            let asset_store: Arc<dyn AssetStore> = ports.asset_store.clone();
            renderer.bind(RendererPorts::new(asset_store));
        }
        let kv: Arc<dyn CacheStore> = ports.cache_store.clone();
        let refs: Arc<dyn RefStore> = ports.ref_store.clone();
        Ok(App {
            runtime: config.runtime,
            token: config.token,
            id: app_id,
            shared: Arc::new(AppShared {
                app,
                kv,
                discord_cache,
                sinks: config.error_sinks,
                http: Arc::new(http),
                states: config.states,
                edge,
                limits: config.limits,
                registry: Arc::clone(&registry),
                ack_deadline: config.ack_deadline,
                refs,
                ref_ttl: config.ref_ttl,
                locales: crate::locale::AppLocales::new(strings.catalogs(), config.locale_resolver),
                strings: Arc::clone(&strings) as Arc<dyn noctis_rules::Translate>,
                payload_keys,
                renderer: config.renderer,
                expired_handler: config.expired_handler,
                run_observer: config.run_observer,
                gateway: GatewayHandle::detached(),
            }),
            strings,
            http_timeout: config.http_timeout,
            http_proxy,
            http_config: config.http_config,
            gateway_url,
            shard_total,
            shard_range,
            intents: config.intents
                | registry.intents()
                | crate::discord_cache::intents(config.discord_cache.resources()),
            ports,
            registry,
            event_capacity,
            overflow_capacity: overflow,
            handler_concurrency: config.handler_concurrency,
            shutdown_grace: config.shutdown_grace,
            publisher: config.publisher,
            resume_sessions: config.resume_sessions,
            presence: config.presence,
            large_threshold: config.large_threshold,
            dev_guild: config.dev_guild,
            lifecycle: Lifecycle::new(),
        })
    }

    /// Hands the folded Templates to the Renderer (a rejection is a
    /// [`BuildViolation::TemplateRejected`]); without a Renderer, warns
    /// when Templates are registered.
    fn register_templates(&mut self, registry: &Registry) {
        let templates = registry.templates();
        match &self.renderer {
            Some(renderer) => {
                if let Err(error) = renderer.register(templates) {
                    self.violations.push(BuildViolation::TemplateRejected {
                        message: error_chain(&error),
                    });
                }
            }
            None if !templates.is_empty() => tracing::warn!(
                target: "noctis::render",
                templates = templates.len(),
                "Templates are registered but no Renderer is installed; ctx.render will fail: \
                 call .theme(..) or .image(..)"
            ),
            None => {}
        }
    }

    /// Sets the hidden run-end observer (`__testing::observe_runs`).
    pub(crate) fn set_run_observer(&mut self, observer: crate::handler::RunObserver) {
        self.run_observer = Some(observer);
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
        let catalogs: Vec<&str> = self
            .catalogs
            .iter()
            .map(noctis_i18n::Catalog::origin)
            .collect();
        f.debug_struct("AppBuilder")
            .field("runtime", &self.runtime)
            .field("token", &self.token)
            .field("app_id", &self.app_id)
            .field("intents", &self.intents)
            .field("http_timeout", &self.http_timeout)
            .field("http_proxy", &self.http_proxy.is_some())
            .field("http_config", &self.http_config)
            .field("gateway_url", &self.gateway_url.is_some())
            .field("shard_total", &self.shard_total)
            .field("shard_range", &self.shard_range)
            .field("states", &self.states)
            .field("ports", &self.ports.set_slots())
            .field("has_renderer", &self.renderer.is_some())
            .field("modules", &self.modules.len())
            .field("event_lane", &self.event_lane)
            .field("handler_concurrency", &self.handler_concurrency)
            .field("shutdown_grace", &self.shutdown_grace)
            .field("error_sinks", &self.error_sinks)
            .field("limits", &self.limits)
            .field("ack_deadline", &self.ack_deadline)
            .field("ref_ttl", &self.ref_ttl)
            .field("publisher", &self.publisher)
            .field("resume_sessions", &self.resume_sessions)
            .field("discord_cache", &self.discord_cache)
            .field("presence", &self.presence)
            .field("large_threshold", &self.large_threshold)
            .field("dev_guild", &self.dev_guild)
            .field("payload_key", &self.payload_key.is_some())
            .field("previous_payload_key", &self.previous_payload_key.is_some())
            .field("expired_handler", &self.expired_handler.is_some())
            .field("locale_resolver", &self.locale_resolver.is_some())
            .field("catalogs", &catalogs)
            .field("plugins", &plugins)
            .field("violations", &self.violations)
            .finish()
    }
}

/// The merge input of the App's strings: the App's catalogs in
/// `AppBuilder::locales` call order, then every mounted Module's in
/// `Module::locales` call order, Modules in `Registry::fold`'s pre-order
/// (roots in mount order, each parent before its children).
fn locale_sources(
    app: &[noctis_i18n::Catalog],
    roots: &[Module],
) -> Vec<(noctis_i18n::Catalog, LocaleOwner)> {
    let mut sources: Vec<_> = app.iter().map(|&c| (c, LocaleOwner::App)).collect();
    let mut stack: Vec<&Module> = roots.iter().rev().collect();
    while let Some(module) = stack.pop() {
        let owner = LocaleOwner::Module(module.id());
        sources.extend(module.catalogs().iter().map(|&c| (c, owner)));
        stack.extend(module.children().iter().rev());
    }
    sources
}

/// The App id when none is set: the token's bot user id in base36, else
/// `default` (with a warning when a token was given).
fn default_app_id(token: Option<&Token>) -> AppId {
    let Some(token) = token else {
        return fallback_app_id();
    };
    if let Some(id) = token.application_id() {
        return AppId::from_application_id(id);
    }
    tracing::warn!(
        target: "noctis::app",
        "could not derive the App id from the bot token; call AppBuilder::app_id"
    );
    fallback_app_id()
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

/// A Shard Range `start..end`, each side 1-10 ASCII digits that fit a `u32`
/// (leading zeros accepted, nothing else: no `..=`, signs or spaces). An
/// empty range parses; `build()` rejects it.
fn parse_shard_range(raw: &str) -> Option<Range<u32>> {
    let (start, end) = raw.split_once("..")?;
    Some(parse_shard_total(start)?..parse_shard_total(end)?)
}

/// A shutdown grace in whole seconds from 1-4 ASCII digits (leading zeros
/// accepted; `build()` rejects more than 900).
fn parse_grace_secs(raw: &str) -> Option<u64> {
    let digits = (1..=4).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| raw.parse().ok()).flatten()
}

/// `true`/`false`/`1`/`0`, ASCII case-insensitive.
fn parse_bool(raw: &str) -> Option<bool> {
    if raw == "1" || raw.eq_ignore_ascii_case("true") {
        Some(true)
    } else if raw == "0" || raw.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
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

/// The App's Discord cache feed: the `DiscordCache` slot's backend, or,
/// when the slot took its default, a `MemoryDiscordCache` of `config`'s
/// resources and message cap (installed into `ports`). A custom backend with
/// the cache off is warned about once.
fn discord_cache_feed(
    ports: &mut Ports,
    config: DiscordCacheConfig,
) -> Arc<crate::discord_cache::CacheFeed> {
    if ports.is_default(PortSlot::DiscordCache) {
        ports.discord_cache_backend = Arc::new(MemoryDiscordCache::with_message_cap(
            config.resources(),
            config.message_cap(),
        ));
    } else if !config.is_enabled() {
        tracing::warn!(
            target: "noctis::app",
            "a DiscordCache backend is set but the Discord cache is disabled \
             (DiscordCacheConfig::none()); it is never read or fed"
        );
    }
    Arc::new(crate::discord_cache::CacheFeed::new(
        Arc::clone(&ports.discord_cache_backend),
        config,
    ))
}

/// `error` and its source chain, joined by `": "`.
pub(crate) fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests;
