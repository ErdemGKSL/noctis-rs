# Modules, events, Middleware, Error Sinks, rate limits

Covers: `Module` composition, Boot Flags, runtime enable/disable, gateway Event
Handlers, Middleware with typed Extensions, Error Sinks, declarative and dynamic rate
limits. FR-5, FR-6, FR-20, FR-22, FR-44..47, FR-73; AD-7, AD-23.

## 1. Modules

```rust
use noctis::prelude::*;                    // Module, EventHandler, EventCtx, HandlerResult, …
use noctis::BaseMeta;                      // Module::base metadata (not in the prelude)

pub fn module() -> Module {
    Module::new("profile")                 // root id: one segment
        .add::<AwardXp>()                  // #[derive(EventHandler)]
        .add::<ProfileShow>()              // #[derive(SlashCommand)] #[command("profile show")]
        .add::<BadgesList>()               // #[derive(SlashCommand)] #[command("profile badges list")]
        .base("profile", BaseMeta::new().description("Profile commands"))  // base command
        .group("profile badges", "Manage badges")   // subcommand group description
        .add::<OpenProfile>()              // #[derive(Button)]: this Module handles its clicks
        .uses::<ToggleModule>()            // a Route its Views render, handled by another Module
        .template::<ProfileCard>()         // image Templates [planned]
        .locales(profile_strings::catalog())   // Module-scoped strings (App-wide keys)
        .before(Middleware::before(load_settings).provides::<GuildSettings>())   // Middleware (§5)
        .after(Middleware::after(audit))
        .module(Module::new("profile/badges").add::<Badge>())   // child = parent + "/" + one segment
        .flags(["profiles"])               // Boot Flags [planned]
}

// main / configure:
builder.module(profile::module()).module(ops::module());
```

Implemented: `Module::new(id)`, `.add::<X>()`, `.uses::<R>()`, `.module(child)`,
`.base(name, BaseMeta)`, `.group(path, description)`, `.before(BeforeMiddleware)`,
`.after(AfterMiddleware)` (§5), `.locales(Catalog)` (merged App-wide at build; see
`i18n.md` §5), `.id()`, `Clone`, `Debug`, and
`AppBuilder::module(Module)`. `.add::<X>()` registers a struct by type: any struct with a
kind derive (`SlashCommand`, `UserCommand`, `MessageCommand`, `EventHandler`, `Button`,
the select kinds, `Modal`) and a `#[handler]` method (the `noctis::Register` trait the
derives implement).

- `AppBuilder::module(..)` mounts a tree; the App is the root. Plugins may mount Modules
  too (in `install`); those trees fold after the ones mounted before `build()`.
- Commands, context menus, event handlers, components and modals run today, components
  and modals with their responses (see `routes-payloads-refs.md` §1 and `responder.md`
  §3b); `#[hook]` joins later [planned].
- One handler per Route id per App: add each component or modal struct to exactly one
  mounted Module, or `build()` fails with ``[CID-04] Route `…` has two handlers``.
- `.uses::<R>()` (`#[must_use]`) declares that this Module's Views build components of
  the Route `R` that another Module handles. `build()` checks it for mounted Modules:
  if no mounted Module (this one, a child, a sibling or another tree) adds `R`, it fails
  with ``[CID-04] Module `…` uses Route `…` (`path::R`), but no mounted Module handles
  it: add `.add::<path::R>()` to a mounted Module``. Modules that render only their
  own Routes need no `uses`. `#[autocomplete(field)]` methods are **not** added separately:
  `.add::<X>()` on the command registers them (`commands.md` §4).
- Registration is data: nothing runs when you add or nest, and the call order of `.add`
  and `.module` does not matter. `build()` folds every mounted tree **once**, pre-order:
  a Module's own registrations in `.add` order, then each child in `.module` order
  (parent before child). Each registration records its Module id and the ancestor path.
  Adding the same event handler twice registers (and runs) it twice; a component or
  modal struct added twice is a CID-04 build error.
- `.base(name, BaseMeta)` declares a base command's description and metadata,
  `.group(path, description)` a subcommand group's description (`noctis::BaseMeta`; both
  `#[must_use]`, kept in call order). Any Module may declare them for commands other
  Modules add: `build()` merges every Module's commands per scope and checks the merged
  set, failing with `BuildViolation::Command` (rule id, Module notes, scope). See
  `commands.md` §1 and §6.
- The same Module tree can be mounted on several Apps (`Module` is `Clone`); each App
  folds its own copy.

### Module trees (FR-44)

```rust
Module::new("profile")
    .add::<Profile>()
    .module(
        Module::new("profile/badges")          // child = parent + "/" + one segment
            .add::<Badge>()
            .module(Module::new("profile/badges/admin").add::<GrantBadge>()),
    )
```

- The id you write **is** the Module's full path; `.module(..)` never rewrites it.
  Grammar: one or more segments of `a-z`, `0-9`, `_`, `-` joined by single `/`,
  1 to 64 characters in total (no uppercase, `.`, `:`, spaces or empty segments).
- A root Module (mounted with `AppBuilder::module`, also from a plugin's `install`) has
  exactly **one** segment; a child's id is exactly `<parent id>/<one segment>`.
- Ids are **unique per App** across every mounted tree, so a tree is mounted at most
  once per App (mounting the same tree on two Apps is fine). A reusable library Module
  is a root with its own one-segment id; it cannot be re-parented without changing ids.
- `Module::new` never fails; `build()` (and `--publish --dry-run`) checks every mounted
  id and lists every problem, after the setter and plugin violations and first among
  the Registry checks:
  - ``invalid Module id `…`: <reason>`` is `BuildViolation::InvalidModuleId { id,
    reason: ModuleIdError }`. `ModuleIdError` is `Empty`, `TooLong { len }`,
    `InvalidChar { index, ch }` or `EmptySegment { index }` (grammar; character
    indexes), and, only when the grammar holds, `PathAtRoot { segments }` (a root with
    more than one segment: nest it with `.module(..)` in its parent) or
    `NotUnderParent { parent }` (rename it `<parent>/<name>`).
  - ``Module id `…` is mounted twice`` is `BuildViolation::DuplicateModule { id }`, one
    per later occurrence in fold order. Give each Module its own id or mount the tree
    once.
- Middleware runs parent before child at every depth: before-Middleware `profile`,
  `profile/badges`, `profile/badges/admin`, then the handler, then the
  after-Middleware in reverse (§5).
- The full-path id is the `module` field of the `noctis.interaction` and `noctis.event`
  spans and `ErrorReport.module` in Error Sinks (§6). No metric has a `module` label.

## 2. Boot Flags (FR-73)

```rust
Module::new("seacord").flags(["seacord"]);
#[command("beta stats", flags = ["beta"])]
```

Selected at startup with `--features seacord,beta`, `--features all`, or
`NOCTIS_FEATURES`. Unselected registrations are not registered, dispatched or
published (dry run included). Unflagged registrations always exist.

## 3. Runtime enable/disable (FR-45)

```rust
ctx.modules().set_enabled("profile", ModuleScope::Guild(guild), false).await?;
ctx.modules().set_enabled("profile", ModuleScope::Global, true).await?;
let on: bool = ctx.modules().is_enabled("profile", Some(guild));
```

Stored in the `ModuleStateStore`; every process picks changes up within the poll
interval (default 5 s). A disabled Module's handlers don't run; its global commands
answer a localized ephemeral "disabled here" (`noctis.disabled_here`). Hook:
`ModuleToggled`.

## 4. Gateway Event Handlers (FR-6)

An event handler is a unit struct, like every handler:

```rust
use noctis::events::{EventCtx, MemberAdd, MessageCreate, ReactionAdd};
use noctis::prelude::*;

/// +1 XP per message. Derives GUILD_MESSAGES (not privileged, no message content).
#[derive(EventHandler)]
#[event]
pub struct AwardXp;

impl AwardXp {
    #[handler]
    async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
        let Some(guild) = ctx.guild_id() else { return Ok(()) };
        let meta = ctx.state::<BotMeta>();         // Option<State<BotMeta>>
        ctx.http().create_message(msg.channel_id).content("hi").await?;   // the App's one client
        Ok(())
    }
}

/// Runs before the concurrent handlers of the event, one event at a time per guild.
/// Needs the privileged GUILD_MEMBERS: `.intents(Intents::GUILD_MEMBERS)` + the Portal.
#[derive(EventHandler)]
#[event(ordered)]
pub struct AuditJoin;
impl AuditJoin {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> { … }
}

#[derive(EventHandler)]
#[event(droppable)]                   // this handler tolerates its event being dropped
pub struct OnReaction;
impl OnReaction {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: ReactionAdd) -> HandlerResult<()> { … }
}

Module::new("profile").add::<AwardXp>().add::<AuditJoin>().add::<OnReaction>();
```

- **Signature:** a `#[derive(EventHandler)]` unit struct with `#[event]` and exactly one
  `#[handler] async fn handle(self, ctx: EventCtx, event: <payload>[, Ext<T> |
  Option<Ext<T>> …]) -> HandlerResult<()>` method (Extensions from Middleware, §5) (any visibility, doc comments and `_` patterns allowed);
  `X::descriptor()` is its `EventHandler`. Arguments: `ordered`, `droppable`, in any
  order, each at most once (`#[event(ordered, droppable)]`). The handler's name in logs
  and errors is `module_path!()::Struct` (e.g. `my_bot::modules::profile::AwardXp`).
- **Errors** (at `cargo check`): `` `#[derive(EventHandler)]` needs a unit struct ``,
  the `#[handler]` errors (`commands.md` §1: `self` by value, `async`, no generics,
  `` a #[handler] method takes `(self, ctx[, path | target | event][, Ext<T> | Option<Ext<T>> …][, r: <Kind>Responder<..>])` ``),
  ``unknown `#[event]` argument `x`: expected `ordered` or `droppable``, ``duplicate
  `#[event]` argument `x``; a free `#[event] async fn` is a compile error whose help
  shows the struct form. A payload
  type outside `noctis::events` (or `InteractionCreate`) is
  "`T` is not a gateway event payload" (trait `EventPayload`, sealed).
- **Payloads:** `noctis::events` re-exports twilight's payload types (table below).
  Never handle interactions here: commands and Routes cover every interaction.
- **Only handled events are parsed:** the gateway deserializes interactions plus the
  event types your handlers take; everything else is skipped before parsing.
- **Intents are derived:** each handler's non-privileged intent is added to the App
  (`app.intents()` = what you requested ∪ derived; with no handlers it is `GUILDS`). A
  handler needing a privileged intent (`GUILD_MEMBERS`, `GUILD_PRESENCES`) you did not
  request with `.intents(..)` fails `build()` with `BuildViolation::MissingIntent {
  module, handler, event, intent }`: `[GW-05] event handler `…` (Module `…`) handles
  GUILD_MEMBER_ADD, which needs the privileged intent GUILD_MEMBERS: add
  `.intents(Intents::GUILD_MEMBERS)` and enable it in the Discord Developer Portal`.
  `MessageCreate` never adds `MESSAGE_CONTENT` or `DIRECT_MESSAGES`: request those
  yourself (DMs and message text). Only guild intents are derived: `DIRECT_MESSAGES`,
  `DIRECT_MESSAGE_REACTIONS` and `DIRECT_MESSAGE_TYPING` are never added
  automatically; request them with `.intents(..)` to receive DM messages, DM
  reactions or DM typing.
- **Several handlers per event all run.** `ordered` handlers run first, sequentially in
  fold order, and in **per-guild order**: events of one guild (guild id hashed onto 64
  sub-queues; guildless events share one) run their ordered handlers one event at a
  time, in arrival order. Then every non-ordered handler of the event runs
  concurrently. Each handler gets its own copy of the payload.
- **Ordered hang hazard:** a hung `ordered` handler blocks its whole sub-queue (every
  guild that hashes to it, and all guildless events in sub-queue 0), and the events
  queued behind it keep their worker permits, so it can eventually hold all
  `handler_concurrency` permits and stop event handling. noctis adds no timeout: bound
  slow work in ordered handlers yourself, e.g. with `tokio::time::timeout`.
- **Bounded:** at most `.handler_concurrency(n)` events (default 256) run their handlers
  at once; further events wait in the event lane. Interactions never wait for this
  bound.
- **Errors and panics:** an `Err` or a caught panic goes to the `ErrorOrigin::Event`
  Error Sink (§6). The default `LogSink` logs at `error` (target `noctis::event`, fields
  `app`, `shard`, `guild_id` (when the event has one), `module`, `handler`, `event_type`,
  plus `error` (the error and its `source()` chain joined with `": "`) or `panic` (the
  `&str`/`String` message, else `"<non-string panic>"`); messages `event handler failed` /
  `event handler panicked`). Other handlers and the shard keep running. Each run is in
  span `noctis.event` and counted in `noctis_handler_duration_seconds` /
  `noctis_handler_errors_total` (`app-and-sharding.md` §11).
- **Droppable:** under overload the event lane drops presence updates, typing starts
  (always, whatever the flag says), and every event type whose handlers are **all**
  `droppable`; any other event waits in the overflow. Design handlers to be idempotent and to tolerate gaps.
- **`EventCtx`** (cheap `Clone`): `shard() -> u32`, `guild_id() ->
  Option<Id<GuildMarker>>`, `module() -> &'static str` (the Module the handler was
  added to), `state::<T>() -> Option<State<T>>`, `http() -> &Arc<Client>` (the App's
  one client), `dispatcher() -> &Dispatcher` (runs a registered Command and replies to
  a message, e.g. a prefix command; `responder.md` §4.1), `kv() -> Kv` (your typed
  key-value data, namespaced per App; `adapters.md` §3), `guild_locale() -> Locale`
  (the resolved Guild Locale: the guild's recorded `preferred_locale`, its sibling,
  then the default; `i18n.md` §7), `cache() -> DiscordCacheReader` (the Discord
  resource cache, already updated with this event; `adapters.md` §1.2; also on
  `BeforeCtx`/`AfterCtx`). No Responder. Planned: `modules()` [planned].
- **`HandlerError` / `HandlerResult<T>`:** `?` converts any `Error + Send + Sync +
  'static`; `HandlerError::new(e)` wraps one explicitly; `err.as_error()` returns the
  inner `&(dyn Error + Send + Sync)` to downcast. `Display`/`Debug` print the inner
  error. Localized user-facing errors: `HandlerError::user(key)` (a user condition:
  ephemeral notice + `debug` log, never an Error Sink), `.with_message(key)` (the notice
  uses `key`, the error still reaches the sink) and `user_message() ->
  Option<MessageKey>`; routing in `responder.md` §5.

### Payload types and their derived intents

| Intent (derived) | `noctis::events` payloads |
| --- | --- |
| `AUTO_MODERATION_CONFIGURATION` | `AutoModerationRuleCreate`, `AutoModerationRuleDelete`, `AutoModerationRuleUpdate` |
| `AUTO_MODERATION_EXECUTION` | `AutoModerationActionExecution` |
| `GUILD_MODERATION` | `BanAdd`, `BanRemove`, `GuildAuditLogEntryCreate` |
| `GUILDS` | `ChannelCreate`, `ChannelDelete`, `ChannelPinsUpdate`, `ChannelUpdate`, `GuildCreate`, `GuildDelete`, `GuildUpdate`, `UnavailableGuild`, `RoleCreate`, `RoleDelete`, `RoleUpdate`, `StageInstanceCreate`, `StageInstanceDelete`, `StageInstanceUpdate`, `ThreadCreate`, `ThreadDelete`, `ThreadListSync`, `ThreadMemberUpdate`, `ThreadMembersUpdate`, `ThreadUpdate` |
| `GUILD_EMOJIS_AND_STICKERS` | `GuildEmojisUpdate`, `GuildStickersUpdate` |
| `GUILD_INTEGRATIONS` | `GuildIntegrationsUpdate`, `IntegrationCreate`, `IntegrationDelete`, `IntegrationUpdate` |
| `GUILD_INVITES` | `InviteCreate`, `InviteDelete` |
| `GUILD_WEBHOOKS` | `WebhooksUpdate` |
| `GUILD_VOICE_STATES` | `VoiceStateUpdate` |
| `GUILD_SCHEDULED_EVENTS` | `GuildScheduledEventCreate`, `GuildScheduledEventDelete`, `GuildScheduledEventUpdate`, `GuildScheduledEventUserAdd`, `GuildScheduledEventUserRemove` |
| `GUILD_MEMBERS` **privileged** | `MemberAdd`, `MemberRemove`, `MemberUpdate` |
| `GUILD_PRESENCES` **privileged** | `PresenceUpdate` |
| `GUILD_MESSAGES` | `MessageCreate`, `MessageDelete`, `MessageDeleteBulk`, `MessageUpdate` |
| `GUILD_MESSAGE_POLLS` | `MessagePollVoteAdd`, `MessagePollVoteRemove` |
| `GUILD_MESSAGE_REACTIONS` | `ReactionAdd`, `ReactionRemove`, `ReactionRemoveAll`, `ReactionRemoveEmoji` |
| `GUILD_MESSAGE_TYPING` | `TypingStart` |
| none | `CommandPermissionsUpdate`, `EntitlementCreate`, `EntitlementDelete`, `EntitlementUpdate`, `MemberChunk`, `UserUpdate`, `VoiceServerUpdate` |

`UnavailableGuild` handlers run on a `GUILD_DELETE` dispatch with `unavailable: true`
(an outage); a guild the bot left or was removed from reaches only `GuildDelete`
handlers. Handling `UnavailableGuild` therefore makes the gateway parse `GUILD_DELETE`. Privileged intents also need enabling in the
Developer Portal; close code 4014 at connect is `StartError::DisallowedIntents`.

## 5. Middleware and Extensions (FR-20)

Middleware are async functions registered on a Module, by value. A **before-Middleware**
runs before the handler (pipeline stage 7) and may insert typed **Extensions** or stop
the run; an **after-Middleware** runs after it (stage 10) and sees the outcome. Every
type is in `noctis` (`BeforeCtx`, `AfterCtx`, `Ext`, `Flow` and `Middleware` also in the
prelude).

```rust
use noctis::prelude::*;                    // Middleware, BeforeCtx, AfterCtx, Flow, Ext, …
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GuildSettings { pub premium: bool }     // Send + Sync + 'static; nothing else needed

/// Loads the guild's settings once per run, for every handler below this Module.
async fn load_settings(ctx: BeforeCtx) -> HandlerResult<Flow> {
    let Some(guild) = ctx.guild_id() else {
        return Ok(Flow::Stop(Some(MessageKey::new("common.guild_only"))));  // ephemeral notice
    };
    let settings: GuildSettings = ctx.kv().get(&format!("settings:{guild}")).await?.unwrap_or_default();
    ctx.insert(settings);                          // a typed Extension
    Ok(Flow::Continue)
}

async fn premium_only(ctx: BeforeCtx) -> HandlerResult<Flow> {
    match ctx.get::<GuildSettings>() {             // Option<Ext<GuildSettings>>
        Some(s) if s.premium => Ok(Flow::Continue),
        _ => Ok(Flow::Stop(Some(MessageKey::new("premium.required")))),
    }
}

async fn audit(ctx: AfterCtx) {
    if !ctx.outcome().is_completed() {             // RunOutcome::Stopped, UserError, Failed, Panicked
        tracing::info!(handler = ctx.handler(), outcome = ?ctx.outcome(), "run did not complete");
    }
}

pub fn module() -> Module {
    Module::new("premium")
        .before(Middleware::before(load_settings).skip_events().provides::<GuildSettings>())
        .before(Middleware::before(premium_only).skip_events())
        .after(Middleware::after(audit))
        .add::<Boost>()
}

impl Boost {                                       // #[derive(Button)] #[button("premium.boost", ..)]
    #[handler]
    async fn handle(self, ctx: ComponentCtx, settings: Ext<GuildSettings>) -> HandlerResult<Responded> {
        let premium: bool = settings.premium;     // Ext<T>: Deref<Target = T>
        …
    }
}
```

- **Shapes.** `Middleware::before(f)` takes `f: Fn(BeforeCtx) -> impl Future<Output =
  HandlerResult<Flow>>` and returns a `BeforeMiddleware`; `Middleware::after(f)` takes
  `f: Fn(AfterCtx) -> impl Future<Output = ()>` and returns an `AfterMiddleware`
  (functions or `move` closures carrying configuration; `Send + Sync + 'static`, futures
  `Send`). Register them with `Module::before(m)` and `Module::after(m)` (both
  `#[must_use]`). A Middleware's name, in logs, spans and reports, is its function's
  path (`my_bot::modules::premium::load_settings`); `.name()` returns it. A closure is
  named after its enclosing function plus `::{{closure}}`, so two closures in one
  function share a name: use named functions where Middleware must be told apart in
  logs and reports.
- **`BeforeMiddleware` builders** (each `#[must_use]`): `.provides::<T>()` declares that it
  inserts `T` (repeats are kept once; the build check below reads it);
  `.events_only()` runs it only for gateway events; `.skip_events()` only for
  interactions and `Dispatcher` Commands (the default runs it for both; the last call
  wins). `AfterMiddleware` has `.events_only()`, `.skip_events()` and `.name()`. Scope an
  auth or I/O Middleware with `skip_events()`, or it runs on every `MESSAGE_CREATE`.
- **Order.** A handler runs the Middleware of its Module **path**: the root Module's
  first, then each child's, down to the Module it was added to; within one Module, in
  registration order. After-Middleware run in reverse: the own Module's last-registered
  first, the root's first-registered last. A sibling's or child's Middleware never runs:

  ```text
  root .before(r1).before(r2) .after(a1).after(a2)
   └─ child .before(c1) .after(ca1) .add::<H>()
  r1 → r2 → c1 → H → ca1 → a2 → a1
  ```

  Before-Middleware run after the auto-defer is armed (stage 6), before Route Guards
  (stage 8: a component's or modal's `owner`, `permissions` and `guard = path`,
  `routes-payloads-refs.md` §1a) and the handler (stage 9): a slow Middleware is
  deferred like a slow handler, a `Flow::Stop` wins over every Guard, and Guards
  cannot read Extensions (only the ctx accessors and `self`); a Guard denial runs
  the after-Middleware with `RunOutcome::Denied` and the run's Extensions. Commands, context menus and events have no stage 8.
- **`Flow`**: `Continue` runs the next Middleware, then the handler. `Stop(Some(key))`
  skips the rest and the handler and shows `key`'s localized ephemeral notice (after an
  auto-defer: the deferred original is deleted, then an ephemeral follow-up; under
  `response = update` only the follow-up; over the `Dispatcher` a public reply to the
  message). `Stop(None)` sends nothing (a type-5 deferred original is deleted); a
  pending interaction then shows Discord's "interaction failed", so deny with
  `Stop(Some(key))`. A stop is one `debug` on target `noctis::middleware` (`middleware`,
  `handler`, "stopped"). Events send nothing on a stop.
- **Errors.** A before-Middleware `Err` or panic is caught and handled like the
  handler's: the handler does not run, the user gets the notice (the error's key, else
  `noctis.error.generic`), a `HandlerError::user(key)` is a `debug`, anything else is
  counted and reported with `ErrorReport.middleware = Some(name)` (§6).
- **After-Middleware run for every outcome** except a shutdown cancellation: completed,
  stopped, denied by a Route Guard, user error, failed or panicked (`ctx.outcome()`), after the notice and before
  the Error Sink. An after-Middleware panic is caught and reported on its own
  (`ReportKind::Panic`, `middleware: Some(name)`, the same origin's sink, one
  `noctis_handler_errors_total`); the remaining after-Middleware still run and the run's
  own outcome is reported unchanged.
- **`BeforeCtx`** (owned, cheap `Clone`): `insert(value)`, `get::<T>() ->
  Option<Ext<T>>`, `contains::<T>()`, and the read accessors `invocation() ->
  Invocation`, `module()`, `handler()`, `shard()`, `guild_id()`, `channel_id()` and
  `user_id()` (`None` for events), `state::<T>() -> Option<State<T>>`, `kv() -> Kv`,
  `http()`. It does not see the event payload or the options. **`AfterCtx`**: the same
  accessors plus `outcome() -> RunOutcome`, `extensions() -> &Extensions`, `get::<T>()`.
- **`Invocation`** (`#[non_exhaustive]`, `Copy`): `Command { path }` (slash or
  `Dispatcher`), `ContextMenu { name }`, `Component { route }`, `Modal { route }`, `Event
  { event_type }` (`"MESSAGE_CREATE"`). **`RunOutcome`** (`#[non_exhaustive]`, `Copy`):
  `Completed`, `Stopped`, `UserError`, `Failed`, `Panicked`, `Denied` (a Route Guard
  refused the click or submit: its notice was sent and the handler did not run; a
  custom Guard's own error or panic is `UserError`/`Failed`/`Panicked` like the
  handler's); `is_completed()`.
- **Extensions** are one run's type map (`T: Send + Sync + 'static`). Inserting a type
  that exists replaces it, so a child Module's value wins, with one `debug` on target
  `noctis::middleware` ("extension replaced", naming both Middleware, never the value).
  The map freezes when the before chain ends: an insert from a clone kept in a spawned
  task is ignored with a `debug`. **`Extensions`** (read-only): `get::<T>() ->
  Option<&T>`, `contains::<T>()`, `len()`, `is_empty()`, `type_names()`; its `Debug`
  lists type names, never values.
- **Handler parameters.** Any `#[handler]` (command, context menu, component, modal,
  event) may take `Ext<T>` or `Option<Ext<T>>` parameters anywhere after `ctx`, among
  the path / target / event and before the responder: `(self, ctx: CommandCtx, path:
  ConfigPath, s: Ext<GuildSettings>, u: Option<Ext<Caller>>, r: CommandResponder)`.
  `Ext<T>` derefs to `T`; `as_ref()`, `clone()` (shares the value) and `into_arc() ->
  Arc<T>`. Errors at `cargo check`: `` `Ext` takes one type: `Ext<T>` `` (`Ext`, `Ext<A,
  B>`, `Ext<'a, A>`), `` the first parameter after `self` is the context (`ctx: …Ctx`);
  put `Ext<T>` after it ``, and `` a #[handler] method takes `(self, ctx[, path | target |
  event][, Ext<T> | Option<Ext<T>> …][, r: <Kind>Responder<..>])` ``.
- **Build check.** `build()` fails with `BuildViolation::MissingExtension { module,
  handler, extension }`, listed with every other violation, for each handler taking
  `Ext<T>` when no before-Middleware on its Module path declares `provides::<T>()` with
  a scope covering the handler (`events_only` does not cover a Command, `skip_events`
  not an event): ``handler `{handler}` (Module `{module}`) takes `Ext<{T}>`, but no
  before-Middleware on its Module path provides it: add
  `.before(Middleware::before(f).provides::<{T}>())` to Module `{module}` or an ancestor,
  or take `Option<Ext<{T}>>` ``. `Option<Ext<T>>` needs no provider.
- **At run time** a required `Ext<T>` that was declared but not inserted (the provider
  returned `Continue` without `insert`) is `ExtensionMissing` (`extension()`,
  `handler()`): ``handler `{handler}` takes `Ext<{T}>`, but no Middleware inserted it in
  this run``; the handler does not run, the user gets the generic notice and the sink
  the error. `Option<Ext<T>>` is then `None`.
- **Where nothing runs.** Autocomplete, the Expired Handler chain (`#[on_stale]`,
  `#[on_expired]`, the App's `expired_handler`), rejected clicks (malformed or unknown
  Route) and the detached test seams run no Middleware; hooks and `#[autocomplete]`
  methods cannot take `Ext<T>` (`` `#[on_stale]`/`#[on_expired]` hooks run no Middleware,
  so they take no `Ext<T>` ``, `` autocomplete runs no Middleware, so it takes no
  `Ext<T>` ``). A filtered event handler runs no Middleware either.
- **Events** run their Module path's Middleware too (no Guards, no Responder): `Ext<T>`
  after the event, `(self, ctx: EventCtx, msg: MessageCreate, s: Ext<T>)`.
- **Extensions never reach a port**: they are not serialized or stored, and live for one
  run. Each Middleware call is in span `noctis.middleware{app, middleware, phase}`
  (`phase` = `before` | `after`).
- **Locale override:** a before-Middleware's `ctx.set_user_locale(Locale)`,
  `ctx.set_guild_locale(Locale)` and `ctx.set_locale(Locale)` (both) replace the
  run's resolved locales; later Middleware, the handler, `reply_text`, notices and
  autocomplete labels see the new value. `BeforeCtx`/`AfterCtx` read them with
  `user_locale()` / `guild_locale()`. A set after the before-chain ended is ignored
  (one `debug`). See `i18n.md` §7.

## 6. Error Sinks (FR-22)

Every handler error, handler panic and event dropped by a full overflow becomes an
`ErrorReport` for the `ErrorSink` of its `ErrorOrigin`. Unset origins use `LogSink`,
which logs at `error`.

```rust
use std::sync::Arc;

use noctis::prelude::*; // ErrorSink, ErrorOrigin
use noctis::{ErrorReport, ReportKind};

struct AlertSink {
    tx: tokio::sync::mpsc::UnboundedSender<String>, // forward; never block in `report`
}

impl ErrorSink for AlertSink {
    fn report(&self, r: ErrorReport<'_>) {
        let what = match r.kind {
            ReportKind::Error(e) => format!("failed: {e}"),
            ReportKind::Panic(msg) => format!("panicked: {msg}"),
            ReportKind::Dropped => "event dropped (overflow full)".to_owned(),
            _ => "other".to_owned(), // #[non_exhaustive]
        };
        let _ = self.tx.send(format!("{} {:?} {what}", r.origin, r.handler));
    }
}

let sink = Arc::new(AlertSink { tx });
builder
    .error_sink(ErrorOrigin::Event, Arc::clone(&sink)) // an `Arc` shares one sink
    .error_sink(ErrorOrigin::Render, sink);
```

- **`ErrorOrigin`** (`#[non_exhaustive]`, `Copy`): `Command`, `Component` (component
  interactions and modal submits), `Autocomplete`, `Event` (event handlers, and events
  dropped by a full overflow), `Render`. `ErrorOrigin::ALL` lists them; `as_str()` /
  `Display` give `command`, `component`, `autocomplete`, `event`, `render`. `Event`,
  `Command` and `Autocomplete` reports are produced today; `Component` and `Render`
  report once their handlers land (Epics 7 and 10).
- **`ErrorReport<'_>`** (`#[non_exhaustive]`, `Copy`, built only by noctis): `origin`,
  `kind: ReportKind`, `app: &str` (`"default"` for now), `shard: Option<u32>`,
  `guild_id: Option<Id<GuildMarker>>`, `module: Option<&str>`, `handler: Option<&str>`
  (handler, command or Route path), `event_type: Option<&str>` (Discord name, e.g.
  `MESSAGE_DELETE`), `extensions: Option<&Extensions>` (the run's Extensions, §5: `Some`,
  possibly empty, for every handler run, stopped or failed ones included; `None` for
  overflow drops, autocomplete and the Expired Handler chain) and `middleware:
  Option<&str>` (the failing Middleware's name, when a Middleware failed or panicked).
  It never carries a payload or token. Copy what you need out of it; it borrows.
- **`ReportKind<'_>`** (`#[non_exhaustive]`): `Error(&HandlerError)`, `Panic(&str)`,
  `Dropped` (a non-droppable event dropped because the overflow was full; `module` and
  `handler` are `None`), `Cancelled` (the handler was still running, or still queued,
  when the shutdown grace period ended; `app-and-sharding.md` §8). Every handler run
  ends in exactly one outcome: Ok, `Error`, `Panic` or `Cancelled`.
- **`ErrorSink`**: `trait ErrorSink: Send + Sync + 'static { fn report(&self, report:
  ErrorReport<'_>); }`. Synchronous: it runs on the worker task (handlers) or the shard
  task (drops), so it must not block or await; spawn a task or forward to a channel for
  I/O. `Arc<S>` implements it. A panicking sink is caught and logged
  (`noctis::error_sink`, `"error sink panicked"`); it never stops a worker or shard.
- **`.error_sink(origin, impl ErrorSink)`**: the last call per origin wins; plugins may
  call it in `install`; `build()` resets the builder to `LogSink` everywhere.
- **`LogSink`** (the default, `Copy`): target `noctis::<origin>`, fields `app` plus
  `shard`, `guild_id`, `module`, `handler`, `event_type`, `middleware` when known (never
  an Extension value), and `error` (source
  chain, at most 16 links) or `panic`. Messages `"<origin> handler failed"`,
  `"<origin> handler panicked"`, `"<origin> handler cancelled at shutdown"` (no `error`
  or `panic` field), `"event overflow is full; event dropped"`.
- A cancelled handler also counts in `noctis_handler_errors_total{kind="event"}`.
- Hook errors (Story 13.5) will report as `Event`. The user-facing localized ephemeral error for interactions arrives with
  Epic 6 [planned].

## 7. Rate limits (FR-46)

Declarative, checked before the handler and before any auto-defer:

```rust
#[command("profile", rate_limit(user, 3, per = "10s"))]                       // on a SlashCommand struct
#[button("vote", rate_limit(member, 1, per = "1m"), defaults(style = Primary, label = "vote"))]  // on a component
```

Scopes: `user`, `member` (user per guild), `channel`, `guild`, `message`. Semantics: N
uses per window (not just a cooldown). A limited interaction gets an immediate
localized ephemeral notice with the time remaining (`noctis.rate_limited`). Stored in
the `RateLimitStore` (share it in a Cluster).

Dynamic (e.g. charge only on success):

```rust
ctx.rate_limits()
    .consume(RateScope::User, "bio-edit", RateLimit::new(1, Duration::from_secs(60)))
    .await?;                                   // Err(RateLimited { retry_after }) when exhausted
ctx.rate_limits().reset(RateScope::User, "bio-edit").await?;
```

Override the notice per App, Command or Route with `rate_limited = fn` [planned]:
`async fn(ctx: &dyn Ctx, retry_after: Duration) -> HandlerResult<StaticMessage>`.
