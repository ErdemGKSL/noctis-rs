# App, sharding and lifecycle

Covers: `App`, `AppBuilder`, environment variables, CLI modes, Publish, sharding,
Shard Ranges and Clusters, the HTTP proxy, presence and gateway commands, Lifecycle
Hooks, shutdown, `State<T>`, plugins, multiple Apps and the process `Runtime`.
Rules: GW-01..07, RL-01..04, REG-01..06. Signatures marked [planned] may change.

## 1. Build and run

```rust
use noctis::prelude::*;

#[tokio::main]                                   // multi-thread runtime is required
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let app = configure(App::from_env()?).build()?;   // Result<App, BuildError>
    app.run().await?;                                 // Result<(), noctis::Error>
    Ok(())
}
```

- `App::builder()` → empty `AppBuilder` (no token; fine for `TestApp`).
- `App::from_env()` → `Result<AppBuilder, noctis::Error>`: `App::builder()` plus every
  `NOCTIS_*` variable below. Code settings applied after it win.
- `AppBuilder::build()` runs every **App-build** check (merged command trees, duplicate
  Routes, Routes with no handler, missing Extensions, Locale clashes, intents) and
  returns **all** violations at once in one `BuildError`. No connection is made.
- `App::run()` checks the tokio runtime (multi-thread) and installs a rustls crypto
  provider if none is set (typed errors, never panics), parses CLI flags, then either
  runs a CLI mode or starts the shards and blocks until shutdown.

### AppBuilder methods [planned]

| Method | Default | Notes |
| --- | --- | --- |
| `.token(Token)` | env `NOCTIS_TOKEN` | `Token` has redacted `Debug`/`Display`; never logged |
| `.app_id("my-bot")` | application id in base62 | `[a-z0-9-]{1,32}`, same on every process of a Cluster; namespaces Adapter keys |
| `.payload_key(PayloadKey)` / `.previous_payload_key(..)` | env | 32 bytes; required for `signed` Routes; identical across a Cluster |
| `.module(Module)` | — | mount a Module tree (ids must be unique) |
| `.state(T)` | — | typed shared state, extracted as `State<T>` (one per type) |
| `.locales(Catalog)` / `.default_locale(Locale)` | — / `Locale::EnUs` (`en-US`) | see `i18n.md` |
| `.locale_resolver(impl LocaleResolver)` | none | per-guild language overrides (pipeline stage 4) |
| `.theme(noctis::image::theme!())` | none | Template theme tokens and keyframes; from `AppBuilderImageExt` (`image` feature), installs the `ImagePlugin` |
| `.image(ImageConfig)` | — | fonts, emoji, render pool (process-scoped, see `images.md`); same extension trait, installs the `ImagePlugin` |
| `.plugin(impl Plugin)` | — | attach a feature plugin, e.g. `noctis::prefix::PrefixPlugin::new("!")` (§12) |
| `.runtime(&Runtime)` | `Runtime::global()` | attach to another process `Runtime` (§10) |
| `.limits(Limits)` | `Limits::default()` | strict mode; see `limits-cheatsheet.md` |
| `.ack_deadline(Duration)` | 2500 ms | auto-defer deadline, measured from gateway receipt |
| `.ref_ttl(Duration)` | 24 h | default TTL for Refs without `ttl = …` |
| `.expired_handler(f)` | localized ephemeral notice | see `routes-payloads-refs.md` |
| `.error_sink(Origin, impl ErrorSink)` | log at `error` | see `modules-middleware-ratelimits.md` |
| `.ref_store(..)`, `.session_store(..)`, `.identify_queue(..)`, `.module_state_store(..)`, `.rate_limit_store(..)`, `.cache_store(..)`, `.render_cache(..)`, `.asset_store(..)`, `.locale_source(..)`, `.guild_access(..)`, `.renderer(..)` | bounded memory defaults | see `adapters.md` |
| `.discord_cache(DiscordCacheConfig)` | only what features need | `DiscordCacheConfig::none()`, `.resources([ResourceType::ROLE, …])`; read with `ctx.cache()` |
| `.discord_cache_backend(impl DiscordCache)` | in-memory (twilight-cache-inmemory) | replace the Discord cache's storage (e.g. one shared by a Cluster) |
| `.gateway_url(url)` | Discord | gateway proxy, or the testkit's `FakeGateway` |
| `.intents(Intents)` | derived | extra intents; derived ones are always added; default for interaction-only bots is `GUILDS` |
| `.shards(ShardRange)` / `.shard_total(u32)` | all / recommended | see §4 |
| `.http_proxy(url)` / `.http_timeout(Duration)` | none / 10 s | see §5 |
| `.publisher(bool)` | `false` | this process publishes at startup |
| `.presence(Presence)` | none | initial presence |
| `.shutdown_grace(Duration)` | 10 s | drain time on shutdown |
| `.boot_flags(BootFlags)` | env/CLI | which flagged Modules exist this run |
| `.build()` | — | `Result<App, BuildError>` |

## 2. Environment variables [planned]

| Variable | Meaning |
| --- | --- |
| `NOCTIS_TOKEN` | bot token (required for `build()` of a real App) |
| `NOCTIS_PAYLOAD_KEY`, `NOCTIS_PAYLOAD_KEY_PREVIOUS` | base64 32-byte integrity keys |
| `NOCTIS_APP_ID` | Adapter namespace |
| `NOCTIS_DEV_GUILD` | default for `--publish-guild` without an id |
| `NOCTIS_SHARDS` | Shard Range, `start..end` (end exclusive), e.g. `32..64` |
| `NOCTIS_SHARD_TOTAL` | fixed shard total |
| `NOCTIS_HTTP_PROXY` | rate-limit proxy URL |
| `NOCTIS_PUBLISHER` | `true` on the one publishing process |
| `NOCTIS_FEATURES` | Boot Flags (`a,b` or `all`), same as `--features` |
| `RUST_LOG` | tracing filter (Noctis uses `tracing`; you install the subscriber) |

No config file is needed. Secrets come only from the environment.

## 3. CLI modes and Publish

`App::run()` reads `std::env::args()`:

| Flag | Effect |
| --- | --- |
| *(none)* | run the bot |
| `--publish` | publish globally, then exit |
| `--publish-guild [<id>]` | publish to one guild (dev; instant) |
| `--publish-clear [--guild <id>]` | delete this App's commands |
| `--dry-run` | with a publish flag: print the full JSON and a human diff, write nothing; runs every App-build check |
| `--features a,b` / `all` | Boot Flags |
| `--no-resume` | non-resumable shutdown; clears stored sessions |

For tests or several Apps, pass arguments explicitly with
`app.run_with_args(["--publish-guild", "123"])`. To add your own flags, split first:
`let (noctis_args, mine) = noctis::cli::NoctisArgs::split(std::env::args_os());`, parse
`mine` with your parser, and call `app.run_with_args(noctis_args)`. An unknown flag
reaching `App::run` is an error.

Publish is **diff-based**: it reads the remote commands and writes nothing when equal
(REG-05: 200 creates/day/guild). It **never** runs on shard ready. In production either
run `--publish` from your deploy step, or set `publisher(true)` / `NOCTIS_PUBLISHER=true`
on exactly one process. Programmatic form [planned]:

```rust
let report = app.publish(PublishScope::Guild(guild_id)).dry_run(true).await?;
println!("{}", report.diff());
```

Dev workflow: `cargo run --release -- --publish-guild <test guild> --dry-run`, then
without `--dry-run`, then `cargo run --release`. Always build and run with `--release`.

## 4. Sharding

Automatic: startup calls `GET /gateway/bot`, uses the recommended shard count (unless
`shard_total` is set), and identifies through the `IdentifyQueue` in buckets of
`max_concurrency` (GW-01). Shards with the same bucket identify in parallel.

- A fixed total smaller than Discord requires (2500 guilds/shard, GW-03) is a
  `BuildError`. A total that isn't a multiple of `max_concurrency` warns.
- Startup refuses when `session_start_limit.remaining` can't cover the shards (GW-02).
- Each Shard is its own task: it reads, parses, filters and enqueues; it never awaits a
  handler. Interactions go to the never-dropped **interaction lane**; other events go to
  the bounded **event lane** (droppable types like presence/typing drop oldest; others
  overflow to a capped buffer; every drop is counted in `noctis_events_dropped_total`).
- Intents and event deserialization are **derived from your handlers** and cache needs.
  A handler needing an intent you didn't allow is a `BuildError` (GW-05). Privileged
  intents (`GUILD_MEMBERS`, `MESSAGE_CONTENT`, `GUILD_PRESENCES`) also need enabling in
  the Developer Portal; close code 4014 at connect is a fatal Shard close.

### Clusters (several processes)

```text
process 0: NOCTIS_SHARDS=0..32   NOCTIS_SHARD_TOTAL=128  NOCTIS_PUBLISHER=true
process 1: NOCTIS_SHARDS=32..64  NOCTIS_SHARD_TOTAL=128
...
all:       NOCTIS_HTTP_PROXY=http://proxy:3000  NOCTIS_PAYLOAD_KEY=<same>  NOCTIS_APP_ID=<same>
```

Checklist:
1. Ranges consecutive, aligned to `max_concurrency` (misaligned only warns).
2. One shared HTTP proxy (§5); without it, startup logs a warning (RL-02).
3. Shared Adapters: `RefStore`, `RateLimitStore`, `ModuleStateStore`, `IdentifyQueue`,
   `SessionStore` (per process key space is automatic), `CacheStore`. See `adapters.md`.
4. Same `NOCTIS_PAYLOAD_KEY` and `app_id` everywhere.
5. Exactly one publisher.

Handlers do not change.

### Session resume

Graceful shutdown closes each Shard resumably and writes `(session_id, seq,
resume_url)` to the `SessionStore` after the final event; a restart within Discord's
window resumes instead of re-identifying. A rejected session falls back to identify.
Events dropped under back-pressure are not replayed (at-most-once).

## 5. HTTP

Noctis owns one twilight HTTP client per App (`ctx.http()` returns
`&noctis::twilight::http::Client` for endpoints Noctis doesn't wrap). With
`http_proxy(url)` set, every request goes through the proxy and the local limiter is
off. Request timeout defaults to 10 s. Raw requests (FR-31) [planned]:
`ctx.http_raw().request(Method::Get, "/applications/@me", None).await?` — still rate
limited / proxied.

## 6. Presence and gateway commands [planned]

```rust
App::from_env()?.presence(Presence::watching("the servers"));
ctx.gateway().set_presence(Presence::playing("v2"), ShardScope::All).await?;
ctx.gateway().request_members(guild_id, MemberQuery::user_ids([id])).await?;
```

A guild outside this process's Shard Range → `GatewayError::NotInRange`; a gateway rate
limit (GW-04, 120/60 s per shard) → `GatewayError::RateLimited`.

## 7. Lifecycle Hooks

```rust
use noctis::lifecycle::{AllShardsReady, ShardFatal};

#[hook]
async fn ready(ctx: HookCtx, ev: AllShardsReady) -> HandlerResult<()> {
    tracing::info!(shards = ev.shard_count, "online");
    Ok(())
}

Module::new("ops").add(ready());
```

Events (`noctis::lifecycle`): `AppStarted`, `ShardReady`, `ShardResumed`,
`ShardDisconnected`, `ShardFatal`, `AllShardsReady`, `PublishCompleted { diff }`,
`PublishFailed`, `ModuleToggled { module, scope, enabled }`, `ShutdownStarted`. Each
fires exactly once per occurrence; a failing hook goes to the Error Sink and never
stops the App.

## 8. Shutdown

On SIGINT/SIGTERM: `ShutdownStarted` fires → shards close (resumable unless
`--no-resume`) → both lanes drain while the Responder and HTTP stay live, up to
`shutdown_grace` → handlers still running are cancelled and reported → ports dropped.
Do not spawn work that must outlive a handler; put it behind a port.

## 9. `State<T>` and handler parameters

```rust
#[handler]
async fn h(ctx: ComponentCtx, p: MyRoute, meta: State<BotMeta>, premium: Ext<Premium>)
    -> HandlerResult<MyView>
```

Parameter order: context, then Payload / options / form / target, then extractors in any
order: `State<T>` (registered with `.state(T)`, `Deref<Target = T>`), `Ext<T>` /
`Option<Ext<T>>` (from Middleware), a Responder alias. Missing `State<T>` or an
`Ext<T>` no Middleware provides is a `BuildError`. Mutable state: put interior
mutability inside `T` (`tokio::sync::RwLock`, atomics) or, better, use a port.

## 10. Multiple Apps (white-label) [planned]

```rust
let runtime = Runtime::global();                                  // process-scoped resources
let main = configure(App::from_env_prefixed("MAIN")?)              // MAIN_NOCTIS_TOKEN …
    .image(image_config.clone())                                   // first App freezes fonts/pool
    .build()?;
let white = configure(App::from_env_prefixed("ACME")?)
    .image(image_config)                                           // same config: fine
    .guild_access(GuildAccess::allow([acme_guild]))
    .build()?;
runtime.run([main, white]).await?;
```

- The same Module tree can be mounted on several Apps. Restrict a command to some Apps
  with `#[command("…", apps = ["main"])]`.
- Apps share Adapters only where you opt in (`.cache_store(Shared::new(store))`);
  otherwise keys are namespaced per App (FR-71).
- Process-scoped resources (render pool, fonts, emoji font, metrics recorder) live in
  the `Runtime` and are frozen by the first App's configuration; a conflicting later
  one is a `BuildError`. Tests use `Runtime::for_test()` (what `TestApp` does), an
  isolated Runtime per test.

## 11. Observability

Spans: `noctis.shard`, `noctis.interaction`, `noctis.event`, `noctis.render`,
`noctis.publish` (fields `app`, `shard`, `guild_id`, `module`, route/command). Metrics
(via the `metrics` facade, prefix `noctis_`, label `app`): lane depth, dropped events,
ack latency, auto-defer count, handler duration/errors, render duration/queue/rejections,
Ref lookups (hit/miss/stale/forged), shard status, HTTP status classes. Install your own
`metrics` recorder to export them.

## 12. Plugins [planned]

Optional features attach to an App through `Plugin` (`fn install(&self, app: &mut
AppBuilder)`); `build()` runs every installed plugin once, in order, before the
App-build checks.

- `ImagePlugin` (`image` feature): installed by `.image(..)` or `.theme(..)`; adds the
  Renderer and the process-scoped render pool and fonts.
- `PrefixPlugin` (`prefix` feature, P2): `.plugin(PrefixPlugin::new("!"))` mounts a
  `MESSAGE_CREATE` handler that runs your slash Commands from `!command` messages
  through `ctx.dispatcher()`; it requires the message-content intent.

You rarely write a plugin yourself; a Module is the unit for bot features.
