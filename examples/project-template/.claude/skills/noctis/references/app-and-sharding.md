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
  a tokio runtime (any flavor): the App's one HTTP client is created there, and the
  Discord HTTP edge (§5) starts serving on that runtime (a loopback listener; a runtime
  without an IO driver still builds, the edge logs `the Discord HTTP edge could not
  start` and HTTP requests then fail). No connection to Discord is made. `build()` moves the configuration out and leaves the builder
  empty (still attached to its Runtime), so a second `build()` gives a fresh App.
- `App::run()` checks, in order: a tokio runtime is current (`StartError::NoRuntime`),
  it is multi-thread (`StartError::NotMultiThread`), a rustls crypto provider is
  installed (installs `ring`'s if none is set; `StartError::CryptoProvider`), the
  Ctrl-C/SIGTERM handlers can be installed (SIGINT and SIGTERM on unix, Ctrl-C
  elsewhere; a runtime without an IO driver is `StartError::SignalHandler`, so build it
  with `enable_all()` as `#[tokio::main]` does), and a token is set
  (`StartError::MissingToken`). Typed errors, never panics. It then loads the runtime
  locale overlays of the `LocaleSource` port (one `load` per locale with a file, none
  without catalogs; a failed load or a rejected overlay returns
  `Err(noctis::Error::LocaleOverlay(..))` before any Discord request; see i18n §8),
  loads the runtime Module state once from the `ModuleStateStore` (an unreadable store
  is one warning and every Module stays enabled) and starts polling its version every
  `.module_state_poll(..)` (`modules-middleware-ratelimits.md` §3),
  runs the startup Publish when `.publisher(true)` (§3), warms one upstream
  HTTP connection (§5; at most `connect_timeout` + 6 s, a failure only warns), reads
  `GET /gateway/bot`, plans the shard total and identify buckets (§4) and starts one
  task per shard. It returns an error if startup fails (see Errors). Once every shard
  has received READY it logs `all {n} shards ready` and keeps running until Ctrl-C or
  SIGTERM, then shuts down gracefully (§8) and returns `Ok(())`. A shard that closes
  fatally after startup is logged as `shard closed fatally` with its cause and the App
  keeps running on the other shards; when **every** shard has ended, `run` runs the
  shutdown and returns `Err(noctis::Error::Run(RunError::AllShardsEnded))`, never a
  silent `Ok(())`. Gateway events reach your `#[event]` handlers through the **worker
  set** (§4); interactions are taken first and, until commands and Routes land (Epics
  6-7), traced and discarded. `run()` first parses the process arguments
  (`std::env::args_os().skip(1)`): with a CLI mode it publishes and returns without
  connecting a shard (§3); an unknown argument is `Err(noctis::Error::Cli(..))`.
- `App::run_with_args(args)` is `run()` with an explicit command line (no program
  name): `app.run_with_args(["--publish-guild", "123"])`. **Tests** start Apps with
  `app.run_with_args(std::iter::empty::<&str>())`, because under `cargo test` the
  harness passes its own flags to the process and `run()` would reject them.
- `App::run_until(shutdown)` is `run()` with your own trigger instead of the signals:
  `shutdown` is any `impl Future<Output = ()> + Send`; when it resolves (also during
  startup) the shutdown of §8 runs and `run_until` returns `Ok(())`. It installs no
  signal handler. Use it in tests and when you embed the App:

  ```rust
  let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
  let running = tokio::spawn(app.run_until(async { let _ = stopped.await; }));
  // ... later
  let _ = stop.send(());
  running.await??;          // JoinError, then noctis::Error
  ```

The built `App` exposes: `app.runtime()` (its `Runtime`), `app.http()` (its one
`Arc<noctis::twilight::http::Client>`, the same `Arc` on every call), `app.http_timeout()`,
`app.http_proxy()` (`Option<&str>`: the normalized proxy authority, e.g. `Some("proxy:3000")`),
`app.gateway_url()` (`Option<&str>`: the normalized gateway URL, e.g. `Some("ws://127.0.0.1:3000")`),
`app.shard_total()` (`Option<u32>`: the fixed shard total, `None` = Discord's recommendation),
`app.shard_range()` (`Option<std::ops::Range<u32>>`: this process's Shard Range, `None` = every shard `0..total`),
`app.intents()` (the requested intents plus the ones your event handlers derive),
`app.shutdown_grace()` (`Duration`, default 10 s),
`app.http_config()` (`HttpConfig`, §5),
`app.app_id()` (`&AppId`: `as_str()`, `Display`; §1 `.app_id`),
`app.state::<T>()` (`Option<State<T>>`), `app.limits()` (`&Limits`), `app.has_renderer()`
(whether a Plugin or `.renderer(..)` set a `Renderer`), `app.renderer()`
(`Option<&Arc<dyn Renderer>>`, the one `ctx.render` uses), `app.is_publisher()`,
`app.resume_sessions()` (`bool`, the builder value, default `true`; `--no-resume` overrides it for that run only, never through this getter),
`app.dev_guild()` (`Option<Id<GuildMarker>>`), `app.gateway()` (`GatewayHandle`) and `app.presence()` (`PresenceHandle`, both §6) and `app.publish(scope)` (§3).

### AppBuilder methods

Implemented:

| Method | Default | Notes |
| --- | --- | --- |
| `.token(Token)` | env `NOCTIS_TOKEN` | `Token::new(s)` / `s.parse::<Token>()`; redacted `Debug`/`Display`; never logged |
| `.state(T)` | — | typed shared state, read as `State<T>` (one per type; a second is `BuildViolation::DuplicateState`) |
| `.intents(Intents)` | `Intents::GUILDS` | adds to the requested set (`noctis::twilight::model::gateway::Intents`); `GUILDS` is always in it. Event handlers add their non-privileged intents themselves; request privileged ones (`GUILD_MEMBERS`, `GUILD_PRESENCES`, `MESSAGE_CONTENT`) here |
| `.module(Module)` | — | mount a Module tree (`modules-middleware-ratelimits.md` §1); trees fold in mount order (plugins included), parent before child; handler intents are derived and added (GW-05). Ids are unique per App, a root id is one segment and a child adds one (`InvalidModuleId`, `DuplicateModule`; `modules-middleware-ratelimits.md` §1, Module trees) |
| `.event_lane(capacity, overflow)` | 4096, 16384 | bound the event lane: `capacity` queued events (1..=1048576) and `overflow` non-droppable events behind a full lane (0..=4194304); else `BuildViolation::InvalidEventLane`; the last call wins; see §4 |
| `.handler_concurrency(usize)` | 256 | how many events run their handlers at once (1..=65536, else `BuildViolation::InvalidHandlerConcurrency`); interactions never wait for it; the last call wins; see §4 |
| `.http_timeout(Duration)` | 10 s | Discord HTTP request timeout, 1 ms..=300 s (else `BuildViolation::InvalidHttpTimeout`) |
| `.http(HttpConfig)` | `HttpConfig::default()` | tune the Discord HTTP edge (§5): PING interval/timeout, callback hedge, connect timeout, DNS TTL bounds, `upstream_h2c`; one `BuildViolation::InvalidHttpConfig` per out-of-range field; the last call wins. No env variable |
| `.http_proxy(impl Into<String>)` | env `NOCTIS_HTTP_PROXY`, else none | send every Discord request through a proxy over plain HTTP, local rate limiter off; `host[:port]` or `http://host[:port]`, validated by `build()` (else `BuildViolation::InvalidHttpProxy`); the last call wins; see §5 |
| `.gateway_url(impl Into<String>)` | env `NOCTIS_GATEWAY_URL`, else Discord | connect every shard to this gateway (a gateway proxy, or the testkit's `FakeGateway`) instead of `wss://gateway.discord.gg`; `ws://host[:port]` or `wss://host[:port]`, validated by `build()` (else `BuildViolation::InvalidGatewayUrl`); the last call wins; see §4 Session resume |
| `.shard_total(u32)` | env `NOCTIS_SHARD_TOTAL`, else Discord's recommendation | fix the shard total; the last call wins. `0` is `BuildViolation::InvalidShardTotal`; too small for Discord is `StartError::ShardTotalTooSmall` at `run()`; see §4 |
| `.shard_range(start..end)` | env `NOCTIS_SHARD_RANGE`, else every shard `0..total` | run only these shards in this process (a Cluster's Shard Range): a half-open `std::ops::Range<u32>`, so `32..64` runs shards 32-63; ids are global (`[id, total]`, bucket `id % max_concurrency`); needs a fixed `.shard_total(..)`, the same on every process. `build()` records `BuildViolation::EmptyShardRange { start, end }` (start not below end), `ShardRangeWithoutTotal { start, end }` (no fixed total) or `ShardRangeOutOfBounds { start, end, total }` (end above the total); a range not aligned to `max_concurrency` only warns at `run()`; the last call wins; see §4 |
| `.shutdown_grace(Duration)` | env `NOCTIS_SHUTDOWN_GRACE`, else 10 s | how long the shutdown waits for queued events and running handlers before it cancels them; `Duration::ZERO..=Duration::from_mins(15)` (else `BuildViolation::InvalidShutdownGrace`); the last call wins; see §8 |
| `.runtime(&Runtime)` | `Runtime::global()` | attach to another process `Runtime` (§10) |
| `.current_runtime()` | — | the Runtime the App attaches to; plugins request resources from it |
| `.plugin(impl Plugin)` / `.plugin_mut::<P>()` | — | attach a plugin (one per type) / change it before `build()` (§12) |
| `.report(BuildViolation)` | — | record a violation (plugins); `build()` returns it with the others |
| `.renderer(impl Renderer)` | none (`.theme(..)`/`.image(..)` set the takumi one) | replace the Renderer with your own (`noctis::image::unstable::Renderer`, outside semver; also `Arc<R>` / `Arc<dyn Renderer>`); one per App: a second one, or one together with `.theme`/`.image`, is `BuildViolation::DuplicateRenderer { first, second }`: an explicit `.renderer(..)` is always `first` and kept, the image plugin's Renderer is always `second` (plugins install in `build`), whatever the call order; see `adapters.md` §2b |
| `.error_sink(ErrorOrigin, impl ErrorSink)` | `LogSink` (logs at `error`) | route one origin's handler errors, panics (and, for `Event`, overflow drops) to your sink; the last call per origin wins; see `modules-middleware-ratelimits.md` §6 |
| `.ref_store(..)`, `.session_store(..)`, `.identify_queue(..)`, `.module_state_store(..)`, `.rate_limit_store(..)`, `.cache_store(..)`, `.render_cache(..)`, `.asset_store(..)`, `.locale_source(..)`, `.guild_access(..)` | bounded memory defaults (`.guild_access`: `AllowAllGuilds`) | each takes `impl Port`; the last call wins; see `adapters.md`; `.guild_access(GuildAccess::allow([..]))` / `deny([..])` decides which guilds the App serves (pipeline stage 3, §10) |
| `.discord_cache(DiscordCacheConfig)` | `DiscordCacheConfig::default()`: `GUILD \| CHANNEL \| ROLE` | which Discord resources are cached for `ctx.cache()` (`noctis::DiscordCacheConfig`/`DiscordCacheResources`; `none()` turns it off); adds their event types to the gateway filter and their intents; `MEMBER` needs `.intents(Intents::GUILD_MEMBERS)` (else `BuildViolation::MissingCacheIntent`, GW-05); no env variable; the last call wins; read back with `app.discord_cache()`; see `adapters.md` §1.2 |
| `.module_state_poll(Duration)` | 5 s | how often a running App polls the `ModuleStateStore`'s `version()` and reloads the runtime Module state: the delay before another process's toggle applies here; 10 ms..=1 h (else `BuildViolation::InvalidModuleStatePoll`); the last call wins; `app.module_state_poll()` reads it; see `modules-middleware-ratelimits.md` §3 |
| `.discord_cache_backend(impl DiscordCache)` | a `MemoryDiscordCache` of the `.discord_cache(..)` resources and message cap | replace the Discord cache's storage (an `Arc` shares one across the Apps of a Cluster; never namespaced); the configuration still decides what is fed and read |
| `.publisher(bool)` | env `NOCTIS_PUBLISHER`, else `false` | a normal run publishes globally once at startup, before any shard connects (never on READY); a failed Publish is one `error` log (target `noctis::publish`) and the App keeps starting; set it on exactly one process; the last call wins; see §3 |
| `.resume_sessions(bool)` | `true` | `true`: each shard resumes its stored session at start and a graceful shutdown closes with 4000 and stores it. `false`: each shard deletes its stored entry before it identifies, and the shutdown closes with 1000 and deletes it. `--no-resume` sets `false` for that run; no env variable; the last call wins; see §4 Session resume |
| `.dev_guild(Id<GuildMarker>)` | env `NOCTIS_DEV_GUILD`, else none | the guild `--publish-guild` without an id publishes to; the last call wins |
| `.payload_key(PayloadKey)` | env `NOCTIS_PAYLOAD_KEY`, else none | the key that signs and verifies `signed` Routes (`noctis::codec::PayloadKey`, 32 bytes: `PayloadKey::from_hex(..)` takes exactly **64 hex digits**, e.g. `openssl rand -hex 32`; `KeyError::Length`/`InvalidChar` otherwise, naming a length or a position, never a character); required when a `signed` Route is mounted (``BuildViolation::MissingPayloadKey``: ``[CID-06] Route `x` is `signed`, but no payload key is configured: set NOCTIS_PAYLOAD_KEY (64 hex digits) or call `.payload_key(..)` ``); identical on every process of a Cluster; never logged (`Debug` shows `payload_key: true`); the last call wins |
| `.previous_payload_key(PayloadKey)` | env `NOCTIS_PAYLOAD_KEY_PREVIOUS`, else none | the previous key during a rotation: ids it signed still verify, new ids use the current key; without a current key it is ``BuildViolation::PreviousKeyWithoutKey`` (``[CID-06] a previous payload key is set without a current one: set NOCTIS_PAYLOAD_KEY (or call `.payload_key(..)`) too``); the last call wins |
| `.ack_deadline(Duration)` | 2000 ms | auto-defer deadline, measured from gateway receipt (queueing counts): 0 ms..=2500 ms, else `BuildViolation::InvalidAckDeadline` (`ack deadline 2.501s is out of range: use a value in 0 ms..=2500 ms`); 0 always defers; no environment variable; the last call wins. Read back with `app.ack_deadline()`; see `responder.md` §2 |
| `.ref_ttl(Duration)` | 24 h | the TTL of every Ref whose Route has no `ttl = "…"`: 1 s..=365 d, else `BuildViolation::InvalidRefTtl` (`ref TTL 0ns is out of range: use a value in 1 s..=365 d`); the last call wins. Read back with `app.ref_ttl()`; see `routes-payloads-refs.md` §7 |
| `.expired_handler(f)` | the `noctis.expired` ephemeral notice | the App-level Expired Handler: `f: Fn(ExpiredCtx, Expired) -> impl Future<Output = HandlerResult<R>>` (an `async fn(ctx: ExpiredCtx, expired: Expired) -> HandlerResult<V2Message>` works), `R` a message or `Responded`; it answers a stale or Ref id of a Route without its own `#[on_stale]`/`#[on_expired]`; `ctx.route()` names the Route; no responder, always auto-deferred, the returned message follows the Route's declared `response`; the last call wins; see `routes-payloads-refs.md` §8 |
| `.app_id(impl Into<String>)` | env `NOCTIS_APP_ID`, else the bot user id read from the token in lowercase base36 (≤ 13 chars; equal to the application id for most bots, call `.app_id` explicitly when it differs), else `default` (with one `warn!` when a token was given but does not decode) | `[a-z0-9-]{1,32}` (`AppId`; not trimmed), validated by `build()` (else `BuildViolation::InvalidAppId { reason: AppIdError }`, listed with every other violation); the last call wins; the same on every process of a Cluster. It is the `app` label of spans, metrics and reports and prefixes every byte-store key with `<app_id>:<port>:` (`adapters.md` §6); read it with `app.app_id()`. The effective id also selects `apps = [..]` / `Module::apps` registrations before the fold (§10) |
| `.build()` | — | `Result<App, BuildError>` |

Planned (signatures may change):

| Method | Default | Notes |
| --- | --- | --- |
| `.locales(Catalog)` | none: available `{en-US}`, default `en-US` | the first catalog's `default` is the App default; see `i18n.md` |
| `.locale_resolver(impl LocaleResolver)` | none | sync `Fn(&LocaleRequest) -> Locales`, called once per run at pipeline stage 4, result used verbatim; a panic keeps the built-in resolution; see `i18n.md` §7 |
| `.theme(noctis::image::theme!())` / `.image(ImageConfig)` | none | from `AppBuilderImageExt` (`image` feature, in the prelude); installs the `ImagePlugin` and with it the Renderer; the last call of each kind wins (see `images.md` §8) |
| `.limits(Limits)` | `Limits::default()` (Strict) | the Limits every Responder validates outgoing messages against before any request; `LimitMode::Lenient` truncates over-long texts with `…` instead of failing with `ResponderError::Invalid`; the last call wins. Read back with `app.limits() -> &Limits`; see `limits-cheatsheet.md` |
| `.presence(Presence)` | none (Discord shows online, no activity) | the presence every shard identifies with; the last call wins; change it at run time with `app.presence()` (§6) |
| `.large_threshold(LargeThreshold)` | `LargeThreshold::default()` (50) | the IDENTIFY `large_threshold`; `LargeThreshold::new::<N>()` compiles only for 50..=250 (GW-06, §6); the last call wins |
| `.boot_flags(BootFlags)` | `all`; `App::from_env()` reads `NOCTIS_FEATURES`, then `--features` (wins) | which flagged Modules and registrations exist this run; `build()` prunes the trees with it before the fold; a selected flag nothing declares is `BuildViolation::UnknownBootFlag`; the last call wins; read back with `app.boot_flags()` (`modules-middleware-ratelimits.md` §2) |

### Errors

Every error is a `thiserror` enum (`#[non_exhaustive]`; match with a `_` arm) whose
message names the cause and a fix. No message, `Debug` or log line ever contains the
token.

- `noctis::Error`: `Env(EnvError)`, `Build(BuildError)`, `Start(StartError)`,
  `Run(RunError)`, `Cli(noctis::cli::CliError)` (a rejected command line, before
  anything else runs), `Publish(noctis::publish::PublishError)` (a CLI mode's
  Publish failed) and `LocaleOverlay(noctis::LocaleOverlayError)` (the startup
  overlay load failed, `Load { locale, source }`, or rejected overlays,
  `Invalid(Vec<OverlayViolation>)`; see i18n §8) (`From` for each, so `?` works in
  `main`).
- `RunError` (`Copy`): `AllShardsEnded`: every shard ended on its own after startup
  (fatal closes; see the `shard closed fatally` logs); the shutdown ran, then `run`
  returned this instead of `Ok(())`. Fix the cause, then restart.
- `BuildError`: `err.violations()` (`&[BuildViolation]`, never empty, in recorded order:
  setters, plugin reports, then checks) or `err.into_violations()`. `Display` is
  `"{n} App-build violation(s)"` then one `"\n  - {violation}"` line each.
- `BuildViolation`: `NoTokioRuntime` (build outside a tokio runtime), `CryptoProvider`
  (no rustls provider could be installed), `DuplicateState { type_name }`,
  `DuplicatePlugin { type_name }`, `ResourceConflict(ResourceConflict)` (§10),
  `InvalidHttpTimeout { actual }`, `InvalidHttpProxy { reason: HttpProxyError }`
  (message `"invalid HTTP proxy: {reason}"`, never the value), `InvalidGatewayUrl {
  reason: GatewayUrlError }` (message `"invalid gateway URL: {reason}"`, never the
  value), `InvalidShardTotal` (`.shard_total(0)`), `EmptyShardRange { start, end }`
  (`"shard range {start}..{end} is empty: use start..end with start below end, e.g. 0..16"`),
  `ShardRangeWithoutTotal { start, end }` (`.shard_range(..)` without `.shard_total(..)`),
  `ShardRangeOutOfBounds { start, end, total }` (end above the total; a total of 0 reports
  only `InvalidShardTotal`), `InvalidShutdownGrace { actual }` (message
  `"shutdown grace {actual:?} is out of range: use a value in 0 s..=900 s"`),
  `InvalidHttpConfig { reason: HttpConfigError }` (message `"invalid HTTP config:
  {reason}"`, one per bad field, in field order), `HttpEdge { kind: std::io::ErrorKind }`
  (`"the Discord HTTP edge could not bind 127.0.0.1:0 ({kind:?})"`),
  `InvalidEventLane { capacity, overflow }`
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
    is logged at `error` (`shard closed fatally`, with `shard`, `close_code` and this
    error's text as `error`), its `noctis_shard_status` gauge goes to 0, and the App
    keeps running; when every shard has stopped, `run` returns `RunError::AllShardsEnded`.
  - `SignalHandler`: the Ctrl-C/SIGTERM handlers could not be installed (the tokio
    runtime has no IO driver: build it with `enable_all()`, as `#[tokio::main]` does).
- `EnvError`: `NotUnicode { var }`, `InvalidToken { var, source: TokenError }`,
  `InvalidShardTotal { var }` (not 1-10 ASCII digits fitting a `u32`),
  `InvalidShardRange { var }` (not `start..end` of two 1-10 ASCII digit numbers fitting a
  `u32`; message names the variable and the format `start..end`, e.g. `0..16`),
  `InvalidShutdownGrace { var }` (not 1-4 ASCII digits of whole seconds),
  `InvalidPublisher { var }` (not `true`/`false`/`1`/`0`, any ASCII case),
  `InvalidDevGuild { var }` (not 1-20 ASCII digits of a non-zero `u64`); the value is
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
- `HttpConfigError` (why `.http(..)` was rejected; one per field, in this order):
  `PingInterval(Duration)` (50 ms..=300 s), `PingTimeout` (50 ms..=60 s), `HedgeAfter`
  (300 ms..=2800 ms), `ConnectTimeout` (100 ms..=30 s), `DnsTtlFloor` and `DnsTtlCap`
  (1 s..=86400 s each), then `DnsTtlOrder { floor, cap }` (floor above cap; checked only
  when both are in range). Message: `"HttpConfig.<field> {actual:?} is out of range: use
  <lo>..=<hi>"`, or `"HttpConfig.dns_ttl_floor {floor:?} is above dns_ttl_cap {cap:?}"`.
- `AckDeadlineMissed { elapsed: Duration }` (`Copy`, `#[non_exhaustive]`): an
  interaction was not acknowledged within 2.9 s of its gateway receipt (Discord allows
  3 s; INT-01). Message: `"interaction not acknowledged in time: {elapsed:?} after
  receipt, past the 2.9 s ack deadline (Discord allows 3 s; INT-01): defer slow work"`.
  The Responder returns it inside its own error
  (`ResponderError::AckDeadlineMissed`, `responder.md` §4); defer slow work instead of
  answering late.
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
  permissions and a fresh `NOCTIS_PAYLOAD_KEY` (32 random bytes as 64 lowercase hex
  digits), and the bot exits so the
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
| `NOCTIS_PAYLOAD_KEY`, `NOCTIS_PAYLOAD_KEY_PREVIOUS` | the payload key and the previous key of a rotation, read by `App::from_env()` after `NOCTIS_DEV_GUILD` with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`): exactly **64 hex digits** (upper or lower case, no `0x`; generate with `openssl rand -hex 32`), else `EnvError::InvalidPayloadKey { var, source: KeyError }`, whose text names the variable and never the value; passed to `.payload_key(..)` / `.previous_payload_key(..)` |
| `NOCTIS_APP_ID` | the App id, read by `App::from_env()` last (after `NOCTIS_PAYLOAD_KEY_PREVIOUS`) with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`) and passed raw to `.app_id(..)`, so an invalid id (`PROD`) fails at `build()` with `BuildViolation::InvalidAppId`; a later `.app_id(..)` wins |
| `NOCTIS_DEV_GUILD` | default for `--publish-guild` without an id, read by `App::from_env()` last (after `NOCTIS_PUBLISHER`) with the same rules (trimmed; empty = unset); 1-20 ASCII digits of a non-zero `u64` (leading zeros allowed), else `EnvError::InvalidDevGuild`; passed to `.dev_guild(..)` |
| `NOCTIS_SHARD_TOTAL` | fixed shard total, read by `App::from_env()` after `NOCTIS_GATEWAY_URL` with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`). It must be 1-10 ASCII digits that fit a `u32`, and leading zeros are allowed (`"007"` = 7). Anything else (`-1`, `+5`, `1e3`, `1_000`, over `4294967295`) → `EnvError::InvalidShardTotal`. It is then passed to `.shard_total(..)`, so `0` fails at `build()` |
| `NOCTIS_SHARD_RANGE` | this process's Shard Range, read by `App::from_env()` after `NOCTIS_SHARD_TOTAL` with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`). Exactly `start..end` (half-open: `32..64` is shards 32-63), each side 1-10 ASCII digits fitting a `u32` (leading zeros allowed). Anything else (`32..=63`, `32-63`, `32..`, `32 .. 64`, `+1..2`, `0x1..2`, `32..64/128`) → `EnvError::InvalidShardRange`. It is then passed to `.shard_range(..)`, so an empty range, a missing `NOCTIS_SHARD_TOTAL` or an end above the total fails at `build()` |
| `NOCTIS_SHUTDOWN_GRACE` | shutdown grace in whole seconds, read by `App::from_env()` after `NOCTIS_SHARD_RANGE` with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`). It must be 1-4 ASCII digits (`"30"`, `"0900"`); anything else (`1.5`, `-1`, `+1`, `10s`, `1e3`, `10000`) → `EnvError::InvalidShutdownGrace`. It is then passed to `.shutdown_grace(..)`, so 901-9999 fail at `build()` |
| `NOCTIS_HTTP_PROXY` | HTTP rate-limit proxy, read by `App::from_env()` after the token with the same rules (trimmed; empty = unset; not Unicode → `EnvError::NotUnicode`), then passed to `.http_proxy(..)`; its content is checked by `build()` (`BuildViolation::InvalidHttpProxy`) |
| `NOCTIS_GATEWAY_URL` | gateway URL, read by `App::from_env()` after `NOCTIS_HTTP_PROXY` with the same rules, then passed to `.gateway_url(..)`; its content is checked by `build()` (`BuildViolation::InvalidGatewayUrl`) |
| `NOCTIS_PUBLISHER` | `true` on the one publishing process; read by `App::from_env()` after `NOCTIS_SHUTDOWN_GRACE` with the same rules; `true`/`false`/`1`/`0` in any ASCII case, else `EnvError::InvalidPublisher`; passed to `.publisher(..)` |
| `NOCTIS_FEATURES` | Boot Flags, read by `App::from_env()` after `NOCTIS_DEV_GUILD` with the same rules (trimmed; empty = unset): `all`, `none` or `a,b` (`BootFlags`), else `EnvError::InvalidBootFlags { var, reason }`; passed to `.boot_flags(..)`. `--features` on the command line wins over it |
| `RUST_LOG` | tracing filter (Noctis uses `tracing`; you install the subscriber) |

No config file is needed. Secrets come only from the environment.

## 3. CLI modes and Publish

`App::run()` parses `std::env::args_os().skip(1)` (`App::run_with_args(args)` takes them
explicitly). Built-in parser, no clap:

| Flag | Effect |
| --- | --- |
| *(none)* | run the bot |
| `--publish` | publish globally (and each `guilds = [..]` Command to its guilds), print the report, exit |
| `--publish-guild [<id>]` | publish **every** Command into one guild (dev mirror; instant), print, exit. No id: `.dev_guild(..)` / `NOCTIS_DEV_GUILD`, else `CliError::MissingDevGuild` |
| `--publish-clear [--guild <id>]` | publish an empty list globally (or to that guild): deletes this App's commands there |
| `--dry-run` | with a publish flag: read, diff and check everything, print the JSON, the diff and the create count, write nothing |
| `--features a,b` / `all` / `none` | Boot Flags. `App::from_env()` reads it from the process command line before `build()` (it wins over `NOCTIS_FEATURES`); `run` only checks that it equals `app.boot_flags()`, else `CliError::BootFlagsMismatch`, and logs the selection at info level (`boot flags selected`). Combines with every mode, `--dry-run` and `--no-resume` |
| `--no-resume` | run the bot with fresh sessions: `.resume_sessions(false)` for this run (`info` log `session resume disabled (--no-resume)`): each shard deletes its stored entry before it identifies, the shutdown closes with 1000 and deletes it. With a publish flag it is `CliError::NoResumeWithPublish` |

Parsing rules: `--publish-guild=<id>`, `--guild=<id>` and `--features=<v>` also work;
`--publish-guild` takes the next argument as its id, and `--features` as its value, only
when it does not start with `-`; an id is 1-20
ASCII digits of a non-zero `u64`. A mode returns `Ok(())` after printing the report to
stdout, or `Err(noctis::Error::Publish(..))`; it installs no signal handler, sends no
`GET /gateway/bot` and starts no shard.

`noctis::cli::CliError` (`#[non_exhaustive]`; each message names the flag and a fix):
`UnknownFlag { flag }` (any other argument, `--` and non-UTF-8 ones included),
`MissingValue { flag }` (`--guild` without a value), `UnexpectedValue { flag }`
(`--publish=1`, `--dry-run=yes`), `InvalidGuildId { flag, value }`, `DuplicateFlag { flag }`,
`ConflictingModes { first, second }`, `DryRunWithoutPublish`, `GuildWithoutClear`
(`--guild` without `--publish-clear`), `MissingDevGuild`, `NoResumeWithPublish`
(`--no-resume` with `--publish`, `--publish-guild` or `--publish-clear`),
`InvalidFeatures { value, reason }` (a `--features` value that is not `all`, `none` or
`a,b`), `MissingFeatures` (`--features` without a value), `BootFlagsMismatch { given,
built }` (a `--features` that differs from the App's built Boot Flags: build it with
`App::from_env()`, or pass the same selection to `.boot_flags(..)`). `NoctisArgs::features()
-> Option<&BootFlags>` reads the parsed selection; iterating `NoctisArgs` renders
`--features <canonical>` last.

Stale guild commands: `--publish` writes only the global target and the guilds the
Registry names, so a `--publish-guild` dev copy stays in that guild after you go global
(every command shows twice) and a guild dropped from `guilds = [..]` keeps its old
commands. Clean such a guild with `cargo run --release -- --publish-clear --guild <id>`.

**Your own flags.** Split first; the rest keeps its order untouched (program name, your
flags, positional and non-UTF-8 arguments, `--` and everything after it). Before `--`,
`--publish`, `--publish-guild`, `--publish-clear`, `--guild`, `--dry-run`, `--no-resume` and `--features` are reserved
by Noctis and always claimed (a bot flag named `--guild` or `--dry-run` gets
`GuildWithoutClear`/`DryRunWithoutPublish`; rename yours), and `--publish-guild` and `--features` also
consume the next argument that does not start with `-` (`--features=<v>` works too):

```rust
use noctis::cli::NoctisArgs;

let (noctis_args, mine) = NoctisArgs::split(std::env::args_os())?; // Result<_, CliError>
let my_flags = MyCli::parse_from(mine);                             // your parser
app.run_with_args(noctis_args).await?;                              // NoctisArgs: IntoIterator
```

`NoctisArgs::parse(args)` parses a line of Noctis flags only; `args.mode()` is
`Option<CliMode>` (`Publish`, `PublishGuild(Option<Id<GuildMarker>>)`,
`PublishClear(Option<Id<GuildMarker>>)`), `args.dry_run()` a `bool` and
`args.no_resume()` a `bool` (`--no-resume`, only without a mode). Iterating a
`NoctisArgs` renders its canonical flags, so it round-trips through `parse`.

**Publish** (`noctis::publish`) never burns the create quota (REG-05: 200 creates per
day per guild). One Publish:

1. opens the `noctis.publish{app, scope, dry_run, clear}` span;
2. needs a token (`PublishError::MissingToken`, no request);
3. reloads the runtime Module state (not for `clear`; a store failure is
   `PublishError::ModuleState`, no request; `modules-middleware-ratelimits.md` §3);
4. reads `GET /applications/@me` (application id and enabled install types);
5. builds every target's JSON from the Registry alone (each guild target without the
   guild-scoped Commands of Modules disabled in that guild; global Commands are never
   filtered) and runs every check before any write: on the dev mirror REG-03
   (`NameClash`), on every guild target REG-02 (`TooManyCommands`, counted after that
   filter), on the global target META-04 (`IntegrationTypeNotEnabled`: an `integration_types` value the
   app has not enabled under Installation in the Developer Portal);
6. per target: `GET …/commands?with_localizations=true`, canonical diff, then one
   bulk-overwrite `PUT` only when the diff is not empty and this is not a dry run. The
   first HTTP error stops the rest.

Targets: `PublishScope::Global` writes the global commands, then each guild of a
`guilds = [..]` Command gets only its own Commands. `PublishScope::Guild(id)` (the
`--publish-guild` dev mirror) writes one guild: the global Commands **without**
`contexts`/`integration_types` (META-03), followed by that guild's own Commands.

The JSON carries only what you declared: `"options": []` on every option-less command,
subcommand and group (FR-61); unset metadata is omitted (Discord applies its default);
`default_member_permissions` is the bit string; the localizations from the merged catalogs are sent (`i18n.md` §4).
The diff ignores Discord's server fields and defaults (unset `contexts` equals
`[0,1,2]`, unset `integration_types` equals the app's enabled types, absent `nsfw`
equals `false`, `1` equals `1.0`); option and choice order matter, command order does
not. Remote commands of other types (an Activity's Entry Point) are kept in every `PUT`.

Programmatic form: `app.publish(scope)` returns a `noctis::publish::Publish` (set
`.dry_run(bool)` / `.clear(bool)`, then `.await` it), which yields a `PublishReport`
holding one `TargetReport` per target:

```rust
use noctis::publish::{PublishError, PublishScope};

let report = app.publish(PublishScope::Guild(guild_id)).dry_run(true).await?;
for target in report.targets() {          // TargetReport
    println!("{}: {} creates, changed: {}, wrote: {}",
        target.scope(), target.creates(), target.changed(), target.wrote());
    println!("{}\n{}", target.json(), target.diff());
}
println!("{report}");                      // what the CLI modes print
assert_eq!(report.creates(), 1);           // over every target; report.wrote(), report.is_dry_run()
```

`Publish` also has `.clear(true)`. `diff()` holds one line per change, creates first,
then updates, then deletes:

```text
+ create chat_input `ping`
~ update chat_input `cfg` (description, options)
- delete user `Report`
```

`PublishError` (`#[non_exhaustive]`, no token or body in any message):
`MissingToken`, `Rejected { route, status, code }` (Discord's error status and JSON
code, e.g. `PUT /applications/7/commands`, 400, `Some(50035)`), `Transport { route }`,
`InvalidResponse { route, reason }`, `IntegrationTypeNotEnabled { command,
integration_type }` (`[META-04] …`), `NameClash { guild, kind, name }` (`[REG-03] …`),
`TooManyCommands { guild, kind, count, max }` (`[REG-02] …`); `kind` is `chat_input`,
`user` or `message`. Each write logs one `info` (target `noctis::publish`, fields
`scope`, `creates`, `diff`); an unchanged target logs `commands unchanged; nothing
written`.

Publish **never** runs on shard READY. In production either run `--publish` from your
deploy step, or set `.publisher(true)` / `NOCTIS_PUBLISHER=true` on exactly one process:
a normal run then publishes globally once before any shard connects, and a failure is
logged, not fatal.

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
when every shard of this process has received READY.

With `.shard_range(start..end)` (or `NOCTIS_SHARD_RANGE`), the process starts only the
shards `start..end` of the fixed total, logs `starting shards {start}..{end} of {total}`,
and its readiness, `noctis_shard_status` gauges and `RunError::AllShardsEnded` cover
only those shards. Ids stay global, so shard 40 of 128 is in bucket `40 %
max_concurrency` on every process.

- `.shard_total(0)` is `BuildViolation::InvalidShardTotal`. At `run()`, a fixed total
  below ⌈recommended × 2 / 5⌉ is `StartError::ShardTotalTooSmall { total, minimum }`
  (Discord recommends about 1 shard per 1000 guilds, and a shard may hold 2500 guilds:
  GW-03). It is returned before any connection opens. The recommended count never
  fails this check.
- A fixed total that isn't a multiple of `max_concurrency` (when it is above 1) only
  logs a warning (`total`, `max_concurrency` fields), and startup continues.
- A Shard Range is aligned when its start, and its end unless it equals the total, are
  multiples of `max_concurrency` (always, when it is 1). A range not aligned only logs
  one warning (target `noctis::app`, fields `start`, `end`, `max_concurrency`: `shard
  range is not aligned to max_concurrency; identify buckets are split across
  processes`), and startup continues.
- Startup refuses with `StartError::SessionStartLimit { required, remaining,
  reset_after }` when `session_start_limit.remaining` can't cover the shards this
  process identifies (its Shard Range, or the whole total; GW-02); the check is per
  process and does not cover the Cluster's total session starts. It is returned
  before any connection opens. GW-03 (`ShardTotalTooSmall`) still checks the total,
  and runs first.
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
- The **Discord cache** (`.discord_cache(..)`, `adapters.md` §1.2) adds the event types
  that keep its resources current to the filter (they are never droppable) and their
  intents. An interaction-only App with the default cache identifies with exactly
  `Intents::GUILDS`; with `DiscordCacheConfig::none()` it parses only interactions.

  | Resource | Event types fed | Intents added | Privileged (request it) |
  | --- | --- | --- | --- |
  | `GUILD` | `GUILD_CREATE/UPDATE/DELETE`, `UNAVAILABLE_GUILD` | `GUILDS` | — |
  | `CHANNEL` | `GUILD_CREATE/DELETE`, `UNAVAILABLE_GUILD`, `CHANNEL_CREATE/UPDATE/DELETE`, `CHANNEL_PINS_UPDATE`, `THREAD_CREATE/UPDATE/DELETE`, `THREAD_LIST_SYNC` | `GUILDS` | — |
  | `ROLE` | `GUILD_CREATE/DELETE`, `UNAVAILABLE_GUILD`, `GUILD_ROLE_CREATE/UPDATE/DELETE` | `GUILDS` | — |
  | `MEMBER` | `GUILD_CREATE/DELETE`, `UNAVAILABLE_GUILD`, `GUILD_MEMBER_ADD/UPDATE/REMOVE`, `GUILD_MEMBERS_CHUNK` | — | `GUILD_MEMBERS` (else `BuildViolation::MissingCacheIntent`, GW-05) |
  | `USER` | `GUILD_CREATE` | `GUILDS` | — |
  | `MESSAGE` | `MESSAGE_CREATE/UPDATE/DELETE`, `MESSAGE_DELETE_BULK` | `GUILD_MESSAGES`, `DIRECT_MESSAGES` | — |

  Interactions feed the cache when `MEMBER`, `USER` or `ROLE` is cached.

### Clusters (several processes)

```text
process 0: NOCTIS_SHARD_RANGE=0..32   NOCTIS_SHARD_TOTAL=128  NOCTIS_PUBLISHER=true
process 1: NOCTIS_SHARD_RANGE=32..64  NOCTIS_SHARD_TOTAL=128
...
all:       NOCTIS_HTTP_PROXY=http://proxy:3000  NOCTIS_PAYLOAD_KEY=<same>  NOCTIS_APP_ID=<same>
```

Every process identifies through one shared `IdentifyQueue` (`.identify_queue(..)`), so
each bucket identifies once per 5 s across the whole Cluster. Across processes that is
an Adapter backed by a shared store (`adapters.md`); for several Apps in one process,
hand each the same instance behind an `Arc`. A queue you supply (shared or not) must
have at least Discord's `max_concurrency` buckets (from `GET /gateway/bot`): a shard
whose bucket is past the queue's size never identifies. The `16` below stands for that
value:

```rust
use std::sync::Arc;
use noctis::ports::memory::MemoryIdentifyQueue;

let queue = Arc::new(MemoryIdentifyQueue::new(std::num::NonZeroU16::new(16).unwrap()));
let a = App::from_env()?.shard_total(128).shard_range(0..64)
    .identify_queue(Arc::clone(&queue)).publisher(true).build()?;
let b = App::from_env()?.shard_total(128).shard_range(64..128)
    .identify_queue(queue).publisher(false).build()?;
```

`.identify_queue(..)` never takes `Shared::new(..)`: the queue is keyed by bucket, not
namespaced.

Checklist:
1. Ranges consecutive, the same `NOCTIS_SHARD_TOTAL` everywhere, aligned to
   `max_concurrency` (start and end multiples of it, or the end equal to the total;
   misaligned only warns).
2. One shared HTTP rate-limit proxy (§5) for every process (`NOCTIS_HTTP_PROXY`); without
   it, `run` logs one `noctis::app` warning: "this process runs a Shard Range of a Cluster
   without an HTTP proxy: … (RL-02)" (50 requests/s per token is shared by all processes).
3. Shared Adapters: `RefStore`, `RateLimitStore`, `ModuleStateStore`, `IdentifyQueue`,
   `SessionStore` (per process key space is automatic), `CacheStore`. See `adapters.md`.
4. Same `NOCTIS_PAYLOAD_KEY` and `app_id` everywhere.
5. Exactly one publisher (`NOCTIS_PUBLISHER=true` / `.publisher(true)`): it publishes
   once at startup, before its `GET /gateway/bot`; the others send no Publish request.

Handlers do not change.

Sizing memory: an idle guild costs about 30 bytes of framework memory (its locale
record; the budget is 1 KiB) plus about 9740 bytes of Discord cache with the default
`DiscordCacheConfig` (`GUILD | CHANNEL | ROLE`, measured with 2 roles and 3 channels per
guild; real guilds with more channels and roles cost more). Multiply by the guilds a
process's Shard Range holds. `.discord_cache(DiscordCacheConfig::none())` drops the
Discord cache share entirely (then `ctx.cache()` getters return `CacheError::Disabled`).
The numbers come from Noctis's `memory_per_guild` bench over 10 000 idle guilds; in a
Noctis checkout, `cargo bench -p noctis-core --bench memory_per_guild` reproduces them.

### Session resume

Only the gateway reads and writes the `SessionStore`. Each Shard's entry lives under
`<app>:session:shard:<id>:<total>` (global id and the shard total, e.g.
`default:session:shard:3:4`) and holds `(session_id, seq, resume_url)`, seq being the
last event received.

- **Read** once when the Shard starts (bounded by 5 s). A valid entry makes it send
  RESUME instead of IDENTIFY, with no `IdentifyQueue` lease, and startup counts its
  RESUMED like a READY (the `ShardResumed` lifecycle event). A miss, a store error or a
  timeout makes it identify; an unreadable or invalid entry is deleted first.
- **Written** after a graceful shutdown's 4000 close is acknowledged by Discord's close
  frame, within the shard close timeout; no TTL. Nothing is written on READY, on
  in-process reconnects or periodically, so a crash removes nothing: an entry kept
  after a resumed start, or an older one left by an unacknowledged close, stays, and
  the next start resumes from its older sequence (Discord replays events already
  handled, or rejects it and the Shard identifies).
- **Deleted** when Discord rejects the stored session (op 9 `d: false`, or a READY
  replaces it): the Shard then identifies with no operator action. A fresh (1000) close
  deletes it too.
- **Cleared** at start, instead of read, with `.resume_sessions(false)` / `--no-resume`:
  the delete (bounded by 5 s; a failure only warns) finishes before the Shard connects,
  so it precedes IDENTIFY and no RESUME is sent. Only this process's Shards under the
  current total are cleared. A shutdown that resolves before `GET /gateway/bot`
  answered has no shard plan and clears nothing; the next `--no-resume` start does.

A different shard total never resumes an old layout's session (the total is in the
key). GW-02's boot check still counts resumed Shards, because a rejected resume falls
back to identify. The memory default resumes only when the same store instance reaches
the next App (`Arc::clone(&store)` into both builders); to resume across process
restarts, use a `SessionStore` that persists (see `adapters.md`).

Delivery is at-most-once: the stored sequence is the last event **received**, so an
event the event lane dropped under back-pressure (or discarded at shutdown) counts as
seen, and a resume never replays it. Nothing buffers or deduplicates dropped events;
every session write has finished when `run()` / `run_until` returns.

With `.gateway_url(..)` (or `NOCTIS_GATEWAY_URL`) set, shards connect to that URL
instead of Discord's gateway, but **resumes go to the `resume_gateway_url` that READY
names**: a gateway proxy must rewrite that field to its own URL, or resumes bypass it.
The testkit's `FakeGateway` already names itself there.

## 5. HTTP

Noctis owns one twilight HTTP client per App (`app.http()` and `EventCtx::http()`
return that same `&Arc<noctis::twilight::http::Client>`, for endpoints Noctis doesn't
wrap).

**The Discord HTTP edge.** Every request of that client passes a loopback forwarder
inside the App (started by `build()`): it forwards only `/api/v10/` requests, byte for
byte (only `Host` and hop-by-hop headers change), to `https://discord.com` over one
tuned connection, and streams the response back unchanged, so twilight's rate limiter
keeps learning buckets. It never adds `Authorization` and never logs a path, header
value or token. The connection is HTTP/2 with PING keep-alive, TCP keepalive and
`TCP_NODELAY`, a 1.5 s connect timeout per address and a cached async DNS. `run()` warms
it before `GET /gateway/bot` and keeps it warm with PING frames only: **never poll an
API endpoint (such as `GET /users/@me`) to keep HTTP warm.** A closed connection, an
unanswered PING or a shard losing the network makes it redial with back-off. A request
that never left (refused connection, refused stream) is retried once; one that may have
reached Discord is not, and fails with a 502 instead of hanging (twilight's timeout
still applies). Interaction callbacks bypass twilight's limiter on this connection,
hedged once if no answer arrives in time (the Responder's `reply`/`defer`, `responder.md`
§4); edits and follow-ups go through twilight with the interaction token. A twilight
`Arc<Client>` kept after its App is dropped gets connection errors.

`HttpConfig` (`#[non_exhaustive]`; start from `HttpConfig::default()` and set fields):

| Field | Default | Range |
| --- | --- | --- |
| `ping_interval` | 20 s | 50 ms..=300 s |
| `ping_timeout` | 5 s | 50 ms..=60 s |
| `hedge_after` | 1000 ms | 300 ms..=2800 ms (shortened so a hedge still fits the 3 s ack deadline) |
| `connect_timeout` | 1500 ms | 100 ms..=30 s |
| `dns_ttl_floor` | 30 s | 1 s..=86 400 s |
| `dns_ttl_cap` | 300 s | 1 s..=86 400 s (not below the floor) |
| `upstream_h2c` | `false` | HTTP/2 without TLS to an `http_proxy` upstream; HTTP/1.1 otherwise |

```rust
let mut http = noctis::HttpConfig::default();
http.ping_interval = std::time::Duration::from_secs(15);
builder.http(http);
```

With `.http_proxy(..)` (or `NOCTIS_HTTP_PROXY`) set, the edge forwards to
`http://{proxy}/api/v10/..` instead of Discord (HTTP/1.1, or h2c with `upstream_h2c`)
and the App's local rate limiter is off (the proxy limits). Accepted forms: `host`, `host:port`, `[ipv6]`, `[ipv6]:port`, each
optionally prefixed with `http://` (any case) and followed by one `/`; HTTPS proxies,
paths and credentials are rejected (`HttpProxyError`). Every request goes through the
edge to the proxy: twilight calls, raw REST, webhooks, interaction callbacks and
interaction webhooks (INT-13). `.http_timeout(..)` (default 10 s) bounds every request,
interaction callbacks included (a callback slower than it is `AckDeadlineMissed`); a
forwarded request over it fails as a twilight timeout.

Discord 429s (RL-01, RL-04): without a proxy, twilight's limiter waits out a `user`-scope
429 that carries valid bucket headers. Every other 429 (every one behind a proxy, and
`shared`, `global` or scope-less ones) is never retried: it fails once as
`RateLimited { scope: RateLimitScope, retry_after: Option<Duration>, discord_code }`
(`noctis::http`), typed as `ResponderError::RateLimited`, `WebhookError::RateLimited` and
`RawHttpError::RateLimited`; a `ctx.http()` twilight error converts with
`RateLimited::from_http_error(&err)` (`Some` only for a rate limit). Publish reports
`PublishError::Rejected { status: 429, .. }`, startup `StartError::GatewayInfo { status:
Some(429) }`. The initial interaction response (respond or defer) is the exception: it
keeps the INT-01 fit rule (one retry when `retry_after` fits the ack deadline) and fails
as `AckDeadlineMissed`, never `RateLimited`. Wait at least `retry_after` before
retrying, never in a loop:

```rust
use noctis::http::RateLimited;

match ctx.http().channel(id).await {
    Ok(response) => { /* .. */ }
    Err(err) if RateLimited::from_http_error(&err).is_some() => { /* try again later */ }
    Err(err) => return Err(err.into()),
}
```

Raw requests (FR-31) for endpoints twilight does not model go through the same client:
`ctx.http().raw(Method::Get, "applications/@me").await?` (`noctis::http::Method`;
`RawHttp` is in the prelude) — still rate limited / proxied, with the bot token and the
timeout (`components-v2.md` §11b).

## 6. Presence and gateway commands

Everything lives in `noctis::gateway` and is always compiled (no cargo feature).

```rust
use noctis::gateway::{
    Activity, GatewayError, GuildMembers, LargeThreshold, MemberRequest, Presence, Status,
};

// Identify: every shard identifies with this presence and `large_threshold`.
builder
    .presence(Presence::watching("/help"))                 // default: none (online, no activity)
    .large_threshold(LargeThreshold::new::<100>());       // default 50; 50..=250 at compile time

// Run time: take the handles before `run` consumes the App, or use `ctx.gateway()`.
let presence = app.presence();                            // == app.gateway().presence()
presence.set(Presence::custom("☕ brewing"))?;            // every shard of this process
presence.set_shard(0, Presence::dnd())?;                  // one shard

async fn lookup(ctx: &CommandCtx, guild: Id<GuildMarker>) -> Result<GuildMembers, GatewayError> {
    let request = MemberRequest::users(guild, [Id::new(7), Id::new(8)]); // 1..=100 ids
    let members = ctx.gateway().request_guild_members(request).await?;
    Ok(members) // members.members, members.presences, members.not_found
}
```

**Presence.** `Presence::new(Status)` or the shortcuts `Presence::online()`, `idle()`,
`dnd()`, `invisible()`, and `Presence::playing(name)`, `listening(name)`,
`watching(name)`, `competing(name)`, `streaming(name, url)`, `custom(state)` (status
`Online` plus that activity). Setters: `.status(Status)`, `.activity(Activity)` (one
activity; a later call replaces it), `.afk(bool)`. `Status` is `Online` (the default),
`Idle`, `DoNotDisturb` or `Invisible`. `Activity::playing`, `listening`, `watching`,
`competing`, `streaming(name, url)` and `custom(state)` (a custom status: type 4, name
"Custom Status") build one activity; `.state(text)` adds the second line. Only `name`,
`type`, `state` and `url` are sent; texts are sent as given (Discord documents no limit).

**`LargeThreshold` (GW-06).** `LargeThreshold::new::<N>()` compiles only for
`N` in 50..=250 (`[GW-06]` compile error otherwise); `.get()` reads it back;
`LargeThreshold::default()` is 50.

**Handles.** `app.gateway()` and `ctx.gateway()` (on `EventCtx`, `CommandCtx`,
`ComponentCtx`, `ModalCtx` and the other component contexts; not on autocomplete) return
the App's `GatewayHandle` (cheap clone); `app.presence()` / `gateway.presence()` a
`PresenceHandle`. They are live while `run`/`run_until` runs the shards, from shard start
until the shards closed; before, after, during the shutdown and in a publish run every
call is `GatewayError::NotRunning`, as is every call on `GatewayHandle::detached()` (what
the detached test contexts return). `GatewayHandle::attach` and `detach` are internal (core
calls them); never call them.

- `PresenceHandle::set(Presence)`: op 3 to every shard of this process, all or none: the
  budget of every shard is checked first, and if one cannot send, nothing is sent.
  `PresenceHandle::set_shard(shard, Presence)`: one shard. Both are synchronous and never
  block. Each shard remembers the latest presence set at run time and sends it again after
  a new session's READY, so a re-identify keeps it (a resume needs nothing).
- `gateway.shard_for_guild(guild)`: the owning shard, `(guild_id >> 22) % total`.
- `gateway.request_guild_members(MemberRequest)`: Request Guild Members (op 8, GW-07) to the
  guild's owning shard; Noctis sets the nonce and collects every `GUILD_MEMBERS_CHUNK` into
  `GuildMembers { members, presences, not_found }` (chunk order). Cancel-safe: dropping the
  future forgets the request. A `MemberChunk` event handler still sees the chunks.

**`MemberRequest`** (one guild by construction; `.guild()` reads it):
`MemberRequest::users(guild, [ids; N])` takes 1..=100 ids at compile time (`[GW-07]`
error for 0 or 101); `MemberRequest::user_list(guild, iter)` checks at run time
(duplicates kept); `MemberRequest::prefix(guild, "ab", limit)` matches names starting with
the query, `limit` 1..=100; `MemberRequest::all(guild)` asks for every member and needs
the `GUILD_MEMBERS` intent; `.presences(true)` adds presences and needs
`GUILD_PRESENCES` (request privileged intents with `.intents(..)`).

**Budget (GW-04).** Each shard accepts at most 110 commands (presence sets plus member
requests) per sliding 60 s: Discord allows 120 per connection and twilight reserves up
to 10 for heartbeats. The next one is `GatewayError::SendLimit { shard, retry_after }`
and is not queued; retry after `retry_after`. Noctis's own presence re-send after
READY is not counted.

**Errors** (`GatewayError`, `#[non_exhaustive]`, `Clone + PartialEq`; messages never
carry the token, nonce or payloads):

| Variant | When |
| --- | --- |
| `NotRunning` | the App is not running the shards (see above) |
| `ShardNotInRange { shard, range }` | `set_shard` outside this process's Shard Range |
| `GuildNotInRange { guild, shard, range }` | the guild's owning shard runs in another Cluster process; nothing is sent (ask that process, e.g. through your own queue) |
| `ShardClosed { shard }` | that shard's task ended (a fatal close code) |
| `SendLimit { shard, retry_after }` | `[GW-04]` the shard's budget of 110 per 60 s is used up |
| `RateLimited { guild, retry_after }` | `[GW-07]` Discord answered the member request with `RATE_LIMITED` |
| `NoUserIds`, `TooManyUserIds { len }` | `[GW-07]` `user_list` with 0 or more than 100 ids |
| `EmptyQuery`, `InvalidLimit { limit }` | `[GW-07]` `prefix` with `""` (use `all`) or a limit outside 1..=100 |
| `MissingIntent { intent }` | `[GW-07]` `all()` without `"GUILD_MEMBERS"`, `.presences(true)` without `"GUILD_PRESENCES"` |
| `Timeout { guild }` | no chunk for 10 s since the request or the last chunk (for example a reconnect lost the answer) |

## 7. Lifecycle Hooks

React to the App's own lifecycle with a hook: a unit struct with `#[derive(Hook)]`
(in the prelude) whose `#[handler]` takes the event as its typed parameter. The event
type selects what the hook reacts to; register it like every handler.

```rust
use noctis::lifecycle::{AllShardsReady, HookCtx, ModuleToggled, ShardFatal};
use noctis::prelude::*;

#[derive(Hook)]                       // one struct per hook, like commands and events
pub struct Online;

impl Online {
    #[handler]
    async fn handle(self, ctx: HookCtx, ev: AllShardsReady) -> HandlerResult<()> {
        tracing::info!(module = ctx.module(), shards = ev.shards.len(), total = ev.total, "online");
        Ok(())
    }
}

#[derive(Hook)]
#[hook(flags = ["audit"])]            // optional: Boot Flags, the only argument
pub struct ModuleAudit;

impl ModuleAudit {
    #[handler]
    async fn handle(self, _ctx: HookCtx, ev: ModuleToggled) -> HandlerResult<()> {
        tracing::info!(module = ev.module, scope = %ev.scope, enabled = ev.enabled, "toggled");
        Ok(())
    }
}

Module::new("ops").add::<Online>().add::<ModuleAudit>();   // src/modules/ops/mod.rs
```

**Signature.** Exactly `async fn <any>(self, ctx: HookCtx, event: E) -> HandlerResult<()>`
with `E` one of the ten types below. Anything else does not compile: another payload
(``` `MessageCreate` is not a lifecycle event```), a missing or extra parameter or a
responder (`lifecycle hooks take (self, ctx: HookCtx, event: <lifecycle event>)`), an
`Ext<T>` (`lifecycle hooks run no Middleware ..`), a non-unit or generic struct
(`` `#[derive(Hook)]` needs a unit struct``), a struct without `#[handler]`, and any
`#[hook(..)]` argument other than `flags = [..]` (``unknown `#[hook]` argument ..``).
`#[on_stale]`, `#[on_expired]` and `#[on_rate_limited]` do not apply to hooks.

**Events** (`noctis::lifecycle`; each a `#[non_exhaustive]` struct with `pub` fields,
`Debug + Clone + PartialEq + Eq`, and a `NAME` through the sealed `HookEvent` trait):

| Event (`NAME`) | Fires | Data |
| --- | --- | --- |
| `AppStarted` (`APP_STARTED`) | once per run, after the locale overlays and the startup Module-state load, before the startup Publish | `publisher: bool` |
| `PublishCompleted` (`PUBLISH_COMPLETED`) | every `App::publish(..)` that resolves `Ok`: the startup Publish, the CLI `--publish*` modes, your own calls; dry runs and clears included | `scope: PublishScope`, `clear: bool`, `report: PublishReport` (`report.targets()[i].diff()`, `report.is_dry_run()`) |
| `PublishFailed` (`PUBLISH_FAILED`) | every `App::publish(..)` that resolves `Err` | `scope`, `dry_run: bool`, `clear: bool`, `error: PublishError` |
| `ShardReady` (`SHARD_READY`) | a shard received READY | `shard: u32` |
| `ShardResumed` (`SHARD_RESUMED`) | a shard received RESUMED | `shard` |
| `ShardDisconnected` (`SHARD_DISCONNECTED`) | a shard's connection closed; it reconnects (also just before a `ShardFatal`) | `shard`, `close_code: Option<u16>` (`None`: no close frame) |
| `ShardFatal` (`SHARD_FATAL`) | a shard stopped for good (a fatal close such as 4014, or retries ran out) | `shard`, `close_code: Option<u16>` |
| `AllShardsReady` (`ALL_SHARDS_READY`) | once per run, after the last shard of this process's range was first ready; never again after reconnects | `shards: Range<u32>`, `total: u32` |
| `ModuleToggled` (`MODULE_TOGGLED`) | `Modules::set_enabled`/`reset` through this App changed a record (an unchanged write fires nothing; changes other processes wrote, seen through the poll, fire nothing here) | `module: &'static str`, `scope: ModuleScope`, `enabled: bool` (the Module's own state in `scope` after the change), `reset: bool` |
| `ShutdownStarted` (`SHUTDOWN_STARTED`) | once per run in which `AppStarted` fired, at step 1 of §8 | `reason: ShutdownReason` (`Signal`, `Requested`, `AllShardsEnded`, `StartupFailed`; `as_str()`/`Display`: `"signal"`, `"requested"`, `"all shards ended"`, `"startup failed"`) |

Each occurrence fires exactly once. A publisher's run reads `AppStarted`,
`PublishCompleted` (the startup Publish runs before `GET /gateway/bot`), `ShardReady`
per shard, `AllShardsReady`, then whatever happens, then `ShutdownStarted` last (shard
events of the closing shards may still follow it). A startup that fails after
`AppStarted` (for example close 4014 before every shard was ready) fires
`ShutdownStarted { reason: StartupFailed }` and `run` still returns its `StartError`; a
missing token or a bad locale overlay fails before `AppStarted`, so nothing fires.

**Delivery.** While `run`/`run_until` runs, every occurrence goes into one ordered
queue drained by one task: per event, in arrival order, every hook registered for it in
Registry fold order (parent before child, `add` order, plugins' Modules included), one
at a time; the next event's hooks start only after the previous event's returned. So a
slow hook delays every later one: keep hooks short and `tokio::spawn` slow work (an
HTTP call, a webhook post). Outside a run (`App::publish(..)` or `app.modules()` before
`run`, the CLI `--publish*` modes) the hooks run inline and the call returns after them.
A hook's own `ctx.modules().set_enabled(..)` fires `ModuleToggled` after it (in a run)
or nested inside it (outside one, and during the end-of-run drain after the queue
closed); it never deadlocks.

**What runs and what does not.** No Middleware, no locale stage, no rate limit, no
timeout. A hook runs whether or not its Module is enabled at runtime (enablement gates
interactions and events only), so an `ops` Module keeps alerting and a Module can
observe its own re-enable. Boot Flags filter hooks like every registration: an
unselected hook is never registered and never fires.

**`HookCtx`** (cheap clone; `Debug` prints the Module and the states, never the HTTP
client): `module()`, `app_id()`, `state::<T>()`, `http()`, `gateway()` (detached outside
a run), `kv()`, `cache()`, `modules()`, `webhooks()`.

**`LifecycleHook`** is the hook's descriptor (`X::descriptor()`, what `Module::add::<X>()`
stores); its `Debug` names the handler and the event. Bot code never builds one by hand.

**Failures.** Every hook runs in span `noctis.hook` (`app`, `module`, `handler`,
`hook_event` = `NAME`, `shard` for shard events) under `catch_unwind`. An `Err` (user
conditions included) or a panic is counted in `noctis_handler_errors_total{kind="hook"}`
and reported to the `ErrorOrigin::Hook` Error Sink (`module`, `handler`,
`event_type` = `NAME`, `shard` for shard events, no `guild_id`); the default `LogSink`
logs `"hook handler failed"` / `"hook handler panicked"` on `noctis::hook`. The later
hooks and the App keep running. Durations go to
`noctis_handler_duration_seconds{kind="hook"}`.

**End of a run.** After the shutdown sequence (§8), `run` waits for the queued hooks
until `shutdown_grace` after `ShutdownStarted`, at least 1 s; then the hook still
running is cancelled and reported once as `ReportKind::Cancelled` (`"hook handler
cancelled at shutdown"`), and queued events are discarded with one `warn` on
`noctis::hook` (`lifecycle hooks discarded at shutdown`, field `events`). Dropping the
`run_until` future stops the queue at once (no `ShutdownStarted`).

## 8. Shutdown

`run()` shuts down on SIGINT/SIGTERM (Ctrl-C on non-unix), `run_until(fut)` when `fut`
resolves, and both also when every shard has ended (then they return
`RunError::AllShardsEnded`). A second signal does nothing extra. The order:

1. `info` log `shutdown started` (`reason = "signal"` from `run()`,
   `"requested"` from `run_until` or `"all shards ended"`, `grace`); the
   `ShutdownStarted` Lifecycle Hook fires here (§7).
2. Every shard closes with code 4000 (resumable; Discord keeps the session); once
   Discord acknowledges the close, the shard's session id, last received sequence and
   resume URL are written to the `SessionStore` (§4 Session resume). With
   `.resume_sessions(false)` / `--no-resume` every shard closes with 1000 instead and its
   stored entry is deleted. From here no new gateway event enters the lanes.
3. Both lanes drain (interactions first) and running handlers finish, for at most
   `shutdown_grace` (default 10 s). `ctx.http()` keeps working the whole time.
4. At the deadline every handler still running, or queued behind one (ordered
   handlers), is cancelled: its future is dropped at its next `.await` and it is
   reported once to the `Event` Error Sink as `ReportKind::Cancelled` (`LogSink`:
   `"event handler cancelled at shutdown"`). Events still in the lanes are discarded
   with one `warn` (`lanes discarded at shutdown`, counts only). Shutdown waits 1 s
   more; a handler that blocks its thread (no `.await`) cannot be cancelled and is
   logged as `handlers still running after cancellation`.
5. Shutdown waits up to 6 s for the shards to close, then `info` log
   `shutdown complete`; the ports drop with the App. The HTTP edge and its warm
   connection stay up through every step and close only after the shards have
   closed, when the App drops.

Shutdown takes at most `shutdown_grace` + 7 s, so keep the grace below your
orchestrator's kill timeout (Kubernetes: 30 s by default). Dropping the `run`
future (e.g. aborting its task) cancels every handler too. Do not spawn work that
must outlive a handler; put it behind a port.

## 9. `State<T>` and handler parameters

```rust
impl MyRoute {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let meta = ctx.state::<BotMeta>();          // today: read state from the context
        …
    }
}
```

Every `#[handler]` method takes `(self, ctx[, path | target | event][, Ext<T> |
Option<Ext<T>> …][, r: <Responder>])`: `Ext<T>` / `Option<Ext<T>>` parameters carry the
typed Extensions a before-Middleware inserted (`modules-middleware-ratelimits.md` §5),
and an `Ext<T>` no Middleware on the handler's Module path provides is a
`BuildViolation::MissingExtension`. Read state with `ctx.state::<T>()`
(`Option<State<T>>`); Middleware read it with `ctx.state::<T>()` too.

[planned] `State<T>` handler parameters (registered with `.state(T)`, `Deref<Target =
T>`) in any order after the path / target, a missing `State<T>` being a `BuildError`.
Mutable state: put interior mutability inside `T` (`tokio::sync::RwLock`, atomics) or,
better, use a port.

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

### Several Apps in one process (white-label bots)

Build each App with its **own** `App::builder()` chain, an explicit `.token(..)` and a
**distinct** `.app_id(..)`, attach both to one `Runtime`, and run them together. Each
App keeps its own HTTP client, shards, Registry, Responder, `Limits`, `app`-labelled
metrics and namespaced ports; process resources (fonts, render pool) come from the
shared `Runtime`.

```rust
use noctis::prelude::*;
use noctis::ports::memory::GuildAccess;
use noctis::{Runtime, Token};

let runtime = Runtime::global();
let acme_guild = noctis::twilight::model::id::Id::new(123);

let mut main = App::builder();
configure(&mut main)                                        // your shared Module tree
    .runtime(&runtime)
    .token(Token::new(std::env::var("MAIN_TOKEN")?)?)
    .app_id("main")
    .guild_access(GuildAccess::deny([acme_guild]));         // main skips the customer
let main = main.build()?;

let mut acme = App::builder();
configure(&mut acme)
    .runtime(&runtime)
    .token(Token::new(std::env::var("ACME_TOKEN")?)?)
    .app_id("acme")                                         // the last `.app_id` wins
    .guild_access(GuildAccess::allow([acme_guild]));        // acme serves only it
let acme = acme.build()?;

tokio::try_join!(main.run(), acme.run())?;
```

`run()` reads the process command line for every App: `--features` must equal each
App's built selection (else `CliError::BootFlagsMismatch`) and `--publish`/`--no-resume`
apply to all of them, so run Apps with different Boot Flag selections with
`run_with_args(..)` each.

There is no `App::from_env_prefixed` or `Runtime::run([..])`: read each App's token
yourself. Two live Apps with the same App id are **not** detected: they would share
port keys and metric series, so always give each App its own `.app_id(..)`.

**Restrict registrations to some Apps.** The same Module tree mounts on every App.
`apps = ["main", ..]` on `#[command]`, `#[user_command]` and `#[message_command]`, or
`Module::new("x").apps(["acme"])` for a whole Module, keeps it only on Apps whose
**effective App id** (the explicit `.app_id(..)`, else the id derived from the token,
else `default`) is listed:

```rust
#[derive(SlashCommand)]
#[command("admin", description = "Main bot only", apps = ["main"])]
struct Admin;

let branding = Module::new("branding").apps(["acme"]).add::<Brand>();
```

- It works like a Boot Flag, in the same pruning pass before `build()` folds the trees,
  after the Boot Flag selection: an unrestricted item always exists, a restricted one
  exists only on a listed App, a pruned Module drops its whole subtree (components,
  events, Middleware, children), and a kept Module's `base`/`group` whose node loses its
  last Command is dropped. A pruned Command is not registered, dispatched, counted for
  intents or published; an interaction for it is "unknown command". `flags` and `apps`
  together: it exists only when both keep it.
- `Module::apps([])` is unrestricted; repeated calls append.
- Ids follow the App id grammar `[a-z0-9-]{1,32}`. In the macros a bad id is a compile
  error: ``invalid App id `Main` in `apps`: App id has `M` at index 0: use only a-z, 0-9
  and - ([a-z0-9-]{1,32})``, `` `apps` needs at least one App id: write `apps = ["main"]`
  or remove it ``, `` `apps` takes a list of string literals: write `apps = ["main"]` ``
  and `` `a` is listed twice in `apps` ``. A bad `Module::apps` id is a
  `BuildViolation::InvalidAppRestriction { module, id, reason: AppIdError }` (``Module
  `m` restricts to App id `Bad`: …``), checked on the full trees. `apps` is not accepted
  on events, components or modals (restrict their Module instead).
- An App without an explicit id is `default` (or the token's id): restricted items are
  pruned there unless that id is listed, so set `.app_id(..)` on every App.

**Serve guilds selectively: `GuildAccessList`.** `.guild_access(..)` takes the App's
list; `noctis::ports::memory::GuildAccess` is the ready one: `GuildAccess::allow(guilds)`
serves only those guilds (an empty allow list serves none), `GuildAccess::deny(guilds)`
serves all but those. Pass an `Arc<GuildAccess>` and keep a clone to `insert(guild) ->
bool` / `remove(guild) -> bool` while the App runs; `contains` and `is_allow_list` read
it. A custom list implements `GuildAccessList::allows` (sync, no I/O; `adapters.md`).
It is pipeline **stage 3**, after the Command or Route is resolved and before any Ref
lookup, the Expired Handler chain, the `LocaleResolver`, rate limits, the auto-defer,
Middleware, Guards and the handler:

| Refused in a guild | What happens |
| --- | --- |
| slash Command, context menu, component, modal submit | one type-4 ephemeral `noctis.module.disabled` notice ("This feature is disabled here."), in the built-in locale (user, then guild, then App default); one `debug` `guild not served` on `noctis::dispatch`; no handler, Middleware, Guard, rate-limit count, report or duration sample; a `one_shot` Ref is never taken; a stale id gets this notice, not the Expired chain |
| autocomplete | one type-8 callback with no choices; the method does not run |
| event (`event.guild_id()` is set) | no handler runs (the Discord cache is still fed); one `trace` |
| `Dispatcher::run_command` (prefix commands) | `Err(DispatchError::GuildNotServed { guild_id })`, nothing sent; `PrefixPlugin` logs it, so two white-label bots sharing a prefix never both reply |

- No guild (a DM, a guildless event) is **always served**; the list is not asked.
- A panic inside `allows` counts as **not served** (fail closed) with one `error` log.
- **Publish ignores it**: it controls serving, not registration; guild-scoped Publish
  is unaffected. Use `apps = [..]` to keep a Command out of an App's Publish.
- It is never namespaced and shared between Apps only if you pass the same `Arc`.

**Adapters** are shared only where you opt in (`.cache_store(Shared::new(store))`);
otherwise keys are namespaced per App (`<app>:<port>:`, FR-71).

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
- `noctis.http{app, kind, method, attempt, reused, status_class}` around each Discord
  HTTP upstream attempt of the edge (§5); `kind` is `forward`, `callback` or `warmup`.
  It never carries a path, a token, an id or a header value (twilight's own `req` span
  does name the URL; filter `twilight_http` if that matters).
- `noctis.publish{app, scope, dry_run, clear}` around each Publish (`scope` is
  `global` or `guild <id>`); its logs use target `noctis::publish`.
- `noctis.render{app, template}` around each `ctx.render` (`template` is the Template
  id).

Metrics (live; every name starts with `noctis_`, every metric has an `app` label, and no
label carries a user, guild, channel or message id or free text):

| Metric | Type | Labels | Meaning |
| --- | --- | --- | --- |
| `noctis_lane_depth` | gauge | `app`, `lane` (`interaction` \| `event`) | items queued in each lane |
| `noctis_event_overflow_depth` | gauge | `app` | non-droppable events waiting behind a full event lane |
| `noctis_events_dropped_total` | counter | `app`, `event_type` | events dropped by back-pressure (`event_type` is the Discord name, e.g. `TYPING_START`; gateway-only kinds are `UNKNOWN`) |
| `noctis_handler_duration_seconds` | histogram | `app`, `kind` | handler run time in seconds, for `Ok`, errors and panics |
| `noctis_handler_errors_total` | counter | `app`, `kind` | handler runs that returned `Err` or panicked |
| `noctis_autodefer_total` | counter | `app` | interactions the auto-defer acknowledged (`responder.md` §2) |
| `noctis_ack_latency_seconds` | histogram | `app` | time from gateway receipt to the initial interaction callback (reply, defer, modal or auto-defer) |
| `noctis_ref_lookups_total` | counter | `app`, `result` (`hit` \| `miss` \| `stale` \| `forged`) | Ref lookups of clicked or submitted Ref ids (`routes-payloads-refs.md` §7); a `RefStore` error is not counted |
| `noctis_rate_limited_total` | counter | `app`, `kind` (`command` \| `component`; context menus count as `command`, modal submits as `component`) | invocations a declared `rate_limit(..)` refused (`modules-middleware-ratelimits.md` §7) |
| `noctis_shard_status` | gauge | `app`, `shard` | `1` after READY/RESUMED, `0` at start and after a disconnect or end; alert on `== 0` |
| `noctis_http_requests_total` | counter | `app`, `status_class` (`1xx`..`5xx` \| `error`) | Discord HTTP upstream attempts by status class; `error` = no response |
| `noctis_http_rate_limited_total` | counter | `app`, `scope` (`user` \| `global` \| `shared` \| `unknown`) | upstream 429 answers by `X-RateLimit-Scope` (RL-04) |
| `noctis_http_invalid_requests_total` | counter | `app`, `status` (`401` \| `403` \| `429`) | upstream 401/403 and non-`shared` 429 answers, which count toward Discord's 10,000 per 10 min per IP ban (RL-03); alert on `increase(..[10m])` summed per egress IP |
| `noctis_http_phase_seconds` | histogram | `app`, `phase` (`dns` \| `connect` \| `tls` \| `ttfb` \| `total`) | dial phases per new connection, `ttfb`/`total` per attempt |
| `noctis_http_connections_total` | counter | `app`, `reason` (`warmup` \| `redial` \| `hedge` \| `demand`) | upstream connections opened |
| `noctis_http_conn_reused_total` | counter | `app`, `reused` (`true` \| `false`) | attempts on a warm vs a new connection |
| `noctis_http_h2_ping_rtt_seconds` | histogram | `app` | HTTP/2 PING round trips of the warm connection |
| `noctis_http_ping_timeouts_total` | counter | `app` | connections dropped because a PING went unanswered |
| `noctis_interaction_ack_hedges_total` | counter | `app`, `outcome` (`won` \| `lost` \| `40060`) | hedged interaction callbacks, and callbacks Discord answered "already acknowledged" |
| `noctis_render_duration_seconds` | histogram | `app`, `template` | successful `ctx.render` time, queue wait included, by Template id |
| `noctis_render_queue_depth` | gauge | `app` | this App's `ctx.render` calls awaiting a render; a cancelled call stops counting, and its job is skipped while queued or stops before its next frame |
| `noctis_render_rejected_total` | counter | `app`, `reason` (`queue_full` \| `unknown_template` \| `unsupported` \| `over_cap`) | renders rejected before drawing |
| `noctis_discord_cache_update_errors_total` | counter | `app` | Discord cache updates that failed or exceeded 1 s; the event or interaction still reaches its handlers (one `noctis::cache` warning per outage) |

`kind` is the handler kind (`ErrorOrigin::as_str()`): `event`, `command` and
`autocomplete` are emitted today; `component` and `render` join with their handlers.

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

`app` is the App id (`app.app_id()`): `.app_id(..)`, `NOCTIS_APP_ID` or the bot user
id read from the token in base36, else `"default"`. Give several Apps in one process distinct
ids, or they share and overwrite the same `noctis_*` series.

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
- `ImagePlugin` (`image` feature): installed by `.image(..)` or `.theme(..)`; checks
  the config ranges and font list, gets the process-scoped render pool (`RenderPool`)
  and font registry (`FontRegistry`, frozen per font list) from the Runtime, decodes
  the App's static images and sets the takumi Renderer (`images.md` §6).
- `noctis::prefix::PrefixPlugin` (`prefix` feature, P2):
  `.plugin(PrefixPlugin::new("!").command::<Warn>())` checks its configuration (prefix,
  opted-in Commands; `BuildViolation::Plugin`) and mounts a `MESSAGE_CREATE` handler
  (Module `noctis-prefix`) that runs the opted-in slash Commands from `!warn <@5> 3`
  messages through `ctx.dispatcher()`. It needs `.intents(Intents::MESSAGE_CONTENT)`
  (else `[GW-05]` `MissingIntent`); see `commands.md` §9.

You rarely write a plugin yourself; a Module is the unit for bot features.
