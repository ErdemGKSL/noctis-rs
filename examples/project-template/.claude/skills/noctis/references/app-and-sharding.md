# App, sharding and lifecycle

Covers: `App`, `AppBuilder`, environment variables, CLI modes, Publish, sharding,
Shard Ranges and Clusters, the HTTP proxy, the gateway URL, presence and gateway commands, Lifecycle
Hooks, shutdown, `State<T>`, plugins, multiple Apps and the process `Runtime`.
Rules: GW-01..07, RL-01..04, REG-01..06. Signatures marked [planned] may change.

## 1. Build and run

```rust
use noctis::prelude::*;

#[tokio::main]                                   // multi-thread runtime is required
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    // `configure(&mut AppBuilder) -> &mut AppBuilder`; the temporary builder lives to
    // the end of the statement.
    let app = configure(&mut App::from_env()?).build()?;   // Result<App, BuildError>
    app.run().await?;                                      // Result<(), noctis::Error>
    Ok(())
}

pub fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
    builder
        .state(BotMeta { version: env!("CARGO_PKG_VERSION") })
        .cache_store(MyCacheStore::new())
}
```

`AppBuilder` is a **mutable builder** (like tokio's `runtime::Builder`): every setter
takes and returns `&mut AppBuilder`, so you can chain, or call setters one by one on a
`let mut builder`.

- `App::builder()` → empty `AppBuilder` attached to `Runtime::global()` (no token; fine
  for `TestApp`).
- `App::from_env()` → `Result<AppBuilder, noctis::Error>`: `App::builder()` plus the
  `NOCTIS_*` variables below. Setters called after it win.
- `AppBuilder::build()` installs every Plugin (§12), runs every **App-build** check and
  returns the `App` or **all** violations at once in one `BuildError`. It must run inside
  a tokio runtime (any flavor): the App's one HTTP client is created there. No
  connection is made. `build()` moves the configuration out and leaves the builder
  empty (still attached to its Runtime), so a second `build()` gives a fresh App.
- `App::run()` checks, in order: a tokio runtime is current (`StartError::NoRuntime`),
  it is multi-thread (`StartError::NotMultiThread`), a rustls crypto provider is
  installed (installs `ring`'s if none is set; `StartError::CryptoProvider`), and a token
  is set (`StartError::MissingToken`). Typed errors, never panics. It then reads
  `GET /gateway/bot`, plans the shard total and identify buckets (§4) and starts one
  task per shard. It returns an error if startup fails (see Errors). Once every shard
  has received READY it logs `all {n} shards ready` and keeps running. It returns
  `Ok(())` only when every shard has stopped, for example after a fatal close.
  Gateway events reach your `#[event]` handlers through the **worker set** (§4);
  interactions are taken first and, until commands and Routes land (Epics 6-7), traced
  and discarded. [planned: parses CLI flags, runs a CLI mode, and stops on
  Ctrl-C/SIGTERM (Story 5.8)]

The built `App` exposes: `app.runtime()` (its `Runtime`), `app.http()` (its one
`Arc<noctis::twilight::http::Client>`, the same `Arc` on every call), `app.http_timeout()`,
`app.http_proxy()` (`Option<&str>`: the normalized proxy authority, e.g. `Some("proxy:3000")`),
`app.gateway_url()` (`Option<&str>`: the normalized gateway URL, e.g. `Some("ws://127.0.0.1:3000")`),
`app.shard_total()` (`Option<u32>`: the fixed shard total, `None` = Discord's recommendation),
`app.intents()` (the requested intents plus the ones your event handlers derive),
`app.state::<T>()` (`Option<State<T>>`) and `app.has_renderer()`
(whether a Plugin or `.renderer(..)` set a `Renderer`).

### AppBuilder methods

Implemented:

| Method | Default | Notes |
| --- | --- | --- |
| `.token(Token)` | env `NOCTIS_TOKEN` | `Token::new(s)` / `s.parse::<Token>()`; redacted `Debug`/`Display`; never logged |
| `.state(T)` | — | typed shared state, read as `State<T>` (one per type; a second is `BuildViolation::DuplicateState`) |
| `.intents(Intents)` | `Intents::GUILDS` | adds to the requested set (`noctis::twilight::model::gateway::Intents`); `GUILDS` is always in it. Event handlers add their non-privileged intents themselves; request privileged ones (`GUILD_MEMBERS`, `GUILD_PRESENCES`, `MESSAGE_CONTENT`) here |
| `.module(Module)` | — | mount a Module tree (`modules-middleware-ratelimits.md` §1); trees fold in mount order (plugins included), parent before child; handler intents are derived and added (GW-05). Unique ids are checked from Story 13.1 [planned] |
| `.event_lane(capacity, overflow)` | 4096, 16384 | bound the event lane: `capacity` queued events (1..=1048576) and `overflow` non-droppable events behind a full lane (0..=4194304); else `BuildViolation::InvalidEventLane`; the last call wins; see §4 |
| `.handler_concurrency(usize)` | 256 | how many events run their handlers at once (1..=65536, else `BuildViolation::InvalidHandlerConcurrency`); interactions never wait for it; the last call wins; see §4 |
| `.http_timeout(Duration)` | 10 s | Discord HTTP request timeout, 1 ms..=300 s (else `BuildViolation::InvalidHttpTimeout`) |
| `.http_proxy(impl Into<String>)` | env `NOCTIS_HTTP_PROXY`, else none | send every Discord request through a proxy over plain HTTP, local rate limiter off; `host[:port]` or `http://host[:port]`, validated by `build()` (else `BuildViolation::InvalidHttpProxy`); the last call wins; see §5 |
| `.gateway_url(impl Into<String>)` | env `NOCTIS_GATEWAY_URL`, else Discord | connect every shard to this gateway (a gateway proxy; the testkit's `FakeGateway` is [planned, Story 8.6]) instead of `wss://gateway.discord.gg`; `ws://host[:port]` or `wss://host[:port]`, validated by `build()` (else `BuildViolation::InvalidGatewayUrl`); the last call wins; see §4 Session resume |
| `.shard_total(u32)` | env `NOCTIS_SHARD_TOTAL`, else Discord's recommendation | fix the shard total; the last call wins. `0` is `BuildViolation::InvalidShardTotal`; too small for Discord is `StartError::ShardTotalTooSmall` at `run()`; see §4 |
| `.runtime(&Runtime)` | `Runtime::global()` | attach to another process `Runtime` (§10) |
| `.current_runtime()` | — | the Runtime the App attaches to; plugins request resources from it |
| `.plugin(impl Plugin)` / `.plugin_mut::<P>()` | — | attach a plugin (one per type) / change it before `build()` (§12) |
| `.report(BuildViolation)` | — | record a violation (plugins); `build()` returns it with the others |
| `.renderer(Arc<dyn Renderer>)` | none | usually set by the image plugin |
| `.error_sink(ErrorOrigin, impl ErrorSink)` | `LogSink` (logs at `error`) | route one origin's handler errors, panics (and, for `Event`, overflow drops) to your sink; the last call per origin wins; see `modules-middleware-ratelimits.md` §6 |
| `.ref_store(..)`, `.session_store(..)`, `.identify_queue(..)`, `.module_state_store(..)`, `.rate_limit_store(..)`, `.cache_store(..)`, `.render_cache(..)`, `.asset_store(..)`, `.locale_source(..)`, `.guild_access(..)` | bounded memory defaults | each takes `impl Port`; the last call wins; see `adapters.md` |
| `.discord_cache_backend(impl DiscordCache)` | in-memory, nothing cached | replace the Discord cache's storage (e.g. one shared by a Cluster) |
| `.build()` | — | `Result<App, BuildError>` |

Planned (signatures may change):

| Method | Default | Notes |
| --- | --- | --- |
| `.shards(ShardRange)` [planned, Story 12.1] | all | this process's Shard Range; see §4 |
| `.shutdown_grace(Duration)` [planned, Story 5.8] | 10 s | drain time on shutdown |
| `.publisher(bool)` [planned, Story 6.9] | `false` | this process publishes at startup |
| `.payload_key(PayloadKey)` / `.previous_payload_key(..)` [planned, Story 7.2] | env | 32 bytes; required for `signed` Routes; identical across a Cluster |
| `.app_id("my-bot")` [planned, Story 7.4] | application id in base62 | `[a-z0-9-]{1,32}`, same on every process of a Cluster; namespaces Adapter keys |
| `.ack_deadline(Duration)` [planned] | 2000 ms | auto-defer deadline, measured from gateway receipt |
| `.ref_ttl(Duration)` [planned] | 24 h | default TTL for Refs without `ttl = …` |
| `.expired_handler(f)` [planned] | localized ephemeral notice | see `routes-payloads-refs.md` |
| `.locales(Catalog)` / `.default_locale(Locale)` [planned] | — / `Locale::EnUs` (`en-US`) | see `i18n.md` |
| `.locale_resolver(impl LocaleResolver)` [planned] | none | per-guild language overrides (pipeline stage 4) |
| `.theme(noctis::image::theme!())` / `.image(ImageConfig)` [planned] | none | from `AppBuilderImageExt` (`image` feature); installs the `ImagePlugin` (see `images.md`) |
| `.limits(Limits)` [planned] | `Limits::default()` | strict mode; see `limits-cheatsheet.md` |
| `.discord_cache(DiscordCacheConfig)` [planned] | only what features need | read with `ctx.cache()` |
| `.presence(Presence)` [planned] | none | initial presence |
| `.boot_flags(BootFlags)` [planned] | env/CLI | which flagged Modules exist this run |

### Errors

Every error is a `thiserror` enum (`#[non_exhaustive]`; match with a `_` arm) whose
message names the cause and a fix. No message, `Debug` or log line ever contains the
token.

- `noctis::Error`: `Env(EnvError)`, `Build(BuildError)`, `Start(StartError)` (`From` for
  each, so `?` works in `main`).
- `BuildError`: `err.violations()` (`&[BuildViolation]`, never empty, in recorded order:
  setters, plugin reports, then checks) or `err.into_violations()`. `Display` is
  `"{n} App-build violation(s)"` then one `"\n  - {violation}"` line each.
- `BuildViolation`: `NoTokioRuntime` (build outside a tokio runtime), `CryptoProvider`
  (no rustls provider could be installed), `DuplicateState { type_name }`,
  `DuplicatePlugin { type_name }`, `ResourceConflict(ResourceConflict)` (§10),
  `InvalidHttpTimeout { actual }`, `InvalidHttpProxy { reason: HttpProxyError }`
  (message `"invalid HTTP proxy: {reason}"`, never the value), `InvalidGatewayUrl {
  reason: GatewayUrlError }` (message `"invalid gateway URL: {reason}"`, never the
  value), `InvalidShardTotal` (`.shard_total(0)`), `InvalidEventLane { capacity, overflow }`
  (`.event_lane(..)` out of range), `InvalidHandlerConcurrency { actual }`,
  `MissingIntent { module, handler, event, intent }` (GW-05: an event handler needs a
  privileged intent the App did not request; message `[GW-05] event handler `{handler}`
  (Module `{module}`) handles {event}, which needs the privileged intent {NAME}: add
  `.intents(Intents::{NAME})` and enable it in the Discord Developer Portal`), `Plugin {
  plugin, message }` (built with `BuildViolation::plugin::<P>("message")`).
- `StartError` (`Copy`; each message names the cause and the fix): `NoRuntime`,
  `NotMultiThread`, `CryptoProvider`, `MissingToken` (see `App::run()` above), then:
  - `InvalidToken`: Discord rejected the token (`GET /gateway/bot` answered 401, or a
    shard was closed with 4004);
  - `GatewayInfo { status: Option<u16> }`: `GET /gateway/bot` failed (a transport
    error, a timeout, a status other than 401, or an unreadable body; `status` is set
    when Discord answered);
  - `InvalidGatewayInfo`: `GET /gateway/bot` reported 0 shards (without a fixed total)
    or a `max_concurrency` of 0;
  - `ShardTotalTooSmall { total, minimum }`: GW-03, see §4;
  - `SessionStartLimit { required, remaining, reset_after: Duration }`: GW-02, see §4;
  - `InvalidIntents { shard }` (close 4013), `DisallowedIntents { shard }` (close 4014:
    enable the privileged intents in the Developer Portal or stop requesting them),
    `FatalClose { shard, code }` (any other close code twilight does not reconnect
    on), `ShardEnded { shard }` (the shard stopped without a close code),
    `GatewayStopped` (every shard stopped before startup finished). Each one is
    returned only while startup is still running. After startup, a shard that stops
    is logged at `error` and does not stop the App.
- `EnvError`: `NotUnicode { var }`, `InvalidToken { var, source: TokenError }`,
  `InvalidShardTotal { var }` (not 1-10 ASCII digits fitting a `u32`); the value is
  never shown.
- `HttpProxyError` (why `.http_proxy(..)` was rejected; checked in this order):
  `Empty` (nothing after an optional `http://` and one trailing `/`),
  `UnsupportedScheme` (any scheme but `http://`, e.g. `https://`), `InvalidChar { index }`
  (outside printable ASCII, e.g. whitespace), `NotAnAuthority` (a path, `?`, `#`, `user@`
  or `\`), `TooLong { len }` (over 255 bytes), `InvalidHost` (empty host, more than one
  `:` without brackets, malformed `[ipv6]`), `InvalidPort` (not 1-5 digits in 1..=65535).
- `GatewayUrlError` (why `.gateway_url(..)` was rejected; checked in this order):
  `Empty` (empty, or nothing after `ws://`/`wss://` and one trailing `/`),
  `UnsupportedScheme` (no `ws://` or `wss://`, any case; e.g. `http://` or a bare
  `host:port`), then the same authority checks as `HttpProxyError`: `InvalidChar { index }`,
  `NotAnAuthority` (a path such as `/gateway`, `?`, `#`, `user@` or `\`), `TooLong { len }`,
  `InvalidHost`, `InvalidPort`. The normalized URL lowercases the scheme and drops the
  trailing `/` (host case is kept).
- `TokenError`: `Empty` (nothing after an optional `"Bot "` prefix), `InvalidChar {
  index }` (a character outside printable ASCII `!`..`~`, e.g. whitespace).

```rust
match configure(&mut App::builder()).build() {
    Ok(app) => { /* … */ }
    Err(err) => {
        for v in err.violations() {
            if let BuildViolation::DuplicateState { type_name } = v { eprintln!("{type_name}"); }
        }
    }
}
```

## Shipping one binary and the first run

A Noctis bot is meant to deploy as a single executable. Locale files are compiled in by
`i18n!`, the Template theme by `theme!()`, the default font by the `default-font`
feature; embed everything else (fonts, images, `.env.example`) with `include_bytes!` /
`include_str!`. Never read asset files at runtime; the only file a bot reads is `.env`.

The project template's `src/bootstrap.rs` shows the first-run pattern. Call
`load_or_create_env(Path::new(".env"))` before logging is set up:

- If `.env` exists, it is loaded. Variables already in the environment win.
- Otherwise `.env` is created from the embedded `.env.example`, with owner-only
  permissions and a fresh 32-byte base64 `NOCTIS_PAYLOAD_KEY`, and the bot exits so the
  owner can set `NOCTIS_TOKEN`.

Memory rules for embedded data:

- Data used once, like `.env.example`, stays a `&'static str` and is never copied to the
  heap. Static data can't be zeroized, but its file-backed pages go cold and the OS
  reclaims them.
- Every buffer built from embedded data, and every secret, is a `zeroize::Zeroizing`
  value, so it is wiped on drop.
- Data used on every interaction, such as locale strings, stays resident.

## 2. Environment variables

| Variable | Meaning |
| --- | --- |
| `NOCTIS_TOKEN` | bot token, read by `App::from_env()`: trimmed of ASCII whitespace; empty (after trim) counts as unset; an optional `Bot ` prefix is stripped. Not Unicode → `EnvError::NotUnicode`; invalid → `EnvError::InvalidToken`. `run()` without a token → `StartError::MissingToken` |
| `NOCTIS_PAYLOAD_KEY`, `NOCTIS_PAYLOAD_KEY_PREVIOUS` [planned, Story 7.2] | base64 32-byte integrity keys |
| `NOCTIS_APP_ID` [planned, Story 7.4] | Adapter namespace |
| `NOCTIS_DEV_GUILD` [planned, Story 6.9] | default for `--publish-guild` without an id |
| `NOCTIS_SHARDS` [planned, Story 12.1] | Shard Range, `start..end` (end exclusive), e.g. `32..64` |
| `NOCTIS_SHARD_TOTAL` | fixed shard total, read by `App::from_env()` after `NOCTIS_GATEWAY_URL` with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`). It must be 1-10 ASCII digits that fit a `u32`, and leading zeros are allowed (`"007"` = 7). Anything else (`-1`, `+5`, `1e3`, `1_000`, over `4294967295`) → `EnvError::InvalidShardTotal`. It is then passed to `.shard_total(..)`, so `0` fails at `build()` |
| `NOCTIS_HTTP_PROXY` | HTTP rate-limit proxy, read by `App::from_env()` after the token with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`), then passed to `.http_proxy(..)`; its content is checked by `build()` (`BuildViolation::InvalidHttpProxy`) |
| `NOCTIS_GATEWAY_URL` | gateway URL, read by `App::from_env()` after `NOCTIS_HTTP_PROXY` with the same rules, then passed to `.gateway_url(..)`; its content is checked by `build()` (`BuildViolation::InvalidGatewayUrl`) |
| `NOCTIS_PUBLISHER` [planned, Story 6.9] | `true` on the one publishing process |
| `NOCTIS_FEATURES` [planned] | Boot Flags (`a,b` or `all`), same as `--features` |
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

Sharding is automatic. `App::run()` calls `GET /gateway/bot` once, with the App's HTTP
client, so `.http_proxy(..)` applies. It uses the recommended shard count unless
`shard_total` is set. Each shard runs in its own task and identifies through the
`IdentifyQueue` port in bucket `shard_id % max_concurrency` (GW-01). Shards in
different buckets identify in parallel, and each bucket identifies once per 5 s. With
20 shards and `max_concurrency` 16, shards 0-15 identify at once and shards 16-19 about
5 s later. If you did not set `.identify_queue(..)`, the default is a
`MemoryIdentifyQueue` sized to Discord's `max_concurrency`. If you did, your port gets
every bucket. Shards connect to `.gateway_url(..)` if it is set, and to Discord's
gateway otherwise. They never use the URL that `/gateway/bot` returns. Startup is done
when every shard has received READY.

- `.shard_total(0)` is `BuildViolation::InvalidShardTotal`. At `run()`, a fixed total
  below ⌈recommended × 2 / 5⌉ is `StartError::ShardTotalTooSmall { total, minimum }`
  (Discord recommends about 1 shard per 1000 guilds, and a shard may hold 2500 guilds:
  GW-03). It is returned before any connection opens. The recommended count never
  fails this check.
- A fixed total that isn't a multiple of `max_concurrency` (when it is above 1) only
  logs a warning (`total`, `max_concurrency` fields), and startup continues.
- Startup refuses with `StartError::SessionStartLimit { required, remaining,
  reset_after }` when `session_start_limit.remaining` can't cover the shards (GW-02).
  It is returned before any connection opens.
- Each Shard is its own task: it reads, parses, filters and enqueues; it never awaits a
  handler. Interactions go to the never-dropped, unbounded **interaction lane**, and the
  App always takes interactions before any event, so a full event lane never delays
  one. Other events go to the bounded **event lane** (default 4096 events). When it is
  full, a droppable event (default set: presence updates and typing starts) evicts the
  oldest queued droppable event; any other event waits in a capped overflow (default
  16384 events) and is dropped with an `error` log beyond it. Every drop is counted in
  `noctis_events_dropped_total`.
- Every event type whose handlers are **all** `#[event(droppable)]` joins the droppable
  set; `.event_lane(capacity, overflow)` changes both bounds.
- **Worker set:** one task drains the lanes, interactions first. Each event takes one of
  `.handler_concurrency(n)` permits (default 256) before its handlers run, and gives it
  back when they have all ended; while it waits for a permit, interactions that arrive
  are still taken at once (they never take a permit). An event's `ordered` handlers run
  first, in per-guild order (64 guild-keyed sub-queues), then its other handlers run
  concurrently. Every handler runs under `catch_unwind`: an error or panic is logged at
  `error` and the shard keeps running.
- Intents and event deserialization are **derived from your handlers**: the gateway
  parses only interactions and the event types your `#[event]` handlers take, and each
  handler's non-privileged intent is added to the App's intents. A handler needing a
  privileged intent you didn't request is `BuildViolation::MissingIntent` (GW-05).
  Privileged intents (`GUILD_MEMBERS`, `MESSAGE_CONTENT`, `GUILD_PRESENCES`) also need
  enabling in the Developer Portal; close code 4014 at connect is a fatal Shard close.
  Cache-driven filters [planned, Story 12.5].

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

With `.gateway_url(..)` (or `NOCTIS_GATEWAY_URL`) set, shards connect to that URL
instead of Discord's gateway, but **resumes go to the `resume_gateway_url` that READY
names**: a gateway proxy must rewrite that field to its own URL, or resumes bypass it.
The testkit's `FakeGateway` [planned, Story 8.6] already names itself there.

## 5. HTTP

Noctis owns one twilight HTTP client per App (`app.http()` and `EventCtx::http()`
return that same `&Arc<noctis::twilight::http::Client>`, for endpoints Noctis doesn't
wrap). With
`.http_proxy(..)` (or `NOCTIS_HTTP_PROXY`) set, every request goes to
`http://{proxy}/api/v10/..` over plain HTTP and the App's local rate limiter is off (the
proxy limits). Accepted forms: `host`, `host:port`, `[ipv6]`, `[ipv6]:port`, each
optionally prefixed with `http://` (any case) and followed by one `/`; HTTPS proxies,
paths and credentials are rejected (`HttpProxyError`). Request timeout defaults to 10 s. Raw requests (FR-31) [planned]:
`ctx.http_raw().request(Method::Get, "/applications/@me", None).await?` — still rate
limited / proxied.

## 6. Presence and gateway commands [planned]

```rust
builder.presence(Presence::watching("the servers"));
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

`#[event]` handlers take exactly `(ctx: EventCtx, event: <payload>)`; read state with
`ctx.state::<T>()` (`Option<State<T>>`). Extractor parameters on `#[event]` handlers
[planned].

Parameter order: context, then Payload / options / form / target, then extractors in any
order: `State<T>` (registered with `.state(T)`, `Deref<Target = T>`), `Ext<T>` /
`Option<Ext<T>>` (from Middleware), a Responder alias. Missing `State<T>` or an
`Ext<T>` no Middleware provides is a `BuildError`. Mutable state: put interior
mutability inside `T` (`tokio::sync::RwLock`, atomics) or, better, use a port.

## 10. The process `Runtime` and multiple Apps

Every App attaches to a `Runtime`: `Runtime::global()` by default, or the one passed to
`.runtime(&runtime)`. It holds **process-scoped resources** (render pool, fonts, emoji
font, metrics recorder) shared by every App attached to it. `Runtime` is a cheap
`Clone` handle; `==` is identity. `Runtime::for_test()` is a fresh, isolated Runtime
(`runtime.is_test() == true`, never equal to `Runtime::global()`); `TestApp` uses one per
test.

A resource is a type implementing `Resource` (mostly plugins do this):

```rust
use noctis::{Resource, Runtime};

struct RenderPool { /* … */ }

impl Resource for RenderPool {
    type Config = usize;                    // PartialEq + Debug; must not hold secrets
    fn init(threads: &usize) -> Self { RenderPool { /* … */ } }
}

let pool = runtime.resource_or_init::<RenderPool>(4)?;   // Result<Arc<RenderPool>, ResourceConflict>
```

The first `resource_or_init::<T>(config)` runs `init` and **freezes** the config; a later
call with an equal config returns the same `Arc`; a different one is a
`ResourceConflict { resource, frozen, requested }` (`Debug` of both configs). Inside a
plugin, turn it into a violation with `app.report(conflict.into())`, so the second App's
`build()` fails with `BuildViolation::ResourceConflict`. Two Apps on two
`Runtime::for_test()` never conflict.

Several Apps in one process [planned]:

```rust
let runtime = Runtime::global();
let main = configure(&mut App::from_env_prefixed("MAIN")?)   // MAIN_NOCTIS_TOKEN … [planned]
    .image(image_config.clone())                              // first App freezes fonts/pool
    .build()?;
let white = configure(&mut App::from_env_prefixed("ACME")?)
    .image(image_config)                                      // same config: fine
    .guild_access(GuildAccess::allow([acme_guild]))
    .build()?;
runtime.run([main, white]).await?;                            // [planned]
```

- The same Module tree can be mounted on several Apps. Restrict a command to some Apps
  with `#[command("…", apps = ["main"])]` [planned].
- Apps share Adapters only where you opt in (`.cache_store(Shared::new(store))`);
  otherwise keys are namespaced per App (FR-71) [planned].

## 11. Observability

noctis installs no tracing subscriber and no `metrics` recorder: install your own (e.g.
`tracing-subscriber`, `metrics-exporter-prometheus`) to collect them. Never put ids or
free text in your own metric labels either.

Spans (live):

- `noctis.shard{app, shard}` around each shard task.
- `noctis.event{app, shard, guild_id, module, handler, event_type}` around each event
  handler run (the `LogSink` failure line is inside it). `guild_id` is empty for
  guildless events.
- `noctis.interaction{app, shard, guild_id, module, path}` around each interaction;
  `module` and `path` are declared but empty until command and Route handlers land
  (Epic 6) [planned].
- `noctis.render` [planned, Epic 10], `noctis.publish` [planned, Story 6.9].

Metrics (live; every name starts with `noctis_`, every metric has an `app` label, and no
label carries a user, guild, channel or message id or free text):

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `noctis_lane_depth` | gauge | `app`, `lane` (`interaction` \| `event`) | items queued in each lane |
| `noctis_event_overflow_depth` | gauge | `app` | non-droppable events waiting behind a full event lane |
| `noctis_events_dropped_total` | counter | `app`, `event_type` | events dropped by back-pressure (`event_type` is the Discord name, e.g. `TYPING_START`; gateway-only kinds are `UNKNOWN`) |
| `noctis_handler_duration_seconds` | histogram | `app`, `kind` | handler run time in seconds, for `Ok`, errors and panics |
| `noctis_handler_errors_total` | counter | `app`, `kind` | handler runs that returned `Err` or panicked |
| `noctis_shard_status` | gauge | `app`, `shard` | `1` after READY/RESUMED, `0` at start and after a disconnect or end; alert on `== 0` |

`kind` is the handler kind (`ErrorOrigin::as_str()`): only `event` is emitted today;
`command`, `component`, `autocomplete` and `render` join with their handlers. Planned:
`noctis_http_requests_total` [planned, Story 5.9], ack latency and auto-defer counts
[planned, Story 6.6], Ref lookups [planned, Story 7.5], render metrics [planned, Epic 10].

The catalog is public: `noctis::observe::METRICS` (`&[MetricInfo]`, each with `name`,
`metric_type: MetricType` (`Counter` | `Gauge` | `Histogram`), `labels` and
`description`) lists every emitted metric, so you can build dashboards from it.
`noctis::observe::describe_metrics()` registers descriptions and units (`Seconds` for
`_seconds`, `Count` for `_total`) with your recorder; `App::run` calls it, so call it
yourself only if you install the recorder later.

```rust
for m in noctis::observe::METRICS {
    println!("{} {:?} {:?}", m.name, m.metric_type, m.labels);
}
```

`app` is `"default"` until `.app_id(..)` [planned, Story 7.4] sets it.

## 12. Plugins

Optional features attach to an App through `Plugin`:

```rust
use noctis::{AppBuilder, BuildViolation, Plugin};

pub struct Greeting(pub &'static str);

pub struct GreetingPlugin { pub greeting: &'static str }

impl Plugin for GreetingPlugin {
    fn install(&self, app: &mut AppBuilder) {
        if self.greeting.is_empty() {
            app.report(BuildViolation::plugin::<Self>("greeting is empty: pass some text"));
            return;
        }
        app.state(Greeting(self.greeting));           // any public builder method
    }
}

let mut builder = App::builder();
builder.plugin(GreetingPlugin { greeting: "hi" });
builder.plugin_mut::<GreetingPlugin>().unwrap().greeting = "hello";   // before build()
let app = builder.build()?;
```

- `build()` calls each plugin's `install` **exactly once, in registration order, before
  the App-build checks**. A plugin added inside an `install` is installed after the
  current batch in the same build; `plugin_mut` cannot see plugins whose batch is running.
- One plugin per type: a second `.plugin(P)` keeps the first and records
  `BuildViolation::DuplicatePlugin`.
- `install` uses only public builder methods (`renderer`, port setters, `intents`,
  `runtime`, `current_runtime`, `report`, `state`); process resources come from
  `app.current_runtime().resource_or_init::<T>(config)` (§10). A panicking `install`
  panics `build()`.
- `ImagePlugin` (`image` feature) [planned]: installed by `.image(..)` or `.theme(..)`;
  adds the Renderer and the process-scoped render pool and fonts.
- `PrefixPlugin` (`prefix` feature, P2) [planned]: `.plugin(PrefixPlugin::new("!"))`
  mounts a `MESSAGE_CREATE` handler that runs your slash Commands from `!command`
  messages through `ctx.dispatcher()`; it requires the message-content intent.

You rarely write a plugin yourself; a Module is the unit for bot features.
