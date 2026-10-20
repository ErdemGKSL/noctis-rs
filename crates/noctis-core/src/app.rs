//! `App` and its mutable `AppBuilder` (FR-1, AD-24).

use std::any::TypeId;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;

use noctis_ports::{
    AssetStore, CacheStore, DiscordCache, GuildAccessList, IdentifyQueue, LocaleSource,
    ModuleStateStore, RateLimitStore, RefStore, RenderCache, Renderer, SessionStore,
};
use twilight_model::gateway::Intents;

use crate::plugin::PluginEntry;
use crate::ports::{PortOverrides, Ports};
use crate::startup::{check_runtime, ensure_crypto_provider};
use crate::state::StateMap;
use crate::{
    BuildError, BuildViolation, EnvError, Error, Plugin, Runtime, StartError, State, Token,
};

/// The environment variable [`App::from_env`] reads the token from.
const TOKEN_VAR: &str = "NOCTIS_TOKEN";
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
    /// Reads `NOCTIS_TOKEN`, trimmed of ASCII whitespace; an empty value counts
    /// as unset. Setters called afterwards win.
    ///
    /// # Errors
    ///
    /// [`Error::Env`] when a variable is not Unicode or holds an invalid token;
    /// the message never shows the value.
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
    #[allow(
        dead_code,
        reason = "read by the gateway and pipeline stories (5.4-5.6)"
    )]
    pub(crate) fn ports(&self) -> &Ports {
        &self.ports
    }

    /// Starts the App.
    ///
    /// Checks, in order, that a multi-thread tokio runtime is current, that a
    /// rustls crypto provider is installed (installing `ring`'s when none is
    /// set) and that a token is set. The gateway is not available yet: after
    /// the checks it logs a warning and returns `Ok(())`.
    ///
    /// # Errors
    ///
    /// [`Error::Start`] with the [`StartError`] of the first failed check.
    ///
    /// ```no_run
    /// # #[tokio::main] async fn main() -> Result<(), noctis_core::Error> {
    /// noctis_core::App::from_env()?.build()?.run().await?;
    /// # Ok(()) }
    /// ```
    #[allow(
        clippy::unused_async,
        reason = "Story 5.4 awaits the shards here; the signature is already final"
    )]
    pub async fn run(self) -> Result<(), Error> {
        check_runtime(true)?;
        ensure_crypto_provider().map_err(|_| StartError::CryptoProvider)?;
        if self.token.is_none() {
            return Err(StartError::MissingToken.into());
        }
        tracing::warn!(target: "noctis::app", "gateway not available yet; App::run returns");
        Ok(())
    }
}

impl fmt::Debug for App {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("runtime", &self.runtime)
            .field("token", &self.token)
            .field("http_timeout", &self.http_timeout)
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
        if let Some(raw) = lookup(TOKEN_VAR) {
            let raw = raw
                .into_string()
                .map_err(|_| EnvError::NotUnicode { var: TOKEN_VAR })?;
            let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
            if !trimmed.is_empty() {
                let token = Token::new(trimmed).map_err(|source| EnvError::InvalidToken {
                    var: TOKEN_VAR,
                    source,
                })?;
                builder.token(token);
            }
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
    /// use noctis_core::{App, BuildViolation};
    ///
    /// let mut builder = App::builder();
    /// builder.report(BuildViolation::CryptoProvider);
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
    /// timeout, the rustls crypto provider (installing `ring`'s when none is
    /// set) and the tokio runtime (any flavor). The builder is left empty,
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
        Ok(App {
            runtime: config.runtime,
            token: config.token,
            http: Arc::new(http.build()),
            http_timeout: config.http_timeout,
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
            .field("states", &self.states)
            .field("ports", &self.ports.set_slots())
            .field("has_renderer", &self.renderer.is_some())
            .field("plugins", &plugins)
            .field("violations", &self.violations)
            .finish()
    }
}

#[cfg(test)]
mod tests;
