# Modules, events, Middleware, Error Sinks, rate limits

Covers: `Module` composition, Boot Flags, runtime enable/disable, gateway Event
Handlers, Middleware with typed Extensions, Error Sinks, declarative and dynamic rate
limits. FR-5, FR-6, FR-20, FR-22, FR-44..47, FR-73; AD-7, AD-23.

## 1. Modules

```rust
pub fn module() -> Module {
    Module::new("profile")                 // path-like id, unique per App: "a/b/c"
        .add(profile())                    // #[command]
        .add(show_profile())               // #[user_command]
        .add(open_profile())               // #[handler]
        .add(award_xp())                   // #[event]
        .template::<ProfileCard>()         // image Templates
        .locales(profile_strings::catalog())   // optional Module-scoped strings
        .before(Middleware::before(load_settings).provides::<GuildSettings>())
        .module(sub_module())              // nesting
        .flags(["profiles"])               // Boot Flags (optional)
}
```

- The App is the root Module: `AppBuilder::module(..)` mounts trees. Duplicate ids fail
  App build. Every registration carries its Module id in logs, metrics and errors.
- `.add(x())` accepts any descriptor from `#[command]`, `#[user_command]`,
  `#[message_command]`, `#[handler]`, `#[autocomplete]`, `#[event]`, `#[hook]`.
- Registration is data: order doesn't matter, nothing runs at registration time. App
  build folds everything into one immutable registry (O(1) dispatch).
- The same Module tree can be mounted on several Apps.

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
use noctis::events::{MemberAdd, MessageCreate};

#[event]
async fn award_xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> { … }

#[event(ordered)]                     // runs before concurrent handlers, in per-guild order
async fn audit_join(ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> { … }
```

- `noctis::events` re-exports twilight's event payload types (`MessageCreate`,
  `MemberAdd`, `GuildCreate`, …). Never handle `InteractionCreate` yourself: commands
  and Routes cover every interaction.
- Registering a handler is what makes the App request that event's intent and
  deserialize it; unhandled event types are never parsed. A privileged intent
  (`MemberAdd` → `GUILD_MEMBERS`) must also be enabled in the Developer Portal.
- Several handlers per event are fine; all run. Concurrent by default.
- `EventCtx` has `guild_id()`, `guild_locale()`, `kv()`, `cache()`, `http()`,
  `modules()`; no Responder.
- Under overload, droppable events (presence, typing…) may be dropped; design event
  handlers to be idempotent and to tolerate gaps.

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
- Events run Middleware too (no Guards, no Responder).

## 6. Error Sinks (FR-22)

```rust
struct SentrySink;
impl ErrorSink for SentrySink {
    fn report(&self, report: ErrorReport<'_>) {
        // report.origin, report.module, report.route_or_command, report.error, report.extensions
    }
}

App::from_env()?
    .error_sink(Origin::Command, SentrySink)
    .error_sink(Origin::Render, SentrySink);
```

Origins: `Command`, `Component` (components and modal submits), `Autocomplete`,
`Event`, `Render`, `Hook`, `Overflow` (dropped events). Default sink logs at `error`.
Panics become error reports too. The user still sees the localized ephemeral error.

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
