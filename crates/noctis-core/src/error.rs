//! Typed errors of App building and startup (AD-13). No message ever contains
//! the bot token.

use std::any::type_name;
use std::fmt;
use std::time::Duration;

use twilight_model::gateway::Intents;

use crate::cli::CliError;
use crate::publish::PublishError;
use noctis_codec::KeyError;

use crate::{
    AppIdError, GatewayUrlError, HttpConfigError, HttpProxyError, Plugin, ResourceConflict,
    TokenError,
};

/// Every error `App::from_env`, `build` and `run` return.
///
/// ```no_run
/// use noctis_core::{App, Error};
///
/// # #[tokio::main] async fn main() -> Result<(), Error> {
/// let app = App::from_env()?.build()?;
/// app.run().await?;
/// # Ok(()) }
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A `NOCTIS_*` environment variable is invalid.
    #[error(transparent)]
    Env(#[from] EnvError),
    /// `AppBuilder::build` found violations.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// `App::run` could not start.
    #[error(transparent)]
    Start(#[from] StartError),
    /// `App::run` stopped without a shutdown request.
    #[error(transparent)]
    Run(#[from] RunError),
    /// The command line `App::run` / `App::run_with_args` got is invalid.
    #[error(transparent)]
    Cli(#[from] CliError),
    /// The Publish of a CLI mode (`--publish`, …) failed.
    #[error(transparent)]
    Publish(#[from] PublishError),
}

/// Why a started `App::run` stopped without a shutdown request.
///
/// ```
/// use noctis_core::RunError;
///
/// assert!(RunError::AllShardsEnded.to_string().contains("shard closed fatally"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RunError {
    /// Every shard stopped on its own (fatal closes) after startup; the
    /// shutdown sequence ran, then `run` returned this instead of `Ok(())`.
    #[error(
        "every shard ended without a shutdown request (see the \"shard closed fatally\" logs): \
         fix the cause, then restart"
    )]
    AllShardsEnded,
}

/// Every violation one `AppBuilder::build` found, in the order recorded:
/// setters, plugins' reports, then the build checks. Never empty.
///
/// ```
/// use noctis_core::{App, BuildViolation};
///
/// // Outside a tokio runtime, and with an out-of-range timeout.
/// let err = App::builder()
///     .http_timeout(std::time::Duration::ZERO)
///     .build()
///     .unwrap_err();
/// assert_eq!(err.violations().len(), 2);
/// assert!(matches!(err.violations()[1], BuildViolation::NoTokioRuntime));
/// assert!(err.to_string().starts_with("2 App-build violation(s)\n  - "));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", Bullets(violations))]
pub struct BuildError {
    violations: Vec<BuildViolation>,
}

/// `"{n} App-build violation(s)"`, then one `"\n  - {violation}"` per entry.
struct Bullets<'a>(&'a [BuildViolation]);

impl fmt::Display for Bullets<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} App-build violation(s)", self.0.len())?;
        self.0.iter().try_for_each(|v| write!(f, "\n  - {v}"))
    }
}

impl BuildError {
    /// `violations` must not be empty.
    pub(crate) fn new(violations: Vec<BuildViolation>) -> Self {
        debug_assert!(!violations.is_empty(), "a BuildError is never empty");
        Self { violations }
    }

    /// The violations, in recorded order.
    ///
    /// ```
    /// let err = noctis_core::App::builder().build().unwrap_err();
    /// assert_eq!(err.violations().len(), 1);
    /// ```
    pub fn violations(&self) -> &[BuildViolation] {
        &self.violations
    }

    /// The violations, by value.
    ///
    /// ```
    /// let err = noctis_core::App::builder().build().unwrap_err();
    /// assert_eq!(err.into_violations().len(), 1);
    /// ```
    pub fn into_violations(self) -> Vec<BuildViolation> {
        self.violations
    }
}

/// One App-build violation.
///
/// ```
/// use noctis_core::BuildViolation;
///
/// let v = BuildViolation::NoTokioRuntime;
/// assert!(v.to_string().contains("#[tokio::main]"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildViolation {
    /// `build()` ran outside a tokio runtime.
    #[error(
        "no tokio runtime: call `build()` inside a tokio runtime, e.g. from `#[tokio::main]` \
         or `tokio::runtime::Runtime::block_on`"
    )]
    NoTokioRuntime,
    /// No rustls crypto provider was set and `ring`'s could not be installed.
    #[error(
        "no rustls crypto provider could be installed: install one before building with \
         `rustls::crypto::ring::default_provider().install_default()`"
    )]
    CryptoProvider,
    /// Two `.state(..)` values of one type; the first is kept.
    #[error(
        "state type `{type_name}` was registered twice: register one value per type, or wrap \
         the second in a newtype"
    )]
    DuplicateState {
        /// The state's type name.
        type_name: &'static str,
    },
    /// Two catalogs (from `AppBuilder::locales` or `Module::locales`) define
    /// one locale key.
    #[error(
        "locale key `{key}` is defined by two catalogs: `{first}` ({first_owner}) and \
         `{second}` ({second_owner})"
    )]
    DuplicateLocaleKey {
        /// The key.
        key: &'static str,
        /// The origin of the catalog merged first (`<crate>/<dir>`).
        first: &'static str,
        /// The origin of the other catalog.
        second: &'static str,
        /// Who registered the `first` catalog.
        first_owner: LocaleOwner,
        /// Who registered the `second` catalog.
        second_owner: LocaleOwner,
    },
    /// Two plugins of one type; the first is kept.
    #[error(
        "plugin `{type_name}` was added twice: add it once; before `build`, \
         change the added one with `AppBuilder::plugin_mut`"
    )]
    DuplicatePlugin {
        /// The plugin's type name.
        type_name: &'static str,
    },
    /// A process resource was requested with a conflicting configuration.
    #[error(transparent)]
    ResourceConflict(#[from] ResourceConflict),
    /// `http_timeout` is outside 1 ms..=300 s.
    #[error("HTTP timeout {actual:?} is out of range: use a value in 1 ms..=300 s")]
    InvalidHttpTimeout {
        /// The rejected timeout.
        actual: Duration,
    },
    /// `http_proxy` is not `host[:port]` or `http://host[:port]`; the message
    /// never contains the value.
    #[error("invalid HTTP proxy: {reason}")]
    InvalidHttpProxy {
        /// Why the proxy was rejected.
        reason: HttpProxyError,
    },
    /// `gateway_url` is not `ws://host[:port]` or `wss://host[:port]`; the
    /// message never contains the value.
    #[error("invalid gateway URL: {reason}")]
    InvalidGatewayUrl {
        /// Why the gateway URL was rejected.
        reason: GatewayUrlError,
    },
    /// `app_id` is not 1-32 characters of `a-z`, `0-9` and `-`.
    #[error("invalid App id: {reason}")]
    InvalidAppId {
        /// Why the App id was rejected.
        reason: AppIdError,
    },
    /// `shard_total(0)`: a fixed shard total must be at least 1.
    #[error("shard total must be at least 1: omit shard_total to use Discord's recommendation")]
    InvalidShardTotal,
    /// `shard_range(start..end)` with `start` not below `end`: the range
    /// holds no shard.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let violation = BuildViolation::EmptyShardRange { start: 5, end: 5 };
    /// assert!(violation.to_string().contains("5..5 is empty"));
    /// ```
    #[error("shard range {start}..{end} is empty: use start..end with start below end, e.g. 0..16")]
    EmptyShardRange {
        /// The range's first shard id.
        start: u32,
        /// The range's end (exclusive).
        end: u32,
    },
    /// `shard_range` without a fixed `shard_total`: every process of a
    /// Cluster must agree on the total, and Discord's recommendation can
    /// change between process starts.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let violation = BuildViolation::ShardRangeWithoutTotal { start: 0, end: 4 };
    /// assert!(violation.to_string().contains("NOCTIS_SHARD_TOTAL"));
    /// ```
    #[error(
        "shard range {start}..{end} needs a fixed shard total: set shard_total \
         (NOCTIS_SHARD_TOTAL) to the same value on every process"
    )]
    ShardRangeWithoutTotal {
        /// The range's first shard id.
        start: u32,
        /// The range's end (exclusive).
        end: u32,
    },
    /// `shard_range(start..end)` with `end` above the fixed shard total.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let violation = BuildViolation::ShardRangeOutOfBounds { start: 2, end: 5, total: 4 };
    /// assert!(violation.to_string().contains("exceeds the shard total 4"));
    /// ```
    #[error(
        "shard range {start}..{end} exceeds the shard total {total}: the end must be at most \
         {total}"
    )]
    ShardRangeOutOfBounds {
        /// The range's first shard id.
        start: u32,
        /// The range's end (exclusive).
        end: u32,
        /// The fixed shard total.
        total: u32,
    },
    /// `event_lane(capacity, overflow)` is out of range: the capacity must be
    /// in 1..=1048576 and the overflow in 0..=4194304.
    #[error(
        "event lane capacity {capacity} / overflow {overflow} is out of range: use a capacity in \
         1..=1048576 and an overflow in 0..=4194304"
    )]
    InvalidEventLane {
        /// The rejected capacity.
        capacity: usize,
        /// The rejected overflow cap.
        overflow: usize,
    },
    /// `shutdown_grace` is outside 0 s..=900 s.
    #[error("shutdown grace {actual:?} is out of range: use a value in 0 s..=900 s")]
    InvalidShutdownGrace {
        /// The rejected grace period.
        actual: Duration,
    },
    /// `ack_deadline` is outside 0 ms..=2500 ms (INT-01).
    #[error("ack deadline {actual:?} is out of range: use a value in 0 ms..=2500 ms")]
    InvalidAckDeadline {
        /// The rejected deadline.
        actual: Duration,
    },
    /// `ref_ttl` is outside 1 s..=365 d (FR-17).
    #[error("ref TTL {actual:?} is out of range: use a value in 1 s..=365 d")]
    InvalidRefTtl {
        /// The rejected TTL.
        actual: Duration,
    },
    /// `handler_concurrency` is outside 1..=65536.
    #[error("handler concurrency {actual} is out of range: use a value in 1..=65536")]
    InvalidHandlerConcurrency {
        /// The rejected value.
        actual: usize,
    },
    /// An [`HttpConfig`](crate::HttpConfig) field is out of range (one
    /// violation per field).
    #[error("invalid HTTP config: {reason}")]
    InvalidHttpConfig {
        /// Which field, and why.
        reason: HttpConfigError,
    },
    /// The Discord HTTP edge could not bind its loopback listener.
    #[error("the Discord HTTP edge could not bind 127.0.0.1:0 ({kind:?})")]
    HttpEdge {
        /// The bind error's kind.
        kind: std::io::ErrorKind,
    },
    /// An event handler needs a privileged intent the App did not request
    /// (GW-05). Non-privileged intents are added automatically.
    #[error(
        "[GW-05] event handler `{handler}` (Module `{module}`) handles {event}, which needs the \
         privileged intent {name}: add `.intents(Intents::{name})` and enable it in the Discord \
         Developer Portal",
        name = IntentName(*intent)
    )]
    MissingIntent {
        /// The id of the handler's Module.
        module: &'static str,
        /// The handler's path (`module_path!()::name`).
        handler: &'static str,
        /// The Discord event name, such as `GUILD_MEMBER_ADD`.
        event: &'static str,
        /// The missing privileged intent.
        intent: Intents,
    },
    /// A chat-input or context-menu command rule broken after merging the
    /// commands of every mounted Module (FR-9, FR-56, FR-57): duplicate
    /// paths, conflicting descriptions or base metadata, tree shape, size and
    /// count limits, or a declared description (`Module::base`,
    /// `Module::group`, a handler's or an option's) that differs from its
    /// owner catalog's default-locale `cmd.<path>.description` /
    /// `cmd.<path>.opt.<o>.description` (CMD-04, once per key). The
    /// [`Violation`](noctis_rules::Violation) carries the
    /// rule id, the path, the values, notes naming the Modules involved and,
    /// last, the scope (`scope: global` or `scope: guild <id>`).
    #[error("{0}")]
    Command(noctis_rules::Violation),
    /// A localized command text from the merged locale catalogs breaks a
    /// Discord rule (LOC-02: the base rules of its field; LOC-03: a
    /// localized option, subcommand or group name collides). Each `(key,
    /// file locale, rule, base rule)` is reported once (one value can break
    /// two base rules, such as CMD-02 and CMD-03), naming the locale whose
    /// file holds the value, never a sibling-derived code.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    /// use noctis_rules::command::{LocalizedField, validate_localized};
    /// use noctis_rules::{Limits, Locale, RuleId};
    ///
    /// let violation = validate_localized(LocalizedField::Name, "Yasakla", &Limits::default())
    ///     .remove(0);
    /// let v = BuildViolation::CommandLocalization {
    ///     rule: RuleId::LOC_02,
    ///     locale: Locale::Tr,
    ///     key: "cmdword.ban",
    ///     origin: "bot/locales",
    ///     violation,
    /// };
    /// assert_eq!(
    ///     v.to_string(),
    ///     "[LOC-02] locale `tr`, key `cmdword.ban` (catalog `bot/locales`): \
    ///      [CMD-03] limit lowercase, actual `Yasakla`\n  = help: use `yasakla`"
    /// );
    /// ```
    #[error("[{rule}] locale `{locale}`, key `{key}` (catalog `{origin}`): {violation}")]
    CommandLocalization {
        /// `LOC-02` (a value breaks its field's base rule) or `LOC-03` (a
        /// localized name collides within one options or children array).
        rule: noctis_rules::RuleId,
        /// The locale whose file holds the value (never a sibling-derived
        /// code).
        locale: noctis_rules::Locale,
        /// The locale key, such as `cmdword.ban` or `cmd.ban.opt.user.name`.
        key: &'static str,
        /// The [`Catalog::origin`](noctis_i18n::Catalog::origin) of the
        /// catalog that defines the key (`<crate>/<dir>`).
        origin: &'static str,
        /// The broken base rule (CMD-01..04, CMD-06, OPT-09 or CTX-01) with
        /// an empty path, or for LOC-03 the colliding name as its path.
        violation: noctis_rules::Violation,
    },
    /// Two handlers share one Route id (CID-04): the first registration in
    /// fold order is kept, and each later one gives one violation.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let v = BuildViolation::DuplicateRoute {
    ///     route: "shop.buy",
    ///     first_module: "a",
    ///     first_handler: "bot::Buy",
    ///     second_module: "b",
    ///     second_handler: "bot::Buy",
    /// };
    /// assert_eq!(
    ///     v.to_string(),
    ///     "[CID-04] Route `shop.buy` has two handlers: `bot::Buy` (Module `a`) and `bot::Buy` \
    ///      (Module `b`): keep one handler per Route id and rename the other Route (renaming \
    ///      breaks its live components)"
    /// );
    /// ```
    #[error(
        "[CID-04] Route `{route}` has two handlers: `{first_handler}` (Module `{first_module}`) \
         and `{second_handler}` (Module `{second_module}`): keep one handler per Route id and \
         rename the other Route (renaming breaks its live components)"
    )]
    DuplicateRoute {
        /// The shared Route id.
        route: &'static str,
        /// The Module of the kept (first) handler.
        first_module: &'static str,
        /// The kept handler's struct path.
        first_handler: &'static str,
        /// The Module of the rejected handler.
        second_module: &'static str,
        /// The rejected handler's struct path.
        second_handler: &'static str,
    },
    /// One Template id is registered twice across the mounted Modules
    /// ([`Module::template`](crate::Module::template)); the first
    /// registration in fold order is kept. Any version counts, and so does
    /// the same type registered twice.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let v = BuildViolation::DuplicateTemplate {
    ///     id: "profile.card",
    ///     first_module: "profile",
    ///     first_type: "bot::ProfileCard",
    ///     second_module: "stats",
    ///     second_type: "bot::StatsCard",
    /// };
    /// assert_eq!(
    ///     v.to_string(),
    ///     "Template id `profile.card` is registered twice: `bot::ProfileCard` (Module \
    ///      `profile`) and `bot::StatsCard` (Module `stats`): give each Template its own id"
    /// );
    /// ```
    #[error(
        "Template id `{id}` is registered twice: `{first_type}` (Module `{first_module}`) and \
         `{second_type}` (Module `{second_module}`): give each Template its own id"
    )]
    DuplicateTemplate {
        /// The shared Template id.
        id: &'static str,
        /// The Module of the kept (first) registration.
        first_module: &'static str,
        /// The kept Template's type path.
        first_type: &'static str,
        /// The Module of the rejected registration.
        second_module: &'static str,
        /// The rejected Template's type path.
        second_type: &'static str,
    },
    /// The App's Renderer rejected the registered Templates at `App::build`
    /// (`Renderer::register`): for the takumi Renderer, a Template whose view
    /// was not made by `#[derive(Template)]`.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let v = BuildViolation::TemplateRejected {
    ///     message: "template `x` is unsupported: its view was not made by \
    ///               `#[derive(Template)]`"
    ///         .to_owned(),
    /// };
    /// assert_eq!(
    ///     v.to_string(),
    ///     "the Renderer cannot render the registered Templates: template `x` is unsupported: \
    ///      its view was not made by `#[derive(Template)]`"
    /// );
    /// ```
    #[error("the Renderer cannot render the registered Templates: {message}")]
    TemplateRejected {
        /// The Renderer's error, with its source chain joined by `": "`.
        message: String,
    },
    /// A mounted Module declares with `Module::uses::<R>()` a Route that no
    /// mounted Module handles (CID-04).
    #[error(
        "[CID-04] Module `{module}` uses Route `{route}` (`{route_type}`), but no mounted Module \
         handles it: add `.add::<{route_type}>()` to a mounted Module"
    )]
    UnmountedRoute {
        /// The id of the declaring Module.
        module: &'static str,
        /// The Route id.
        route: &'static str,
        /// The Route struct's type path.
        route_type: &'static str,
    },
    /// A handler takes `Ext<T>`, but no before-Middleware on its Module
    /// path (its Module or an ancestor) declares
    /// [`provides::<T>()`](crate::BeforeMiddleware::provides) with a scope
    /// covering the handler's kind (FR-20, AD-9). Take `Option<Ext<T>>` to
    /// accept a run without `T`.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let v = BuildViolation::MissingExtension {
    ///     module: "shop",
    ///     handler: "bot::Buy",
    ///     extension: "bot::Settings",
    /// };
    /// assert_eq!(
    ///     v.to_string(),
    ///     "handler `bot::Buy` (Module `shop`) takes `Ext<bot::Settings>`, but no \
    ///      before-Middleware on its Module path provides it: add \
    ///      `.before(Middleware::before(f).provides::<bot::Settings>())` to Module `shop` or an \
    ///      ancestor, or take `Option<Ext<bot::Settings>>`"
    /// );
    /// ```
    #[error(
        "handler `{handler}` (Module `{module}`) takes `Ext<{extension}>`, but no \
         before-Middleware on its Module path provides it: add \
         `.before(Middleware::before(f).provides::<{extension}>())` to Module `{module}` or an \
         ancestor, or take `Option<Ext<{extension}>>`"
    )]
    MissingExtension {
        /// The id of the handler's Module.
        module: &'static str,
        /// The handler's path (`module_path!()::Struct`).
        handler: &'static str,
        /// The required Extension's type name.
        extension: &'static str,
    },
    /// A previous payload key is set without a current one (CID-06):
    /// [`AppBuilder::previous_payload_key`](crate::AppBuilder::previous_payload_key)
    /// or `NOCTIS_PAYLOAD_KEY_PREVIOUS` only verifies; the current key
    /// signs.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// assert!(BuildViolation::PreviousKeyWithoutKey.to_string().starts_with("[CID-06]"));
    /// ```
    #[error(
        "[CID-06] a previous payload key is set without a current one: set NOCTIS_PAYLOAD_KEY \
         (or call `.payload_key(..)`) too"
    )]
    PreviousKeyWithoutKey,
    /// A mounted Route is `signed`, but no payload key is configured
    /// (CID-06): its ids could neither be signed nor verified.
    ///
    /// ```
    /// use noctis_core::BuildViolation;
    ///
    /// let v = BuildViolation::MissingPayloadKey { route: "shop.buy" };
    /// assert_eq!(
    ///     v.to_string(),
    ///     "[CID-06] Route `shop.buy` is `signed`, but no payload key is configured: set \
    ///      NOCTIS_PAYLOAD_KEY (64 hex digits) or call `.payload_key(..)`"
    /// );
    /// ```
    #[error(
        "[CID-06] Route `{route}` is `signed`, but no payload key is configured: set \
         NOCTIS_PAYLOAD_KEY (64 hex digits) or call `.payload_key(..)`"
    )]
    MissingPayloadKey {
        /// The signed Route's id.
        route: &'static str,
    },
    /// A plugin reported a problem with [`AppBuilder::report`](crate::AppBuilder::report).
    #[error("plugin `{plugin}`: {message}")]
    Plugin {
        /// The plugin's type name.
        plugin: &'static str,
        /// What is wrong and how to fix it.
        message: String,
    },
}

impl BuildViolation {
    /// A [`BuildViolation::Plugin`] naming plugin `P`.
    ///
    /// ```
    /// use noctis_core::{AppBuilder, BuildViolation, Plugin};
    ///
    /// struct Fonts;
    /// impl Plugin for Fonts {
    ///     fn install(&self, app: &mut AppBuilder) {
    ///         app.report(BuildViolation::plugin::<Self>("no font configured: call .font(..)"));
    ///     }
    /// }
    /// ```
    pub fn plugin<P: Plugin>(message: impl Into<String>) -> Self {
        Self::Plugin {
            plugin: type_name::<P>(),
            message: message.into(),
        }
    }
}

/// Who registered a locale catalog, named by
/// [`BuildViolation::DuplicateLocaleKey`].
///
/// ```
/// use noctis_core::LocaleOwner;
///
/// assert_eq!(LocaleOwner::App.to_string(), "AppBuilder::locales");
/// assert_eq!(LocaleOwner::Module("shop").to_string(), "Module `shop`");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocaleOwner {
    /// The App, through `AppBuilder::locales`.
    App,
    /// A mounted Module, through `Module::locales`.
    Module(
        /// The Module's id.
        &'static str,
    ),
}

impl fmt::Display for LocaleOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::App => f.write_str("AppBuilder::locales"),
            Self::Module(id) => write!(f, "Module `{id}`"),
        }
    }
}

/// Why `App::run` could not start.
///
/// ```
/// use noctis_core::StartError;
///
/// assert!(StartError::NotMultiThread.to_string().contains("new_multi_thread"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StartError {
    /// `App::run` was polled outside a tokio runtime.
    #[error(
        "`App::run` was called outside a tokio runtime: run it from `#[tokio::main]` \
         (multi-thread)"
    )]
    NoRuntime,
    /// The current tokio runtime is not the multi-thread flavor.
    #[error(
        "`App::run` needs a multi-thread tokio runtime: use #[tokio::main] (multi-thread) or \
         Builder::new_multi_thread()"
    )]
    NotMultiThread,
    /// No rustls crypto provider was set and `ring`'s could not be installed.
    #[error(
        "no rustls crypto provider could be installed: install one before `App::run` with \
         `rustls::crypto::ring::default_provider().install_default()`"
    )]
    CryptoProvider,
    /// The App has no bot token.
    #[error("no bot token: set NOCTIS_TOKEN and use `App::from_env()`, or call `.token(..)`")]
    MissingToken,
    /// Discord rejected the token: `GET /gateway/bot` answered 401, or a
    /// shard was closed with 4004.
    #[error(
        "Discord rejected the bot token (HTTP 401 or close 4004): copy the token again from the \
         Discord Developer Portal"
    )]
    InvalidToken,
    /// `GET /gateway/bot` failed: a transport error, a timeout, a status
    /// other than 401 or a body that could not be decoded.
    #[error(
        "could not read GET /gateway/bot{}: check the network, the HTTP proxy and \
         `http_timeout`, then retry",
        StatusSuffix(*status)
    )]
    GatewayInfo {
        /// The HTTP status, when Discord answered with one.
        status: Option<u16>,
    },
    /// `GET /gateway/bot` answered a recommended shard count or a
    /// `max_concurrency` of 0.
    #[error(
        "GET /gateway/bot returned 0 shards or a max_concurrency of 0: retry later, or check \
         the HTTP proxy in front of Discord"
    )]
    InvalidGatewayInfo,
    /// The fixed shard total is below the GW-03 minimum
    /// (⌈recommended × 2 / 5⌉, from 2500 guilds per shard).
    #[error(
        "shard total {total} is too small: Discord's recommendation needs at least {minimum} \
         shards (2500 guilds per shard); raise shard_total or omit it"
    )]
    ShardTotalTooSmall {
        /// The fixed total.
        total: u32,
        /// The smallest total allowed.
        minimum: u32,
    },
    /// The session start budget cannot cover every shard (GW-02).
    #[error(
        "{required} shards must identify but only {remaining} session starts remain: wait \
         {reset_after:?} for Discord's limit to reset, or run fewer shards"
    )]
    SessionStartLimit {
        /// The identifies startup needs.
        required: u32,
        /// The session starts left.
        remaining: u32,
        /// When the budget resets.
        reset_after: Duration,
    },
    /// A shard was closed with 4013 (invalid intents).
    #[error(
        "shard {shard} was closed with 4013 (invalid intents): request only intents Discord \
         defines"
    )]
    InvalidIntents {
        /// The shard number.
        shard: u32,
    },
    /// A shard was closed with 4014 (disallowed intents).
    #[error(
        "shard {shard} was closed with 4014 (disallowed intents): enable the privileged intents \
         in the Developer Portal or stop requesting them"
    )]
    DisallowedIntents {
        /// The shard number.
        shard: u32,
    },
    /// A shard was closed with a code twilight does not reconnect on.
    #[error(
        "shard {shard} was closed with fatal code {code}: see Discord's gateway close codes and \
         fix the cause before restarting"
    )]
    FatalClose {
        /// The shard number.
        shard: u32,
        /// The close code.
        code: u16,
    },
    /// A shard's stream ended without a close code during startup.
    #[error(
        "shard {shard} stopped during startup without a close code: check the gateway URL and \
         the network, then restart"
    )]
    ShardEnded {
        /// The shard number.
        shard: u32,
    },
    /// Every shard task stopped before startup finished.
    #[error(
        "the gateway stopped before every shard was ready: check the logs for the shard errors, \
         then restart"
    )]
    GatewayStopped,
    /// The Ctrl-C/SIGTERM handler could not be installed (the tokio runtime
    /// has no IO driver).
    #[error(
        "the Ctrl-C/SIGTERM handler could not be installed: build the tokio runtime with \
         enable_all() (as #[tokio::main] does)"
    )]
    SignalHandler,
}

/// An interaction was not acknowledged within the ack deadline: 2.9 s
/// after the gateway received it (Discord allows 3 s; INT-01). The
/// interaction token can no longer be used for the initial response.
///
/// ```
/// use std::time::Duration;
///
/// fn explain(error: &noctis_core::AckDeadlineMissed) -> String {
///     format!("{error} ({:?})", error.elapsed)
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error(
    "interaction not acknowledged in time: {elapsed:?} after receipt, past the 2.9 s ack \
     deadline (Discord allows 3 s; INT-01): defer slow work"
)]
pub struct AckDeadlineMissed {
    /// The time from receipt to the moment the deadline was detected.
    pub elapsed: Duration,
}

/// The constant name of a privileged intent, such as `GUILD_MEMBERS`.
struct IntentName(Intents);

impl fmt::Display for IntentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = if self.0 == Intents::GUILD_MEMBERS {
            "GUILD_MEMBERS"
        } else if self.0 == Intents::GUILD_PRESENCES {
            "GUILD_PRESENCES"
        } else if self.0 == Intents::MESSAGE_CONTENT {
            "MESSAGE_CONTENT"
        } else {
            return write!(f, "{:?}", self.0);
        };
        f.write_str(name)
    }
}

/// `" (HTTP {status})"`, or nothing.
struct StatusSuffix(Option<u16>);

impl fmt::Display for StatusSuffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(status) => write!(f, " (HTTP {status})"),
            None => Ok(()),
        }
    }
}

/// A `NOCTIS_*` environment variable `App::from_env` could not use. The
/// message never contains the value.
///
/// ```
/// use noctis_core::{EnvError, TokenError};
///
/// let err = EnvError::InvalidToken { var: "NOCTIS_TOKEN", source: TokenError::Empty };
/// assert!(err.to_string().contains("NOCTIS_TOKEN"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EnvError {
    /// The variable is not valid Unicode.
    #[error("environment variable `{var}` is not valid Unicode: set it to the plain value")]
    NotUnicode {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable holds an invalid bot token.
    #[error(
        "environment variable `{var}` holds an invalid bot token: copy the token again from the \
         Discord Developer Portal"
    )]
    InvalidToken {
        /// The variable's name.
        var: &'static str,
        /// Why the token was rejected.
        #[source]
        source: TokenError,
    },
    /// The variable is not a shard total (1-10 ASCII digits fitting a `u32`).
    #[error(
        "environment variable `{var}` is not a shard total: set a whole number of shards such as 16"
    )]
    InvalidShardTotal {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable is not a Shard Range (`start..end`, each side 1-10 ASCII
    /// digits fitting a `u32`).
    ///
    /// ```
    /// use noctis_core::EnvError;
    ///
    /// let err = EnvError::InvalidShardRange { var: "NOCTIS_SHARD_RANGE" };
    /// assert!(err.to_string().contains("start..end, e.g. 0..16"));
    /// ```
    #[error("environment variable `{var}` is not a shard range: set start..end, e.g. 0..16")]
    InvalidShardRange {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable is not a shutdown grace period (1-4 ASCII digits of
    /// whole seconds).
    #[error(
        "environment variable `{var}` is not a shutdown grace period: set whole seconds such as 10"
    )]
    InvalidShutdownGrace {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable is not a publisher switch (`true`, `false`, `1` or `0`,
    /// any ASCII case).
    #[error("environment variable `{var}` is not a publisher switch: set true or false")]
    InvalidPublisher {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable is not a guild id (1-20 ASCII digits, not 0).
    #[error(
        "environment variable `{var}` is not a guild id: set your test guild's numeric id \
         (Developer Mode, right-click the server, Copy Server ID)"
    )]
    InvalidDevGuild {
        /// The variable's name.
        var: &'static str,
    },
    /// The variable is not a payload key (exactly 64 ASCII hex digits).
    /// Neither the message nor the source shows a key character.
    ///
    /// ```
    /// use noctis_codec::{KeyError, PayloadKey};
    /// use noctis_core::EnvError;
    ///
    /// let err = EnvError::InvalidPayloadKey {
    ///     var: "NOCTIS_PAYLOAD_KEY",
    ///     source: "00".parse::<PayloadKey>().unwrap_err(),
    /// };
    /// assert!(err.to_string().contains("NOCTIS_PAYLOAD_KEY"));
    /// assert!(matches!(err, EnvError::InvalidPayloadKey { source: KeyError::Length { len: 2 }, .. }));
    /// ```
    #[error(
        "environment variable `{var}` is not a payload key: set 64 hex digits, such as the \
         output of `openssl rand -hex 32`"
    )]
    InvalidPayloadKey {
        /// The variable's name.
        var: &'static str,
        /// Why the key was rejected (a length or a position, never a
        /// character).
        #[source]
        source: KeyError,
    },
}
