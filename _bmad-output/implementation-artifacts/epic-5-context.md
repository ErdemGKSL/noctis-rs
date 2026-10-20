# Epic 5 Context: A running sharded bot

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author build an App from a token, typed state and Adapters, run it sharded across every core with correct identify concurrency, and handle gateway events without ever stalling heartbeats or dropping interactions. Startup mistakes become typed errors, optional features attach through one Plugin seam, observability is consistent, and shutdown is graceful. Everything is tested end to end against local Discord HTTP and gateway fakes in the testkit, so no network is needed.

## Stories

- Story 5.1: App builder, Runtime, Plugin seam and startup safety
- Story 5.2: Discord HTTP fake and the test Runtime
- Story 5.3: Discord gateway fake
- Story 5.4: Automatic sharding with bucketed identify
- Story 5.5: Interaction Lane and bounded event lane
- Story 5.6: Modules and typed Event Handlers with derived filters and intents
- Story 5.7: Observability contract and Error Sinks
- Story 5.8: Graceful shutdown and fatal-close reporting

## Requirements & Constraints

- `.build()` returns an `App` or one `BuildError` listing every violation; unset ports default to bounded in-memory implementations. `App::from_env()` reads `NOCTIS_*` variables.
- Startup never panics: outside a multi-thread tokio runtime, missing/failed rustls provider install, invalid token (401), disallowed intents (4014), insufficient `session_start_limit.remaining`, or a too-small fixed shard total are all typed errors naming the cause.
- Shard total defaults to Discord's recommendation; a fixed total not a multiple of `max_concurrency > 1` only warns. Same-bucket shards (`shard_id % max_concurrency`) identify in parallel.
- Reading sockets and heartbeating never wait on handlers; a 10 s handler must not delay heartbeats.
- Interactions are never dropped and reach their handler within the 3 s deadline even with the event lane and overflow full.
- Event lane full: droppable types (presence, typing, others marked) drop oldest droppable; others go to a metered overflow with a hard cap, beyond which they drop with an error log. Every drop counted per event type.
- Only event types with a registered handler are deserialized. Two handlers for one event both run; ordered handlers run first, in per-guild order.
- No panics on any Discord/user input: handler panics become error-sink events, shard keeps running.
- Token is a newtype with redacted `Debug`/`Display`, never logged.
- Shutdown on Ctrl-C/SIGTERM finishes in-flight work within a configurable grace period; fatal shard closes are logged and reported, never silent.
- Every public API change updates the Usage Skill (`references/app-and-sharding.md` etc.), reconciles the project template, re-blesses the API snapshot, and passes `skill-check`.

## Technical Decisions

- **One HTTP client:** core builds exactly one `Arc<twilight_http::Client>` per App (request timeout default 10 s) and passes it to the gateway; no other crate builds a Discord HTTP client. `.http_proxy(url)` sets `proxy(url, true)` + `ratelimiter(None)`; `.gateway_url(url)` uses twilight's gateway `proxy_url`. No test-only branch in core.
- **TLS:** `rustls` 0.23.46, `default-features = false`, features `ring, std, tls12, logging`; `App::run` installs `ring` provider if none set. twilight 0.17.1 gateway features `rustls-platform-verifier, twilight-http, zstd`; http features `rustls-platform-verifier, decompression`. `cargo deny` bans `aws-lc-rs`/`aws-lc-sys`.
- **Runtime:** defined in `noctis-core`, re-exported as `noctis::Runtime`; owns metrics recorder and a `TypeId`-keyed map of `Arc<dyn Any + Send + Sync>` resources via `resource_or_init::<T>(config)`. First App's config freezes a resource; conflicting later config is a `BuildError`. `Runtime::global()` default; `AppBuilder::runtime(&Runtime)`; `Runtime::for_test()` isolated, `is_test() == true`.
- **Builder shape:** `AppBuilder` is a tokio-style mutable builder: setters `fn x(&mut self, ..) -> &mut Self`, `build(&mut self) -> Result<App, BuildError>`; template `configure(b: &mut AppBuilder) -> &mut AppBuilder`. `build()` installs ring and requires a current tokio runtime (else `BuildViolation::NoTokioRuntime`); `App::run` requires multi-thread. Errors in noctis-core, re-exported at root: `Error{Env,Build,Start}`, `BuildError` (non-empty `Vec<BuildViolation>`), `StartError`, `EnvError`, `TokenError`. `from_env()` reads `NOCTIS_TOKEN` (trimmed, empty = unset) and `NOCTIS_HTTP_PROXY`; each later story adds its own variable. Missing token is a `StartError::MissingToken` at run, not a build error. `Token::new` strips one `Bot `, needs printable ASCII; errors carry only a char index. `http_timeout` 1 ms..=300 s. Resources: `Resource { type Config; fn init(&Config) -> Self }`; plugins forward conflicts via `AppBuilder::report`. deps-check rule `http-client` limits `twilight-http` to core, gateway, facade.
- **HTTP proxy:** `http_proxy` takes `host[:port]` or `http://host[:port]`; anything else is `BuildViolation::InvalidHttpProxy { reason: HttpProxyError }` whose message never echoes the value; `App::http_proxy()` returns the normalized authority.
- **Plugin:** `trait Plugin: Send + Sync + 'static { fn install(&self, app: &mut AppBuilder); }`; one per type, `plugin_mut::<P>()`; `build()` calls each `install` once in registration order before Registry fold and checks; install uses only public builder methods (`renderer`, port setters, `intents`, `runtime`, `module`).
- **Lanes:** `noctis-gateway` defines lanes and `Inbound { shard, received_at: Instant, kind: Interaction(Box<Interaction>, RawJson) | Event(Box<Event>) }`. `RawJson` (raw `d` as `Bytes`, interactions only) is a gateway-layer type. Shard tasks parse with `twilight_gateway::parse` and only `try_send`. Identify goes through the `IdentifyQueue` port, never `InMemoryQueue::default()`.
- **Workers:** core owns a semaphore-bounded worker set draining the interaction lane first; ordered handlers use guild-keyed sub-queues (guild id hashed to fixed sub-queue). Every future runs as `AssertUnwindSafe(fut).catch_unwind()`.
- **Modules/Registry:** `Module::new("a/b")`, `#[event]` handlers, `Module::module(child)` nesting folded parent-before-child with ancestor path recorded; Registry folded once from descriptors. Intents derived from handlers; missing intent is a `BuildError` naming GW-05, handler and intent; default intents `Intents::GUILDS`.
- **Handlers stateless:** no per-message closures/timers/tasks; shared state via `State<T>` registered at build; no globals beyond Runtime resources.
- **Errors:** per-crate `thiserror` enums wrapped by `#[non_exhaustive] noctis::Error`; `HandlerError` is the type-erased handler error. No `unwrap`/`expect` on external input.
- **Observability:** spans `noctis.shard`, `noctis.event`, `noctis.interaction` with fields `app`, `shard`, `guild_id`, `module`, route/command path. Metrics via `metrics` facade, `noctis_` prefix, `app` label: `noctis_lane_depth{lane}`, `noctis_events_dropped_total{event_type}`, `noctis_event_overflow_depth`, `noctis_shard_status{shard}`, `noctis_handler_duration_seconds`/`noctis_handler_errors_total{kind}`, `noctis_http_requests_total`. Labels never carry user/guild/channel/message ids or free text.
- **Error Sinks:** `ErrorSink` per origin (Command, Component/Modal, Autocomplete, Event, Render); default logs at `error`; receives handler errors, panics, hook errors, overflow drops, and handlers cancelled at shutdown.
- **Shutdown order:** mark shutdown started; `gateway.close_all(Resumable | Fresh)`; drain both lanes until empty or grace ends with HTTP/Responder live; await `gateway.closed()`; drop ports. Gateway reports `ShardEvent`s on a channel; only core turns them into hooks (Lifecycle Hook exposure is Story 13.5).
- **Fakes:** `noctis_testkit::fake::FakeDiscordHttp` (hyper 1.12, hyper-util 0.1.21, http-body-util 0.1.5; per-test routes, canned `GET /gateway/bot` with configurable shards/`max_concurrency`/`session_start_limit`, request log) and `FakeGateway` (tokio-websockets 0.13; HELLO, IDENTIFY/READY, RESUME/RESUMED, heartbeat ACKs with controllable interval and timing hook, scripted dispatches, close codes 4004/4014/resumable, frame log). E2E tests live in `crates/noctis-testkit/tests/`. FakeDiscordHttp is HTTP/1.1 on 127.0.0.1:0, exact method+path under `/api/v10`, 404 in Discord JSON shape, 64 MiB body cap, per-route delay; not re-exported via the facade `testkit` feature until `TestApp` (8.6).
- **Gateway URL and shard config:** `.gateway_url()` / `NOCTIS_GATEWAY_URL` accepts `ws[s]://host[:port]` (authority only, one trailing `/`), validated at `build()` into `BuildViolation::InvalidGatewayUrl` without echoing the value; authority checks are shared with `http_proxy`. `noctis_gateway::shard_config(token, intents, gateway_url, queue)` is the single place App settings become a twilight shard `Config`, pre-checking runtime, URL and TLS so twilight cannot panic; 5.4's `App::run` builds every shard through it with its `IdentifyQueue`-backed queue.
- **FakeGateway details:** uncompressed text frames only; READY's `resume_gateway_url` points back to itself; scripted via `FakeGatewayConfig`, `dispatch`, `send_raw`, validated `close`; sessions named `fake-session-{n}` counting every IDENTIFY answered; protocol errors use close codes 4001-4005; crate-internal until `TestApp` (8.6).

## Cross-Story Dependencies

- 5.2 and 5.3 fakes underpin tests for 5.4–5.8 and Epics 6–7; 5.2 adds `.http_proxy`, 5.3 `.gateway_url`.
- Uses Epic 3 ports (`IdentifyQueue`, bounded memory stores) and Epic 1 rules (GW-01/02/03/05).
- 5.6 adds `AppBuilder::module(..)` that 5.1's Plugin seam later uses; Story 12.5 extends event filters for cache needs; Story 13.1 adds Module unique-id checks; Story 12.4 adds Cluster proxy semantics; Story 13.5 exposes shard-status Lifecycle Hooks; Story 8.6 `TestApp` reuses the fakes.
