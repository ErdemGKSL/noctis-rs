//! Noctis: a Discord bot framework on twilight, with built-in sharding, a
//! Components V2 UI kit and image rendering.
//!
//! Bot code depends on this crate only; the `noctis-*` crates are internal.
//!
//! # Modules
//!
//! - The crate root: [`App`], [`AppBuilder`], [`Token`], [`State`],
//!   [`Runtime`], [`Plugin`], [`Resource`], [`Module`], [`Register`], the
//!   kind derives ([`SlashCommand`](macro@SlashCommand),
//!   [`UserCommand`](macro@UserCommand),
//!   [`MessageCommand`](macro@MessageCommand),
//!   [`EventHandler`](macro@EventHandler), [`Button`](macro@Button), the
//!   select derives and [`Modal`](macro@Modal)), the [`handler`](macro@handler),
//!   [`autocomplete`](macro@autocomplete), [`on_stale`](macro@on_stale),
//!   [`on_expired`](macro@on_expired) and
//!   [`on_rate_limited`](macro@on_rate_limited) method attributes,
//!   [`CommandHandler`], [`CommandCtx`], [`CommandPath`],
//!   [`UnknownCommandPath`], [`CommandMetadata`]
//!   ([`Permissions`], [`InteractionContext`], [`IntegrationType`]),
//!   [`ContextMenuHandler`], [`MenuKind`], [`MenuTargetError`],
//!   [`ComponentCtx`], [`ModalSubmitCtx`], [`ComponentHandler`],
//!   [`ModalHandler`], [`ComponentKind`], [`Expired`], [`ExpiredCtx`],
//!   the declared rate limits ([`RateScope`], [`RateLimit`],
//!   [`RateLimitHit`], [`RateLimitedCtx`], [`RateLimits`],
//!   [`RateLimitError`]), [`HandlerError`], [`HandlerResult`], [`HttpConfig`] (the Discord HTTP
//!   edge's tuning) and the errors ([`Error`], [`BuildError`],
//!   [`BuildViolation`], [`StartError`], [`RunError`],
//!   [`LocaleOverlayError`] and [`OverlayViolation`] (runtime locale
//!   overlays rejected at startup), [`EnvError`],
//!   [`TokenError`], [`ResourceConflict`], [`HttpConfigError`],
//!   [`AckDeadlineMissed`]).
//! - [`events`]: the gateway event payloads `#[derive(EventHandler)]`
//!   handlers take, and [`events::EventCtx`].
//! - [`gateway`]: the bot's presence (identify and run time), the
//!   `large_threshold`, and Request Guild Members sent to the owning shard.
//! - [`twilight`]: the twilight crates Noctis builds on, [`twilight::http`]
//!   and [`twilight::model`].
//! - [`rules`]: Discord limits, rule ids, text length and locales.
//! - [`prelude`]: `use noctis::prelude::*;` brings every kind derive,
//!   [`handler`](macro@handler), [`autocomplete`](macro@autocomplete), the
//!   [`Payload`](macro@Payload), [`PayloadEnum`](macro@PayloadEnum) and
//!   [`Choices`](macro@Choices) derives, the `Route` and `Payload` traits,
//!   [`Rgb`](ui::Rgb), the contexts and responders, and the macros
//!   [`components!`], [`label!`], [`placeholder!`], [`content!`] and
//!   [`rgb!`] into scope.
//! - [`ui`]: the Components V2 message model: one type per component,
//!   placement checked by the compiler, components built from kind-derived
//!   structs, and attachments.
//! - [`codec`]: Payload field encodings ([`codec::Field`],
//!   [`codec::BoundedStr`]), the checks custom fields are tested with, and the
//!   traits the derives implement.
//! - [`ports`]: the Adapter port traits, the shared op vocabulary, typed
//!   values and the render contract.
//! - [`bytes`]: [`Bytes`](bytes::Bytes), the value type every byte-store port
//!   takes and returns.
//! - `image` (feature `image`): image Templates (`#[derive(Template)]`,
//!   `TemplateView`, `view!`, `tw!`), the node tree and the Theme
//!   (`theme!`).
//!
//! # One struct per interaction
//!
//! Every interaction is one struct (AD-6, AD-26). A kind derive names the
//! kind; its one attribute carries the id and the routing arguments (and,
//! for components and modals, the mandatory `defaults(..)`); the handler is
//! the struct's `#[handler]` method, taking `self`; and
//! [`Module::add::<T>()`](Module::add) registers it. A struct without a
//! `#[handler]` method does not compile (`[CID-04]` for components and
//! modals).
//!
//! | Derive | Attribute | `#[handler]` parameters |
//! | --- | --- | --- |
//! | [`SlashCommand`](macro@SlashCommand) | `#[command("path", description = "…", ..)]` | `(self, ctx: CommandCtx[, path: <Struct>Path][, r: CommandResponder<..>])` |
//! | [`UserCommand`](macro@UserCommand) / [`MessageCommand`](macro@MessageCommand) | `#[user_command("Name", ..)]` / `#[message_command("Name", ..)]` | `(self, ctx: CommandCtx, target: ResolvedUser \| ResolvedMessage[, r])` |
//! | [`EventHandler`](macro@EventHandler) | optional `#[event(ordered, droppable)]` | `(self, ctx: EventCtx, event: <payload>)` |
//! | [`Button`](macro@Button), the select derives | `#[button("id", <codec args>, response = .., auto_defer = .., defaults(..))]`, … | `(self, ctx: ComponentCtx[, r: ComponentResponder<..>])` |
//! | [`Modal`](macro@Modal) | `#[modal("id", <codec args>, defaults(title = "…"))]` | `(self, ctx: ModalSubmitCtx[, r: ModalResponder])` |
//!
//! A component struct is a Route: its fields are the Payload of its
//! `custom_id`, and the derive proves at compile time that every id fits
//! Discord's 100 characters (CID-01). `X { .. }.build()` builds the
//! component from the declared `defaults(..)`; the setters override one
//! field each.
//!
//! ```
//! use noctis::codec::RouteShape;
//! use noctis::prelude::*;
//! use twilight_model::id::Id;
//!
//! pub struct ItemMarker;
//!
//! /// `shop.buy: F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)`
//! #[derive(Button, Debug, PartialEq)]
//! #[button("shop.buy", defaults(style = Primary, label = "Buy"))]
//! struct BuyButton {
//!     item: Id<ItemMarker>,
//!     qty: u8,
//! }
//!
//! impl BuyButton {
//!     #[handler]
//!     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
//!         # unimplemented!()
//!         // A click runs this on the decoded value; see "Answering
//!         // components" for the responses.
//!     }
//! }
//!
//! assert_eq!(BuyButton::ID.as_str(), "shop.buy");
//! assert_eq!(BuyButton::SHAPE, RouteShape::INLINE);
//! assert_eq!(<BuyButton as Payload>::MAX_ENCODED_LEN, Some(14));
//! let button = BuyButton { item: Id::new(7), qty: 1 }.build().label("Buy one");
//! let _ = button;
//! let _shop = Module::new("shop").add::<BuyButton>();
//! ```
//!
//! The kind attribute takes the id (1 to 24 characters of `[a-z0-9._-]`,
//! not starting with `_`) and, in any order, the codec arguments `by_ref`,
//! `overflow = spill`, `schema = n` (1..=61) and `signed` (which needs the
//! payload key, `NOCTIS_PAYLOAD_KEY`), and for components the runtime
//! arguments `response = update | ephemeral | public` and `auto_defer =
//! true | false`; every select kind carries the select nonce. Each component struct also gets a
//! `__noctis_route_width_<Name>` test that prints its width breakdown
//! (`cargo test -- --nocapture noctis_route_width`).
//!
//! # App
//!
//! [`App::from_env`] (or [`App::builder`]) returns a mutable [`AppBuilder`]
//! whose setters take and return `&mut AppBuilder`; [`AppBuilder::build`]
//! installs the [`Plugin`]s, checks everything and returns an [`App`] or one
//! [`BuildError`] listing every violation. Unset ports default to bounded
//! in-memory Adapters. Each App has an [`AppId`] ([`AppBuilder::app_id`] or
//! `NOCTIS_APP_ID`; by default the bot user id read from the token, in
//! base36): it is the `app` label of every span, metric and report, and
//! every byte-store key is prefixed with `<app_id>:<port>:`, so Apps sharing
//! one Adapter never see each other's data unless it is passed as
//! [`ports::Shared::new`]. Handlers store bot data through `ctx.kv()`, a
//! typed [`Kv`] over the App's `CacheStore`. [`App::run`] returns a typed
//! [`StartError`] outside a multi-thread tokio runtime or without a token,
//! and installs rustls's `ring` crypto provider when none is set.
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
//! # Publishing commands
//!
//! Commands reach Discord only through Publish, which writes nothing when
//! the remote commands already equal the Registry's (REG-05). From the
//! command line: `cargo run --release -- --publish-guild <id> --dry-run`
//! prints the JSON, the diff and the create count; without `--dry-run` it
//! writes; `--publish` publishes globally and `--publish-clear` deletes.
//! Each mode returns without connecting a shard. In code, see
//! [`publish`] and [`cli`]; [`AppBuilder::publisher`] publishes once at
//! startup (never on READY).
//!
//! ```no_run
//! use noctis::publish::PublishScope;
//! use noctis::twilight::model::id::Id;
//!
//! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let app = noctis::App::from_env()?.build()?;
//! let report = app.publish(PublishScope::Guild(Id::new(42))).dry_run(true).await?;
//! println!("{report}");
//! # Ok(()) }
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
//! connection errors. For an endpoint twilight does not model,
//! [`http::RawHttp`] adds `.raw(method, path)` to that same client
//! (`ctx.http().raw(Method::Get, "applications/@me").await?`): the request
//! passes the edge, the client's rate limiter and the configured proxy.
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
//! Modules; [`AppBuilder::module`] mounts a tree.
//! `#[derive(EventHandler)]` on a unit struct, with a `#[handler]` method
//! taking `(self, ctx: EventCtx, event: <payload>)`, is an event handler
//! [`Module::add`] registers; the payload types are in [`events`].
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
//! #[derive(EventHandler)]
//! struct AwardXp;
//!
//! impl AwardXp {
//!     #[handler]
//!     async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
//!         if let Some(xp) = ctx.state::<Xp>() {
//!             xp.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//!         }
//!         let _ = msg.author.id;
//!         Ok(())
//!     }
//! }
//!
//! /// Runs before the concurrent handlers, in per-guild order.
//! #[derive(EventHandler)]
//! #[event(ordered)]
//! struct AuditJoin;
//!
//! impl AuditJoin {
//!     #[handler]
//!     async fn handle(self, ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> {
//!         let _ = (ctx.module(), ev.user.id);
//!         Ok(())
//!     }
//! }
//!
//! fn module() -> Module {
//!     Module::new("profile")
//!         .add::<AwardXp>()
//!         .module(Module::new("profile/audit").add::<AuditJoin>())
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
//! checked once with the same rules as `#[derive(SlashCommand)]` (tree shape, sizes,
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
//! #[derive(SlashCommand)]
//! #[command("config set", description = "Change a setting")]
//! struct ConfigSet;
//!
//! impl ConfigSet {
//!     #[handler]
//!     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
//!         Ok(Components::new().push(TextDisplay::new("Saved.")))
//!     }
//! }
//!
//! #[derive(SlashCommand)]
//! #[command("config get prefix", description = "Show the prefix")]
//! struct ConfigGetPrefix;
//!
//! impl ConfigGetPrefix {
//!     #[handler]
//!     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Components> {
//!         Ok(Components::new().push(TextDisplay::new("Prefix: `!`")))
//!     }
//! }
//!
//! # #[tokio::main(flavor = "current_thread")] async fn main() {
//! let settings = Module::new("settings").add::<ConfigSet>().base(
//!     "config",
//!     BaseMeta::new()
//!         .description("Configure the bot")
//!         .default_member_permissions(Permissions::MANAGE_GUILD),
//! );
//! let prefix = Module::new("prefix")
//!     .add::<ConfigGetPrefix>()
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
//! `reply` / `reply_as` / `defer_reply` / `defer_reply_as` consume it into
//! an acknowledged responder offering only `edit_reply`, `fetch_reply`,
//! `follow_up` and `delete_reply` (INT-03, INT-04, INT-08, INT-10). A
//! failure or a panic shows the user a short localized ephemeral notice,
//! then goes to the `Command` Error Sink (see "Errors shown to users").
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
//! #[derive(SlashCommand)]
//! #[command("ping", description = "Check that the bot answers")]
//! struct Ping;
//!
//! impl Ping {
//!     #[handler]
//!     async fn handle(self, ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
//!         let acked = r.defer_reply().await?;
//!         let text = format!("Pong ({})", ctx.response_locale());
//!         acked.edit_reply(Components::new().push(TextDisplay::new(text))).await?;
//!         Ok(acked.done())
//!     }
//! }
//!
//! #[derive(EventHandler)]
//! struct Prefix;
//!
//! impl Prefix {
//!     #[handler]
//!     async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
//!         if msg.content == "!ping" {
//!             let invocation = CommandInvocation::new(CommandInput::new("ping"), msg.channel_id, msg.id, msg.author.id)
//!                 .guild_id(msg.guild_id);
//!             ctx.dispatcher().run_command(invocation).await?;
//!         }
//!         Ok(())
//!     }
//! }
//!
//! let _module = Module::new("ping").add::<Ping>().add::<Prefix>();
//! ```
//!
//! # Answering components
//!
//! A click or a modal submit runs the handler of its Route with the
//! decoded struct (an id that reaches no handler, a forged or undecodable
//! one, shows the user a generic ephemeral notice, CID-06). Its declared
//! `response` (`update`, the default, `ephemeral` or `public`) shapes the
//! auto-defer (type 6 for `update`, else type 5) and a returned message:
//! under `update` the clicked message is replaced in place. A handler that
//! needs more takes a [`ComponentResponder`] (or [`ModalResponder`]):
//! `update` / `defer_update` act on the clicked message, `reply` /
//! `defer_reply` send a new one, and `show_modal` needs `auto_defer =
//! false` ([`ComponentResponder<Manual>`](ComponentResponder)). A
//! slash-opened modal submit has no message to update
//! ([`ResponderError::UpdateUnavailable`]). `signed` Routes need the
//! payload key ([`AppBuilder::payload_key`], `NOCTIS_PAYLOAD_KEY`).
//!
//! ```
//! use noctis::prelude::*;
//! use noctis::ui::{Components, TextDisplay};
//!
//! #[derive(Button)]
//! #[button("shop.buy", defaults(style = Primary, label = "Buy"))]
//! struct Buy {
//!     item: u64,
//! }
//!
//! impl Buy {
//!     #[handler]
//!     async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
//!         let panel = Components::new().push(TextDisplay::new(format!("Bought item {}.", self.item)));
//!         Ok(r.update(panel).await?.done())
//!     }
//! }
//!
//! let _shop = Module::new("shop").add::<Buy>();
//! ```
//!
//! # Errors shown to users
//!
//! When a Command handler returns `Err`, panics, or its options fail to
//! decode or validate, noctis sends the user one ephemeral notice (FR-22):
//! the key of [`HandlerError::with_message`] or [`HandlerError::user`],
//! `noctis.validation.failed` for options, else `noctis.error.generic`.
//! While unanswered it is the reply; after a defer (also the auto-defer) the
//! deferred original is deleted and the notice is an ephemeral follow-up
//! (AD-23); after a reply it is a follow-up; after a modal nothing is sent.
//! Over the [`Dispatcher`] it is a public message. Then the error is logged
//! with its full chain and reported to the `Command` Error Sink.
//! [`HandlerError::user`] and a failed option validator are user
//! conditions: only the notice and a `debug` log, no Error Sink. Message
//! keys resolve from the catalogs passed to [`AppBuilder::locales`] (with the
//! `i18n` feature, the `catalog()` that `noctis::i18n!` generates), and the built-in
//! `noctis.*` texts are English unless a locale file overrides them; a key no
//! catalog has shows the generic text with a `noctis::i18n` warning.
//!
//! ```
//! use noctis::prelude::*;
//! use noctis::rules::MessageKey;
//! use noctis::ui::{Components, TextDisplay};
//!
//! // Bot keys: with no catalog that defines them (`AppBuilder::locales`)
//! // these show the generic text and log a `noctis::i18n` warning.
//! const GUILD_ONLY: MessageKey = MessageKey::new("common.guild_only");
//! const SAVE_FAILED: MessageKey = MessageKey::new("shop.save_failed");
//!
//! #[derive(SlashCommand)]
//! #[command("buy", description = "Buy an item")]
//! struct Buy;
//!
//! impl Buy {
//!     #[handler]
//!     async fn handle(self, ctx: CommandCtx) -> HandlerResult<Components> {
//!         // A known condition: the notice only, logged at debug.
//!         let guild = ctx.guild_id().ok_or_else(|| HandlerError::user(GUILD_ONLY))?;
//!         // A real error with a friendly message: notice, log and Error Sink.
//!         std::fs::metadata("/nonexistent")
//!             .map_err(|e| HandlerError::new(e).with_message(SAVE_FAILED))?;
//!         Ok(Components::new().push(TextDisplay::new(format!("Bought in {guild}"))))
//!     }
//! }
//!
//! let _module = Module::new("shop").add::<Buy>();
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
//! I/O. Pass an `Arc` clone to share one sink between origins. With a
//! custom `Command` or `Autocomplete` sink, noctis still logs the full error
//! once through [`LogSink`] (inside the `noctis.interaction` span) before
//! calling it, so the error chain is always in the logs (FR-22).
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
//! - `image` (off by default): image Templates (`#[derive(Template)]`,
//!   `TemplateView`), `view!`/`tw!` with compile-time Tailwind class checks,
//!   the Theme (`theme!`), and the takumi Renderer with its bounded render
//!   pool (`.theme(..)`/`.image(..)`, `ctx.render`). It needs a C compiler:
//!   the WebP encoder (libwebp) is built from source.
//! - `image-gif` (implies `image`): GIF and APNG output for animated
//!   Templates. Without it, `animation(.., format = Gif)` or `Apng` is a
//!   compile error naming the feature; the default animated format,
//!   animated WebP, needs only `image`.
//! - `image-emoji` (implies `image`): colour Unicode emoji in image
//!   Templates. It bundles the Twemoji Mozilla `COLRv0` font as a last-resort
//!   emoji font, after every registered font, so author fonts and plain
//!   digits keep their glyphs. Without it, an App that registers a Template
//!   logs one `[IMG-04]` warning at build, and Unicode emoji draw as boxes.
//!   Emoji graphics: Twemoji, Copyright Twitter, Inc and other contributors,
//!   CC-BY-4.0 (<https://creativecommons.org/licenses/by/4.0/>); font build:
//!   Mozilla twemoji-colr (twemoji-colr-font 15.0.3), OFL-1.1. A bot that
//!   posts the rendered images should credit Twemoji. Discord custom emoji
//!   (`<:name:id>`) in Template data are drawn as fetched images with or
//!   without this feature.
//! - `default-font` (implies `image`): bundles Inter 4.1 (OFL-1.1) as the
//!   last-resort font family `"Inter"` of image Templates, after every font
//!   registered with `ImageConfig::font`. Without it, an App that registers
//!   a Template must register a font, or `App::build` fails (`[IMG-01]`).
//! - `i18n`: typed locale files: the `i18n!` macro and the `i18n` module
//!   (`Catalog`, `format`). Register a catalog App-wide with
//!   `AppBuilder::locales` or ship it with a Module through
//!   `Module::locales`; every catalog merges into one App-wide set.
//!   Its locales drive the resolution of every run's
//!   `ctx.user_locale()`/`ctx.guild_locale()` ([`Locales`]; override it
//!   with a [`LocaleResolver`] or a before-Middleware).
//! - `prefix`: opt-in prefix commands: the [`prefix`] module's
//!   `PrefixPlugin` runs opted-in slash Commands from messages such as
//!   `!warn <@5> 3`; it needs the privileged `MESSAGE_CONTENT` intent.
//! - `testkit`: [`testkit`], the bot's App on local Discord fakes with
//!   simulated interactions, message snapshots and the memory check;
//!   enable it in `[dev-dependencies]` only.
#![deny(missing_docs)]

pub use noctis_core::{
    Access, AckDeadlineMissed, Acked, AfterCtx, AfterMiddleware, App, AppBuilder, AppId,
    AppIdError, AutoDefer, AutocompleteChoice, AutocompleteCtx, AutocompleteError,
    AutocompleteValue, BaseMeta, BeforeCtx, BeforeMiddleware, BootFlagError, BootFlags, BuildError,
    BuildViolation, CacheDisabled, CacheError, CallbackResource, CanShowModal, Command, CommandCtx,
    CommandHandler, CommandInput, CommandInvocation, CommandMetadata, CommandOption, CommandPath,
    CommandResponder, Component, ComponentCtx, ComponentHandler, ComponentKind, ComponentResponder,
    ContextMenuHandler, DeferMode, DiscordCacheConfig, DiscordCacheReader, DiscordCacheResources,
    DispatchError, Dispatcher, EnvError, Ephemerality, Error, ErrorOrigin, ErrorReport, ErrorSink,
    Expired, ExpiredCtx, Ext, ExtensionMissing, Extensions, Flow, Focused, FollowupHandle,
    GatewayUrlError, HandlerError, HandlerResult, HttpConfig, HttpConfigError, HttpProxyError,
    IntegrationType, InteractionContext, IntoEdit, IntoResponse, InvalidOption, Invocation, Kv,
    LocaleOverlayError, LocaleOwner, LocaleRequest, LocaleResolver, Locales, LogSink, Manual,
    MenuKind, MenuTargetError, Middleware, ModalHandler, ModalResponder, ModalSubmit,
    ModalSubmitCtx, Module, ModuleIdError, NoticeText, OptionError, OptionValue, Options,
    OutgoingMessage, OverlayViolation, Partial, PartialOptions, Pending, Permissions, Plugin,
    RateLimit, RateLimitError, RateLimitHit, RateLimitedCtx, RateLimits, RateScope, RefOps,
    Register, ReplyText, ReportKind, Resource, ResourceConflict, Responded, Responder,
    ResponderError, ResponderKind, ResponderState, RunError, RunOutcome, Runtime, SelectedValues,
    StartError, State, SuggestionLabel, SuggestionValue, Suggestions, Token, TokenError,
    UnknownCommandPath, Visibility,
};

/// Derives a slash Command from a struct: the struct's named fields are its
/// options (a unit struct has none), `#[command("path or pattern", ..)]`
/// declares it, and its `#[handler]` method answers it. The Command Path or
/// Pattern and every option literal are checked at compile time;
/// [`Module::add::<T>()`](Module::add) registers it.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, TextDisplay};
///
/// /// Rustdoc only: never the Command's description.
/// #[derive(SlashCommand)]
/// #[command("config (set|get) (prefix|welcome)", description = "Configure the bot")]
/// struct Config;
///
/// impl Config {
///     #[handler]
///     async fn handle(self, ctx: CommandCtx, path: ConfigPath) -> HandlerResult<Components> {
///         let text = match path {
///             ConfigPath::SetPrefix | ConfigPath::SetWelcome => "Saved.",
///             ConfigPath::GetPrefix | ConfigPath::GetWelcome => "Current value: `!`",
///         };
///         assert_eq!(ctx.command_path(), path.as_str());
///         Ok(Components::new().push(TextDisplay::new(text)))
///     }
/// }
///
/// #[derive(SlashCommand)]
/// #[command("ping", description = "Check that the bot answers", response = ephemeral)]
/// struct Ping;
///
/// impl Ping {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
///         let acked = r.reply(Components::new().push(TextDisplay::new("Pong!"))).await?;
///         acked.follow_up(Components::new().push(TextDisplay::new("Still here.")), Visibility::Public).await?;
///         Ok(acked.done())
///     }
/// }
///
/// assert_eq!(ConfigPath::ALL[0].as_str(), "config set prefix");
/// assert_eq!(Config::descriptor().paths(), ["config set prefix", "config set welcome", "config get prefix", "config get welcome"]);
/// assert_eq!(Ping::descriptor().ephemerality(), Ephemerality::Ephemeral);
/// let _module = Module::new("settings").add::<Config>().add::<Ping>();
/// ```
///
/// The first argument of `#[command(..)]` is a string literal: a Command
/// Path of 1 to 3 words separated by single ASCII spaces (1 word a command,
/// 2 a subcommand, 3 a subcommand in a group; see
/// [`rules::command::PathKind`]), or a Command Pattern with groups `(a|b)`
/// and optional groups `(a|b)?`, expanded left to right by
/// [`rules::command::expand`]. Then, in any order, each at most once:
///
/// - `description = "…"`: the description, 1 to 100 characters, required.
///   Doc comments stay rustdoc and never become it.
/// - `max_expansions = n`: the pattern's expansion cap (default 25).
/// - `response = public | ephemeral` (default `public`): who sees the
///   Command's reply; [`CommandHandler::ephemerality`] reads it. The
///   auto-defer, `reply`, `defer_reply` and a returned message use it;
///   `reply_as` and `defer_reply_as` take one explicitly, `follow_up` takes
///   its own [`Visibility`], and no edit changes it once the reply is sent
///   (INT-10). `response = update` is an `[INT-04]` compile error: only
///   components update their message.
/// - `auto_defer = true | false` (default `true`): with `false` no
///   auto-defer is armed, so the handler answers within 3 s itself (a
///   missed deadline is logged), and `show_modal` becomes available. The
///   `#[handler]` method must then take `r: CommandResponder<`[`Manual`]`>`;
///   [`CommandHandler::auto_defer`] reads it. A mismatch is a compile error
///   citing `[INT-05]`: `auto_defer = false` without that responder, that
///   responder without `auto_defer = false`, or `auto_defer = false`
///   without any responder parameter.
/// - the metadata arguments (see "Metadata" below).
///
/// Every check of [`rules::command::validate_pattern_spacing`],
/// [`rules::command::expand`] and [`rules::command::validate_tree`] runs at
/// expansion; each violation is a compile error naming its rule id, all of
/// them at once.
///
/// # The `#[handler]` method
///
/// `(self, ctx: CommandCtx[, path: <Struct>Path][, r: CommandResponder<..>])`.
/// A Path (no group) takes no `path`. A Pattern with a group generates `enum
/// <Struct>Path` (with the struct's visibility); the handler takes it after
/// `ctx` (a missing or extra `path` is a compile error naming it). Its
/// variants concatenate the chosen alternatives in `UpperCamelCase` (words
/// split on `-` and `_`; `Bare` when every group is absent; `_` before a
/// leading digit; `Self_` for `Self`); two equal names are a compile error.
/// The enum has `ALL` (expansion order), `as_str()` and implements
/// [`CommandPath`]. `self` holds the decoded options: they are decoded
/// before the handler runs (a missing, mistyped or out-of-range value is an
/// [`OptionError`], never a panic), then each field's validator runs (a
/// failure is an [`InvalidOption`] carrying its
/// [`MessageKey`](crate::rules::MessageKey)), and only then the handler.
/// Unknown incoming options are ignored. [`CommandHandler::options`] lists
/// the declarations.
///
/// The method returns `HandlerResult<R>` with `R:` [`IntoResponse`]: a
/// [`V2Message`](crate::ui::V2Message), [`Components`](crate::ui::Components)
/// or [`LegacyMessage`](crate::ui::LegacyMessage) is sent as the reply
/// (callback type 4) or, after `defer_reply`/`reply`, as an edit of the reply
/// (INT-12); the [`Responded`] from `done()` means the handler answered
/// itself. `HandlerResult<()>` does not compile: a Command must answer.
/// Every V2 message gets `IS_COMPONENTS_V2` and passes the Limits validator
/// ([`AppBuilder::limits`]) before any request. An `Err` or a panic sends
/// the user a localized ephemeral notice, then goes to the `Command`
/// [`ErrorSink`] (see the crate docs, "Errors shown to users").
///
/// A handler that has not answered [`AppBuilder::ack_deadline`] after the
/// interaction arrived (default 2000 ms, 0-2500 ms) is acknowledged by the
/// auto-defer: one type-5 callback carrying only the declared `EPHEMERAL`
/// flag (INT-01), unless it is declared `auto_defer = false`. Its later
/// `reply` or returned message then edits the reply, so at most one
/// callback is ever sent (INT-03). After 15 minutes the interaction token
/// is expired: edits and deletes of public messages use the bot token,
/// everything else is [`ResponderError::TokenExpired`] (INT-02). A
/// user-install-only interaction sends at most 5 follow-ups
/// ([`ResponderError::FollowupCap`], INT-11). The [`Dispatcher`] has no
/// auto-defer.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::Modal;
///
/// # fn feedback_form() -> Modal { unimplemented!() }
/// #[derive(SlashCommand)]
/// #[command("feedback", description = "Send feedback", auto_defer = false)]
/// struct Feedback;
///
/// impl Feedback {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> {
///         Ok(r.show_modal(feedback_form()).await?.done())
///     }
/// }
///
/// assert!(!Feedback::descriptor().auto_defer());
/// ```
///
/// ```compile_fail,E0277
/// use noctis::prelude::*;
///
/// #[derive(SlashCommand)]
/// #[command("ping", description = "d")]
/// struct Ping;
///
/// impl Ping {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<()> { // not a Command response
///         Ok(())
///     }
/// }
/// ```
///
/// # Options
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
/// #[derive(SlashCommand)]
/// #[command("warn", description = "Warn a member")]
/// struct Warn {
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
/// impl Warn {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
///         let _ = (self.member.user.id, self.severity, self.days, self.reason);
///         Ok(LegacyMessage::new("Warned."))
///     }
/// }
///
/// assert_eq!(Warn::descriptor().options().len(), 4);
/// assert_eq!(Warn::OPTIONS[2].name, "days");
/// assert!(!Warn::OPTIONS[2].required);
/// ```
///
/// Field attribute `#[option(..)]`, arguments in any order, each at most
/// once (fields are options in declaration order, never reordered):
///
/// | Argument | Takes | Option types |
/// | --- | --- | --- |
/// | `description = "…"` | 1 to 100 characters, required (CMD-04) | all |
/// | `rename = "…"` | the option name (default: the field name, `r#` stripped; CMD-01..03) | all |
/// | `min_value = n`, `max_value = n` | unsuffixed integer or float literal, optionally negated (OPT-03..05) | INTEGER, NUMBER |
/// | `min_length = n`, `max_length = n` | unsuffixed integer, 0..=6000 and 1..=6000 (OPT-06) | STRING |
/// | `channel_types = [GuildText, …]` | twilight `ChannelType` variant names (OPT-07) | CHANNEL |
/// | `file_types = ["image", ".pdf"]` | `image`, `video`, `audio` or `.ext`, at most 10 (OPT-13) | ATTACHMENT |
/// | `autocomplete` | a bare flag: the struct's [`autocomplete`](macro@autocomplete) method for this field answers it (OPT-10, OPT-11) | STRING, INTEGER, NUMBER |
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
/// The derive also generates `<Struct>Partial` ([`Partial<Struct>`](Partial)):
/// the same fields, each an `Option`, which the struct's autocomplete
/// methods get (OPT-12).
///
/// A missing description does not compile:
///
/// ```compile_fail
/// use noctis::prelude::*;
///
/// #[derive(SlashCommand)]
/// #[command("days", description = "d")]
/// struct Days {
///     days: i64, // [CMD-04]
/// }
/// ```
///
/// # Metadata
///
/// These arguments set the Command's [`CommandMetadata`], read with
/// [`CommandHandler::metadata`]. An argument left out stays unset (`None`,
/// or no guilds) and is omitted when the Command is published, so Discord
/// applies its default. They belong to the **base** command of every path
/// the struct declares (META-02): a per-path form such as
/// `permissions(set = …)` is a `[META-02]` compile error, and Commands
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
/// #[derive(SlashCommand)]
/// #[command("ban", description = "Ban a member", permissions = BAN_MEMBERS | MANAGE_GUILD, contexts = [Guild], nsfw = false)]
/// struct Ban;
///
/// impl Ban {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
///         Ok(LegacyMessage::new("Banned."))
///     }
/// }
///
/// let handler = Ban::descriptor();
/// let meta = handler.metadata();
/// assert_eq!(meta.default_member_permissions, Some(Permissions::BAN_MEMBERS | Permissions::MANAGE_GUILD));
/// assert_eq!(meta.contexts, Some(&[InteractionContext::Guild][..]));
/// assert_eq!(meta.integration_types, None);
/// assert_eq!(meta.nsfw, Some(false));
/// ```
pub use noctis_macros::SlashCommand;

/// Marks the suggestion method of the option `field` of a
/// [`SlashCommand`](macro@SlashCommand) struct, which carries
/// `#[option(.., autocomplete)]` (FR-10, maintainer decisions §8):
/// `#[autocomplete(field)] async fn <any>(ctx: AutocompleteCtx, typed: &str,
/// filled: Partial<Self>) -> HandlerResult<Suggestions<T>>`, with `T` the
/// field's type without `Option` (`i64`, `f64` or `String`).
///
/// The method has no `self` (the options are incomplete). `typed` is the
/// focused option's raw text, as typed so far (a JSON number arrives as its
/// text, `""` when absent): parse it yourself. `filled` holds every other
/// option as an `Option` (OPT-12: untrusted, partially filled, validators do
/// not run). [`Module::add::<T>()`](Module::add) registers the methods with
/// the Command.
///
/// Noctis sends the [`Suggestions`] (callback type 8): more than 25 are cut
/// to the first 25 with a warning (INT-07); a label over 100 characters is
/// cut with `…` and a warning; an empty label or a value Discord would
/// reject (a `String` over 100 characters, an `i64` outside ±2^53, a
/// non-finite `f64`) is dropped with a warning. A
/// [`MessageKey`](crate::rules::MessageKey) label is
/// rendered in the user's locale. When the method errors, panics or has not
/// returned 2.5 s after the interaction arrived
/// ([`AutocompleteError::DeadlineMissed`]), Discord gets an empty list and
/// the error goes to the Autocomplete Error Sink
/// ([`ErrorOrigin::Autocomplete`]). There is no defer: `auto_defer` or
/// `response` here is an `[INT-06]` compile error.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::LegacyMessage;
///
/// #[derive(SlashCommand)]
/// #[command("config set prefix", description = "Set the prefix")]
/// struct SetPrefix {
///     #[option(description = "The new prefix", autocomplete)]
///     value: String,
///     #[option(description = "How many suggestions")]
///     limit: Option<i64>,
/// }
///
/// impl SetPrefix {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
///         Ok(LegacyMessage::new(format!("Prefix set to {}", self.value)))
///     }
///
///     #[autocomplete(value)]
///     async fn value(_ctx: AutocompleteCtx, typed: &str, filled: Partial<Self>) -> HandlerResult<Suggestions<String>> {
///         let limit = usize::try_from(filled.limit.unwrap_or(5)).unwrap_or(0);
///         Ok(["!", "?", "."]
///             .into_iter()
///             .filter(|p| p.starts_with(typed))
///             .take(limit)
///             .map(|p| (p.to_owned(), p))
///             .collect())
///     }
/// }
///
/// let _module = Module::new("config").add::<SetPrefix>();
/// ```
///
/// A flagged option without its method does not compile (`[OPT-11]`):
///
/// ```compile_fail
/// use noctis::prelude::*;
/// use noctis::ui::LegacyMessage;
///
/// #[derive(SlashCommand)]
/// #[command("find", description = "d")]
/// struct Find {
///     #[option(description = "d", autocomplete)]
///     query: String,
/// }
///
/// impl Find {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx) -> HandlerResult<LegacyMessage> {
///         Ok(LegacyMessage::new("found"))
///     }
/// }
/// ```
///
/// A `Partial` field is always an `Option`:
///
/// ```compile_fail,E0308
/// use noctis::prelude::*;
///
/// #[derive(SlashCommand)]
/// #[command("o", description = "d")]
/// struct O {
///     #[option(description = "d")]
///     limit: i64,
/// }
///
/// fn read(partial: Partial<O>) {
///     let limit: i64 = partial.limit; // [OPT-12]
///     let _ = limit;
/// }
/// ```
pub use noctis_macros::autocomplete;

/// Marks the one `async` method of a kind-derived struct that handles its
/// interaction (AD-6, AD-26). It takes no arguments; the routing arguments
/// live on the kind attribute. The method takes `self` (or `mut self`) by
/// value and is not generic; any name works. Beside it the macro emits the
/// public `fn descriptor()`, the descriptor
/// [`Module::add::<T>()`](Module::add) registers. Its parameters after
/// `self` depend on the kind (see the crate docs, "One struct per
/// interaction"); a wrong parameter is a type error naming the expected
/// ones. A kind-derived struct without a `#[handler]` method does not
/// compile (`[CID-04]` for components and modals), and two `#[handler]`
/// methods are a duplicate-definition error.
///
/// ```compile_fail
/// use noctis::prelude::*;
///
/// #[derive(Button)]
/// #[button("shop.buy", defaults(style = Primary, label = "Buy"))]
/// struct Buy { item: u64 } // [CID-04]: no #[handler] method
///
/// let _ = Buy { item: 1 }.build();
/// ```
pub use noctis_macros::handler;

/// Marks the method of a component or modal struct that answers a
/// **stale** `custom_id`: one rendered before a deploy that changed the
/// Route's `schema`, its body kind (`by_ref`, `overflow = spill`) or a flag
/// (`signed`, a select's nonce) (FR-60, AD-23 stage 2). It takes no
/// arguments and no `self` (the Payload did not decode):
/// `async fn <any>(ctx: ComponentCtx[, r: ComponentResponder<..>]) ->
/// HandlerResult<R>`, or `ModalSubmitCtx` and `ModalResponder<..>` on a
/// modal. It inherits the Route's `response` and `auto_defer` (an
/// `auto_defer = false` Route's hook takes the `Manual` responder, INT-05)
/// and answers like the `#[handler]`: a returned View updates the clicked
/// message under the default `response = update`.
///
/// A stale id runs `#[on_stale]`, else `#[on_expired]` with
/// [`Expired::Stale`], else [`AppBuilder::expired_handler`], else the
/// default `noctis.expired` notice. A struct has at most one `#[on_stale]`;
/// `#[handler]`'s `descriptor()` attaches it, so `Module::add::<T>()`
/// registers it with the handler.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, TextDisplay, V2Message};
///
/// fn render_shop(page: u16) -> V2Message {
///     V2Message::new(Components::new().push(TextDisplay::new(format!("page {page}"))))
/// }
///
/// #[derive(Button)]
/// #[button("shop.next", schema = 2, defaults(style = Secondary, label = "Next"))]
/// struct Next { page: u16 }
///
/// impl Next {
///     #[handler]
///     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<V2Message> {
///         Ok(render_shop(self.page + 1))
///     }
///
///     /// A pre-`schema = 2` button: re-render from the first page.
///     #[on_stale]
///     async fn restart(_ctx: ComponentCtx) -> HandlerResult<V2Message> {
///         Ok(render_shop(0))
///     }
/// }
///
/// assert!(format!("{:?}", Next::descriptor()).contains("on_stale: true"));
/// ```
pub use noctis_macros::on_stale;

/// Marks the method of a component or modal struct that answers an
/// **expired** `custom_id`: a Ref whose record is missing or expired
/// ([`Expired::RefMissing`]) or, without an [`on_stale`](macro@on_stale)
/// method, a stale id ([`Expired::Stale`]). It takes no arguments and no
/// `self`: `async fn <any>(ctx: ComponentCtx, expired: Expired[, r:
/// ComponentResponder<..>]) -> HandlerResult<R>` (`ModalSubmitCtx` and
/// `ModalResponder<..>` on a modal), and inherits the Route's `response`
/// and `auto_defer` like `#[on_stale]`. Without it the App's
/// [`AppBuilder::expired_handler`] runs, else the default `noctis.expired`
/// ephemeral notice.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, TextDisplay, V2Message};
///
/// #[derive(Button)]
/// #[button("ban.ok", by_ref, defaults(style = Danger, label = "Confirm"))]
/// struct ConfirmBan { reason: String }
///
/// impl ConfirmBan {
///     #[handler]
///     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<V2Message> {
///         Ok(V2Message::new(Components::new().push(TextDisplay::new("banned"))))
///     }
///
///     #[on_expired]
///     async fn gone(_ctx: ComponentCtx, expired: Expired) -> HandlerResult<V2Message> {
///         let text = format!("This confirmation is gone ({}): run /ban again", expired.as_str());
///         Ok(V2Message::new(Components::new().push(TextDisplay::new(text))))
///     }
/// }
///
/// assert!(format!("{:?}", ConfirmBan::descriptor()).contains("on_expired: true"));
/// ```
pub use noctis_macros::on_expired;

/// Marks the method of a Command, context-menu, component or modal struct
/// that answers an invocation one of its declared `rate_limit(..)`s
/// refused (FR-46, INT-14). It takes no arguments and no `self` (the
/// handler did not run): `async fn <any>(ctx: RateLimitedCtx, hit:
/// RateLimitHit) -> HandlerResult<V2Message>`. Its message is sent
/// ephemeral as the initial response; it wins over
/// [`AppBuilder::rate_limited_handler`], and without either Noctis sends the
/// `noctis.rate_limited` notice in the User Locale. An error, a panic or a
/// missed ack deadline sends the default notice instead.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, TextDisplay, V2Message};
///
/// #[derive(SlashCommand)]
/// #[command("daily", description = "Claim", rate_limit(user, 1, per = "1d", manual))]
/// struct Daily;
///
/// impl Daily {
///     #[handler]
///     async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
///         // Only a successful claim is charged.
///         ctx.rate_limits().hit(RateScope::User).await?;
///         ctx.reply_text("claimed").await
///     }
///
///     #[on_rate_limited]
///     async fn slow(_ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
///         let text = format!("Next claim {}", hit.relative_timestamp());
///         Ok(V2Message::new(Components::new().push(TextDisplay::new(text))))
///     }
/// }
///
/// let debug = format!("{:?}", Daily::descriptor());
/// assert!(debug.contains("on_rate_limited: true"), "{debug}");
/// assert_eq!(<Daily as noctis::__private::core::__SlashCommand>::RATE_LIMITS.len(), 1);
/// ```
pub use noctis_macros::on_rate_limited;

/// Derives a `USER` context-menu command from a unit struct:
/// `#[user_command("Name", ..)]` declares it and the `#[handler]` method
/// `(self, ctx: CommandCtx, target: ResolvedUser[, r: CommandResponder<..>])`
/// answers it.
///
/// The first argument is the menu name, a string literal of 1 to 32
/// characters (UTF-16 counted; mixed case, spaces, apostrophes and emoji are
/// fine, CTX-01). Then the metadata arguments of
/// [`SlashCommand`](macro@SlashCommand) (`permissions`, `contexts`,
/// `integration_types`, `nsfw`, `guilds`), `response` and `auto_defer` (with
/// `auto_defer = false` the handler takes `CommandResponder<Manual>`, and
/// [`ContextMenuHandler::auto_defer`] reads it). A context-menu command has
/// no description and no options (`[CTX-02]` otherwise, including a struct
/// with fields); its type comes from the derive (CTX-03). It answers like a
/// slash Command: `R` is an [`IntoResponse`] value, and an optional
/// [`CommandResponder`] comes after the target.
///
/// The target is the right-clicked user,
/// [`ResolvedUser`](crate::ui::ResolvedUser) (`member` is `None` when the
/// target is not a member of the guild, including outside a guild);
/// [`MessageCommand`](macro@MessageCommand) gives a
/// [`ResolvedMessage`](crate::ui::ResolvedMessage). `ctx.command_path()` is
/// the menu name. An interaction without its target in the resolved data
/// fails with a [`MenuTargetError`] and the handler does not run.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, LegacyMessage, ResolvedMessage, ResolvedUser, TextDisplay};
/// use noctis::{CommandMetadata, MenuKind};
///
/// #[derive(UserCommand)]
/// #[user_command("Show profile", contexts = [Guild], response = ephemeral)]
/// struct ShowProfile;
///
/// impl ShowProfile {
///     #[handler]
///     async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Components> {
///         assert_eq!(ctx.command_path(), "Show profile");
///         Ok(Components::new().push(TextDisplay::new(format!("<@{}>", target.user.id))))
///     }
/// }
///
/// #[derive(MessageCommand)]
/// #[message_command("Quote")]
/// struct Quote;
///
/// impl Quote {
///     #[handler]
///     async fn handle(self, _ctx: CommandCtx, target: ResolvedMessage, r: CommandResponder) -> HandlerResult<Responded> {
///         Ok(r.reply(LegacyMessage::new(format!("> {}", target.message.content))).await?.done())
///     }
/// }
///
/// assert_eq!(ShowProfile::descriptor().kind(), MenuKind::User);
/// assert_eq!(ShowProfile::descriptor().ephemerality(), Ephemerality::Ephemeral);
/// assert_eq!(Quote::descriptor().menu_name(), "Quote");
/// assert_eq!(Quote::descriptor().metadata(), &CommandMetadata::new());
/// let _module = Module::new("profile").add::<ShowProfile>().add::<Quote>();
/// ```
pub use noctis_macros::UserCommand;

/// Derives a `MESSAGE` context-menu command from a unit struct: like
/// [`UserCommand`](macro@UserCommand) with `#[message_command("Name", ..)]`
/// and `target: ResolvedMessage`
/// ([`ResolvedMessage`](crate::ui::ResolvedMessage)).
pub use noctis_macros::MessageCommand;

/// Derives a typed gateway event handler from a unit struct, whose
/// `#[handler]` method takes `(self, ctx: EventCtx, event: <payload>) ->
/// HandlerResult<()>`. See [`events`] and
/// [Modules and events](crate#modules-and-events).
///
/// The optional `#[event(..)]` takes, in any order, each at most once:
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
pub use noctis_macros::EventHandler;

/// Derives an interactive button (styles 1-4) from a struct: the struct is
/// a Route whose fields are the Payload of its `custom_id`, and its
/// `#[handler]` method `(self, ctx: ComponentCtx[, r: ComponentResponder<..>])
/// -> HandlerResult<R>` handles the click: `App::build` maps the Route id to
/// it (one handler per Route id, CID-04) and a click on a component built
/// from the struct runs it on the decoded value. `R` is a message (sent per
/// the declared `response`) or the `Responded` from the responder's
/// `done()`.
///
/// `#[button("id", <codec args>, defaults(..))]`: the Route id (CID-03), the
/// codec arguments `by_ref`, `overflow = spill`, `schema = n`, `signed`, and
/// the mandatory `defaults(..)` with `style` (`Primary`, `Secondary`,
/// `Success` or `Danger`) and `label` (at most 80 characters, CV2-08) or
/// `emoji` (`"<:name:id>"`, `"<a:name:id>"` or a Unicode emoji), and
/// optionally `disabled = true`. Values are literals, checked at compile
/// time; omitted keys are not sent. Link and Premium buttons have no handler:
/// they stay plain builders ([`LinkButton`](crate::ui::LinkButton),
/// [`PremiumButton`](crate::ui::PremiumButton)).
///
/// The runtime arguments (every component kind):
///
/// - `response = update | ephemeral | public` (default `update`): `update`
///   auto-defers with type 6 and a returned message updates the clicked
///   message in place; `ephemeral` / `public` auto-defer with type 5 and a
///   returned message is a new message; `reply` and `defer_reply` default to
///   it (`public` under `update`).
/// - `auto_defer = true | false` (default `true`): `false` arms no
///   auto-defer, so the handler answers within 3 s itself and takes
///   [`ComponentResponder<Manual>`](crate::ComponentResponder), which has
///   `show_modal` (INT-05; a mismatch is an `[INT-05]` compile error).
///
/// - `ttl = "15m"` (units `d`, `h`, `m`, `s` in that order, 1 s..=365 d)
///   and `one_shot` (the Ref is taken on first use), on a `by_ref` or
///   `overflow = spill` Route only: the Ref's TTL (default
///   [`AppBuilder::ref_ttl`], 24 h) and whether it is consumed by its first
///   click. On an inline Route they are a `[CID-03]` compile error.
///
/// `owner`, `permissions`, `guard` and `rate_limit` arrive with later
/// stories and are a `[CID-03]` compile error until then.
///
/// `X { .. }.build()` returns a [`RouteButton<X>`](crate::ui::RouteButton)
/// from the defaults; its setters (`label`, `emoji`, `style`, `disabled`)
/// override one field each.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{ButtonStyle, Components};
///
/// #[derive(Button)]
/// #[button("shop.buy", defaults(style = Primary, label = "Buy", emoji = "🛒"))]
/// struct Buy {
///     item: u64,
/// }
///
/// impl Buy {
///     #[handler]
///     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
///         # unimplemented!()
///     }
/// }
///
/// let view: Components = components! {
///     row [ Buy { item: 1 }.build(), Buy { item: 2 }.build().label("Buy two").style(ButtonStyle::Success) ],
/// };
/// assert_eq!(view.len(), 1);
/// let _shop = Module::new("shop").add::<Buy>();
/// ```
///
/// A button that opens a modal is `auto_defer = false`; one that answers
/// with a new ephemeral message declares `response = ephemeral`:
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, TextDisplay};
///
/// #[derive(Modal)]
/// #[modal("profile.bio", defaults(title = "Bio"))]
/// struct BioForm {
///     #[text_input("Bio")]
///     bio: String,
/// }
///
/// impl BioForm {
///     #[handler]
///     async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Components> {
///         Ok(Components::new().push(TextDisplay::new(format!("Bio: {}", self.bio))))
///     }
/// }
///
/// #[derive(Button)]
/// #[button("profile.edit", auto_defer = false, defaults(style = Secondary, label = "Edit"))]
/// struct EditBio;
///
/// impl EditBio {
///     #[handler]
///     async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
///         let form = BioForm { bio: String::new() }.build();
///         Ok(r.show_modal(form).await?.done())
///     }
/// }
///
/// #[derive(Button)]
/// #[button("profile.peek", response = ephemeral, defaults(style = Primary, label = "Peek"))]
/// struct Peek;
///
/// impl Peek {
///     #[handler]
///     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
///         Ok(Components::new().push(TextDisplay::new("Only you see this.")))
///     }
/// }
///
/// let _profile = Module::new("profile").add::<BioForm>().add::<EditBio>().add::<Peek>();
/// ```
pub use noctis_macros::Button;

/// Derives a String Select from a struct (the struct is its Route, which
/// always carries the select nonce): `#[string_select("id", <codec args>,
/// defaults(placeholder = "…", ..))]`, and a `#[handler]` method `(self,
/// ctx: ComponentCtx)`.
///
/// `defaults(..)` takes `placeholder` (required, at most 150 characters),
/// `min_values` (0..=25), `max_values` (1..=25), `disabled` and `options =
/// <a #[derive(Choices)] enum>` (at most 25 variants, CV2-09). Omitted keys
/// are not sent: Discord's `min_values = 1` and `max_values = 1` apply.
///
/// With `options`, `X { .. }.build()` returns a
/// [`StringSelect`](crate::ui::StringSelect) with the enum's options.
/// Without, it returns a [`StringSelectBuilder<X>`](crate::ui::StringSelectBuilder),
/// which can be placed only after `.options(iter)` (a `[CV2-09]` compile
/// error otherwise). Setters: `placeholder`, `min_values`, `max_values`,
/// `disabled`, `options`.
///
/// ```
/// use noctis::prelude::*;
/// use noctis::ui::{Components, SelectOption};
///
/// #[derive(StringSelectMenu)]
/// #[string_select("ticket.kind", defaults(placeholder = "Pick a topic"))]
/// struct TicketKind;
///
/// impl TicketKind {
///     #[handler]
///     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
///         # unimplemented!()
///     }
/// }
///
/// let view: Components = components! {
///     row [ TicketKind.build().options([SelectOption::new("Billing", "billing")]) ],
/// };
/// assert_eq!(view.len(), 1);
/// ```
pub use noctis_macros::StringSelectMenu;

/// Derives a User Select (`#[user_select("id", defaults(placeholder = "…",
/// ..))]`): like [`StringSelectMenu`](macro@StringSelectMenu) without
/// `options`; `.build()` returns a [`UserSelect`](crate::ui::UserSelect).
pub use noctis_macros::UserSelectMenu;

/// Derives a Role Select (`#[role_select("id", defaults(placeholder = "…",
/// ..))]`); `.build()` returns a [`RoleSelect`](crate::ui::RoleSelect).
pub use noctis_macros::RoleSelectMenu;

/// Derives a Mentionable Select (`#[mentionable_select("id",
/// defaults(placeholder = "…", ..))]`); `.build()` returns a
/// [`MentionableSelect`](crate::ui::MentionableSelect).
pub use noctis_macros::MentionableSelectMenu;

/// Derives a Channel Select (`#[channel_select("id", defaults(placeholder =
/// "…", channel_types = [GuildText, ..], ..))]`, with twilight
/// `ChannelType` variant names); `.build()` returns a
/// [`ChannelSelect`](crate::ui::ChannelSelect).
pub use noctis_macros::ChannelSelectMenu;

/// Withdrawn forms: each fails with a `= help:` showing the struct form.
#[doc(hidden)]
pub use noctis_macros::{Options, Route, command, event, message_command, user_command};

pub mod cli {
    //! The built-in command line: `--publish`, `--publish-guild [<id>]`,
    //! `--publish-clear [--guild <id>]` and `--dry-run` (no clap).
    //!
    //! [`App::run`](crate::App::run) parses `std::env::args_os().skip(1)`;
    //! an unknown argument is [`CliError::UnknownFlag`]. Tests and several
    //! Apps pass arguments with
    //! [`App::run_with_args`](crate::App::run_with_args). A bot with its own
    //! flags splits first:
    //!
    //! ```no_run
    //! use noctis::cli::NoctisArgs;
    //!
    //! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //! let (noctis_args, mine) = NoctisArgs::split(std::env::args_os())?;
    //! // parse `mine` (program name first) with your own parser, then:
    //! let app = noctis::App::from_env()?.build()?;
    //! app.run_with_args(noctis_args).await?;
    //! # let _ = mine;
    //! # Ok(()) }
    //! ```

    pub use noctis_core::cli::{CliError, CliMode, NoctisArgs};
}

pub mod publish {
    //! Publish: writes the Registry's commands to Discord only when they
    //! differ from the remote ones (REG-05: 200 creates per day per guild).
    //!
    //! [`App::publish`](crate::App::publish) reads the application, builds
    //! the JSON from the Registry alone, runs every check before any write
    //! (META-04; REG-03 and REG-02 on the `Guild` dev mirror), then per
    //! target diffs the canonical forms and sends one bulk-overwrite `PUT`
    //! only when they differ. A dry run writes nothing. Publish never runs
    //! on shard READY; it runs in the `noctis.publish` span.
    //!
    //! ```no_run
    //! use noctis::publish::PublishScope;
    //! use noctis::twilight::model::id::Id;
    //!
    //! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //! let app = noctis::App::from_env()?.build()?;
    //! let report = app.publish(PublishScope::Guild(Id::new(42))).dry_run(true).await?;
    //! println!("{report}");
    //! # Ok(()) }
    //! ```

    pub use noctis_core::publish::{
        Publish, PublishError, PublishReport, PublishScope, TargetReport,
    };
}

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

pub mod events {
    //! Gateway event payloads for `#[derive(EventHandler)]` handlers, the
    //! [`EventCtx`] they receive and the [`EventHandler`] descriptor their
    //! `descriptor()` returns.
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
    //! #[derive(EventHandler)]
    //! struct AwardXp;
    //!
    //! impl AwardXp {
    //!     #[handler]
    //!     async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    //!         let _ = (ctx.guild_id(), msg.author.id);
    //!         Ok(())
    //!     }
    //! }
    //!
    //! let module = Module::new("profile").add::<AwardXp>();
    //! ```
    //!
    //! A type that is not a payload does not compile:
    //!
    //! ```compile_fail
    //! use noctis::events::EventCtx;
    //! use noctis::prelude::*;
    //!
    //! #[derive(EventHandler)]
    //! struct H;
    //!
    //! impl H {
    //!     #[handler]
    //!     async fn handle(self, ctx: EventCtx, ev: String) -> HandlerResult<()> {
    //!         Ok(())
    //!     }
    //! }
    //! ```
    //!
    //! Interactions are handled by commands and components, never by an
    //! event handler:
    //!
    //! ```compile_fail
    //! use noctis::events::EventCtx;
    //! use noctis::prelude::*;
    //! use noctis::twilight::model::gateway::payload::incoming::InteractionCreate;
    //!
    //! #[derive(EventHandler)]
    //! struct H;
    //!
    //! impl H {
    //!     #[handler]
    //!     async fn handle(self, ctx: EventCtx, ev: InteractionCreate) -> HandlerResult<()> {
    //!         Ok(())
    //!     }
    //! }
    //! ```
    //!
    //! The first parameter is the context:
    //!
    //! ```compile_fail
    //! use noctis::events::MessageCreate;
    //! use noctis::prelude::*;
    //!
    //! #[derive(EventHandler)]
    //! struct H;
    //!
    //! impl H {
    //!     #[handler]
    //!     async fn handle(self, ctx: u8, ev: MessageCreate) -> HandlerResult<()> {
    //!         Ok(())
    //!     }
    //! }
    //! ```
    //!
    //! And a handler returns `HandlerResult<()>`:
    //!
    //! ```compile_fail
    //! use noctis::events::{EventCtx, MessageCreate};
    //! use noctis::prelude::*;
    //!
    //! #[derive(EventHandler)]
    //! struct H;
    //!
    //! impl H {
    //!     #[handler]
    //!     async fn handle(self, ctx: EventCtx, ev: MessageCreate)  {
    //!         
    //!     }
    //! }
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

pub mod http {
    //! Raw REST requests to endpoints twilight does not model (FR-31,
    //! FR-52).
    //!
    //! [`RawHttp`] (in the prelude) adds [`raw`](RawHttp::raw) to the App's
    //! one twilight client, so it works on `ctx.http()` and
    //! [`App::http`](crate::App::http). The request goes through that
    //! client: the edge (or `AppBuilder::http_proxy`), its rate limiter (off
    //! when proxied), its timeout and its bot token
    //! ([`RawRequest::without_token`] for webhook and interaction-token
    //! routes). The path is the part after `/api/v10/`, query included; an
    //! invalid one is [`RawHttpError::InvalidPath`] and nothing is sent.
    //! Only 2xx responses are a [`RawResponse`]; other statuses are
    //! [`RawHttpError::Status`]. The Responder never sees these requests:
    //! answer interactions with `ctx.respond`/`ctx.update`.
    //!
    //! Every request is bounded by `AppBuilder::http_timeout` (default
    //! 10 s), interaction callbacks included. A Discord 429 that twilight's
    //! limiter cannot wait out (every 429 behind a shared rate-limit proxy,
    //! and a `global`, `shared` or header-less one without) is never retried:
    //! it is [`RateLimited`] with its [`RateLimitScope`], `retry_after` and
    //! Discord `code`, typed on every Noctis surface
    //! (`ResponderError::RateLimited`, `WebhookError::RateLimited`,
    //! [`RawHttpError::RateLimited`]) and recovered from a raw twilight call
    //! with [`RateLimited::from_http_error`]. The initial interaction
    //! response (respond or defer) is the exception: it keeps the INT-01 fit
    //! rule (one retry when `retry_after` fits the ack deadline) and fails as
    //! `AckDeadlineMissed`, never `RateLimited`. Wait before sending it
    //! again; never retry it in a loop.
    //!
    //! ```no_run
    //! use noctis::http::RateLimited;
    //! use noctis::prelude::*;
    //!
    //! /// `None` when Discord rate limited the read: try again later.
    //! async fn channel_name(ctx: &CommandCtx, id: u64) -> Result<Option<String>, HandlerError> {
    //!     let id = noctis::twilight::model::id::Id::new(id);
    //!     match ctx.http().channel(id).await {
    //!         Ok(response) => Ok(response.model().await?.name),
    //!         Err(error) if RateLimited::from_http_error(&error).is_some() => Ok(None),
    //!         Err(error) => Err(error.into()),
    //!     }
    //! }
    //! # let _ = channel_name;
    //! ```
    //!
    //! ```no_run
    //! use noctis::http::{Method, RawHttp};
    //! use noctis::prelude::*;
    //!
    //! async fn pin_count(ctx: &CommandCtx, channel: u64) -> Result<usize, HandlerError> {
    //!     let pins = ctx.http().raw(Method::Get, format!("channels/{channel}/pins")).await?;
    //!     let pins: Vec<serde_json::Value> = pins.json()?;
    //!     Ok(pins.len())
    //! }
    //! # let _ = pin_count;
    //! ```

    pub use noctis_core::{
        Method, RateLimitScope, RateLimited, RawHttp, RawHttpError, RawRequest, RawResponse,
    };
}

pub mod gateway {
    //! Presence and gateway commands (FR-53): what the bot shows and what
    //! it asks the gateway for.
    //!
    //! - [`AppBuilder::presence`](crate::AppBuilder::presence) sets the
    //!   [`Presence`] every shard identifies with: a [`Status`] and at most
    //!   one [`Activity`] (`Presence::watching("/help")`,
    //!   `Presence::custom("☕ brewing")`, `Presence::dnd()`, ...).
    //!   [`AppBuilder::large_threshold`](crate::AppBuilder::large_threshold)
    //!   takes a [`LargeThreshold`]: `LargeThreshold::new::<N>()` compiles only
    //!   for 50..=250 (GW-06, a `[GW-06]` compile error otherwise).
    //! - [`App::presence`](crate::App::presence) (or
    //!   `ctx.gateway().presence()`) is a [`PresenceHandle`]:
    //!   [`set`](PresenceHandle::set) changes the presence on every shard of
    //!   this process, all or none, [`set_shard`](PresenceHandle::set_shard)
    //!   on one. Both are synchronous and never block; a shard sends the
    //!   latest presence again after it re-identifies.
    //! - `ctx.gateway()` / [`App::gateway`](crate::App::gateway) is a
    //!   [`GatewayHandle`]:
    //!   [`request_guild_members`](GatewayHandle::request_guild_members) sends
    //!   a [`MemberRequest`] (one guild; [`users`](MemberRequest::users) takes
    //!   1..=100 ids at compile time, [`user_list`](MemberRequest::user_list)
    //!   and [`prefix`](MemberRequest::prefix) check at run time,
    //!   [`all`](MemberRequest::all) needs `GUILD_MEMBERS`) to the guild's
    //!   owning shard and returns the collected [`GuildMembers`].
    //!
    //! Every refusal is a [`GatewayError`], never a silent drop: the App is
    //! not running, a shard or guild outside this process's Shard Range, a
    //! shard over its budget of 110 commands per 60 s (`SendLimit`, GW-04),
    //! `RATE_LIMITED` from Discord (`RateLimited`, GW-07), a missing intent,
    //! or no answer within 10 s (`Timeout`). No cargo feature is needed.
    //!
    //! ```no_run
    //! use noctis::gateway::{GatewayError, GuildMembers, LargeThreshold, MemberRequest, Presence};
    //! use noctis::prelude::*;
    //! use noctis::twilight::model::id::Id;
    //!
    //! async fn lookup(ctx: &CommandCtx) -> Result<GuildMembers, GatewayError> {
    //!     ctx.gateway().presence().set(Presence::custom("looking up members"))?;
    //!     let guild = Id::new(1);
    //!     let request = MemberRequest::prefix(guild, "ab", 10)?;
    //!     ctx.gateway().request_guild_members(request).await
    //! }
    //!
    //! # fn main() -> Result<(), noctis::Error> {
    //! let mut builder = noctis::App::from_env()?;
    //! builder
    //!     .presence(Presence::watching("/help"))
    //!     .large_threshold(LargeThreshold::new::<100>());
    //! let _ = MemberRequest::users(Id::new(1), [Id::new(2), Id::new(3)]);
    //! # let _ = lookup;
    //! # Ok(()) }
    //! ```

    pub use noctis_core::gateway::{
        Activity, GatewayError, GatewayHandle, GuildMembers, LargeThreshold, MemberRequest,
        Presence, PresenceHandle, Status,
    };
}

pub mod webhook {
    //! Views over webhooks (FR-65): log channels, personas and cross-posts
    //! with the same Components V2 safety as interaction replies.
    //!
    //! - [`Webhooks`] (`ctx.webhooks()` on every context, `app.webhooks()`):
    //!   [`create`](Webhooks::create) a webhook the App owns in a
    //!   [`WebhookChannel`] (bot token, `MANAGE_WEBHOOKS`), or wrap stored
    //!   credentials with [`owned`](Webhooks::owned) or
    //!   [`foreign`](Webhooks::foreign). A bad token is
    //!   [`WebhookError::InvalidToken`] before any request.
    //! - [`Webhook<O>`](Webhook) with [`Ownership`] `O` = [`Owned`]
    //!   ([`OwnedWebhook`]) or [`Foreign`] ([`ForeignWebhook`]): an owned
    //!   one sends any `V2Message` (Route buttons, selects, Refs, exactly as
    //!   a reply); a foreign one sends only a
    //!   [`StaticMessage`](crate::ui::StaticMessage) (WH-02: anything else is
    //!   a `[WH-02]` compile error; convert a run-time tree with
    //!   `StaticMessage::try_from`). [`token`](Webhook::token) is a secret
    //!   to store, never to log; `Debug` redacts it.
    //!   [`in_channel`](Webhook::in_channel) declares the channel kind of
    //!   stored credentials.
    //! - [`ExecuteWebhook`] (from `send`): `.flags(CreateFlags)`,
    //!   `.username(WebhookName)`, `.avatar_url(..)`,
    //!   `.thread(`[`WebhookThread`]`)`; it resolves to a
    //!   [`WebhookMessage`], whose `edit` keeps the attachments the message
    //!   holds and whose `delete` passes its thread.
    //!   [`EditWebhookMessage`] (`.flags(EditFlags)`, `.thread(id)`) and
    //!   [`DeleteWebhookMessage`] (`.thread(id)`) act by message id.
    //! - Always `?with_components=true` on execute and edit (WH-01, no
    //!   setter); every edit sends `content: null`, `embeds: []` and
    //!   `poll: null` (WH-03); names are 1-80 characters without `clyde` or
    //!   `discord` (WH-04, `webhook_name!`); a forum or media channel needs
    //!   a thread, and a `thread_name` has 1-100 characters (WH-05, checked
    //!   before sending when the channel kind is known; Discord's 220001 and
    //!   220003 map to the same typed error). Webhook-token requests carry no
    //!   bot `Authorization` header.
    //!
    //! An owned webhook for a log channel, and a foreign one for a mirror:
    //!
    //! ```no_run
    //! use noctis::prelude::*;
    //! use noctis::twilight::model::channel::ChannelType;
    //! use noctis::twilight::model::id::Id;
    //! use noctis::twilight::model::id::marker::ChannelMarker;
    //! use noctis::ui::{StaticComponents, StaticContainer, TextDisplay};
    //! use noctis::webhook::{WebhookChannel, WebhookError, WebhookThread};
    //! use noctis::webhook_name;
    //!
    //! async fn log(ctx: &CommandCtx, channel: Id<ChannelMarker>) -> Result<(), WebhookError> {
    //!     let hook = ctx
    //!         .webhooks()
    //!         .create(WebhookChannel::new(channel, ChannelType::GuildText), webhook_name!("Mod log"))
    //!         .await?;
    //!     let card = || StaticComponents::new().push(StaticContainer::new().push(TextDisplay::new("Case #1 opened")));
    //!     let sent = hook.send(card()).username(webhook_name!("Case bot")).await?;
    //!     sent.edit(StaticComponents::new().push(TextDisplay::new("Case #1 closed"))).await?;
    //!     // Store `hook.id()` and `hook.token()` to reuse it with `ctx.webhooks().owned(..)`.
    //!     Ok(())
    //! }
    //!
    //! async fn mirror(ctx: &CommandCtx, id: u64, token: &str) -> Result<(), WebhookError> {
    //!     let hook = ctx.webhooks().foreign(Id::new(id), token)?.in_channel(ChannelType::GuildForum);
    //!     hook.send(card())
    //!         .thread(WebhookThread::Create("Daily log".into()))
    //!         .await?;
    //!     Ok(())
    //! }
    //! # fn card() -> StaticComponents { StaticComponents::new().push(TextDisplay::new("Mirrored")) }
    //! # let _ = (log, mirror);
    //! ```

    pub use noctis_core::{
        DeleteWebhookMessage, EditWebhookMessage, ExecuteWebhook, Foreign, ForeignWebhook, Owned,
        OwnedWebhook, Ownership, Webhook, WebhookChannel, WebhookError, WebhookMessage,
        WebhookThread, Webhooks,
    };
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

/// An alt text literal, checked at `cargo check` against ATT-06 (at most
/// 1024 characters); yields a [`ui::AltText`].
pub use noctis_macros::alt;
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
/// A webhook name literal, checked at `cargo check` against WH-04 (1 to 80
/// characters, no `clyde` or `discord`); yields a [`ui::WebhookName`].
pub use noctis_macros::webhook_name;

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
    //!   id, codec shape (inline, by Ref or spill, with an optional select
    //!   nonce and integrity tag), schema and Payload; implemented by the
    //!   component and modal derives of the [`prelude`](crate::prelude)
    //!   (`#[derive(Button)]`, the select derives, `#[derive(Modal)]`).
    //! - [`SelectRoute`]: a Route with a select nonce (every select derive
    //!   implements it), the only kind a message select is built from.
    //! - [`Payload`] and [`PayloadDecode`]: an ordered field list, its
    //!   worst-case width [`Payload::MAX_ENCODED_LEN`], written through
    //!   [`FieldWriter`] and read through [`FieldReader`].
    //! - [`CodecError`] and [`Section`]: why an inline body or a `custom_id`
    //!   did not decode, and which layout section was wrong.
    //! - [`Ref`] and [`RefError`]: the 11-character Ref a `by_ref` (or
    //!   spilled) `custom_id` carries, as `ctx.current_ref()` returns it for
    //!   [`RefOps`](crate::RefOps); [`Ref::mint`] draws one from the OS
    //!   CSPRNG.
    //! - [`PayloadKey`] and [`KeyError`]: the 32-byte key that signs `signed`
    //!   Routes (`NOCTIS_PAYLOAD_KEY`, 64 hex digits; see
    //!   [`AppBuilder::payload_key`](crate::AppBuilder::payload_key)); [`Sign`],
    //!   [`Verify`] and [`Tag`]: the integrity-tag traits it implements and the
    //!   6-character tag.
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
        FieldReader, FieldWriter, KeyError, Payload, PayloadDecode, PayloadKey, Ref, RefError,
        Route, RouteId, RouteShape, Section, SelectRoute, Sign, Tag, Verify, check,
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
    //! - [`DiscordCache`]: the typed Discord resource cache (owned
    //!   `twilight_model` values, never a guard). Handlers read it with
    //!   `ctx.cache()` ([`DiscordCacheReader`](crate::DiscordCacheReader));
    //!   [`AppBuilder::discord_cache`](crate::AppBuilder::discord_cache) takes a
    //!   [`DiscordCacheConfig`](crate::DiscordCacheConfig) (default
    //!   `GUILD | CHANNEL | ROLE`, or `none()`), and core feeds the backend
    //!   before handlers see an event. It is implemented for `Arc<P>`, so one
    //!   backend can serve several Apps; it is never namespaced.
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
    //! - [`Namespaced`]: core's wrapper around each byte store, prefixing
    //!   every key with `<app_id>:<port>:` (port segments `ref`, `session`,
    //!   `module_state`, `rate_limit`, `cache`, `render_cache`, `asset`);
    //!   [`NamespacedPort`] is the sealed trait of the seven `dyn Port` types.
    //! - [`Shared`]: pass `Shared::new(adapter)` to a byte-store setter to
    //!   share its entries between Apps under `_shared:<port>:`; [`IntoPort`]
    //!   is the sealed trait those setters take.
    //!
    //! Every port is `Send + Sync + 'static` and dyn-compatible, so the App
    //! holds it as `Arc<dyn Port>`; every byte store is also implemented for
    //! `Arc<P>`, so one instance can be handed to several Apps with
    //! `Arc::clone`. Adapters store keys verbatim and never prefix them, and
    //! no method ever receives a secret.
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
        RenderCache, RenderCap, RenderError, RenderJob, RenderOutput, Renderer, RendererPorts,
        SessionStore, Size, Template, TemplateDescriptor, ValueError, decode_template,
        decode_value, encode_value,
    };

    pub use noctis_core::{IntoPort, Namespaced, NamespacedPort, Shared};

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

#[cfg(feature = "image")]
pub mod image {
    //! Image Templates (feature `image`, FR-32, FR-66; AD-15, AD-22).
    //!
    //! A Template is a typed value with fixed render settings:
    //! `#[derive(Template)]` reads `#[template(id, version, size, format[,
    //! animation(..)])]`, and [`TemplateView`] draws it as a [`Node`] tree,
    //! usually written with [`view!`]. Register it with
    //! [`Module::template`](crate::Module::template).
    //!
    //! - [`view!`] and [`tw!`]: literal Tailwind v4 classes are checked at
    //!   compile time against Noctis's strict grammar and the crate's
    //!   `noctis-theme.css` (`[TW-01]`); a v3 name suggests its v4 rename.
    //! - The node tree: [`Node`], [`Tag`], [`Classes`], [`Style`] (typed
    //!   dynamic values), [`IntoNodes`] and [`div`], [`span`], [`text`]. User
    //!   data enters a tree only as text: no constructor turns a string into
    //!   markup or classes.
    //! - The Theme: [`Theme`], parsed from `noctis-theme.css` ([`ThemeError`],
    //!   [`ThemeErrorKind`], `[TW-02]`); [`theme!`] embeds it. Its tokens are
    //!   read through [`ThemeTokens`] by [`ThemeNamespace`].
    //! - The contract: [`Template`], [`Frame`], [`Size`], [`Format`],
    //!   [`AnimationSpec`], [`AnimatedFormat`] and [`TemplateDescriptor`].
    //! - The class errors: [`InvalidClasses`], [`UnknownClass`] and
    //!   [`Suggestion`].
    //! - Rendering: [`AppBuilderImageExt`] (`.theme(..)`, `.image(..)`,
    //!   also in the prelude) adds the [`ImagePlugin`], which installs the
    //!   App's takumi Renderer on the process-scoped render pool
    //!   ([`ImageConfig`]: threads and queue capacity). A handler renders
    //!   with `ctx.render(template).await?`, which returns an
    //!   [`Attachment`](crate::ui::Attachment) or a [`RenderError`]
    //!   (`QueueFull` when the queue is full).
    //! - Per-job caps: [`RenderCaps`] (`ImageConfig::caps`; defaults 4 MP per
    //!   job, 1 MP per frame, 90 frames, 6 s) are checked before a job enters
    //!   the queue; a job over one fails with `RenderError::OverCap`, naming
    //!   the [`RenderCap`]. Admission is weighted: a still counts 1 unit of
    //!   the queue capacity, an animation `ceil(frames / 8)`; a quarter of
    //!   the capacity is reserved for stills, which also run before queued
    //!   animations. Dropping the `ctx.render` future gives the render up:
    //!   a queued job is skipped, a running animation stops between frames.
    //! - Animations (FR-69): a Template's `animation(..)` or, per call,
    //!   `ctx.render_with(template, RenderOptions::new().animation(spec))`
    //!   ([`RenderOptions`]) render one frame per `1/fps`; the view gets
    //!   `Frame { index, count }`, and CSS animations play over the frame
    //!   time: `animate-spin|ping|pulse|bounce`, `animate-<token>` for a
    //!   Theme `--animate-<token>` (whose `@keyframes` the Theme defines),
    //!   or `animate-[<keyframes>_1s_…]`. A still draws the frame at 0 ms.
    //!   The fps is capped per format ([`AnimatedFormat::max_fps`]: GIF 50,
    //!   WebP and APNG 90; `RenderError::OverCap` with `RenderCap::Fps`).
    //!   Every frame has the Template's size: a frame root that declares
    //!   another pixel size (`Style::width_px`, `w-[Npx]`, …) fails with
    //!   `RenderError::SizeMismatch`. `ctx.render_report` returns a
    //!   [`RenderReport`] with the encoded `format` and `settings_used`.
    //! - Fonts (FR-35, `[IMG-01]`): [`Font`] (`Font::from_static`,
    //!   `Font::from_bytes`; TTF, OTF, TTC or WOFF2), registered with
    //!   `ImageConfig::font` in priority order. Text with no `font-*` family
    //!   draws with the first font; a glyph a font lacks (or its
    //!   `Font::unicode_range` excludes) falls back to the next family, then
    //!   to the bundled `"Inter"` (feature `default-font`), so mixed scripts
    //!   need no Template change. `Font::family` renames a font. The fonts
    //!   form the Runtime's font registry: every App on one Runtime must
    //!   register the same list (else `BuildViolation::ResourceConflict`).
    //!   A Template with no font at all fails `App::build` with
    //!   `RenderError::NoFont`.
    //! - Static images (`[IMG-02]`): a [`StaticImage`] (`const`, id
    //!   `[a-z0-9]([a-z0-9._-]*[a-z0-9])?`, `&'static` PNG, JPEG or WebP
    //!   bytes, at most 16 MP) registered with `ImageConfig::image` and
    //!   drawn with `<img src={LOGO} tw=".." />` ([`img`]). An image the App
    //!   did not register fails the render with `RenderError::UnknownImage`.
    //! - Remote images (FR-67, `[IMG-03]`): a Template declares them in
    //!   `#[asset]` fields (`#[asset(required)]` to fail instead of drawing
    //!   the fallback) of type [`RemoteImage`], `Option<RemoteImage>` or
    //!   `Vec<RemoteImage>`, built with `RemoteImage::avatar(&user, 256)`,
    //!   `RemoteImage::guild_icon(&guild, 128)`, `RemoteImage::url(..)`
    //!   ([`RemoteUrlError`]) or `RemoteImage::placeholder()`, and draws them
    //!   with `<img src={&self.avatar} />` ([`ImageSrc`]). Before the job is
    //!   admitted the App fetches every declared image from an allow-listed
    //!   origin (the Discord CDN by default; [`RemoteConfig`] via
    //!   `ImageConfig::remote`), with a timeout, byte, pixel and per-job caps
    //!   and bounded concurrency, decodes it off the async runtime and caches
    //!   the bytes in the App's `AssetStore`. A failed image draws the
    //!   fallback with a `noctis::image` warning; a required one fails with
    //!   `RenderError::RemoteImage`. Drawing an undeclared remote image is
    //!   `RenderError::UndeclaredImage`; more than `max_per_job` is
    //!   `RenderError::OverCap` with `RenderCap::RemoteImages`.
    //! - Emoji (FR-68, `[IMG-04]`): Unicode emoji draw in colour with the
    //!   `image-emoji` feature (the bundled Twemoji font, a last resort after
    //!   every registered font; CC-BY-4.0 Twemoji graphics). Without it, an
    //!   App that registers a Template logs one `[IMG-04]` warning at build.
    //!   Discord custom emoji (`<:name:id>`, `<a:name:id>`) in any string of a
    //!   Template's data are found before the render, fetched like remote
    //!   images (`RemoteImage::custom_emoji(id, animated)`, 128 px, at most
    //!   `RemoteConfig::max_emoji` per job, default 32; the rest and failures
    //!   draw the fallback) and drawn inline as `1.375em` images; the markup
    //!   is never drawn. A token in literal `view!` text is a compile error;
    //!   one only the view writes is `RenderError::UndeclaredImage`.
    //! - Cache (FR-36): every render is keyed by blake3 over the Noctis and
    //!   takumi versions, the Template id and version, the Theme, the
    //!   Template's data, the App's fonts, static images and remote-image
    //!   CDN and fallback, the size, format, animation and the effective
    //!   size budget. Identical concurrent renders run once (process-local
    //!   single-flight), and results are stored in the App's `RenderCache`
    //!   (`AppBuilder::render_cache`; `MemoryRenderCache`, 64 MiB and 1 day
    //!   by default, `MemoryRenderCache::with_limits`), so an identical later
    //!   render fetches, queues and draws nothing. Errors and renders that
    //!   drew a remote-image fallback are never stored. The attachment is
    //!   named by the key (`<first 16 hex>.<ext>`), except a degraded
    //!   (uncached) render, which is named by its bytes. The view's code is not in
    //!   the key: bump the Template's `version` when its look changes.
    //! - [`unstable`]: renderer-facing items outside semver.
    //!
    //! ```
    //! use noctis::image::{Frame, Node, Style, Template, TemplateView, view};
    //! use serde::{Deserialize, Serialize};
    //!
    //! #[derive(Template, Serialize, Deserialize)]
    //! #[template(id = "card", version = 1, size = (600, 200), format = Png)]
    //! struct Card {
    //!     name: String,
    //!     progress: f32,
    //! }
    //!
    //! impl TemplateView for Card {
    //!     fn view(&self, _frame: Frame) -> Node {
    //!         view! {
    //!             <div tw="flex flex-col w-full h-full p-8 bg-surface">
    //!                 <span tw="text-4xl text-white">{&self.name}</span>
    //!                 <div tw="h-4 rounded-full bg-brand"
    //!                      style={Style::new().width_pct(self.progress * 100.0)} />
    //!             </div>
    //!         }
    //!     }
    //! }
    //!
    //! assert_eq!(<Card as Template>::ID, "card");
    //! let module = noctis::Module::new("profile").template::<Card>();
    //! # let _ = module;
    //! ```

    /// `#[derive(Template)]`: implements [`Template`] from
    /// `#[template(id = "..", version = N, size = (w, h), format = ..[,
    /// animation(duration_ms = .., fps = ..[, format = ..])])]`, checked at
    /// `cargo check`; the animated `format` defaults to
    /// [`AnimatedFormat::DEFAULT`], and `Gif`/`Apng` need `image-gif`.
    /// `Serialize` and `Deserialize` are required too. `#[asset]` /
    /// `#[asset(required)]` fields declare [`RemoteImage`]s (`[IMG-03]`).
    pub use noctis_macros::Template;
    /// `theme!()`: the crate's `noctis-theme.css` as a [`Theme`], validated
    /// at `cargo check` (`[TW-02]`); `theme!("path")` reads a file relative
    /// to the calling file.
    pub use noctis_macros::theme;
    /// `tw!("classes")`: checked [`Classes`] (`[TW-01]`), usable in a
    /// `const`.
    pub use noctis_macros::tw;
    /// `view! { <div tw="..">..</div> }`: one root element as a [`Node`];
    /// every `tw = "…"` literal is checked at `cargo check` (`[TW-01]`).
    pub use noctis_macros::view;

    pub use noctis_core::{RenderOptions, RenderReport};
    pub use noctis_image::tw::{
        InvalidClasses, Suggestion, Theme, ThemeError, ThemeErrorKind, ThemeNamespace, ThemeTokens,
        UnknownClass,
    };
    pub use noctis_image::{AppBuilderImageExt, ImageConfig, ImagePlugin, RenderCaps};
    pub use noctis_image::{
        Classes, IntoNodes, Node, Style, Tag, TemplateView, div, img, span, text,
    };
    pub use noctis_image::{Font, StaticImage};
    pub use noctis_image::{ImageSrc, RemoteConfig, RemoteImage, RemoteUrlError};
    pub use noctis_ports::{
        AnimatedFormat, AnimationSpec, EncodeSettings, Format, Frame, OutputFormat, RenderCap,
        RenderError, RenderJob, RenderOutput, Size, Template, TemplateDescriptor,
    };

    pub mod unstable {
        //! Renderer-facing items. **Semver does not cover them**: they may
        //! change in any release. Bots do not need them.
        //!
        //! - [`ViewFn`]: what a Template's
        //!   [`ErasedView`](crate::ports::ErasedView) holds; it decodes the
        //!   Template value and calls its view.
        //! - [`ClassCheck`] and [`ClassCheckMode`]: the runtime half of
        //!   `[TW-01]`, which validates `Classes::dynamic` strings.

        pub use noctis_image::{ClassCheck, ClassCheckMode, ViewFn};
    }
}

pub mod prelude {
    //! The derives, the method attributes and the traits they implement:
    //! `use noctis::prelude::*;`.
    //!
    //! - The kind derives, one struct per interaction:
    //!   [`SlashCommand`](macro@SlashCommand) (with `#[command]` and
    //!   `#[option]`), [`UserCommand`](macro@UserCommand),
    //!   [`MessageCommand`](macro@MessageCommand),
    //!   [`EventHandler`](macro@EventHandler) (with `#[event]`),
    //!   [`Button`](macro@Button), [`StringSelectMenu`](macro@StringSelectMenu),
    //!   [`UserSelectMenu`](macro@UserSelectMenu),
    //!   [`RoleSelectMenu`](macro@RoleSelectMenu),
    //!   [`MentionableSelectMenu`](macro@MentionableSelectMenu),
    //!   [`ChannelSelectMenu`](macro@ChannelSelectMenu) and
    //!   [`Modal`](macro@Modal) (implementing [`ModalForm`], see
    //!   [`ui`](crate::ui#modal-forms)); [`handler`](macro@handler) marks the
    //!   method that handles the interaction and
    //!   [`autocomplete`](macro@autocomplete) an option's suggestion method;
    //!   [`on_stale`](macro@on_stale) and [`on_expired`](macro@on_expired)
    //!   answer stale and expired ids, [`Expired`] says which;
    //!   [`on_rate_limited`](macro@on_rate_limited) answers a declared
    //!   `rate_limit(..)` hit with [`RateLimitedCtx`] and [`RateLimitHit`],
    //!   and handlers count their limits with [`RateScope`].
    //! - [`Module`] (registration with `Module::add::<T>()`), [`App`],
    //!   [`AppBuilder`] and [`State`]: building an App and reading its typed
    //!   state.
    //! - The contexts [`CommandCtx`], [`EventCtx`], [`AutocompleteCtx`],
    //!   [`ComponentCtx`], [`ModalSubmitCtx`] and [`ExpiredCtx`] (the App's
    //!   `expired_handler`); [`HandlerError`] and
    //!   [`HandlerResult`].
    //! - [`CommandResponder`], [`ComponentResponder`], [`ModalResponder`],
    //!   [`Manual`], [`Responded`], [`IntoResponse`], [`Ephemerality`] and
    //!   [`Visibility`]: how handlers answer.
    //! - [`Options`](trait@Options) (what `SlashCommand` implements),
    //!   [`CommandOption`] (the option field types), [`MessageKey`] (what
    //!   option validators return), [`Partial`] and [`Suggestions`]
    //!   (autocomplete).
    //! - [`Route`] and [`Payload`]: the traits a component struct implements;
    //!   `#[derive(Payload)]` for a hand-written Route's Payload and
    //!   [`PayloadEnum`]: `#[derive(PayloadEnum)]` on a unit enum of at most
    //!   62 variants, a one-character [`Field`](crate::codec::Field).
    //! - [`Choices`]: the trait and `#[derive(Choices)]` on a unit enum, an
    //!   option set.
    //! - [`Rgb`] and the macros [`components!`], [`label!`],
    //!   [`placeholder!`], [`content!`], [`rgb!`] and [`webhook_name!`].
    //! - [`Text`] (a literal or a [`MessageKey`]) and the per-render
    //!   overrides [`ButtonOverrides`], [`StringSelectOverrides`],
    //!   [`UserSelectOverrides`], [`RoleSelectOverrides`],
    //!   [`MentionableSelectOverrides`], [`ChannelSelectOverrides`] and
    //!   [`InputOverrides`] (in a modal's generated `<X>Overrides`).
    //! - [`ErrorSink`] and [`ErrorOrigin`]: routing handler failures.
    //! - [`RefOps`]: Ref deletions returned with a response,
    //!   `(message, RefOps)`.
    //! - [`Middleware`], [`BeforeCtx`], [`AfterCtx`], [`Flow`] and [`Ext`]:
    //!   Module Middleware (`Module::before`, `Module::after`) and the typed
    //!   Extensions handlers take as `Ext<T>` parameters.
    //! - [`Access`]: what a custom Route Guard (`guard = path::to::fn`)
    //!   returns.
    //! - [`RawHttp`]: `.raw(method, path)` on `ctx.http()` and `app.http()`
    //!   ([`http`](crate::http)).
    //! - With the `image` feature, `AppBuilderImageExt`: `.theme(..)` and
    //!   `.image(..)` on the [`AppBuilder`], which install the Renderer
    //!   behind `ctx.render`.

    /// `.theme(..)` and `.image(..)` on the [`AppBuilder`] (feature `image`).
    #[cfg(feature = "image")]
    pub use crate::image::AppBuilderImageExt;
    pub use noctis_codec::{Payload, Route};
    pub use noctis_core::events::EventCtx;
    pub use noctis_core::{
        Access, AfterCtx, App, AppBuilder, AutocompleteCtx, BeforeCtx, CommandCtx, CommandOption,
        CommandResponder, ComponentCtx, ComponentResponder, Ephemerality, ErrorOrigin, ErrorSink,
        Expired, ExpiredCtx, Ext, Flow, HandlerError, HandlerResult, IntoResponse, Manual,
        Middleware, ModalResponder, ModalSubmitCtx, Module, Options, Partial, RateLimitHit,
        RateLimitedCtx, RateScope, RawHttp, RefOps, Responded, State, Suggestions, Visibility,
    };
    pub use noctis_macros::{
        Button, ChannelSelectMenu, Choices, EventHandler, MentionableSelectMenu, MessageCommand,
        Modal, Payload, PayloadEnum, RoleSelectMenu, SlashCommand, StringSelectMenu, UserCommand,
        UserSelectMenu,
    };
    /// Withdrawn forms: each fails with a `= help:` showing the struct form.
    #[doc(hidden)]
    pub use noctis_macros::{Options, Route, command, event, message_command, user_command};
    pub use noctis_macros::{autocomplete, handler, on_expired, on_rate_limited, on_stale};
    pub use noctis_macros::{components, content, label, placeholder, rgb, webhook_name};
    pub use noctis_rules::MessageKey;
    pub use noctis_ui::{
        ButtonOverrides, ChannelSelectOverrides, Choices, InputOverrides,
        MentionableSelectOverrides, ModalForm, Rgb, RoleSelectOverrides, StringSelectOverrides,
        Text, UserSelectOverrides,
    };
}

/// Typed content strings from YAML locale files (FR-42; LOC-01, LOC-04):
/// `noctis::i18n!(default = "en-US", dir = "locales")`, both optional, in
/// any order, `dir` relative to the crate root.
///
/// The files are the `dir/*.yaml` entries named by exact Discord locale
/// codes (`en-US.yaml`, `tr.yaml`); any other stem (`en.yaml`) or a `.yml`
/// extension is a `[LOC-01]` compile error, and other entries are ignored.
/// Each file is one YAML map of maps whose leaves are strings; every file
/// read is `include_bytes!`-ed, so editing it rebuilds the crate. The
/// macro generates, where it is invoked:
///
/// - `pub struct Strings`: `Strings::of(locale)` picks the locale's file,
///   its [`Locale::sibling`](rules::Locale::sibling)'s, else the default's;
///   its fields are the root maps (nested node structs), its methods the
///   leaves. A leaf without placeholders returns `&'static str`; one with
///   `{name}` or positional `{0}` placeholders takes one
///   `impl Display` per name (`arg0`, `arg1`, … for indexes) and returns a
///   `String` with every occurrence replaced. A keyword segment is a raw
///   identifier (`r#type`).
/// - `pub mod keys`: one [`MessageKey`](rules::MessageKey) const per
///   content key, dots to `_` and uppercased (`setup.need_channel` is
///   `keys::SETUP_NEED_CHANNEL`), for `HandlerError::with_message`,
///   overrides and `defaults(label = keys::X)`.
/// - `pub fn catalog() -> noctis::i18n::Catalog`, for
///   [`AppBuilder::locales`].
///
/// A key missing from the default file, or a call with the wrong arguments,
/// does not compile; a key another file lacks is one `[LOC-04]`
/// `deprecated` warning per file (silence it with `#![allow(deprecated)]`)
/// and falls back at run time. The reserved root maps `cmd`, `cmdword`,
/// `ctxcmd` and `noctis` get no accessors; `noctis.*` keys override the
/// built-in notices.
///
/// At run time the merged catalogs decide each run's [`Locales`] (FR-43):
/// a locale "has a file" when any catalog ships one, and the App's default
/// is the first catalog's `default`. An interaction's User Locale is
/// Discord's `locale`, then its `guild_locale`, then the default; its
/// Guild Locale is `guild_locale`, then `locale` (in a DM, or when the
/// guild's code has no file), then the default; a guild event uses the guild's recorded `preferred_locale`,
/// then the default. Each step takes the code's own file or its sibling's
/// (`en-GB` → `en-US`). A [`LocaleResolver`]
/// ([`AppBuilder::locale_resolver`]) and a before-Middleware
/// (`BeforeCtx::set_locale`) may override the result; pass
/// `ctx.response_locale()` to `Strings::of`.
///
/// ```
/// use noctis::rules::Locale;
///
/// noctis::i18n!(dir = "tests/fixtures/i18n/typed");
///
/// let tr = Strings::of(Locale::Tr);
/// assert_eq!(tr.shop.buy(), "Satın al");
/// assert_eq!(tr.shop.left(3), "3 kaldı");
/// assert_eq!(Strings::of(Locale::De).greet("Ada"), "Hi Ada");
/// assert_eq!(keys::SHOP_BUY.as_str(), "shop.buy");
/// assert_eq!(catalog().get(Locale::Tr, "greet"), Some("Selam {name}"));
/// ```
///
/// A wrong argument count does not compile:
///
/// ```compile_fail
/// noctis::i18n!(dir = "tests/fixtures/i18n/typed");
/// let _ = Strings::of(noctis::rules::Locale::Tr).shop.left();
/// ```
#[cfg(feature = "i18n")]
pub use noctis_macros::i18n;

#[cfg(feature = "i18n")]
pub mod i18n {
    //! The runtime half of [`i18n!`](crate::i18n!): [`Catalog`], the
    //! compiled files `catalog()` returns (pass it to
    //! [`AppBuilder::locales`](crate::AppBuilder::locales)), and
    //! [`format`], the placeholder substitution the accessors use.
    //!
    //! ```
    //! use noctis::rules::Locale;
    //!
    //! noctis::i18n!(dir = "tests/fixtures/i18n/typed");
    //!
    //! let c = catalog();
    //! assert_eq!(c.default_locale(), Locale::EnUs);
    //! assert_eq!(c.locales().collect::<Vec<_>>(), [Locale::EnUs, Locale::Tr]);
    //! assert_eq!(c.placeholders("shop.left"), Some(&["n"][..]));
    //! let n: &dyn std::fmt::Display = &2;
    //! assert_eq!(noctis::i18n::format("{n} left", &[("n", n)]), "2 left");
    //! ```

    pub use noctis_i18n::{Catalog, format};
}

#[cfg(feature = "prefix")]
pub mod prefix {
    //! Prefix commands (feature `prefix`, P2, FR-14): [`PrefixPlugin`] runs
    //! opted-in slash Commands from messages such as `!warn <@5> 3 rude`,
    //! through the same `#[handler]`.
    //!
    //! - **Opt-in by type.** `PrefixPlugin::new("!").command::<Warn>()`: only
    //!   opted-in Commands answer, and each must also be mounted in a Module.
    //!   The prefix is 1 to 32 Unicode scalars without whitespace, matched
    //!   case-sensitively and followed directly by the Command Path, whose
    //!   words match case-insensitively (`!WARN` runs `warn`).
    //! - **Intent.** The plugin's `MESSAGE_CREATE` handler needs the
    //!   privileged `MESSAGE_CONTENT` intent: without
    //!   `.intents(Intents::MESSAGE_CONTENT)` the build fails with `[GW-05]`
    //!   `MissingIntent`. `GUILD_MESSAGES` is derived; DMs also need
    //!   `DIRECT_MESSAGES`, requested by the bot.
    //! - **Arguments.** Positional, in option declaration order, separated
    //!   by whitespace. `"…"` makes one argument of several words (`\"`
    //!   and `\\` escape). The last non-ATTACHMENT option, when it is a
    //!   STRING, takes the rest of the line verbatim. ATTACHMENT options take
    //!   the message's attachments in order. USER, ROLE, CHANNEL and
    //!   MENTIONABLE take a mention (`<@5>`, `<@!5>`, `<@&7>`, `<#9>`) or a
    //!   bare id; INTEGER, NUMBER and BOOLEAN (`true/false/yes/no/on/off`)
    //!   are parsed; choices match by value, then by name.
    //! - **Errors.** A bad argument is the same [`OptionError`](crate::OptionError)
    //!   a slash decode produces (`WrongType`, `Missing`, `OutOfRange`,
    //!   `UnknownChoice`, `Malformed` for grammar, `Constraint` for
    //!   OPT-03/04/05/06/13 limits): the handler does not run, the user gets
    //!   a public `noctis.validation.failed` reply and the Command Error Sink
    //!   gets the error.
    //! - **Ignored.** Messages from bots or webhooks, system messages, text
    //!   without the prefix or an opted-in path, and Commands whose
    //!   `contexts` exclude the place get no answer.
    //! - **Limits.** A message carries no permission data, so a Command
    //!   declaring `permissions`, `nsfw = true` or a CHANNEL option with
    //!   `channel_types` cannot be opted in (build violation). Only users
    //!   (mentioned or the author) and attachments are resolved:
    //!   `ResolvedRole`, `ResolvedChannel` and member data are not, so such
    //!   fields fail with `Unresolved`; take ids (`Id<RoleMarker>`) instead.
    //!   No per-guild prefix, named arguments or edited messages.
    //!
    //! ```no_run
    //! use noctis::prefix::PrefixPlugin;
    //! use noctis::prelude::*;
    //! use noctis::twilight::model::gateway::Intents;
    //!
    //! #[derive(SlashCommand)]
    //! #[command("say", description = "Repeat a text")]
    //! struct Say {
    //!     #[option(description = "What to say")]
    //!     text: String,
    //! }
    //!
    //! impl Say {
    //!     #[handler]
    //!     async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
    //!         ctx.reply_text(self.text).await
    //!     }
    //! }
    //!
    //! # fn main() -> Result<(), noctis::BuildError> {
    //! let mut builder = App::builder();
    //! builder
    //!     .intents(Intents::MESSAGE_CONTENT)
    //!     .module(Module::new("fun").add::<Say>())
    //!     // `!say hello world` runs the same handler as `/say`.
    //!     .plugin(PrefixPlugin::new("!").command::<Say>());
    //! let app = builder.build()?;
    //! # let _ = app;
    //! # Ok(())
    //! # }
    //! ```

    pub use noctis_prefix::PrefixPlugin;
}

/// Paths the derive macros expand to; not public API.
#[doc(hidden)]
pub mod __private {
    pub use noctis_codec as codec;
    pub use noctis_core as core;
    pub use noctis_i18n as i18n;
    #[cfg(feature = "image")]
    pub use noctis_image as image;
    pub use noctis_ports as ports;
    pub use noctis_rules as rules;
    pub use noctis_ui as ui;
    pub use twilight_model as model;
}

pub mod ui {
    //! The Components V2 message model.
    //!
    //! Every component is its own type with by-value, chainable setters
    //! (text is `impl Into<Cow<'static, str>>`; the label, placeholder,
    //! title and description of Route components and modals are
    //! `impl Into<`[`Text`]`>`, a literal or a locale key), and serializes
    //! straight to Discord's JSON with unset fields left out:
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
    //!   [`UserSelect`] (5), [`RoleSelect`] (6), [`MentionableSelect`] (7, its
    //!   defaults [`MentionableId`]s) and [`ChannelSelect`] (8).
    //! - [`Text`]: display text that is a literal or a
    //!   [`MessageKey`](crate::rules::MessageKey); the Responder translates a
    //!   key in the response locale before validating (User Locale first for
    //!   an ephemeral send or a modal, Guild Locale first for a public one;
    //!   a key with no translation is sent as the key, with a `warn`).
    //! - Per-render overrides: [`ButtonOverrides`], [`StringSelectOverrides`],
    //!   [`UserSelectOverrides`], [`RoleSelectOverrides`],
    //!   [`MentionableSelectOverrides`] and [`ChannelSelectOverrides`]
    //!   (`.overrides(..)`), and for modals [`ModalBuilder`], [`InputField`]
    //!   and [`InputOverrides`].
    //! - [`Components`]: a message body; it numbers every component in
    //!   pre-order from 1, and no API sets a numeric id (CV2-14).
    //! - [`ContainerComponent`] and [`TopLevelComponent`]: any component of a
    //!   Container or of the top level, for lists built at runtime.
    //! - [`RawComponent`]: the escape hatch for a component type Noctis does
    //!   not model (FR-31, CV2-16), checked JSON sent verbatim at the top
    //!   level or in a Container; it counts [`RawComponent::count`] toward
    //!   CV2-03 and carries no `id`. [`unknown_components`] picks the
    //!   unmodelled components out of a received message's raw
    //!   `components`; `ctx.unknown_components()` gives them for a click or
    //!   a modal submit.
    //! - [`Attachment`], [`AttachmentOrigin`], [`AttachmentSet`] and
    //!   [`AttachmentRef`]: content-addressed files; [`File`] takes only an
    //!   [`AttachmentRef`], which only [`AttachmentSet::add`] returns (CV2-12);
    //!   [`Media`] takes an [`AttachmentRef`] or a URL, and an `attachment://`
    //!   name given as a URL (or a ref from another set) is checked at runtime
    //!   by ATT-05. The Responder uploads only
    //!   [`V2Message::used_attachments`] (the members the tree references),
    //!   keeps already-uploaded ones by id on an edit (MSG-08) and checks the
    //!   Upload Budget (ATT-01..03) before any request. A rendered image in a
    //!   [`Thumbnail`] or [`MediaItem`] needs that component's own `.alt(..)`
    //!   ([`AltText`], `alt!`): an ATT-06 error in Strict mode, a warning in
    //!   Lenient mode (FR-30).
    //! - [`UiError`]: a bad attachment extension, a mismatched Ref slot, an
    //!   edit of a V2 message back into a legacy one
    //!   ([`UiError::V2ToLegacy`]), or a message or value over its Limits
    //!   ([`UiError::Limits`]).
    //! - User text: [`escape_markdown`] (backslash-escapes `\ * _ ~` `` ` ``
    //!   `| [ ]` everywhere, and `# > - +` and a numbered list's `.` at a line
    //!   start) and [`escape_mentions`] (a U+200B after `@` before
    //!   `everyone`/`here`, and after `<` before `@ # /`). Noctis never
    //!   escapes on its own; call them on names, reasons and bios before they
    //!   go into a [`TextDisplay`], once each, in either order. A message
    //!   pings nobody unless [`V2Message::mentions`] opts in.
    //!
    //! ```
    //! use noctis::ui::{escape_markdown, escape_mentions};
    //!
    //! let name = escape_mentions(&escape_markdown("**Ana** @everyone"));
    //! assert_eq!(name, "\\*\\*Ana\\*\\* @\u{200B}everyone");
    //! ```
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
    //! [`content!`](crate::content) ([`Content`], MSG-01, 2000),
    //! [`alt!`](crate::alt) ([`AltText`], ATT-06, 1024) and
    //! [`rgb!`](crate::rgb) ([`Rgb`], CV2-13, `0x000000..=0xFFFFFF`). Each
    //! works in a `const`. Text known only at runtime (localized text
    //! included) goes through `Label::try_new(text, &limits)` (or
    //! `Placeholder`, `Content`, `AltText`) and `Rgb::try_from(u32)`; none
    //! of them ever truncates.
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
    //! button    := value.build() .setter(..)* | link(url = "..", label = "..") | premium(sku)
    //! select    := value.build() .setter(..)*
    //! ```
    //!
    //! `value.build()` is a component built from a kind-derived struct value
    //! (`Buy { item: 1 }.build()`); its kind is the type's, so placement is
    //! checked by trait bounds: a one-item row takes a button or a select, a
    //! row of 2 to 5 only buttons (`[CV2-06]`), an accessory only a button
    //! (`[CV2-07]`).
    //!
    //! It expands into the builders above (so the placement traits still
    //! apply to `{hole}`s and `..spread`s) and fails `cargo check` with the
    //! rule id, spanned on the offending node or literal, when the literal
    //! tree breaks CV2-03 (40 components; a hole counts 1, a spread 0),
    //! CV2-04 (4000 literal text characters; non-literal text counts 0),
    //! CV2-05, CV2-06, CV2-07, CV2-08 (literal `.label(..)`s and link URLs),
    //! CV2-09 (literal `.placeholder(..)`, `.min_values(..)`,
    //! `.max_values(..)` and array-literal `.options([..])`), CV2-10, CV2-11,
    //! CV2-13 (literal accents, Separator spacing), CV2-17 (literal media
    //! URLs) or CID-02 (two components built from the same literal value). The runtime [`validate`] still checks the built
    //! message before sending.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::rules::Limits;
    //! use noctis::ui::{ButtonStyle, TextDisplay, V2Message, validate};
    //!
    //! #[derive(Button)]
    //! #[button("t.press", defaults(style = Secondary, label = "Press"))]
    //! struct Press {
    //!     n: u8,
    //! }
    //!
    //! impl Press {
    //!     #[handler]
    //!     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
    //!         # unimplemented!()
    //!     }
    //! }
    //!
    //! let name = String::from("noctis");
    //! let footer = TextDisplay::new("-# footer");
    //! let body = components! {
    //!     container(accent = rgb!(0x5865_F2)) [
    //!         text("### Settings"),
    //!         separator(spacing = large),
    //!         section [ text("Logs"), text(name); accessory = Press { n: 1 }.build().label("Change") ],
    //!         row [ Press { n: 2 }.build().label("Save").style(ButtonStyle::Primary), link(url = "https://x.y", label = "Docs") ],
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
    //! - [`MessageBody`]: the borrowed JSON body core's Responder sends;
    //!   [`V2Message::webhook_edit_body`] is the Edit Webhook Message body,
    //!   which always clears `content`, `embeds` and `poll` (WH-03).
    //! - [`StaticMessage`]: a V2 message without interactive components, the
    //!   only message a foreign webhook sends (WH-02,
    //!   [`webhook`](crate::webhook)). It is built from [`StaticComponents`],
    //!   [`StaticContainer`], [`StaticSection`] (a [`Thumbnail`] or
    //!   [`LinkButton`] accessory) and [`StaticActionRow`] (link buttons),
    //!   whose placement traits ([`StaticTopLevel`], [`StaticContainerChild`],
    //!   [`StaticAccessory`], [`StaticRowButton`]) reject a Route button, a
    //!   Premium button or a select with `[WH-02]` at `cargo check`; a
    //!   run-time tree converts with `StaticMessage::try_from(v2)`
    //!   ([`UiError::Limits`] naming every WH-02 node, raw JSON included). It
    //!   converts into [`V2Message`]. [`IntoStaticMessage`] is what a foreign
    //!   send takes.
    //! - [`WebhookName`] (`webhook_name!`): a webhook name or `username`,
    //!   1-80 characters without `clyde` or `discord` (WH-04).
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
    //! - Budgets: [`Budget`]`::of(&fixed, &limits)` reports what the fixed
    //!   parts of a message leave of CV2-03 ([`Budget::components_left`]) and
    //!   CV2-04 ([`Budget::text_left`]), saturating at 0;
    //!   [`Budget::fits`]`(&item)` is how many copies of one Container child
    //!   fit (`min(components_left / c, text_left / t)`), and
    //!   [`Budget::fits_prefix`]`(items)` how many of a list fit in order,
    //!   stopping at the first miss (FR-62). They count only the budgets, not
    //!   per-field rules or CID-02. Measure variable fixed text at its
    //!   largest, and pass `ctx.limits()` from a handler.
    //!
    //! ```
    //! use noctis::rules::Limits;
    //! use noctis::ui::{Budget, Components, Container, Separator, TextDisplay, V2Message};
    //!
    //! let fixed = V2Message::new(Components::new().push(
    //!     Container::new().push(TextDisplay::new("## Roles")).push(Separator::small()),
    //! ));
    //! let budget = Budget::of(&fixed, &Limits::default());
    //! assert_eq!((budget.components_left(), budget.text_left()), (37, 3992));
    //! assert_eq!(budget.fits(&TextDisplay::new("x".repeat(230))), 17);
    //! ```
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
    //!   `#[derive(Modal)]` struct (`Modal::from(x)`, `X { .. }.build()` or
    //!   `X::blank(..)`, through a [`ModalBuilder`]), so its `custom_id` is
    //!   the struct's Route encoding (MOD-08); push 1 to 5
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
    //! use noctis::ui::{Modal, ModalLabel, ResolvedAttachment, TextDisplay, TextInput, TextInputStyle, validate};
    //!
    //! #[derive(Choices, Clone, Copy, PartialEq, Eq, Debug)]
    //! enum Severity {
    //!     #[choice("Low", value = "l")]
    //!     Low,
    //!     #[choice("High", value = "h")]
    //!     High,
    //! }
    //!
    //! #[derive(Modal)]
    //! #[modal("mod.report", defaults(title = "Report a user"))]
    //! struct Report {
    //!     id: u64,
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
    //! impl Report {
    //!     #[handler]
    //!     async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
    //!         # unimplemented!()
    //!     }
    //! }
    //!
    //! // `severity` has no empty value, so `blank` is a `ModalBuilder`.
    //! let mut modal = Modal::from(Report::blank(7));
    //! assert_eq!(validate(&mut modal, &Limits::default()).unwrap().components, 9);
    //! let json = serde_json::to_value(&modal).unwrap();
    //! assert_eq!(json["custom_id"], "0mod.report:7");
    //! assert_eq!(json["components"][1]["component"]["type"], 4);
    //! # let _ = (ModalLabel::new("x", TextInput::new("x", TextInputStyle::Short)), TextDisplay::new("x"));
    //! ```
    //!
    //! # Modal forms
    //!
    //! `#[derive(Modal)]` (in the [`prelude`](crate::prelude)) declares a
    //! modal as a struct that is its own Route: `#[modal("id", <codec args>,
    //! defaults(title = "…"))]` takes the id, the codec arguments of a
    //! component struct and the mandatory literal title (1 to 45 characters,
    //! MOD-01). Fields without an input attribute are the Payload, encoded in
    //! the `custom_id` and length-proven like a component's (CID-01). Every
    //! other field is one top-level component, in declaration order; its
    //! name is the input key (`custom_id`, `r#` stripped) and it carries
    //! exactly one attribute whose first literal is the Label text (every
    //! Label attribute also takes `description = "…"`). A modal struct is
    //! its submitted data, not its layout: `X { .. }.build()` returns a
    //! [`ModalBuilder`] whose inputs are pre-filled from the field values
    //! (FR-63), and `X::from_json(bytes)` ([`ModalForm`]) decodes the raw
    //! modal-submit interaction JSON, Payload included. A
    //! `#[handler]` method `(self, ctx: ModalSubmitCtx[, r: ModalResponder])
    //! -> HandlerResult<R>` handles the submission: a modal submit whose
    //! `custom_id` names the Route runs it on the decoded struct (one handler
    //! per Route id, CID-04). Its declared response is `update`: a returned
    //! message updates the message the modal was opened from (a new public
    //! message for a slash-opened modal), and `ModalResponder::update` of a
    //! slash-opened modal is `ResponderError::UpdateUnavailable`.
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
    //! - Pre-fill: a non-empty Text Input value becomes its `value`; a chosen
    //!   `Choices` option of a String Select, Radio Group or Checkbox Group is
    //!   marked `default`; picked users, roles, mentionables and channels
    //!   become `default_values`; a `true` Checkbox starts checked. `None`,
    //!   `""`, `vec![]` and `false` send nothing; File Uploads and Text
    //!   Displays take none. Pre-fill never changes the `custom_id` or the
    //!   decode.
    //! - `X::blank(<Payload fields in order>)` returns `Self` with empty
    //!   inputs when every input field type is syntactically `Option<_>`,
    //!   `Vec<_>`, `String`, `bool` or `()`, so
    //!   `X { bio: Some(v), ..X::blank(user) }.build()` pre-fills what it
    //!   names; otherwise (a required `Choices` enum or `Resolved*` field, or
    //!   a type alias) it returns the empty [`ModalBuilder`]. `impl From<X>
    //!   for Modal` makes `show_modal(X::blank(p))` read the same either way.
    //! - On the [`ModalBuilder`], for one render: `.title(..)` (MOD-01),
    //!   `.value(X::FIELD, v)` through the generated typed handle
    //!   [`InputField`]`<X, T>` (one `X::<FIELD>` const per input but File
    //!   Uploads and Text Displays, `T` the field type), and
    //!   `.overrides(XOverrides { title, <input>: `[`InputOverrides`]` { label,
    //!   description, placeholder }, .. })`, the generated per-modal struct
    //!   (presentation only; each `Some` replaces, later calls win). An
    //!   input named `title`, or one whose handle would shadow a derived
    //!   const (`ID`, `SHAPE`, `SCHEMA`, `DEFAULTS`, `RUNTIME`, `ROUTE`,
    //!   `RESPONSE`, `GUARDED`), is a compile error.
    //! - Pre-fill limits are validator rules checked before sending, in
    //!   every mode: a value over 4000 or over its own `max_length`
    //!   (MOD-05), more defaults than `max_values` (CV2-09; MOD-07 for a
    //!   Checkbox Group) or two defaults in a Radio Group (MOD-07).
    //! - Every literal is checked at `cargo check` against the
    //!   [`rules::modal`](crate::rules::modal) rules (MOD-01, MOD-02, MOD-04..07,
    //!   CV2-09, CID-01 for keys, CV2-04 for the Text Displays). Input
    //!   attribute texts are literals only; localize them per render with a
    //!   [`Text`] key in [`InputOverrides`]. `defaults(title = keys::T)` is
    //!   a locale key, resolved and checked at run time. Option counts come from
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
    //!   (`[MOD-09]`), never a panic. The Payload is decoded from the
    //!   `custom_id` with the struct's Route: a `custom_id` of another Route
    //!   or a malformed one is [`SubmitError::Malformed`].
    //! - A [`ResolvedAttachment`]'s URL is ephemeral: download it promptly with
    //!   [`ResolvedAttachment::download`] through a [`FileFetcher`], which
    //!   checks the CDN host, `Limits::attachment_bytes` and the length
    //!   ([`DownloadError`]), and re-check its content type.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{ResolvedAttachment, SubmitError};
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
    //! #[modal("mod.report", defaults(title = "Report a user"))]
    //! struct ReportForm {
    //!     id: u64,
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
    //! impl ReportForm {
    //!     #[handler]
    //!     async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
    //!         # unimplemented!()
    //!     }
    //! }
    //!
    //! let modal = noctis::ui::Modal::from(ReportForm::blank(7));
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
    //! assert_eq!(form.id, 7);
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
    //! #[derive(Modal)]
    //! #[modal("mod.form", defaults(title = "T"))]
    //! struct F {
    //!     #[checkbox("Agree")]
    //!     agree: String, // a Checkbox is `bool`
    //! }
    //!
    //! impl F {
    //!     #[handler]
    //!     async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
    //!         # unimplemented!()
    //!     }
    //! }
    //! ```
    //!
    //! So does a modal without a `#[handler]` method (`[CID-04]`):
    //!
    //! ```compile_fail
    //! use noctis::prelude::*;
    //!
    //! #[derive(Modal)]
    //! #[modal("mod.form", defaults(title = "T"))]
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
    //! A [`StringSelectBuilder`] (a String Select whose struct declares no
    //! options) is placed nowhere until `.options(..)` makes it a
    //! [`StringSelect`] (`[CV2-09]`).
    //!
    //! # Components from kind-derived structs
    //!
    //! Interactive components are built only from a kind-derived struct
    //! value: `X { .. }.build()` of a [`Button`](macro@crate::Button) or a
    //! select derive returns the component with the struct's declared
    //! `defaults(..)` and the value as its Payload; there is no string
    //! `custom_id`. Each setter overrides one field: `label`, `emoji`,
    //! `style` and `disabled` on a [`RouteButton`]; `placeholder`,
    //! `min_values`, `max_values`, `disabled`, `options` (String Select),
    //! `default_values` (option values on a String Select, ids on the
    //! others; duplicates removed), `default_users` / `default_roles` /
    //! `default_channels` and `channel_types` on the selects. `.overrides(..)`
    //! takes the kind's overrides struct ([`ButtonOverrides`],
    //! [`StringSelectOverrides`], …) and applies each `Some` field as its
    //! setter would; setters and overrides compose and the later call wins.
    //! Overrides change one render only and are validated with the message
    //! (Strict errors, Lenient truncates display text). A struct without a `#[handler]`
    //! method does not compile (`[CID-04]`). `custom_id`s that need a select
    //! nonce, signature or Ref are finalized by the Responder before sending;
    //! serializing one before is an error naming the Route.
    //!
    //! ```
    //! use noctis::prelude::*;
    //! use noctis::ui::{
    //!     ActionRow, ButtonStyle, Components, Container, LinkButton, Rgb, Section, Separator,
    //!     TextDisplay,
    //! };
    //!
    //! #[derive(Button)]
    //! #[button("shop.buy", defaults(style = Primary, label = "Buy"))]
    //! struct Buy {
    //!     item: u64,
    //! }
    //!
    //! impl Buy {
    //!     #[handler]
    //!     async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
    //!         # unimplemented!()
    //!     }
    //! }
    //!
    //! let body = Components::new().push(
    //!     Container::new()
    //!         .accent(Rgb::new(0x58, 0x65, 0xF2))
    //!         .push(Section::new([TextDisplay::new("**Sword** · 255 g")], Buy { item: 255 }.build()))
    //!         .push(Separator::small())
    //!         .push(ActionRow::buttons([LinkButton::new("https://example.com").label("Docs")])),
    //! );
    //! let json = serde_json::to_value(&body).unwrap();
    //! assert_eq!(json[0]["components"][0]["accessory"]["custom_id"], "0shop.buy:47");
    //! assert_eq!(json[0]["components"][0]["accessory"]["style"], 1);
    //!
    //! // Several fields for one render; the rest comes from `defaults(..)`.
    //! let again = Buy { item: 255 }.build().overrides(ButtonOverrides {
    //!     label: Some("Buy again".into()),
    //!     disabled: Some(true),
    //!     ..Default::default()
    //! });
    //! let json = serde_json::to_value(&again).unwrap();
    //! assert_eq!((&json["label"], &json["disabled"], &json["style"]), (&"Buy again".into(), &true.into(), &1.into()));
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
        ActionRow, AltText, Arity, Attachment, AttachmentOrigin, AttachmentRef, AttachmentSet,
        Budget, Button, ButtonOverrides, ButtonStyle, CallbackFlags, ChannelSelect,
        ChannelSelectOverrides, Checkbox, CheckboxGroup, CheckboxOptions, Choices, Components,
        Container, ContainerChild, ContainerComponent, Content, CostReport, CreateFlags,
        DownloadError, EditFlags, Emoji, File, FileFetcher, FileUpload, GalleryItems, InputField,
        InputOverrides, Label, LegacyMessage, LegacyRows, LimitWarning, LinkButton, Media,
        MediaGallery, MediaItem, Mentionable, MentionableId, MentionableSelect,
        MentionableSelectOverrides, Mentions, MessageBody, MessageFormat, Modal, ModalBuilder,
        ModalChannelSelect, ModalComponent, ModalForm, ModalInput, ModalLabel,
        ModalMentionableSelect, ModalOption, ModalRoleSelect, ModalStringSelect, ModalSubmission,
        ModalTopLevel, ModalUserSelect, Placeholder, PremiumButton, RadioGroup, RadioOptions,
        RawComponent, ResolvedAttachment, ResolvedChannel, ResolvedMessage, ResolvedRole,
        ResolvedUser, Rgb, RoleSelect, RoleSelectOverrides, Roles, RouteButton, RowButtons,
        RowItem, RowSelect, Section, SectionAccessory, SectionTexts, SelectOption, Separator,
        Spacing, StringSelect, StringSelectBuilder, StringSelectOptions, StringSelectOverrides,
        SubmitError, SubmittedValue, Text, TextDisplay, TextInput, TextInputStyle, Thumbnail,
        TopLevel, TopLevelComponent, UiError, UserSelect, UserSelectOverrides, Users, V2Message,
        Validate, cost, escape_markdown, escape_mentions, unknown_components, validate,
    };
    pub use noctis_ui::{
        IntoStaticMessage, StaticAccessory, StaticActionRow, StaticComponents, StaticContainer,
        StaticContainerChild, StaticMessage, StaticRowButton, StaticSection, StaticTopLevel,
        WebhookName,
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
    //! - [`boot_flag`]: the Boot Flag grammar ([`boot_flag::check`],
    //!   [`boot_flag::BootFlagError`], also re-exported as
    //!   [`crate::BootFlagError`]) of `flags = [..]`, `Module::flags` and
    //!   `--features`.
    //! - [`MessageKey`] and [`Translate`]: localization keys and the translation port.
    //! - [`command`]: Command Pattern expansion ([`command::expand`]), the
    //!   command name and tree rules ([`command::validate_name`],
    //!   [`command::validate_tree`], [`command::validate_merge`]), the option
    //!   rules ([`command::validate_options`], [`command::measure_options`]), the
    //!   context-menu name rule ([`command::validate_context_menu_name`]) and the
    //!   command metadata rules ([`command::validate_meta`]) and the autocomplete
    //!   response rules ([`command::validate_autocomplete`]) that
    //!   `derive(SlashCommand)` and `App::build` call.
    //! - [`component`]: Components V2 trees and legacy Action Rows
    //!   ([`component::validate_components`],
    //!   [`component::validate_legacy_components`], [`component::cost`]) and
    //!   single values such as [`component::validate_button_label`] and
    //!   [`component::validate_accent_color`] (CV2-03..13, CV2-15..17).
    //! - [`modal`]: modals ([`modal::validate_modal`]) and single modal values
    //!   (MOD-01..07).
    //! - [`message`]: legacy `content`, allowed mentions, nonce, stickers,
    //!   non-empty bodies ([`message::validate_v2_not_empty`],
    //!   [`message::validate_legacy_not_empty`]), webhook names
    //!   ([`message::validate_webhook_name`]), webhook threads
    //!   ([`message::validate_webhook_thread`] over a
    //!   [`message::WebhookThreadDecl`]) and the non-interactive components of
    //!   a foreign webhook ([`message::validate_static_components`]) (MSG-01,
    //!   MSG-03..06, MSG-09, WH-02, WH-04, WH-05).
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
    pub use noctis_rules::{attachment, boot_flag, command, component, message, modal};
}

#[cfg(feature = "testkit")]
pub mod testkit {
    //! The bot's own App on local Discord fakes, driven by simulated
    //! interactions (FR-51). Enable the `testkit` feature in
    //! `[dev-dependencies]` only.
    //!
    //! - [`TestApp`]: `TestApp::build(configure(&mut App::builder()))` builds
    //!   the bot's builder (its Modules and Plugins install as in
    //!   production) against a local HTTP and gateway fake on
    //!   `Runtime::for_test()`, and runs it. It needs a multi-thread tokio
    //!   runtime: `#[tokio::test(flavor = "multi_thread")]`.
    //! - Seeds: [`TestGuild`] (channels, roles, locale), [`TestUser`]
    //!   (name, permissions, roles); [`TestApp::DEFAULT_USER`] is always
    //!   seeded. Every id a run references must be seeded
    //!   ([`TestError::Unknown`]).
    //! - Runs: [`TestApp::command`], [`TestApp::user_command`],
    //!   [`TestApp::message_command`], [`TestApp::click`],
    //!   [`TestApp::select`], [`TestApp::submit`] and
    //!   [`TestApp::autocomplete`] return a builder ([`CommandRun`],
    //!   [`MenuRun`], [`ClickRun`], [`SelectRun`], [`SubmitRun`],
    //!   [`AutocompleteRun`]) that ends in `.run().await`. Each run is one
    //!   `INTERACTION_CREATE` through the gateway fake into the real
    //!   pipeline; it ends when the pipeline task ends.
    //! - [`Response`]: [`ResponseKind`] (a type-5 or type-6 defer followed by
    //!   an edit counts as `Reply` or `Update`), ephemerality, the final
    //!   message ([`TestMessage`]), follow-ups, a modal ([`ModalForm`]),
    //!   suggestions, built-in notices and every HTTP call ([`HttpCall`]:
    //!   its JSON, the `payload_json` of a multipart send, and its uploads
    //!   as [`RecordedFile`]s). The fake echoes attachments like Discord, so
    //!   a later click carries them and an edit keeps them by id.
    //!   Finders ([`TestComponent`]) return `Result`, so a test uses `?`.
    //!   [`WireCost`] counts the message as sent, like [`crate::ui::cost`].
    //! - Snapshots: [`Response::message_json`] and [`render_json`] mask only
    //!   the per-send parts of each `custom_id` (select nonce `NN`, Ref
    //!   `~RRRRRRRRRRR`, integrity tag `TTTTTT`), so `insta` snapshots are
    //!   stable.
    //! - [`fake`] and [`TestApp::http_fake`]: the HTTP fake behind the App;
    //!   register answers for endpoints the built-in Discord wire does not
    //!   model (Create Webhook, Execute Webhook, a raw REST call) with
    //!   `app.http_fake().route(Method::POST, "/channels/5/webhooks",
    //!   FakeResponse::json(StatusCode::OK, &body))` or `route_with`; a
    //!   registered route wins, an unknown path is Discord's `404`.
    //! - [`assert_memory_flat`]: live heap stays within
    //!   [`MEMORY_FLAT_SLACK_BYTES`] over N runs (NFR-2). It needs the
    //!   [`dhat`] allocator in its own test binary with one `#[test]`.
    //!
    //! ```no_run
    //! use noctis::prelude::*;
    //! use noctis::testkit::{ResponseKind, TestApp, TestError, TestGuild, TestUser};
    //! use noctis::twilight::model::id::Id;
    //!
    //! fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
    //!     builder // .module(..) mounts the bot's Modules
    //! }
    //!
    //! # async fn flow() -> Result<(), TestError> {
    //! let app = TestApp::build(configure(&mut App::builder()))
    //!     .await?
    //!     .guild(TestGuild::new(Id::new(10), "Guild").text_channel(Id::new(20), "logs"))
    //!     .user(TestUser::new(Id::new(40)));
    //! let step1 = app.command("setup start").by(Id::new(40)).in_guild(Id::new(10)).run().await?;
    //! assert_eq!(step1.kind(), ResponseKind::Reply);
    //! let next = app.click(step1.button("Next")?).run().await?;
    //! assert!(!next.button("Back")?.is_disabled());
    //! app.stop().await
    //! # }
    //! ```

    pub mod fake {
        //! The HTTP fake behind a [`TestApp`](super::TestApp)
        //! ([`TestApp::http_fake`](super::TestApp::http_fake)): register
        //! answers for endpoints the built-in Discord wire does not model
        //! with [`FakeDiscordHttp::route`] / [`FakeDiscordHttp::route_with`],
        //! and read what the App sent as [`RecordedRequest`]s.

        pub use noctis_testkit::fake::{
            FakeDiscordHttp, FakeResponse, GatewayBot, HeaderMap, HeaderName, HeaderValue,
            HttpProtocol, Method, RecordedHttpConnection, RecordedRequest, SessionStartLimit,
            StatusCode,
        };
    }
    pub use noctis_testkit::{
        AutocompleteRun, ClickRun, CommandRun, HttpCall, MEMORY_FLAT_SLACK_BYTES, MemoryReport,
        MenuRun, ModalForm, RecordedFile, Response, ResponseKind, SelectRun, SubmitRun, TestApp,
        TestComponent, TestError, TestGuild, TestMessage, TestOption, TestUser, WireCost,
        assert_memory_flat, dhat, render_json,
    };
}
