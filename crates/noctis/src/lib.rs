//! Noctis: a Discord bot framework on twilight, with built-in sharding, a
//! Components V2 UI kit and image rendering.
//!
//! Bot code depends on this crate only; the `noctis-*` crates are internal.
//!
//! # Modules
//!
//! - The crate root: [`App`], [`AppBuilder`], [`Token`], [`State`],
//!   [`Runtime`], [`Plugin`], [`Resource`], [`Module`], [`event`](macro@event),
//!   [`command`](macro@command), [`CommandHandler`], [`CommandCtx`],
//!   [`CommandPath`], [`UnknownCommandPath`], [`CommandMetadata`]
//!   ([`Permissions`], [`InteractionContext`], [`IntegrationType`]),
//!   [`user_command`](macro@user_command),
//!   [`message_command`](macro@message_command), [`ContextMenuHandler`],
//!   [`MenuKind`], [`MenuTargetError`], [`HandlerError`], [`HandlerResult`], [`HttpConfig`] (the Discord HTTP
//!   edge's tuning) and the errors ([`Error`], [`BuildError`],
//!   [`BuildViolation`], [`StartError`], [`RunError`], [`EnvError`],
//!   [`TokenError`], [`ResourceConflict`], [`HttpConfigError`],
//!   [`AckDeadlineMissed`]).
//! - [`events`]: the gateway event payloads `#[event]` handlers take, and
//!   [`events::EventCtx`].
//! - [`twilight`]: the twilight crates Noctis builds on, [`twilight::http`]
//!   and [`twilight::model`].
//! - [`rules`]: Discord limits, rule ids, text length and locales.
//! - [`prelude`]: `use noctis::prelude::*;` brings the [`Route`](prelude::Route),
//!   [`Payload`](prelude::Payload) and [`PayloadEnum`](prelude::PayloadEnum)
//!   derives, the `Route` and `Payload` traits,
//!   [`RouteComponents`](prelude::RouteComponents), [`Rgb`](ui::Rgb) and the
//!   macros [`components!`], [`label!`], [`placeholder!`], [`content!`] and
//!   [`rgb!`], and the [`event`](macro@event), [`command`](macro@command),
//!   [`user_command`](macro@user_command) and
//!   [`message_command`](macro@message_command) attributes with
//!   [`CommandCtx`] into scope.
//! - [`ui`]: the Components V2 message model: one type per component,
//!   placement checked by the compiler, components built from Routes, and
//!   attachments.
//! - [`codec`]: Payload field encodings ([`codec::Field`],
//!   [`codec::BoundedStr`]), the checks custom fields are tested with, and the
//!   traits the derives implement.
//! - [`ports`]: the Adapter port traits, the shared op vocabulary, typed
//!   values and the render contract.
//! - [`bytes`]: [`Bytes`](bytes::Bytes), the value type every byte-store port
//!   takes and returns.
//!
//! # Routes
//!
//! `#[derive(Route)]` turns a struct into a Route: a typed `custom_id` whose
//! fields are its Payload. The derive proves at compile time that every id
//! fits Discord's 100 characters (CID-01), and a struct that could exceed it
//! fails `cargo check` with the route id and the worst case.
//!
//! ```
//! use noctis::codec::RouteShape;
//! use noctis::prelude::*;
//! use twilight_model::id::Id;
//!
//! pub struct ItemMarker;
//!
//! /// `shop.buy: F 1 + route 8 + sep 1 + S 0 + E 0 + N 0 + body 14 + T 0 = 24 (limit 100)`
//! #[derive(Route, Debug, PartialEq)]
//! #[route("shop.buy")]
//! struct BuyButton {
//!     item: Id<ItemMarker>,
//!     qty: u8,
//! }
//!
//! assert_eq!(BuyButton::ID.as_str(), "shop.buy");
//! assert_eq!(BuyButton::SHAPE, RouteShape::INLINE);
//! assert_eq!(<BuyButton as Payload>::MAX_ENCODED_LEN, Some(14));
//!
//! /// A unit enum is a one-character field.
//! #[derive(PayloadEnum, Debug, Clone, Copy, PartialEq)]
//! enum Size {
//!     Small,
//!     Large,
//! }
//!
//! #[derive(Route)]
//! #[route("shop.size", schema = 1, view)]
//! struct SizePicker {
//!     size: Size,
//! }
//! assert_eq!(SizePicker::SCHEMA, 1);
//! ```
//!
//! `#[route("id", args…)]` takes the route id (1 to 24 characters of
//! `[a-z0-9._-]`, not starting with `_`) and, in any order, the codec
//! arguments `by_ref`, `overflow = spill`, `schema = n` (1..=61), `view`,
//! `select` and `signed`. Each derived Route also gets a
//! `__noctis_route_width_<Name>` test that prints its width breakdown
//! (`cargo test -- --nocapture noctis_route_width`).
//!
//! # App
//!
//! [`App::from_env`] (or [`App::builder`]) returns a mutable [`AppBuilder`]
//! whose setters take and return `&mut AppBuilder`; [`AppBuilder::build`]
//! installs the [`Plugin`]s, checks everything and returns an [`App`] or one
//! [`BuildError`] listing every violation. Unset ports default to bounded
//! in-memory Adapters. [`App::run`] returns a typed [`StartError`] outside a
//! multi-thread tokio runtime or without a token, and installs rustls's
//! `ring` crypto provider when none is set.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use noctis::prelude::*;
//! use noctis::twilight::model::gateway::Intents;
//!
//! struct BotMeta {
//!     name: &'static str,
//! }
//!
//! fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
//!     builder
//!         .state(BotMeta { name: "noctis" })
//!         .intents(Intents::GUILD_MESSAGES)
//!         .http_timeout(Duration::from_secs(5))
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), noctis::Error> {
//!     // `NOCTIS_TOKEN` holds the bot token; temporaries live to the end of
//!     // the statement.
//!     let app = configure(&mut App::from_env()?).build()?;
//!     let meta: State<BotMeta> = app.state().expect("registered in configure");
//!     assert_eq!(meta.name, "noctis");
//!     app.run().await
//! }
//! ```
//!
//! # Shutdown
//!
//! [`App::run`] runs until Ctrl-C or SIGTERM; [`App::run_until`] runs until
//! a future you pass resolves. Either way the shutdown logs `shutdown
//! started`, closes every shard with code 4000 (resumable), finishes the
//! queued events and the running handlers within
//! [`shutdown_grace`](AppBuilder::shutdown_grace) (default 10 s,
//! `NOCTIS_SHUTDOWN_GRACE`) while HTTP keeps working, cancels what is left
//! (each cancelled handler is reported to the `Event` [`ErrorSink`] as
//! [`ReportKind::Cancelled`]), waits for the shards to close and logs
//! `shutdown complete`. A shard that closes fatally after startup is logged
//! as `shard closed fatally` with its cause and its status gauge goes to 0;
//! when every shard has ended, `run` returns [`RunError::AllShardsEnded`]
//! instead of `Ok(())`.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use noctis::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), noctis::Error> {
//!     let app = App::from_env()?
//!         .shutdown_grace(Duration::from_secs(15))
//!         .build()?;
//!     let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
//!     let running = tokio::spawn(app.run_until(async {
//!         let _ = stopped.await;
//!     }));
//!     // ... later, from anywhere:
//!     let _ = stop.send(());
//!     running.await.expect("not cancelled")
//! }
//! ```
//!
//! # Discord HTTP
//!
//! [`App::http`] is the App's one twilight client. Its requests pass a
//! loopback edge inside the App that `build()` starts on the current tokio
//! runtime: it forwards only `/api/v10/` requests, byte for byte, to Discord
//! (or to [`AppBuilder::http_proxy`]) over one tuned connection. `run`
//! warms that connection before `GET /gateway/bot` and keeps it warm with
//! HTTP/2 PING frames, never with API requests; a closed connection, an
//! unanswered PING or a shard's network loss makes it redial. The defaults
//! are tuned for Discord; [`AppBuilder::http`] takes an [`HttpConfig`] to
//! change them. A twilight client kept after its App is dropped gets
//! connection errors.
//!
//! ```
//! use std::time::Duration;
//!
//! use noctis::{AppBuilder, HttpConfig};
//!
//! fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
//!     let mut http = HttpConfig::default();
//!     http.ping_interval = Duration::from_secs(15);
//!     builder.http(http)
//! }
//! # let _ = configure;
//! ```
//!
//! # Modules and events
//!
//! A [`Module`] groups registrations under a path-like id and nests other
//! Modules; [`AppBuilder::module`] mounts a tree. `#[event]` turns an
//! `async fn(ctx: EventCtx, event: <payload>) -> HandlerResult<()>` into a
//! descriptor for [`Module::add`]; the payload types are in [`events`].
//! `build()` folds every tree once, parent before child, and derives the
//! gateway filter (only interactions and handled event types are
//! deserialized) and the intents (a missing privileged intent is
//! [`BuildViolation::MissingIntent`], GW-05). Every handler of an event
//! runs: `ordered` ones first, one event at a time per guild, then the
//! others concurrently. At most
//! [`handler_concurrency`](AppBuilder::handler_concurrency) events run at
//! once (default 256); interactions never wait for that bound. A handler
//! error or panic goes to the `Event` [`ErrorSink`] (by default logged at
//! `error`) and the shard keeps running.
//!
//! ```no_run
//! use noctis::events::{EventCtx, MemberAdd, MessageCreate};
//! use noctis::prelude::*;
//! use noctis::twilight::model::gateway::Intents;
//!
//! struct Xp(std::sync::atomic::AtomicU64);
//!
//! #[event]
//! async fn award_xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
//!     if let Some(xp) = ctx.state::<Xp>() {
//!         xp.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//!     }
//!     let _ = msg.author.id;
//!     Ok(())
//! }
//!
//! /// Runs before the concurrent handlers, in per-guild order.
//! #[event(ordered)]
//! async fn audit_join(ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> {
//!     let _ = (ctx.module(), ev.user.id);
//!     Ok(())
//! }
//!
//! fn module() -> Module {
//!     Module::new("profile")
//!         .add(award_xp())
//!         .module(Module::new("profile/audit").add(audit_join()))
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), noctis::Error> {
//!     let app = App::from_env()?
//!         .state(Xp(0.into()))
//!         .intents(Intents::GUILD_MEMBERS) // privileged: enable it in the Portal too
//!         .module(module())
//!         .build()?;
//!     app.run().await
//! }
//! ```
//!
//! # Merging Commands
//!
//! `build()` also merges the slash Commands of every mounted Module: handlers
//! whose paths share a base word become one base command, in each scope
//! (global, or each guild of `guilds = [..]`) separately. The merged set is
//! checked once with the same rules as `#[command]` (tree shape, sizes,
//! counts, metadata agreement, duplicate paths and context-menu names); each
//! broken rule is one [`BuildViolation::Command`] whose notes name the
//! Modules and the scope. [`Module::base`] declares a base command's
//! description and [`BaseMeta`] (permissions, contexts, integration types,
//! nsfw), [`Module::group`] a subcommand group's description. A base or group
//! nobody describes takes the description of the handler below it from the
//! lexicographically smallest Module id (then the smallest path), and
//! `build()` logs a warning naming it.
//!
//! ```
//! use noctis::prelude::*;
//! use noctis::ui::{Components, TextDisplay};
//! use noctis::{BaseMeta, Permissions};
//!
//! #[command("config set", description = "Change a setting")]
//! async fn config_set(_ctx: CommandCtx) -> HandlerResult<Components> {
//!     Ok(Components::new().push(TextDisplay::new("Saved.")))
//! }
//!
//! #[command("config get prefix", description = "Show the prefix")]
//! async fn config_get_prefix(_ctx: CommandCtx) -> HandlerResult<Components> {
//!     Ok(Components::new().push(TextDisplay::new("Prefix: `!`")))
//! }
//!
//! # #[tokio::main(flavor = "current_thread")] async fn main() {
//! let settings = Module::new("settings").add(config_set()).base(
//!     "config",
//!     BaseMeta::new()
//!         .description("Configure the bot")
//!         .default_member_permissions(Permissions::MANAGE_GUILD),
//! );
//! let prefix = Module::new("prefix")
//!     .add(config_get_prefix())
//!     .group("config get", "Read a setting");
//! // One `/config` with `set` and `get prefix`, MANAGE_GUILD for both.
//! assert!(App::builder().module(settings).module(prefix).build().is_ok());
//! # }
//! ```
//!
//! # Answering Commands
//!
//! When an `APPLICATION_COMMAND` interaction arrives, noctis resolves its
//! Command by default (unlocalized) names in the scope Discord sent
//! (LOC-06), runs the handler (never waiting for a worker permit) and sends
//! what it returned: a message is the reply, or an edit of the original
//! once the handler has deferred or replied (INT-12). A handler that needs
//! several messages takes a [`CommandResponder`], a typestate whose
//! `reply` / `defer` consume it into an acknowledged responder offering
//! only `edit_original`, `followup` and `delete_original` (INT-03, INT-04,
//! INT-08, INT-10). Failures and panics go to the `Command` Error Sink.
//! [`EventCtx::dispatcher`](crate::events::EventCtx::dispatcher) gives the
//! [`Dispatcher`], which runs a registered Command from an event handler,
//! such as a prefix command, replying to the invoking message.
//!
//! ```
//! use noctis::events::{EventCtx, MessageCreate};
//! use noctis::prelude::*;
//! use noctis::ui::{Components, TextDisplay};
//! use noctis::{CommandInput, CommandInvocation};
//!
//! #[command("ping", description = "Check that the bot answers")]
//! async fn ping(ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
//!     let acked = r.defer(Ephemerality::Public).await?;
//!     let text = format!("Pong ({})", ctx.response_locale());
//!     acked.edit_original(Components::new().push(TextDisplay::new(text))).await?;
//!     Ok(acked.done())
//! }
//!
//! #[event]
//! async fn prefix(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
//!     if msg.content == "!ping" {
//!         let invocation = CommandInvocation::new(CommandInput::new("ping"), msg.channel_id, msg.id, msg.author.id)
//!             .guild_id(msg.guild_id);
//!         ctx.dispatcher().run_command(invocation).await?;
//!     }
//!     Ok(())
//! }
//!
//! let _module = Module::new("ping").add(ping()).add(prefix());
//! ```
//!
//! # Observability and Error Sinks
//!
//! Every handler error, handler panic and event dropped by a full event
//! overflow becomes an [`ErrorReport`] for the [`ErrorSink`] of its
//! [`ErrorOrigin`] (`Command`, `Component` (components and modals),
//! `Autocomplete`, `Event`, `Render`). Unset origins use [`LogSink`], which
//! logs at `error` with target `noctis::<origin>`. A sink is synchronous
//! and runs on the worker or shard task: spawn or forward to a channel for
//! I/O. Pass an `Arc` clone to share one sink between origins.
//!
//! noctis installs no tracing subscriber and no metrics recorder. Its spans
//! are `noctis.shard`, `noctis.event` and `noctis.interaction`, and its
//! metrics are listed in [`observe::METRICS`]: every name starts with
//! `noctis_`, every metric has an `app` label, and no label carries an id
//! or free text.
//!
//! ```
//! use std::sync::Arc;
//!
//! use noctis::prelude::*;
//! use noctis::{ErrorReport, ReportKind};
//!
//! struct Alert;
//!
//! impl ErrorSink for Alert {
//!     fn report(&self, report: ErrorReport<'_>) {
//!         if let ReportKind::Panic(message) = report.kind {
//!             eprintln!("page on-call: {} handler panicked: {message}", report.origin);
//!         }
//!     }
//! }
//!
//! let alert = Arc::new(Alert);
//! let mut builder = App::builder();
//! builder
//!     .error_sink(ErrorOrigin::Event, Arc::clone(&alert))
//!     .error_sink(ErrorOrigin::Render, alert);
//! ```
//!
//! # Cargo features
//!
//! All features are additive, and the default features build without the image
//! stack.
//!
//! - `image`: image rendering through takumi (off by default).
//! - `image-gif`: GIF/APNG output for rendered images.
//! - `image-emoji`: Twemoji colour emoji in image Templates.
//! - `default-font`: a bundled default font for image Templates.
//! - `i18n`: the localization runtime.
//! - `prefix`: opt-in prefix commands.
//! - `testkit`: Discord fakes and test helpers; enable it in
//!   `[dev-dependencies]` only.
#![deny(missing_docs)]

pub use noctis_core::{
    AckDeadlineMissed, Acked, App, AppBuilder, BaseMeta, BuildError, BuildViolation, Command,
    CommandCtx, CommandHandler, CommandInput, CommandInvocation, CommandMetadata, CommandOption,
    CommandPath, CommandResponder, ContextMenuHandler, Descriptor, DispatchError, Dispatcher,
    EnvError, Ephemerality, Error, ErrorOrigin, ErrorReport, ErrorSink, FollowupHandle,
    GatewayUrlError, HandlerError, HandlerResult, HttpConfig, HttpConfigError, HttpProxyError,
    IntegrationType, InteractionContext, IntoResponse, InvalidOption, LogSink, MenuKind,
    MenuTargetError, ModalMarker, Module, NoModal, OpensModal, OptionError, OptionValue, Options,
    OutgoingMessage, Pending, Permissions, Plugin, ReportKind, Resource, ResourceConflict,
    Responded, Responder, ResponderError, ResponderKind, ResponderState, RunError, Runtime,
    StartError, State, Token, TokenError, UnknownCommandPath, Visibility,
};

/// Derives [`Options`](trait@Options) for a struct with named fields: each
/// field is one option of a slash Command, in declaration order (never
/// reordered), and every literal is checked at compile time.
///
/// A `#[command]` handler takes the struct as its last parameter. On each
/// invocation the interaction's options are decoded into it (a missing,
/// mistyped or out-of-range value is an [`OptionError`], never a panic),
/// then each field's validator runs (a failure is an [`InvalidOption`]
/// carrying its [`MessageKey`](crate::rules::MessageKey)), and only then the
/// handler. Unknown incoming options are ignored.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{LegacyMessage, ResolvedUser};
///
/// #[derive(Choices, Clone, Copy, Debug, PartialEq)]
/// enum Severity {
///     #[choice("Low")]
///     Low,
///     #[choice("High", value = "hi")]
///     High,
/// }
///
/// fn positive(days: &i64, _ctx: &CommandCtx) -> Result<(), MessageKey> {
///     if *days > 0 { Ok(()) } else { Err(MessageKey::new("warn.days.positive")) }
/// }
///
/// #[derive(Options)]
/// struct WarnOptions {
///     /// Rustdoc only: never the option's description.
///     #[option(description = "Who to warn")]
///     member: ResolvedUser,
///     #[option(description = "How bad it is")]
///     severity: Severity,
///     #[option(description = "How long, in days", min_value = 1, max_value = 30, validate = positive)]
///     days: Option<i64>,
///     #[option(description = "Why", max_length = 200)]
///     reason: Option<String>,
/// }
///
/// #[command("warn", description = "Warn a member")]
/// async fn warn(_ctx: CommandCtx, options: WarnOptions) -> HandlerResult<LegacyMessage> {
///     let _ = (options.member.user.id, options.severity, options.days, options.reason);
///     Ok(LegacyMessage::new("Warned."))
/// }
///
/// assert_eq!(warn().options().len(), 4);
/// assert_eq!(WarnOptions::OPTIONS[2].name, "days");
/// assert!(!WarnOptions::OPTIONS[2].required);
/// ```
///
/// Field attribute `#[option(..)]`, arguments in any order, each at most
/// once:
///
/// | Argument | Takes | Option types |
/// | --- | --- | --- |
/// | `description = "…"` | 1 to 100 characters, required (CMD-04) | all |
/// | `rename = "…"` | the option name (default: the field name, `r#` stripped; CMD-01..03) | all |
/// | `min_value = n`, `max_value = n` | unsuffixed integer or float literal, optionally negated (OPT-03..05) | INTEGER, NUMBER |
/// | `min_length = n`, `max_length = n` | unsuffixed integer, 0..=6000 and 1..=6000 (OPT-06) | STRING |
/// | `channel_types = [GuildText, …]` | twilight `ChannelType` variant names (OPT-07) | CHANNEL |
/// | `file_types = ["image", ".pdf"]` | `image`, `video`, `audio` or `.ext`, at most 10 (OPT-13) | ATTACHMENT |
/// | `autocomplete = path` | an existing function; sets the autocomplete flag (OPT-10, OPT-11) | STRING, INTEGER, NUMBER |
/// | `validate = path` | `fn(&T, &CommandCtx) -> Result<(), MessageKey>`, called only for present values | all |
///
/// Field types ([`CommandOption`]; any other is an `[OPT-02]` compile
/// error); wrap a type in `Option<T>` to make the option optional, and put
/// optional fields after required ones (OPT-01):
///
/// | Type | Option type |
/// | --- | --- |
/// | `String`, a `#[derive(Choices)]` enum | STRING |
/// | `i64` | INTEGER |
/// | `f64` | NUMBER |
/// | `bool` | BOOLEAN |
/// | `Id<UserMarker>`, [`ResolvedUser`](crate::ui::ResolvedUser) | USER |
/// | `Id<ChannelMarker>`, [`ResolvedChannel`](crate::ui::ResolvedChannel) | CHANNEL |
/// | `Id<RoleMarker>`, [`ResolvedRole`](crate::ui::ResolvedRole) | ROLE |
/// | `Id<GenericMarker>`, [`Mentionable`](crate::ui::Mentionable) | MENTIONABLE |
/// | `Id<AttachmentMarker>`, [`ResolvedAttachment`](crate::ui::ResolvedAttachment) | ATTACHMENT |
///
/// Rules that do not depend on the field type are compile errors at
/// expansion, all at once (CMD-01..04, CMD-06, CMD-13, OPT-01, OPT-13
/// entries). Rules that do are free `const _` items over the field's
/// [`CommandOption::KIND`]: they fail as `error[E0080]: evaluation
/// panicked: [RULE-ID] …` (OPT-03..08, OPT-10, OPT-11, OPT-13 misuse).
///
/// A missing description does not compile:
///
/// ```compile_fail
/// use noctis::prelude::*;
///
/// #[derive(Options)]
/// struct O {
///     days: i64, // [CMD-04]
/// }
/// ```
pub use noctis_macros::Options;

pub mod observe {
    //! The observability contract: the metric catalog ([`METRICS`]) and
    //! [`describe_metrics`].
    //!
    //! Metrics go through the `metrics` facade; install a recorder (e.g.
    //! `metrics-exporter-prometheus`) to collect them. Spans:
    //! `noctis.shard{app, shard}`,
    //! `noctis.event{app, shard, guild_id, module, handler, event_type}`,
    //! `noctis.interaction{app, shard, guild_id, module, path}`,
    //! `noctis.dispatch{app, guild_id, module, path}` and
    //! `noctis.http{app, kind, method, attempt, reused, status_class}` (one
    //! per Discord HTTP upstream attempt; `kind` is `forward`, `callback` or
    //! `warmup`). No span field or label carries a path, token, id or header
    //! value.
    //!
    //! ```
    //! for metric in noctis::observe::METRICS {
    //!     assert!(metric.name.starts_with("noctis_"));
    //!     assert_eq!(metric.labels[0], "app");
    //! }
    //! ```

    pub use noctis_core::observe::{METRICS, MetricInfo, MetricType, describe_metrics};
}

/// Declares a slash Command: checks its Command Path or Pattern at compile
/// time and turns the `async fn` into `fn name() -> CommandHandler`, which
/// [`Module::add`] takes.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, TextDisplay};
///
/// /// Rustdoc only: never the Command's description.
/// #[command("config (set|get) (prefix|welcome)", description = "Configure the bot")]
/// async fn config(ctx: CommandCtx, path: ConfigPath) -> HandlerResult<Components> {
///     let text = match path {
///         ConfigPath::SetPrefix | ConfigPath::SetWelcome => "Saved.",
///         ConfigPath::GetPrefix | ConfigPath::GetWelcome => "Current value: `!`",
///     };
///     assert_eq!(ctx.command_path(), path.as_str());
///     Ok(Components::new().push(TextDisplay::new(text)))
/// }
///
/// #[command("ping", description = "Check that the bot answers", ephemeral)]
/// async fn ping(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
///     let acked = r.reply(Components::new().push(TextDisplay::new("Pong!"))).await?;
///     acked.followup(Components::new().push(TextDisplay::new("Still here.")), Visibility::Public).await?;
///     Ok(acked.done())
/// }
///
/// assert_eq!(ConfigPath::ALL[0].as_str(), "config set prefix");
/// assert_eq!(config().paths(), ["config set prefix", "config set welcome", "config get prefix", "config get welcome"]);
/// assert_eq!(ping().ephemerality(), Ephemerality::Ephemeral);
/// let _module = Module::new("settings").add(config()).add(ping());
/// ```
///
/// The first argument is a string literal: a Command Path of 1 to 3 words
/// separated by single ASCII spaces (1 word a command, 2 a subcommand, 3 a
/// subcommand in a group; see [`rules::command::PathKind`]), or a Command
/// Pattern with groups `(a|b)` and optional groups `(a|b)?`, expanded left to
/// right by [`rules::command::expand`]. Then, in any order, each at most
/// once:
///
/// - `description = "…"`: the description, 1 to 100 characters, required.
///   Doc comments stay rustdoc and never become it.
/// - `max_expansions = n`: the pattern's expansion cap (default 25).
/// - `ephemeral` (no value): every response of the Command is visible only
///   to the invoking user; [`CommandHandler::ephemerality`] reads it. It is
///   fixed (INT-10): `reply` and a returned message use it, `defer` and
///   `followup` take theirs explicitly and no edit changes it.
/// - `opens_modal` (no value): the handler may open a modal as its first
///   response. It must take `r: CommandResponder<`[`OpensModal`]`>`, the
///   only responder with `modal(..)`, and no auto-defer is armed for it
///   (a modal must be the first response); [`CommandHandler::opens_modal`]
///   reads it. A mismatch is a compile error citing `[INT-05]`: the flag
///   without that responder, that responder without the flag, or the flag
///   without any responder parameter.
///
/// Every check of [`rules::command::validate_pattern_spacing`],
/// [`rules::command::expand`] and [`rules::command::validate_tree`] runs at
/// expansion; each violation is a compile error naming its rule id, all of
/// them at once.
///
/// A path (no group) takes `(ctx: CommandCtx)`. A pattern with a group
/// generates `enum <Fn>Path` (the fn name in `UpperCamelCase` plus `Path`,
/// with the fn's visibility) and takes `(ctx: CommandCtx, path: <Fn>Path)`.
/// Either form may take a trailing options struct, `options: O` with `O:
/// Options` (see [`derive(Options)`](derive@Options)); the struct is decoded
/// and validated before the handler runs, and
/// [`CommandHandler::options`] lists its declarations.
/// Its variants concatenate the chosen alternatives in `UpperCamelCase`
/// (words split on `-` and `_`; `Bare` when every group is absent; `_` before
/// a leading digit; `Self_` for `Self`); two equal names are a compile error.
/// The enum has `ALL` (expansion order), `as_str()` and implements
/// [`CommandPath`].
///
/// # Answering
///
/// A handler may take a [`CommandResponder`] right after `ctx` (and the
/// path), before the options: `(ctx[, path][, r: CommandResponder][,
/// options])`; any other order is a compile error listing these forms. It
/// returns `HandlerResult<R>` with `R:` [`IntoResponse`]: a
/// [`V2Message`](crate::ui::V2Message), [`Components`](crate::ui::Components)
/// or [`LegacyMessage`](crate::ui::LegacyMessage) is sent as the reply
/// (callback type 4) or, after `defer`/`reply`, as an edit of the original
/// (INT-12); the [`Responded`] from `done()` means the handler answered
/// itself. `HandlerResult<()>` does not compile: a Command must answer.
/// Every V2 message gets `IS_COMPONENTS_V2` and passes the Limits validator
/// ([`AppBuilder::limits`]) before any request. An `Err` or a panic goes to
/// the `Command` [`ErrorSink`] and nothing is sent (the user-facing notice
/// arrives with Story 6.8).
///
/// A handler that has not answered [`AppBuilder::ack_deadline`] after the
/// interaction arrived (default 2000 ms, 0-2500 ms) is acknowledged by the
/// auto-defer: one type-5 callback carrying only the declared `EPHEMERAL`
/// flag (INT-01). Its later `reply` or returned message then edits the
/// original, so at most one callback is ever sent (INT-03). After 15
/// minutes the interaction token is expired: edits and deletes of public
/// messages use the bot token, everything else is
/// [`ResponderError::TokenExpired`] (INT-02). A user-install-only
/// interaction sends at most 5 follow-ups ([`ResponderError::FollowupCap`],
/// INT-11). The [`Dispatcher`] has no auto-defer.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::Modal;
///
/// # fn feedback_form() -> Modal { unimplemented!() }
/// #[command("feedback", description = "Send feedback", opens_modal)]
/// async fn feedback(_ctx: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded> {
///     Ok(r.modal(feedback_form()).await?.done())
/// }
///
/// assert!(feedback().opens_modal());
/// ```
///
/// ```compile_fail,E0277
/// use noctis::prelude::*;
///
/// #[command("ping", description = "d")]
/// async fn ping(_ctx: CommandCtx) -> HandlerResult<()> { // not a Command response
///     Ok(())
/// }
/// ```
///
/// # Metadata
///
/// These arguments set the Command's [`CommandMetadata`], read with
/// [`CommandHandler::metadata`]. An argument left out stays unset (`None`,
/// or no guilds) and is omitted when the Command is published, so Discord
/// applies its default. They belong to the **base** command of every path
/// the handler declares (META-02): a per-path form such as
/// `permissions(set = …)` is a `[META-02]` compile error, and handlers
/// sharing a base, in any Module, must agree, unset included, unless a
/// [`Module::base`] declaration sets the field (checked at App build,
/// [`BuildViolation::Command`]).
///
/// | Argument | Takes | Rules |
/// | --- | --- | --- |
/// | `permissions = MANAGE_GUILD \| BAN_MEMBERS` | `default_member_permissions`: [`Permissions`] names joined by `\|` (a path's last segment counts, `Permissions::MANAGE_GUILD` works); the names are [`rules::command::PERMISSIONS`] (twilight's, so `USE_SLASH_COMMANDS`); an unknown name or other syntax fails. For admin-only use `ADMINISTRATOR` | META-01 |
/// | `contexts = [Guild, BotDm, PrivateChannel]` | [`InteractionContext`] names, at least one, no duplicates, declared order | META-03 |
/// | `integration_types = [GuildInstall, UserInstall]` | [`IntegrationType`] names, same shape | META-03, META-04 |
/// | `nsfw = true` | `true` or `false` | CMD-07 |
/// | `guilds = [123456789012345678]` | guild id integer literals (non-zero, distinct): the Command is published only to these guilds, and `contexts` / `integration_types` are then a `[META-03]` error | META-03 |
///
/// `dm_permission` does not exist (`[META-05]`; use `contexts = [Guild]`).
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::LegacyMessage;
/// use noctis::{InteractionContext, Permissions};
///
/// #[command("ban", description = "Ban a member", permissions = BAN_MEMBERS | MANAGE_GUILD, contexts = [Guild], nsfw = false)]
/// async fn ban(_ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
///     Ok(LegacyMessage::new("Banned."))
/// }
///
/// let handler = ban();
/// let meta = handler.metadata();
/// assert_eq!(meta.default_member_permissions, Some(Permissions::BAN_MEMBERS | Permissions::MANAGE_GUILD));
/// assert_eq!(meta.contexts, Some(&[InteractionContext::Guild][..]));
/// assert_eq!(meta.integration_types, None);
/// assert_eq!(meta.nsfw, Some(false));
/// ```
pub use noctis_macros::command;

/// Declares a `USER` context-menu command: checks its name at compile time
/// and turns `async fn name(ctx: CommandCtx, target: ResolvedUser[, r:
/// CommandResponder]) -> HandlerResult<R>` into `fn name() ->
/// ContextMenuHandler`, which
/// [`Module::add`] takes.
///
/// The first argument is the menu name, a string literal of 1 to 32
/// characters (UTF-16 counted; mixed case, spaces, apostrophes and emoji are
/// fine, CTX-01). Then the metadata arguments of [`command`](macro@command)
/// (`permissions`, `contexts`, `integration_types`, `nsfw`, `guilds`) and
/// `ephemeral`. A context-menu command has no description and no options
/// (`[CTX-02]` otherwise); its type comes from the macro (CTX-03). It
/// answers like a [`command`](macro@command): `R` is an [`IntoResponse`]
/// value, and an optional [`CommandResponder`] comes after the target.
///
/// The handler's target is the right-clicked user,
/// [`ResolvedUser`](crate::ui::ResolvedUser) (`member` is `None` when the
/// target is not a member of the guild, including outside a
/// guild); [`message_command`](macro@message_command) gives a
/// [`ResolvedMessage`](crate::ui::ResolvedMessage). `ctx.command_path()` is
/// the menu name. An interaction without its target in the resolved data
/// fails with a [`MenuTargetError`] and the handler does not run.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, LegacyMessage, ResolvedMessage, ResolvedUser, TextDisplay};
/// use noctis::{CommandMetadata, MenuKind};
///
/// #[user_command("Show profile", contexts = [Guild], ephemeral)]
/// async fn show_profile(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Components> {
///     assert_eq!(ctx.command_path(), "Show profile");
///     Ok(Components::new().push(TextDisplay::new(format!("<@{}>", target.user.id))))
/// }
///
/// #[message_command("Quote")]
/// async fn quote(_ctx: CommandCtx, target: ResolvedMessage, r: CommandResponder) -> HandlerResult<Responded> {
///     Ok(r.reply(LegacyMessage::new(format!("> {}", target.message.content))).await?.done())
/// }
///
/// assert_eq!(show_profile().kind(), MenuKind::User);
/// assert_eq!(show_profile().ephemerality(), Ephemerality::Ephemeral);
/// assert_eq!(quote().menu_name(), "Quote");
/// assert_eq!(quote().metadata(), &CommandMetadata::new());
/// let _module = Module::new("profile").add(show_profile()).add(quote());
/// ```
pub use noctis_macros::user_command;

/// Declares a `MESSAGE` context-menu command: like
/// [`user_command`](macro@user_command), with `target:
/// ResolvedMessage` ([`ResolvedMessage`](crate::ui::ResolvedMessage)).
pub use noctis_macros::message_command;

/// Turns `async fn name(ctx: EventCtx, event: <payload>) -> HandlerResult<()>`
/// into `fn name() -> EventHandler`, which [`Module::add`] takes. See
/// [`events`] and [Modules and events](crate#modules-and-events).
///
/// Arguments, in any order, each at most once:
///
/// - `ordered`: the handler runs before the concurrent handlers of the
///   event, one event at a time per guild. Ordered handlers share 64
///   sub-queues keyed by a hash of the guild id (guildless events use
///   sub-queue 0). A hung ordered handler blocks its whole sub-queue, and
///   the events queued behind it keep their worker permits, so it can
///   eventually hold every `handler_concurrency` permit and stop all event
///   handling. noctis adds no timeout: bound slow work in ordered handlers
///   yourself, for example with `tokio::time::timeout`.
/// - `droppable`: the event may be dropped when the event lane is full, if
///   every handler of its type is droppable. `TYPING_START` and
///   `PRESENCE_UPDATE` are always droppable, whatever the flag says.
pub use noctis_macros::event;

pub mod events {
    //! Gateway event payloads for `#[event]` handlers, the [`EventCtx`]
    //! they receive and the [`EventHandler`] descriptor `#[event]` builds.
    //!
    //! Each payload type derives the gateway intent it needs; the table
    //! lists them. Non-privileged intents are added to the App
    //! automatically. A privileged one (`GUILD_MEMBERS`, `GUILD_PRESENCES`)
    //! must be requested with [`AppBuilder::intents`](crate::AppBuilder::intents)
    //! and enabled in the Discord Developer Portal; otherwise `build()`
    //! fails with [`BuildViolation::MissingIntent`](crate::BuildViolation::MissingIntent)
    //! (GW-05). Only handled event types are deserialized.
    //!
    //! | Intent | Payloads |
    //! | --- | --- |
    //! | `AUTO_MODERATION_CONFIGURATION` | `AutoModerationRuleCreate`, `AutoModerationRuleDelete`, `AutoModerationRuleUpdate` |
    //! | `AUTO_MODERATION_EXECUTION` | `AutoModerationActionExecution` |
    //! | `GUILD_MODERATION` | `BanAdd`, `BanRemove`, `GuildAuditLogEntryCreate` |
    //! | `GUILDS` | `ChannelCreate`, `ChannelDelete`, `ChannelPinsUpdate`, `ChannelUpdate`, `GuildCreate`, `GuildDelete`, `GuildUpdate`, `UnavailableGuild`, `RoleCreate`, `RoleDelete`, `RoleUpdate`, `StageInstanceCreate`, `StageInstanceDelete`, `StageInstanceUpdate`, `ThreadCreate`, `ThreadDelete`, `ThreadListSync`, `ThreadMemberUpdate`, `ThreadMembersUpdate`, `ThreadUpdate` |
    //! | `GUILD_EMOJIS_AND_STICKERS` | `GuildEmojisUpdate`, `GuildStickersUpdate` |
    //! | `GUILD_INTEGRATIONS` | `GuildIntegrationsUpdate`, `IntegrationCreate`, `IntegrationDelete`, `IntegrationUpdate` |
    //! | `GUILD_INVITES` | `InviteCreate`, `InviteDelete` |
    //! | `GUILD_WEBHOOKS` | `WebhooksUpdate` |
    //! | `GUILD_VOICE_STATES` | `VoiceStateUpdate` |
    //! | `GUILD_SCHEDULED_EVENTS` | `GuildScheduledEventCreate`, `GuildScheduledEventDelete`, `GuildScheduledEventUpdate`, `GuildScheduledEventUserAdd`, `GuildScheduledEventUserRemove` |
    //! | `GUILD_MEMBERS` (privileged) | `MemberAdd`, `MemberRemove`, `MemberUpdate` |
    //! | `GUILD_PRESENCES` (privileged) | `PresenceUpdate` |
    //! | `GUILD_MESSAGES` | `MessageCreate`, `MessageDelete`, `MessageDeleteBulk`, `MessageUpdate` |
    //! | `GUILD_MESSAGE_POLLS` | `MessagePollVoteAdd`, `MessagePollVoteRemove` |
    //! | `GUILD_MESSAGE_REACTIONS` | `ReactionAdd`, `ReactionRemove`, `ReactionRemoveAll`, `ReactionRemoveEmoji` |
    //! | `GUILD_MESSAGE_TYPING` | `TypingStart` |
    //! | none | `CommandPermissionsUpdate`, `EntitlementCreate`, `EntitlementDelete`, `EntitlementUpdate`, `MemberChunk`, `UserUpdate`, `VoiceServerUpdate` |
    //!
    //! `MessageCreate` derives `GUILD_MESSAGES` only: direct messages need
    //! `.intents(Intents::DIRECT_MESSAGES)` and message text needs the
    //! privileged `MESSAGE_CONTENT`, both requested explicitly. Only guild
    //! intents are derived: `DIRECT_MESSAGES`, `DIRECT_MESSAGE_REACTIONS`
    //! and `DIRECT_MESSAGE_TYPING` are never added automatically and must be
    //! requested with `.intents(..)`.
    //!
    //! `UnavailableGuild` handlers run on a `GUILD_DELETE` dispatch with
    //! `unavailable: true` (an outage); a guild the bot left or was removed
    //! from reaches only `GuildDelete` handlers.
    //!
    //! ```
    //! use noctis::events::{EventCtx, MessageCreate};
    //! use noctis::prelude::*;
    //!
    //! #[event]
    //! async fn award_xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    //!     let _ = (ctx.guild_id(), msg.author.id);
    //!     Ok(())
    //! }
    //!
    //! let module = Module::new("profile").add(award_xp());
    //! ```
    //!
    //! A type that is not a payload does not compile:
    //!
    //! ```compile_fail
    //! use noctis::events::EventCtx;
    //! use noctis::prelude::*;
    //!
    //! #[event]
    //! async fn h(ctx: EventCtx, text: String) -> HandlerResult<()> {
    //!     Ok(())
    //! }
    //! ```
    //!
    //! Interactions are handled by commands and Routes, never by `#[event]`:
    //!
    //! ```compile_fail
    //! use noctis::events::EventCtx;
    //! use noctis::prelude::*;
    //! use noctis::twilight::model::gateway::payload::incoming::InteractionCreate;
    //!
    //! #[event]
    //! async fn h(ctx: EventCtx, i: InteractionCreate) -> HandlerResult<()> {
    //!     Ok(())
    //! }
    //! ```
    //!
    //! The first parameter is the context:
    //!
    //! ```compile_fail
    //! use noctis::events::MessageCreate;
    //! use noctis::prelude::*;
    //!
    //! #[event]
    //! async fn h(ctx: u8, msg: MessageCreate) -> HandlerResult<()> {
    //!     Ok(())
    //! }
    //! ```
    //!
    //! And a handler returns `HandlerResult<()>`:
    //!
    //! ```compile_fail
    //! use noctis::events::{EventCtx, MessageCreate};
    //! use noctis::prelude::*;
    //!
    //! #[event]
    //! async fn h(ctx: EventCtx, msg: MessageCreate) {}
    //! ```

    pub use noctis_core::events::{
        AutoModerationActionExecution, AutoModerationRuleCreate, AutoModerationRuleDelete,
        AutoModerationRuleUpdate, BanAdd, BanRemove, ChannelCreate, ChannelDelete,
        ChannelPinsUpdate, ChannelUpdate, CommandPermissionsUpdate, EntitlementCreate,
        EntitlementDelete, EntitlementUpdate, GuildAuditLogEntryCreate, GuildCreate, GuildDelete,
        GuildEmojisUpdate, GuildIntegrationsUpdate, GuildScheduledEventCreate,
        GuildScheduledEventDelete, GuildScheduledEventUpdate, GuildScheduledEventUserAdd,
        GuildScheduledEventUserRemove, GuildStickersUpdate, GuildUpdate, IntegrationCreate,
        IntegrationDelete, IntegrationUpdate, InviteCreate, InviteDelete, MemberAdd, MemberChunk,
        MemberRemove, MemberUpdate, MessageCreate, MessageDelete, MessageDeleteBulk,
        MessagePollVoteAdd, MessagePollVoteRemove, MessageUpdate, PresenceUpdate, ReactionAdd,
        ReactionRemove, ReactionRemoveAll, ReactionRemoveEmoji, RoleCreate, RoleDelete, RoleUpdate,
        StageInstanceCreate, StageInstanceDelete, StageInstanceUpdate, ThreadCreate, ThreadDelete,
        ThreadListSync, ThreadMemberUpdate, ThreadMembersUpdate, ThreadUpdate, TypingStart,
        UnavailableGuild, UserUpdate, VoiceServerUpdate, VoiceStateUpdate, WebhooksUpdate,
    };
    pub use noctis_core::{EventCtx, EventHandler, EventPayload};
}

pub mod twilight {
    //! The twilight crates Noctis builds on, for the Discord endpoints and
    //! models Noctis does not wrap.
    //!
    //! - [`http`]: `twilight-http`; [`App::http`](crate::App::http) is the
    //!   App's one [`Client`](http::Client).
    //! - [`model`]: `twilight-model`, e.g. [`Intents`](model::gateway::Intents).
    //!
    //! ```
    //! use noctis::twilight::model::gateway::Intents;
    //!
    //! assert!(Intents::GUILDS.contains(Intents::GUILDS));
    //! ```

    pub use twilight_http as http;
    pub use twilight_model as model;
}

/// A literal Components V2 tree, checked at `cargo check`; yields a
/// [`ui::Components`]. See [`ui`](ui#literal-macros-and-components).
pub use noctis_macros::components;
/// Legacy message content literal, checked at `cargo check` against MSG-01;
/// yields a [`ui::Content`].
pub use noctis_macros::content;
/// A button label literal, checked at `cargo check` against CV2-08; yields a
/// [`ui::Label`].
pub use noctis_macros::label;
/// A select placeholder literal, checked at `cargo check` against CV2-09;
/// yields a [`ui::Placeholder`].
pub use noctis_macros::placeholder;
/// A `0xRRGGBB` colour literal, checked at `cargo check` against CV2-13;
/// yields a [`ui::Rgb`].
pub use noctis_macros::rgb;

pub mod codec {
    //! Payload field encodings for `custom_id`s.
    //!
    //! - [`Field`] and [`FieldDecode`]: a Payload field's encoding, worst-case
    //!   width (`MAX_LEN`) and canonical decoding. Built in for `bool`,
    //!   `u8`..`u64`, `i8`..`i64`, Discord ids, `Option<T>`, [`BoundedStr`],
    //!   `String`, `Cow<str>` and `Vec<T>`.
    //! - [`BoundedStr`] and [`BoundedStrError`]: printable-ASCII text of at most
    //!   `N` characters, `2 * N` wide once escaped.
    //!   [`FieldDecode::absent`] is a field's value when it is missing from
    //!   the end of an older `custom_id`: `Some(None)` for `Option<T>`, `None`
    //!   otherwise, so appending trailing `Option` fields needs no schema bump.
    //! - [`FieldError`]: why a field did not decode.
    //! - [`check`]: [`check::assert_round_trip`] and
    //!   [`check::assert_worst_case`] for the tests of custom fields.
    //! - [`Route`], [`RouteId`], [`RouteShape`] and [`BodyKind`]: a Route's
    //!   id, codec shape (inline, by Ref or spill, with optional View envelope,
    //!   select nonce and integrity tag), schema and Payload; usually derived
    //!   with `#[derive(Route)]` from the [`prelude`](crate::prelude).
    //! - [`SelectRoute`]: a Route declared `select` (`#[route("…", select)]`
    //!   implements it), the only kind a message select is built from.
    //! - [`Payload`] and [`PayloadDecode`]: an ordered field list, its
    //!   worst-case width [`Payload::MAX_ENCODED_LEN`], written through
    //!   [`FieldWriter`] and read through [`FieldReader`].
    //! - [`CodecError`] and [`Section`]: why an inline body or a `custom_id`
    //!   did not decode, and which layout section was wrong.
    //!
    //! ```
    //! use noctis::codec::{BoundedStr, Field, FieldDecode, FieldError, check};
    //!
    //! assert_eq!(<u8 as Field>::MAX_LEN, Some(2));
    //! assert_eq!(<BoundedStr<6> as Field>::MAX_LEN, Some(12));
    //! const SKU: BoundedStr<6> = BoundedStr::from_static("ab:12");
    //! check::assert_round_trip(&SKU);
    //!
    //! #[derive(Debug, Clone, Copy, PartialEq)]
    //! struct Percent(u8);
    //!
    //! impl Field for Percent {
    //!     const MAX_LEN: Option<usize> = <u8 as Field>::MAX_LEN;
    //!     fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result {
    //!         self.0.encode(out)
    //!     }
    //! }
    //!
    //! impl<'de> FieldDecode<'de> for Percent {
    //!     fn decode(raw: &'de str) -> Result<Self, FieldError> {
    //!         let v = u8::decode(raw)?;
    //!         if v <= 100 { Ok(Self(v)) } else { Err(FieldError::Overflow) }
    //!     }
    //! }
    //!
    //! check::assert_round_trip(&Percent(42));
    //! check::assert_worst_case(&Percent(100));
    //! ```

    pub use noctis_codec::{
        BodyKind, BoundedStr, BoundedStrError, CodecError, Field, FieldDecode, FieldError,
        FieldReader, FieldWriter, Payload, PayloadDecode, Route, RouteId, RouteShape, Section,
        SelectRoute, check,
    };
}

pub mod bytes {
    //! [`Bytes`]: the opaque value type of the byte-store ports (re-exported
    //! from the `bytes` crate).

    pub use bytes::Bytes;
}

pub mod ports {
    //! Adapter ports: swappable storage, coordination and rendering seams.
    //!
    //! - Byte stores, each a subset of the shared op vocabulary (`get`,
    //!   `put`, `put_if_absent`, `take`, `delete`, `touch`, `incr_with_ttl`):
    //!   [`RefStore`], [`SessionStore`], [`ModuleStateStore`] (plus
    //!   `version`), [`RateLimitStore`], [`CacheStore`], [`RenderCache`] and
    //!   [`AssetStore`].
    //! - [`IdentifyQueue`] and [`Lease`]: identify coordination per bucket.
    //! - [`DiscordCache`]: the typed Discord resource cache.
    //! - [`LocaleSource`]: runtime overlays of content strings.
    //! - [`GuildAccessList`]: per-App guild allow and deny lists (sync).
    //! - [`PortError`], [`ValueError`] and [`BoxFuture`]: what every port
    //!   method returns.
    //! - [`encode_value`] and [`decode_value`]: typed values as postcard with
    //!   a leading schema byte; a schema mismatch reads as a miss.
    //! - The render contract: [`Template`], [`ErasedView`],
    //!   [`TemplateDescriptor`], [`RenderJob`], [`decode_template`], [`Size`],
    //!   [`Format`], [`AnimationSpec`], [`AnimatedFormat`], [`Frame`],
    //!   [`OutputFormat`], [`RenderOutput`], [`EncodeSettings`],
    //!   [`RenderError`] and the [`Renderer`] port.
    //! - [`memory`]: the bounded in-memory default of every port.
    //!
    //! Every port is `Send + Sync + 'static` and dyn-compatible, so the App
    //! holds it as `Arc<dyn Port>`. Keys are opaque and stored verbatim, and no
    //! method ever receives a secret.
    //!
    //! ```
    //! use std::sync::Arc;
    //!
    //! use noctis::ports::{CacheStore, decode_value, encode_value};
    //!
    //! let _store: Option<Arc<dyn CacheStore>> = None;
    //!
    //! let raw = encode_value(1, &("profile", 42u32)).unwrap();
    //! assert_eq!(raw[0], 1);
    //! assert_eq!(decode_value::<(String, u32)>(1, &raw), Ok(Some(("profile".to_owned(), 42))));
    //! assert_eq!(decode_value::<(String, u32)>(2, &raw), Ok(None));
    //! ```

    pub use noctis_ports::{
        AnimatedFormat, AnimationSpec, AssetStore, BoxFuture, CacheStore, DiscordCache,
        EncodeSettings, ErasedView, Format, Frame, GuildAccessList, IdentifyQueue, Lease,
        LocaleSource, ModuleStateStore, OutputFormat, PortError, RateLimitStore, RefStore,
        RenderCache, RenderError, RenderJob, RenderOutput, Renderer, SessionStore, Size, Template,
        TemplateDescriptor, ValueError, decode_template, decode_value, encode_value,
    };

    pub mod memory {
        //! The bounded in-memory defaults: what an App runs on when you swap
        //! nothing.
        //!
        //! | Store | Port | `max_bytes` | `max_ttl` |
        //! | --- | --- | --- | --- |
        //! | [`MemoryRefStore`] | [`RefStore`](super::RefStore) | 32 MiB | 7 days |
        //! | [`MemorySessionStore`] | [`SessionStore`](super::SessionStore) | 1 MiB | none |
        //! | [`MemoryModuleStateStore`] | [`ModuleStateStore`](super::ModuleStateStore) | 16 MiB | none |
        //! | [`MemoryRateLimitStore`] | [`RateLimitStore`](super::RateLimitStore) | 16 MiB | 1 day |
        //! | [`MemoryCacheStore`] | [`CacheStore`](super::CacheStore) | 32 MiB | none |
        //! | [`MemoryRenderCache`] | [`RenderCache`](super::RenderCache) | 64 MiB | 1 day |
        //! | [`MemoryAssetStore`] | [`AssetStore`](super::AssetStore) | 32 MiB | 1 day |
        //!
        //! Each store has `DEFAULT_LIMITS`, `new()`, `with_limits(`[`MemoryLimits`]`)`,
        //! `limits()` and `usage() -> `[`MemoryUsage`]. An entry weighs
        //! `key.len() + value.len() + MemoryLimits::ENTRY_OVERHEAD` bytes; a
        //! full budget evicts the least recently used entry, a single write
        //! larger than the budget is a `PortError::InvalidValue`, and every TTL
        //! is clamped to `max_ttl`. The defaults are caches: capacity may evict
        //! any entry, swap a persistent adapter for durable data.
        //!
        //! [`MemoryDiscordCache`] caches only the chosen
        //! [`DiscordCacheResources`] and at most a per-channel message cap.
        //! [`NoLocaleOverlay`] and [`AllowAllGuilds`] are the defaults of
        //! `LocaleSource` and `GuildAccessList`.
        //!
        //! [`MemoryIdentifyQueue`] grants identifies by bucket
        //! (`shard_id % max_concurrency`) in one process: one Lease per bucket
        //! per [`IDENTIFY_INTERVAL`](MemoryIdentifyQueue::IDENTIFY_INTERVAL)
        //! (5 s), buckets in parallel, FIFO within a bucket. It does not cover
        //! several processes: a Cluster implementation must serialize per
        //! bucket across processes (see the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md)).
        //!
        //! ```
        //! use std::num::NonZeroU16;
        //! use std::sync::Arc;
        //! use std::time::Duration;
        //!
        //! use noctis::ports::{CacheStore, IdentifyQueue};
        //! use noctis::ports::memory::{MemoryCacheStore, MemoryIdentifyQueue, MemoryLimits};
        //!
        //! let store: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new());
        //! # let _ = store;
        //! assert_eq!(MemoryCacheStore::DEFAULT_LIMITS.max_bytes(), 32 * 1024 * 1024);
        //! assert_eq!(MemoryCacheStore::DEFAULT_LIMITS.max_ttl(), None);
        //!
        //! const SMALL: MemoryLimits = MemoryLimits::new(1024 * 1024, Some(Duration::from_secs(60)));
        //! let small = MemoryCacheStore::with_limits(SMALL);
        //! assert_eq!(small.limits(), SMALL);
        //! assert_eq!(small.usage().entries, 0);
        //!
        //! let identify = MemoryIdentifyQueue::new(NonZeroU16::new(16).unwrap());
        //! assert_eq!(identify.max_concurrency().get(), 16);
        //! let q: Arc<dyn IdentifyQueue> = Arc::new(identify);
        //! # let _ = q;
        //! ```

        pub use noctis_ports::memory::{
            AllowAllGuilds, DiscordCacheResources, MemoryAssetStore, MemoryCacheStore,
            MemoryDiscordCache, MemoryIdentifyQueue, MemoryLimits, MemoryModuleStateStore,
            MemoryRateLimitStore, MemoryRefStore, MemoryRenderCache, MemorySessionStore,
            MemoryUsage, NoLocaleOverlay,
        };
    }
}

pub mod prelude {
    //! The derives and the traits they implement: `use noctis::prelude::*;`.
    //!
    //! - [`Route`]: the trait and `#[derive(Route)]`, which also derives the
    //!   Payload and proves the 100-character bound at compile time.
    //! - [`Payload`]: the trait and `#[derive(Payload)]`, for the Payload of a
    //!   hand-written `Route`.
    //! - [`PayloadEnum`]: `#[derive(PayloadEnum)]` on a unit enum of at most 62
    //!   variants, a one-character [`Field`](crate::codec::Field).
    //! - [`RouteComponents`]: `.button()`, `.string_select(..)`,
    //!   `.user_select()`, `.role_select()`, `.mentionable_select()`,
    //!   `.channel_select()` and `.modal(title)` on a Route value (see
    //!   [`ui`](crate::ui)).
    //! - [`Rgb`] and the macros [`components!`], [`label!`],
    //!   [`placeholder!`], [`content!`] and [`rgb!`].
    //! - [`Modal`](macro@Modal): `#[derive(Modal)]` on a form struct,
    //!   implementing [`ModalForm`] (see [`ui`](crate::ui#modal-forms)).
    //! - [`Choices`]: the trait and `#[derive(Choices)]` on a unit enum, a
    //!   modal option set.
    //! - [`App`], [`AppBuilder`] and [`State`]: building an App and reading
    //!   its typed state.
    //! - [`event`](macro@event), [`Module`], [`EventCtx`], [`HandlerError`]
    //!   and [`HandlerResult`]: typed gateway Event Handlers grouped in
    //!   Modules.
    //! - [`command`](macro@command) and [`CommandCtx`]: slash Commands;
    //!   [`user_command`](macro@user_command) and
    //!   [`message_command`](macro@message_command): context-menu commands;
    //!   [`CommandResponder`], [`OpensModal`], [`Responded`],
    //!   [`IntoResponse`], [`Ephemerality`] and [`Visibility`]: how they
    //!   answer.
    //! - [`Options`](trait@Options): the trait and `#[derive(Options)]`,
    //!   a Command's typed options; [`CommandOption`] (the field types) and
    //!   [`MessageKey`] (what option validators return).
    //! - [`ErrorSink`] and [`ErrorOrigin`]: routing handler failures.

    pub use noctis_codec::{Payload, Route};
    pub use noctis_core::events::EventCtx;
    pub use noctis_core::{
        App, AppBuilder, CommandCtx, CommandOption, CommandResponder, Ephemerality, ErrorOrigin,
        ErrorSink, HandlerError, HandlerResult, IntoResponse, Module, OpensModal, Options,
        Responded, State, Visibility,
    };
    pub use noctis_macros::{Choices, Modal, Options, Payload, PayloadEnum, Route};
    pub use noctis_macros::{command, event, message_command, user_command};
    pub use noctis_macros::{components, content, label, placeholder, rgb};
    pub use noctis_rules::MessageKey;
    pub use noctis_ui::{Choices, ModalForm, Rgb, RouteComponents};
}

/// Paths the derive macros expand to; not public API.
#[doc(hidden)]
pub mod __private {
    pub use noctis_codec as codec;
    pub use noctis_core as core;
    pub use noctis_rules as rules;
    pub use noctis_ui as ui;
    pub use twilight_model as model;
}

pub mod ui {
    //! The Components V2 message model.
    //!
    //! Every component is its own type with by-value, chainable setters
    //! (text is `impl Into<Cow<'static, str>>`), and serializes straight to
    //! Discord's JSON with unset fields left out:
    //!
    //! - Layout: [`Container`] (type 17, [`Container::push`],
    //!   [`Container::accent`] with an [`Rgb`], [`Container::spoiler`]),
    //!   [`Section`] (type 9, `Section::new([TextDisplay; N], accessory)`) and
    //!   [`ActionRow`] (type 1, [`ActionRow::buttons`] or [`ActionRow::select`]).
    //! - Content: [`TextDisplay`] (type 10), [`Separator`] (type 14, with
    //!   [`Spacing`]), [`Thumbnail`] (type 11), [`MediaGallery`] (type 12, of
    //!   [`MediaItem`]s), [`File`] (type 13) and [`Media`] (a URL or an
    //!   attachment).
    //! - Buttons (type 2): [`RouteButton`] (styles 1-4, [`ButtonStyle`],
    //!   [`Emoji`]), [`LinkButton`], [`PremiumButton`], and [`Button`] holding
    //!   any of them.
    //! - Selects: [`StringSelect`] (type 3, of [`SelectOption`]s),
    //!   [`UserSelect`] (5), [`RoleSelect`] (6), [`MentionableSelect`] (7) and
    //!   [`ChannelSelect`] (8).
    //! - [`Components`]: a message body; it numbers every component in
    //!   pre-order from 1, and no API sets a numeric id (CV2-14).
    //! - [`ContainerComponent`] and [`TopLevelComponent`]: any component of a
    //!   Container or of the top level, for lists built at runtime.
    //! - [`Attachment`], [`AttachmentOrigin`], [`AttachmentSet`] and
    //!   [`AttachmentRef`]: content-addressed files; [`File`] takes only an
    //!   [`AttachmentRef`], which only [`AttachmentSet::add`] returns (CV2-12);
    //!   [`Media`] takes an [`AttachmentRef`] or a URL, and an `attachment://`
    //!   name given as a URL (or a ref from another set) is checked at runtime
    //!   by ATT-05.
    //! - [`UiError`]: a bad attachment extension, a mismatched Ref slot, an
    //!   edit of a V2 message back into a legacy one
    //!   ([`UiError::V2ToLegacy`]), or a message or value over its Limits
    //!   ([`UiError::Limits`]).
    //!
    //! # Arity
    //!
    //! Array constructors take only legal lengths, as sealed traits on
    //! [`Arity`]`<N>`: [`ActionRow::buttons`] 1 to 5 ([`RowButtons`],
    //! CV2-06), [`Section::new`] 1 to 3 texts ([`SectionTexts`], CV2-07) and
    //! [`MediaGallery::new`] 1 to 10 items ([`GalleryItems`], CV2-10). Any
    //! other length fails `cargo check` with the rule id. A length known only
    //! at runtime goes through [`ActionRow::try_buttons`],
    //! [`Section::try_new`] or [`MediaGallery::try_new`], which return
    //! [`UiError::Limits`] instead.
    //!
    //! ```compile_fail,E0277
    //! use noctis::ui::{ActionRow, LinkButton};
    //!
    //! let link = || LinkButton::new("https://x.y");
    //! let _ = ActionRow::buttons([link(), link(), link(), link(), link(), link()]); // [CV2-06]
    //! ```
    //!
    //! ```
    //! use noctis::rules::{Limits, RuleId};
    //! use noctis::ui::{ActionRow, LinkButton, UiError};
    //!
    //! let urls = vec!["https://a.b"; 6];
    //! let six = ActionRow::try_buttons(urls.into_iter().map(LinkButton::new), &Limits::default());
    //! assert!(matches!(six, Err(UiError::Limits { violations }) if violations[0].rule == RuleId::CV2_06));
    //! ```
    //!
    //! # Literal macros and `components!`
    //!
    //! The literal macros measure their literal at `cargo check` with the
    //! `noctis-rules` function the runtime path calls:
    //! [`label!`](crate::label) ([`Label`], CV2-08, at most 80 characters),
    //! [`placeholder!`](crate::placeholder) ([`Placeholder`], CV2-09, 150),
    //! [`content!`](crate::content) ([`Content`], MSG-01, 2000) and
    //! [`rgb!`](crate::rgb) ([`Rgb`], CV2-13, `0x000000..=0xFFFFFF`). Each
    //! works in a `const`. Text known only at runtime (localized text
    //! included) goes through `Label::try_new(text, &limits)` (or
    //! `Placeholder`, `Content`) and `Rgb::try_from(u32)`; none of them ever
    //! truncates.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{Label, LegacyMessage, LinkButton};
    //!
    //! const SAVE: Label = label!("Save");
    //! const ACCENT: Rgb = rgb!(0x5865_F2);
    //! let link = LinkButton::new("https://x.y").label(SAVE);
    //! let notice = LegacyMessage::new(content!("Saved."));
    //! assert_eq!(ACCENT, Rgb::new(0x58, 0x65, 0xF2));
    //! # let _ = (link, notice, placeholder!("Pick one"));
    //! ```
    //!
    //! ```compile_fail
    //! let name = "Save";
    //! let _ = noctis::label!(name); // label! takes one string literal
    //! ```
    //!
    //! ```compile_fail
    //! let _ = noctis::rgb!(-1); // rgb! takes one non-negative integer literal
    //! ```
    //!
    //! [`components!`](crate::components) writes a fixed-shape tree:
    //!
    //! ```text
    //! components! { node,* }
    //! node      := container | section | text | gallery | file | separator | row | {hole} | ..spread
    //! container := container [ "(" accent = expr ")" ] "[" node,* "]"
    //! section   := section "[" text,* ";" accessory = (thumbnail | button | {hole}) "]"
    //! text      := text "(" string-literal | expr ")"
    //! thumbnail := thumbnail "(" media [, alt = text] ")"      // a string literal media is a URL
    //! gallery   := gallery "[" item "(" media [, alt = text] ")",* "]"
    //! file      := file "(" attachment_ref ")"
    //! separator := separator [ "(" [spacing = small | large] [, divider = expr] ")" ]  // both optional, any order
    //! row       := row "[" (button | {hole}),* "]" | row "[" select "]"
    //! button    := route.button( [label] ) .setter(..)* | link(url = "..", label = "..") | premium(sku)
    //! select    := route.string_select(options) .setter(..)* | route.user_select() .setter(..)* | ..
    //! ```
    //!
    //! It expands into the builders above (so the placement traits still
    //! apply to `{hole}`s and `..spread`s) and fails `cargo check` with the
    //! rule id, spanned on the offending node or literal, when the literal
    //! tree breaks CV2-03 (40 components; a hole counts 1, a spread 0),
    //! CV2-04 (4000 literal text characters; non-literal text counts 0),
    //! CV2-05, CV2-06, CV2-07, CV2-08 (literal labels and link URLs), CV2-09
    //! (literal placeholders, `min_values`, `max_values` and array-literal
    //! options), CV2-10, CV2-11, CV2-13 (literal accents, Separator spacing),
    //! CV2-17 (literal media URLs) or CID-02 (two components of the same
    //! literal Route value). The runtime [`validate`] still checks the built
    //! message before sending.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::rules::Limits;
    //! use noctis::ui::{ButtonStyle, TextDisplay, V2Message, validate};
    //!
    //! #[derive(Route)]
    //! #[route("t.press")]
    //! struct Press {
    //!     n: u8,
    //! }
    //! # impl noctis::__private::ui::Handled for Press {}
    //!
    //! let name = String::from("noctis");
    //! let footer = TextDisplay::new("-# footer");
    //! let body = components! {
    //!     container(accent = rgb!(0x5865_F2)) [
    //!         text("### Settings"),
    //!         separator(spacing = large),
    //!         section [ text("Logs"), text(name); accessory = Press { n: 1 }.button("Change") ],
    //!         row [ Press { n: 2 }.button("Save").style(ButtonStyle::Primary), link(url = "https://x.y", label = "Docs") ],
    //!     ],
    //!     {footer},
    //! };
    //! let mut msg = V2Message::new(body);
    //! assert_eq!(validate(&mut msg, &Limits::default()).unwrap().components, 11);
    //! ```
    //!
    //! ```compile_fail
    //! use noctis::components;
    //!
    //! let _ = components! { box [] }; // unknown node
    //! ```
    //!
    //! # Messages
    //!
    //! - [`V2Message`]: [`Components`] plus an [`AttachmentSet`] and
    //!   [`Mentions`]; every body sets `IS_COMPONENTS_V2` (CV2-01), and it has
    //!   no `content`, `embeds`, `poll`, `sticker_ids` or
    //!   `shared_client_theme` (CV2-02).
    //! - [`LegacyMessage`]: plain `content` and at most 5 Action Rows
    //!   ([`Arity`]`<N>: `[`LegacyRows`], CV2-15); the only legacy path, never
    //!   for interactive UI.
    //! - Flags per endpoint (MSG-07): [`CreateFlags`] (Create Message, Execute
    //!   Webhook), [`EditFlags`] (Edit Message, Edit Original Interaction
    //!   Response, Edit Webhook Message, callback type 7) and
    //!   [`CallbackFlags`] (callback type 4 and follow-ups, INT-09). None
    //!   offers `IS_COMPONENTS_V2`.
    //! - [`Mentions`], [`Users`], [`Roles`]: pings are opt-in; the default is
    //!   `parse: []` (MSG-05), and `Users::All` cannot carry ids (MSG-03).
    //! - [`MessageFormat`]: the format of the message an edit replaces; a
    //!   legacy-to-V2 edit clears `content`, `embeds`, `poll` and
    //!   `sticker_ids`.
    //! - [`MessageBody`]: the borrowed JSON body core's Responder sends.
    //!
    //! # Limits
    //!
    //! - [`validate`]`(&mut msg, &limits)`: checks a built [`V2Message`],
    //!   [`LegacyMessage`] or [`Modal`] (the sealed [`Validate`] trait) against
    //!   [`Limits`](crate::rules::Limits) before sending and returns a
    //!   [`CostReport`], or [`UiError::Limits`]` { violations }` with every
    //!   violation. It lowers the tree into
    //!   [`rules::component`](crate::rules::component) descriptors, so counts,
    //!   paths and order match `components!`, and adds CID-02 (duplicate
    //!   `custom_id`s), ATT-05 (`attachment://` references), ATT-06, MSG-01,
    //!   MSG-04 and MSG-09 (empty bodies).
    //! - [`cost`]`(&msg, &limits)`: the [`CostReport`] (components at every
    //!   depth, text characters, warnings) without changing or sending.
    //! - Paths name each node by kind and position, as in
    //!   `container[0].section[2].accessory.label`, `row[0].select[0].options[3].label`,
    //!   `attachments[0].description` or `allowed_mentions.users`.
    //! - Strict mode (the default) never changes the message. Lenient mode
    //!   truncates display text with `…` first (Text Displays to the CV2-04
    //!   total, button labels, select placeholders and option labels and
    //!   descriptions, gallery, Thumbnail and attachment alt text, legacy
    //!   content), each a [`LimitWarning::Truncated`]; counts, URLs, values
    //!   and ids still fail.
    //! - [`LimitWarning::MissingAlt`]: a Thumbnail or gallery item that is not
    //!   a rendered image has no alt text (both modes). Every warning is also
    //!   logged at WARN with target `noctis::ui`.
    //!
    //! ```
    //! use noctis::rules::{LimitMode, Limits, RuleId};
    //! use noctis::ui::{ActionRow, Components, LinkButton, UiError, V2Message, validate};
    //!
    //! let row = || ActionRow::buttons([LinkButton::new("https://x.y").label("a".repeat(81))]);
    //! let mut msg = V2Message::new(Components::new().push(row()));
    //! let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
    //!     panic!("label too long");
    //! };
    //! assert_eq!(violations[0].rule, RuleId::CV2_08);
    //! assert_eq!(violations[0].path, "row[0].button[0].label");
    //!
    //! let mut limits = Limits::default();
    //! limits.mode = LimitMode::Lenient;
    //! let report = validate(&mut msg, &limits).unwrap();
    //! assert_eq!(report.components, 2);
    //! assert_eq!(report.warnings.len(), 1);
    //! ```
    //!
    //! # Modals
    //!
    //! - [`Modal`]: interaction callback `data` (type 9), built only from a
    //!   Route value with `.modal(title)` ([`RouteComponents::modal`]), so its
    //!   `custom_id` is the Route's encoding (MOD-08); push 1 to 5
    //!   [`ModalLabel`]s or [`TextDisplay`]s ([`ModalTopLevel`], MOD-02).
    //!   [`ModalComponent`] holds either, for lists built at runtime.
    //! - [`ModalLabel`] (type 18): Discord's Label component, with text, an
    //!   optional description and exactly one input ([`ModalInput`]). It is
    //!   not [`Label`], the checked button-label text.
    //! - Inputs, each built from a string key (its `custom_id`, the submitted
    //!   field's name): [`TextInput`] (type 4, [`TextInputStyle`]; `.value`
    //!   pre-fills it), [`ModalStringSelect`] (3), [`ModalUserSelect`] (5),
    //!   [`ModalRoleSelect`] (6), [`ModalMentionableSelect`] (7),
    //!   [`ModalChannelSelect`] (8), [`FileUpload`] (19), [`RadioGroup`] (21)
    //!   and [`CheckboxGroup`] (22) of [`ModalOption`]s, and [`Checkbox`] (23).
    //! - A message component in a modal fails `cargo check` with `[MOD-03]`;
    //!   modal selects have no `disabled` and a [`Checkbox`] has no `required`.
    //!   A modal never has an Action Row or a Text Input `label`.
    //! - [`validate`] and [`cost`] take a [`Modal`]: MOD-01..07 and CV2-09 in
    //!   the [`rules::modal`](crate::rules::modal) order, then CID-01 and
    //!   CID-02 on the input keys (paths like `label[2].text_input.custom_id`).
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::rules::Limits;
    //! use noctis::ui::{
    //!     Checkbox, FileUpload, ModalLabel, ModalOption, RadioGroup, TextDisplay, TextInput,
    //!     TextInputStyle, validate,
    //! };
    //!
    //! #[derive(Route)]
    //! #[route("mod.report")]
    //! struct Report {
    //!     id: u64,
    //! }
    //! # impl noctis::__private::ui::Handled for Report {}
    //!
    //! let mut modal = Report { id: 7 }
    //!     .modal("Report a user")
    //!     .push(TextDisplay::new("Tell us what happened."))
    //!     .push(ModalLabel::new("Reason", TextInput::new("reason", TextInputStyle::Paragraph).max_length(500)))
    //!     .push(ModalLabel::new("Severity", RadioGroup::new("sev", [ModalOption::new("Low", "l"), ModalOption::new("High", "h")])))
    //!     .push(ModalLabel::new("Evidence", FileUpload::new("files").required(false).min_values(0).max_values(3)))
    //!     .push(ModalLabel::new("I agree", Checkbox::new("ok")));
    //! assert_eq!(validate(&mut modal, &Limits::default()).unwrap().components, 9);
    //! let json = serde_json::to_value(&modal).unwrap();
    //! assert_eq!(json["components"][1]["component"]["type"], 4);
    //! ```
    //!
    //! ```compile_fail,E0277
    //! use noctis::prelude::*;
    //! use noctis::ui::LinkButton;
    //!
    //! #[derive(Route)]
    //! #[route("mod.form")]
    //! struct Form;
    //! # impl noctis::__private::ui::Handled for Form {}
    //!
    //! let _ = Form.modal("T").push(LinkButton::new("https://x.y")); // [MOD-03]
    //! ```
    //!
    //! # Modal forms
    //!
    //! `#[derive(Modal)]` (in the [`prelude`](crate::prelude)) declares a
    //! modal as a struct and implements [`ModalForm`]: `T::modal(route)`
    //! builds the [`Modal`] and `T::from_json(bytes)` decodes the raw
    //! modal-submit interaction JSON. `#[modal(route = R, title = "…")]`
    //! names the submit Route and the literal title. One field is one
    //! top-level component, in declaration order; its name is the input key
    //! (`custom_id`, `r#` stripped) and it carries exactly one attribute whose
    //! first literal is the Label text (every Label attribute also takes
    //! `description = "…"`):
    //!
    //! | Attribute | Arguments | Field types |
    //! | --- | --- | --- |
    //! | `text_input("L", …)` | `style = short\|paragraph`, `min_length`, `max_length`, `placeholder` | `String`, `Option<String>` |
    //! | `string_select("L", …)` | `placeholder`, `min_values`, `max_values` | `C`, `Option<C>`, `Vec<C>` (`C: Choices`) |
    //! | `user_select` / `role_select` / `mentionable_select` / `channel_select("L", …)` | `placeholder`, `min_values`, `max_values`; `channel_types = [GuildText, …]` | [`ResolvedUser`] / [`ResolvedRole`] / [`Mentionable`] / [`ResolvedChannel`], `Option<_>`, `Vec<_>` |
    //! | `file_upload("L", …)` | `min_values`, `max_values`, `file_types = ["image", ".pdf"]` | [`ResolvedAttachment`], `Option<_>`, `Vec<_>` |
    //! | `radio_group("L")` | | `C`, `Option<C>` |
    //! | `checkbox_group("L", …)` | `min_values`, `max_values` | `Vec<C>` |
    //! | `checkbox("L")` | | `bool` |
    //! | `text_display("content")` | | `()` |
    //!
    //! - `Option<_>` is optional (`"required": false`), a plain type required;
    //!   a `Vec<_>` is required unless `min_values = 0`. `min_values` /
    //!   `max_values` are for `Vec` fields only.
    //! - Every literal is checked at `cargo check` against the
    //!   [`rules::modal`](crate::rules::modal) rules (MOD-01, MOD-02, MOD-04..07,
    //!   CV2-09, CID-01 for keys, CV2-04 for the Text Displays). Localized
    //!   texts are [planned, Epic 11]. Option counts come from
    //!   [`Choices::Count`]: a Radio Group takes 2-10 options
    //!   ([`RadioOptions`]), a Checkbox Group 1-10 ([`CheckboxOptions`]), a
    //!   String Select 1-25 ([`StringSelectOptions`]).
    //! - `#[derive(Choices)]` on a unit enum: `#[choice("Label")]` or
    //!   `#[choice("Label", value = "v", description = "d")]` per variant (the
    //!   value defaults to the variant name); labels and values 1-100,
    //!   reported as OPT-09 (the same limit as MOD-07 and CV2-09), and
    //!   descriptions at most 100 (MOD-07). The derive also implements
    //!   [`CommandOption`](crate::CommandOption), so the enum is a STRING
    //!   command option with these choices.
    //! - [`ResolvedUser::from_resolved`], [`ResolvedRole::from_resolved`],
    //!   [`ResolvedChannel::from_resolved`], [`Mentionable::from_resolved`],
    //!   [`ResolvedAttachment::from_resolved`] and
    //!   [`ResolvedMessage::from_resolved`] look an id up in an
    //!   interaction's resolved data. [`ResolvedUser`] and [`ResolvedMessage`]
    //!   are also the targets of context-menu commands.
    //! - Decoding ([`ModalSubmission::from_slice`], then
    //!   [`ModalForm::from_submission`]) reads Label-wrapped and legacy
    //!   Action-Row-wrapped inputs, including Radio Groups, Checkbox Groups
    //!   and Checkboxes, joins selects to the resolved users, members, roles
    //!   and channels and uploads to their attachments, and keeps unknown
    //!   components as `serde_json::Value`s ([`ModalSubmission::unknown`]).
    //!   Every missing, ill-typed or out-of-bounds value is a [`SubmitError`]
    //!   (`[MOD-09]`), never a panic. The `custom_id` is not checked against
    //!   the Route.
    //! - A [`ResolvedAttachment`]'s URL is ephemeral: download it promptly with
    //!   [`ResolvedAttachment::download`] through a [`FileFetcher`], which
    //!   checks the CDN host, `Limits::attachment_bytes` and the length
    //!   ([`DownloadError`]), and re-check its content type.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{ResolvedAttachment, SubmitError};
    //!
    //! #[derive(Route)]
    //! #[route("mod.report")]
    //! struct Report {
    //!     id: u64,
    //! }
    //! # impl noctis::__private::ui::Handled for Report {}
    //!
    //! #[derive(Choices, Clone, Copy, PartialEq, Eq, Debug)]
    //! enum Severity {
    //!     #[choice("Low", value = "l")]
    //!     Low,
    //!     #[choice("High", value = "h")]
    //!     High,
    //! }
    //!
    //! #[derive(Modal, Debug)]
    //! #[modal(route = Report, title = "Report a user")]
    //! struct ReportForm {
    //!     #[text_display("Tell us what happened.")]
    //!     _intro: (),
    //!     #[text_input("Reason", style = paragraph, max_length = 500)]
    //!     reason: String,
    //!     #[radio_group("Severity")]
    //!     severity: Severity,
    //!     #[file_upload("Evidence", min_values = 0, max_values = 3)]
    //!     files: Vec<ResolvedAttachment>,
    //!     #[checkbox("I agree")]
    //!     agree: bool,
    //! }
    //!
    //! let modal = ReportForm::modal(Report { id: 7 });
    //! assert_eq!(modal.len(), 5);
    //!
    //! let bytes = br#"{"type": 5, "data": {"custom_id": "0mod.report:7", "components": [
    //!     {"type": 10, "content": "Tell us what happened."},
    //!     {"type": 18, "component": {"type": 4, "custom_id": "reason", "value": "spam"}},
    //!     {"type": 18, "component": {"type": 21, "custom_id": "severity", "value": "h"}},
    //!     {"type": 18, "component": {"type": 19, "custom_id": "files", "values": []}},
    //!     {"type": 18, "component": {"type": 23, "custom_id": "agree", "value": true}}
    //! ]}}"#;
    //! let form = ReportForm::from_json(bytes)?;
    //! assert_eq!((form.reason.as_str(), form.severity, form.agree), ("spam", Severity::High, true));
    //! assert!(form.files.is_empty());
    //! # Ok::<(), SubmitError>(())
    //! ```
    //!
    //! A field type the attribute does not take fails `cargo check`:
    //!
    //! ```compile_fail,E0277
    //! use noctis::prelude::*;
    //!
    //! #[derive(Route)]
    //! #[route("mod.form")]
    //! struct Form;
    //! # impl noctis::__private::ui::Handled for Form {}
    //!
    //! #[derive(Modal)]
    //! #[modal(route = Form, title = "T")]
    //! struct F {
    //!     #[checkbox("Agree")]
    //!     agree: String, // a Checkbox is `bool`
    //! }
    //! ```
    //!
    //! So does a Route without a handler (`[CID-04]`):
    //!
    //! ```compile_fail,E0277
    //! use noctis::prelude::*;
    //!
    //! #[derive(Route)]
    //! #[route("mod.form")]
    //! struct Form;
    //!
    //! #[derive(Modal)]
    //! #[modal(route = Form, title = "T")]
    //! struct F {
    //!     #[checkbox("Agree")]
    //!     agree: bool,
    //! }
    //! ```
    //!
    //! # Placement
    //!
    //! The placing methods are bounded by sealed traits, so a misplaced
    //! component fails `cargo check` with the rule id (CV2-05, CV2-07):
    //!
    //! | Component | Top level ([`TopLevel`]) | Container ([`ContainerChild`]) | Section accessory ([`SectionAccessory`]) | Action Row |
    //! | --- | --- | --- | --- | --- |
    //! | [`Container`] | yes | no | no | no |
    //! | [`Section`], [`TextDisplay`], [`MediaGallery`], [`File`], [`Separator`], [`ActionRow`] | yes | yes | no | no |
    //! | [`Thumbnail`] | no | no | yes | no |
    //! | [`RouteButton`], [`LinkButton`], [`PremiumButton`], [`Button`] | no | no | yes | 1-5 ([`RowItem`]) |
    //! | the five selects | no | no | no | alone ([`RowSelect`]) |
    //!
    //! # Components from Routes
    //!
    //! [`RouteComponents`] (also in the [`prelude`](crate::prelude)) builds
    //! buttons, selects and modals from a Route value; there is no string
    //! `custom_id`.
    //! The Route needs a handler (`#[handler]`), or the call fails with
    //! `[CID-04]`; a select also needs a Route declared `select`
    //! ([`SelectRoute`](crate::codec::SelectRoute)). `custom_id`s that need a
    //! View envelope, select nonce, signature or Ref are finalized by the
    //! Responder before sending.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{
    //!     ActionRow, ButtonStyle, Components, Container, LinkButton, Rgb, Section, Separator,
    //!     TextDisplay,
    //! };
    //!
    //! #[derive(Route)]
    //! #[route("shop.buy")]
    //! struct Buy {
    //!     item: u64,
    //! }
    //! # impl noctis::__private::ui::Handled for Buy {}
    //!
    //! let body = Components::new().push(
    //!     Container::new()
    //!         .accent(Rgb::new(0x58, 0x65, 0xF2))
    //!         .push(Section::new([TextDisplay::new("**Sword** · 255 g")], Buy { item: 255 }.button().label("Buy")))
    //!         .push(Separator::small())
    //!         .push(ActionRow::buttons([LinkButton::new("https://example.com").label("Docs")])),
    //! );
    //! let json = serde_json::to_value(&body).unwrap();
    //! assert_eq!(json[0]["components"][0]["accessory"]["custom_id"], "0shop.buy:47");
    //! # let _ = ButtonStyle::Primary;
    //! ```
    //!
    //! A V2 create body:
    //!
    //! ```
    //! use noctis::ui::{Components, CreateFlags, Mentions, TextDisplay, Users, V2Message};
    //!
    //! let msg = V2Message::new(Components::new().push(TextDisplay::new("Saved")))
    //!     .mentions(Mentions::none().users(Users::All));
    //! let json = serde_json::to_value(msg.create_body(CreateFlags::SUPPRESS_NOTIFICATIONS)).unwrap();
    //! assert_eq!(json["flags"], 32768 | 4096);
    //! assert_eq!(json["allowed_mentions"], serde_json::json!({"parse": ["users"]}));
    //! ```

    pub use noctis_ui::{
        ActionRow, Arity, Attachment, AttachmentOrigin, AttachmentRef, AttachmentSet, Button,
        ButtonStyle, CallbackFlags, ChannelSelect, Checkbox, CheckboxGroup, CheckboxOptions,
        Choices, Components, Container, ContainerChild, ContainerComponent, Content, CostReport,
        CreateFlags, DownloadError, EditFlags, Emoji, File, FileFetcher, FileUpload, GalleryItems,
        Label, LegacyMessage, LegacyRows, LimitWarning, LinkButton, Media, MediaGallery, MediaItem,
        Mentionable, MentionableSelect, Mentions, MessageBody, MessageFormat, Modal,
        ModalChannelSelect, ModalComponent, ModalForm, ModalInput, ModalLabel,
        ModalMentionableSelect, ModalOption, ModalRoleSelect, ModalStringSelect, ModalSubmission,
        ModalTopLevel, ModalUserSelect, Placeholder, PremiumButton, RadioGroup, RadioOptions,
        ResolvedAttachment, ResolvedChannel, ResolvedMessage, ResolvedRole, ResolvedUser, Rgb,
        RoleSelect, Roles, RouteButton, RouteComponents, RowButtons, RowItem, RowSelect, Section,
        SectionAccessory, SectionTexts, SelectOption, Separator, Spacing, StringSelect,
        StringSelectOptions, SubmitError, SubmittedValue, TextDisplay, TextInput, TextInputStyle,
        Thumbnail, TopLevel, TopLevelComponent, UiError, UserSelect, Users, V2Message, Validate,
        cost, validate,
    };
}

pub mod rules {
    //! Discord limits, rule ids, text length and locales.
    //!
    //! - [`Limits`] and [`LimitMode`]: every numeric Discord limit, each field
    //!   naming its rule id; the App builder hook that takes them is planned for a
    //!   later story.
    //! - [`RuleId`], [`Violation`], [`Bound`] and [`Actual`]: the rule ids of the
    //!   Discord Rules Catalog and the error that cites them.
    //! - [`discord_len`] and [`truncate`]: the only length function for user text,
    //!   and `…` truncation that respects it.
    //! - [`Locale`] and [`ParseLocaleError`]: Discord's 32 locale codes.
    //! - [`MessageKey`] and [`Translate`]: localization keys and the translation port.
    //! - [`command`]: Command Pattern expansion ([`command::expand`]), the
    //!   command name and tree rules ([`command::validate_name`],
    //!   [`command::validate_tree`], [`command::validate_merge`]), the option
    //!   rules ([`command::validate_options`], [`command::measure_options`]), the
    //!   context-menu name rule ([`command::validate_context_menu_name`]) and the
    //!   command metadata rules ([`command::validate_meta`]) and the autocomplete
    //!   response rules ([`command::validate_autocomplete`]) that `#[command]`,
    //!   `derive(Options)` and `App::build` call.
    //! - [`component`]: Components V2 trees and legacy Action Rows
    //!   ([`component::validate_components`],
    //!   [`component::validate_legacy_components`], [`component::cost`]) and
    //!   single values such as [`component::validate_button_label`] and
    //!   [`component::validate_accent_color`] (CV2-03..13, CV2-15..17).
    //! - [`modal`]: modals ([`modal::validate_modal`]) and single modal values
    //!   (MOD-01..07).
    //! - [`message`]: legacy `content`, allowed mentions, nonce, stickers,
    //!   non-empty bodies ([`message::validate_v2_not_empty`],
    //!   [`message::validate_legacy_not_empty`]) and webhook names (MSG-01,
    //!   MSG-03..06, MSG-09, WH-04).
    //! - [`attachment`]: file sizes, counts, alt text and `attachment://`
    //!   references (ATT-01..03, ATT-05, ATT-06).
    //!
    //! ```
    //! use noctis::rules::{discord_len, LimitMode, Limits, Locale, RuleId};
    //!
    //! assert_eq!(Limits::default().mode, LimitMode::Strict);
    //! let mut limits = Limits::default();
    //! limits.mode = LimitMode::Lenient;
    //! assert!(!limits.truncates(RuleId::CV2_03));
    //! assert!(limits.truncates(RuleId::CV2_04));
    //!
    //! assert_eq!("en-US".parse::<Locale>(), Ok(Locale::EnUs));
    //! assert_eq!(Locale::EnUs.sibling(), Some(Locale::EnGb));
    //! assert_eq!(discord_len("😀"), 2);
    //! ```
    //!
    //! ```
    //! use noctis::rules::command::{CommandDecl, LocalizedText, expand, validate_tree};
    //! use noctis::rules::{Limits, RuleId};
    //!
    //! let limits = Limits::default();
    //! let paths = expand("config (set|get) (prefix|welcome)", &limits).unwrap();
    //! assert_eq!(
    //!     paths,
    //!     ["config set prefix", "config set welcome", "config get prefix", "config get welcome"]
    //! );
    //!
    //! let paths = expand("config (set|get) (prefix|welcome)?", &limits).unwrap();
    //! let decls: Vec<_> = paths
    //!     .iter()
    //!     .map(|p| CommandDecl::handler("config", p).description(LocalizedText::new("d")))
    //!     .collect();
    //! let errs = validate_tree(&decls, &limits).unwrap_err();
    //! assert_eq!(errs[0].rule, RuleId::CMD_11);
    //! ```
    //!
    //! ```
    //! use noctis::rules::command::{OptionDecl, OptionType, validate_options};
    //! use noctis::rules::{Limits, RuleId};
    //!
    //! let options = [
    //!     OptionDecl::new("reason", OptionType::String, "Why"),
    //!     OptionDecl::new("days", OptionType::Integer, "How long").required(true),
    //! ];
    //! let errs = validate_options(&options, &Limits::default()).unwrap_err();
    //! assert_eq!(errs[0].rule, RuleId::OPT_01);
    //! assert_eq!(errs[0].path, "days");
    //! ```
    //!
    //! ```
    //! use noctis::rules::component::{Component, validate_components};
    //! use noctis::rules::modal::validate_modal_title;
    //! use noctis::rules::{Limits, RuleId};
    //!
    //! let limits = Limits::default();
    //! let tree = vec![Component::Text("a"); 41];
    //! let errs = validate_components(&tree, &limits).unwrap_err();
    //! assert_eq!(errs[0].rule, RuleId::CV2_03);
    //! assert_eq!(errs[0].path, "components");
    //!
    //! assert_eq!(validate_modal_title(&"a".repeat(46), &limits)[0].rule, RuleId::MOD_01);
    //! ```

    pub use noctis_rules::len::{discord_len, truncate};
    pub use noctis_rules::{
        Actual, Bound, LimitMode, Limits, Locale, MessageKey, ParseLocaleError, RuleId, Translate,
        Violation,
    };
    pub use noctis_rules::{attachment, command, component, message, modal};
}
