# Modules, events, Middleware, Error Sinks, rate limits

Covers: `Module` composition, Boot Flags, runtime enable/disable, gateway Event
Handlers, Middleware with typed Extensions, Error Sinks, declarative and dynamic rate
limits. FR-5, FR-6, FR-20, FR-22, FR-44..47, FR-73; AD-7, AD-23.

## 1. Modules

```rust
use noctis::prelude::*;                    // Module, #[event], EventCtx, HandlerResult, …
use noctis::BaseMeta;                      // Module::base metadata (not in the prelude)

pub fn module() -> Module {
    Module::new("profile")                 // path-like id: "a/b/c"
        .add(award_xp())                   // #[event]
        .add(profile_show())               // #[command("profile show")]
        .add(badges_list())                // #[command("profile badges list")]
        .base("profile", BaseMeta::new().description("Profile commands"))  // base command
        .group("profile badges", "Manage badges")   // subcommand group description
        .add(open_profile())               // #[handler] [planned, Epic 7]
        .template::<ProfileCard>()         // image Templates [planned]
        .locales(profile_strings::catalog())   // Module-scoped strings [planned]
        .before(Middleware::before(load_settings).provides::<GuildSettings>())   // [planned, Story 7.6]
        .module(sub_module())              // nesting
        .flags(["profiles"])               // Boot Flags [planned]
}

// main / configure:
builder.module(profile::module()).module(ops::module());
```

Implemented: `Module::new(id)`, `.add(descriptor)`, `.module(child)`, `.base(name,
BaseMeta)`, `.group(path, description)`, `.id()`, `Clone`, `Debug`, and
`AppBuilder::module(Module)`.

- `AppBuilder::module(..)` mounts a tree; the App is the root. Plugins may mount Modules
  too (in `install`); those trees fold after the ones mounted before `build()`.
- `.add(x())` takes any `Descriptor` (a sealed trait). Today that is the
  `EventHandler` an `#[event]` function returns and the `#[command]`, `#[user_command]`
  and `#[message_command]` descriptors; `#[handler]`, `#[autocomplete]` and `#[hook]`
  descriptors join in later stories [planned].
- Registration is data: nothing runs when you add or nest, and the call order of `.add`
  and `.module` does not matter. `build()` folds every mounted tree **once**, pre-order:
  a Module's own registrations in `.add` order, then each child in `.module` order
  (parent before child). Each registration records its Module id and the ancestor path.
  Adding the same descriptor twice registers (and runs) it twice.
- `.base(name, BaseMeta)` declares a base command's description and metadata,
  `.group(path, description)` a subcommand group's description (`noctis::BaseMeta`; both
  `#[must_use]`, kept in call order). Any Module may declare them for commands other
  Modules add: `build()` merges every Module's commands per scope and checks the merged
  set, failing with `BuildViolation::Command` (rule id, Module notes, scope). See
  `commands.md` §1 and §6.
- The same Module tree can be mounted on several Apps (`Module` is `Clone`); each App
  folds its own copy.
- Ids are not validated yet: unique, path-shaped ids are checked from Story 13.1
  [planned]; use `a/b/c` ids now so nothing breaks then.

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

```rust
use noctis::events::{EventCtx, MemberAdd, MessageCreate, ReactionAdd};
use noctis::prelude::*;

/// +1 XP per message. Derives GUILD_MESSAGES (not privileged, no message content).
#[event]
async fn award_xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    let Some(guild) = ctx.guild_id() else { return Ok(()) };
    let meta = ctx.state::<BotMeta>();         // Option<State<BotMeta>>
    ctx.http().create_message(msg.channel_id).content("hi").await?;   // the App's one client
    Ok(())
}

/// Runs before the concurrent handlers of the event, one event at a time per guild.
/// Needs the privileged GUILD_MEMBERS: `.intents(Intents::GUILD_MEMBERS)` + the Portal.
#[event(ordered)]
async fn audit_join(ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> { … }

#[event(droppable)]                   // this handler tolerates its event being dropped
async fn on_reaction(ctx: EventCtx, ev: ReactionAdd) -> HandlerResult<()> { … }

Module::new("profile").add(award_xp()).add(audit_join()).add(on_reaction());
```

- **Signature:** exactly `async fn name(ctx: EventCtx, event: <payload>) -> HandlerResult<()>`,
  a plain `async fn` (no `const`/`unsafe`/`extern`, no `self`, no generics or `where`),
  any visibility, doc comments and `_` patterns allowed. `#[event]` turns it into
  `fn name() -> EventHandler` for `.add(..)`. Arguments: `ordered`, `droppable`, in any
  order, each at most once (`#[event(ordered, droppable)]`). The handler's name in logs
  and errors is `module_path!()::name` (e.g. `my_bot::modules::profile::award_xp`).
- **Errors** (at `cargo check`): ``#[event]` needs an `async fn``, ``#[event]` handlers
  take `(ctx: EventCtx, event: <payload>)``, ``#[event]` handlers cannot be generic``,
  ``#[event]` handlers must be a plain `async fn``, ``unknown `#[event]` argument `x`:
  expected `ordered` or `droppable``, ``duplicate `#[event]` argument `x``. A payload
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
  one client). No Responder. Planned: `guild_locale()`, `kv()`, `cache()`,
  `modules()` [planned].
- **`HandlerError` / `HandlerResult<T>`:** `?` converts any `Error + Send + Sync +
  'static`; `HandlerError::new(e)` wraps one explicitly; `err.as_error()` returns the
  inner `&(dyn Error + Send + Sync)` to downcast. `Display`/`Debug` print the inner
  error. Localized user-facing errors (`HandlerError::user(key)`) [planned].

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

```rust
#[derive(Clone)]
pub struct GuildSettings { pub premium: bool }

async fn load_settings(ctx: &mut BeforeCtx<'_>) -> HandlerResult<Flow> {
    let Some(guild) = ctx.guild_id() else { return Ok(Flow::Continue) };
    let settings = ctx.kv().get(&format!("settings:{guild}")).await?.unwrap_or(GuildSettings { premium: false });
    ctx.insert(settings);                       // typed Extension
    Ok(Flow::Continue)
}

async fn premium_only(ctx: &mut BeforeCtx<'_>) -> HandlerResult<Flow> {
    match ctx.get::<GuildSettings>() {
        Some(s) if s.premium => Ok(Flow::Continue),
        _ => Ok(Flow::Stop(Some(keys::PREMIUM_REQUIRED))),   // localized ephemeral notice
    }
}

async fn audit(ctx: &AfterCtx<'_>, outcome: &Outcome) { tracing::info!(ok = outcome.is_ok(), "done"); }

Module::new("premium")
    .before(Middleware::before(load_settings).provides::<GuildSettings>())
    .before(Middleware::before(premium_only))
    .after(Middleware::after(audit));

#[handler]
async fn h(ctx: ComponentCtx, p: MyRoute, settings: Ext<GuildSettings>) -> HandlerResult<MyView> { … }
```

- Order: parent Module before child, then registration order; after-Middleware in
  reverse. Before-Middleware runs **before** Route Guards, then the handler (pipeline
  stages 7 → 8 → 9).
- `Flow::Stop(Option<MessageKey>)` stops processing (optionally with a notice).
- `ctx.set_locale(locale)` overrides the resolved locale.
- Inserting an Extension type that already exists replaces it (the child Module wins).
- `Ext<T>` that no before-Middleware on every path `provides::<T>()` is a `BuildError`;
  use `Option<Ext<T>>` to opt out.
- Extensions never reach a port; they live for one interaction.
- Events run Middleware too (no Guards, no Responder) [planned, Story 7.6].

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
  `Display` give `command`, `component`, `autocomplete`, `event`, `render`. Only `Event`
  reports are produced today; the other origins report once their handlers land
  (Epics 6, 7 and 10).
- **`ErrorReport<'_>`** (`#[non_exhaustive]`, `Copy`, built only by noctis): `origin`,
  `kind: ReportKind`, `app: &str` (`"default"` for now), `shard: Option<u32>`,
  `guild_id: Option<Id<GuildMarker>>`, `module: Option<&str>`, `handler: Option<&str>`
  (handler, command or Route path), `event_type: Option<&str>` (Discord name, e.g.
  `MESSAGE_DELETE`). It never carries a payload or token. Copy what you need out of it;
  it borrows.
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
  `shard`, `guild_id`, `module`, `handler`, `event_type` when known, and `error` (source
  chain, at most 16 links) or `panic`. Messages `"<origin> handler failed"`,
  `"<origin> handler panicked"`, `"<origin> handler cancelled at shutdown"` (no `error`
  or `panic` field), `"event overflow is full; event dropped"`.
- A cancelled handler also counts in `noctis_handler_errors_total{kind="event"}`.
- Hook errors (Story 13.5) will report as `Event`. The user-facing localized ephemeral error for interactions arrives with
  Epic 6 [planned].

## 7. Rate limits (FR-46)

Declarative, checked before the handler and before any auto-defer:

```rust
#[command("profile", rate_limit(user, 3, per = "10s"))]
#[handler(rate_limit(member, 1, per = "1m"))]
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
`async fn(ctx: &dyn Ctx, retry_after: Duration) -> HandlerResult<StaticViewBox>`.
