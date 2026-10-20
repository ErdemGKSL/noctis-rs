---
title: 'Story 5.4: Automatic sharding with bucketed identify'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: '19b7c81b4ea05e8ef99c14c540738f0b90652628'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `App::run` stops after its safety checks: no shard ever connects. Bots need the recommended shard count, identify buckets from `max_concurrency` through the `IdentifyQueue` port (GW-01), refusal when the identify budget or a fixed total is too small (GW-02, GW-03), typed errors for a bad token or disallowed intents (FR-1), and shard tasks that turn raw gateway messages into `Inbound` items (FR-2, AD-10, AD-21).

**Approach:** `App::run` reads `GET /gateway/bot` with the App's one HTTP client, works out a sharding plan with a pure core function, and starts `noctis_gateway::Gateway`. The gateway spawns one tokio task per shard, built with `shard_config` and a twilight `Queue` adapter over the `IdentifyQueue` port. Each task parses raw messages with `twilight_gateway::parse`, pushes `Inbound` items into a non-blocking `InboundSink`, and reports `ShardEvent`s to core. Core maps fatal closes during startup to `StartError`s. Adds `AppBuilder::shard_total(u32)` and `NOCTIS_SHARD_TOTAL`.

## Boundaries & Constraints

**Always:**
- **Builder (core `app.rs`):** `AppBuilder::shard_total(&mut self, total: u32) -> &mut Self` stores the raw value; the last call wins. `build()` checks it right after the gateway URL check and before the crypto and runtime checks: `0` records `BuildViolation::InvalidShardTotal`, with message `"shard total must be at least 1: omit shard_total to use Discord's recommendation"`. Add the getter `App::shard_total(&self) -> Option<u32>`; the `App` field is `Option<NonZeroU32>`. Both `Debug` impls print it.
- **Env:** `from_lookup` reads `NOCTIS_SHARD_TOTAL` after `NOCTIS_GATEWAY_URL`, using `read_var`'s rules. The value must be 1-10 ASCII digits that parse as a `u32` (leading zeros are allowed, `0` is accepted here and rejected by `build()`). Anything else is the new `EnvError::InvalidShardTotal { var: &'static str }`, whose message is `"environment variable `{var}` is not a shard total: set a whole number of shards such as 16"` and never contains the value.
- **Plan (new `crates/noctis-core/src/sharding.rs`):** `pub(crate) fn plan(info: GatewayInfo, fixed: Option<NonZeroU32>) -> Result<ShardPlan, StartError>`. `GatewayInfo { recommended: u32, max_concurrency: u16, remaining: u32, reset_after_ms: u64 }` is copied from twilight's `BotConnectionInfo`. `ShardPlan { total: NonZeroU32, max_concurrency: NonZeroU16, misaligned: bool }`. Checks run in this order:
  1. `max_concurrency == 0` ⇒ `InvalidGatewayInfo`.
  2. total = `fixed`, or else `recommended`, where `recommended == 0` ⇒ `InvalidGatewayInfo`.
  3. if `fixed` is set and `total < minimum`, with `minimum = ceil(recommended × 2 / 5)` computed in `u64` (Discord recommends about 1 shard per 1000 guilds, and a shard may hold at most 2500) ⇒ `ShardTotalTooSmall { total, minimum }` (GW-03).
  4. `remaining < total` ⇒ `SessionStartLimit { required: total, remaining, reset_after: Duration::from_millis(reset_after_ms) }` (GW-02).
  5. `misaligned = fixed.is_some() && max_concurrency > 1 && total % max_concurrency != 0`.
- **StartError (core `error.rs`, all variants stay `Copy`):** add `InvalidToken` (`"Discord rejected the bot token (HTTP 401 or close 4004): copy the token again from the Discord Developer Portal"`), `GatewayInfo { status: Option<u16> }` (GET /gateway/bot failed: a transport error, a timeout, a non-401 status or an undecodable body), `InvalidGatewayInfo`, `ShardTotalTooSmall { total: u32, minimum: u32 }`, `SessionStartLimit { required: u32, remaining: u32, reset_after: Duration }`, `InvalidIntents { shard: u32 }` (4013), `DisallowedIntents { shard: u32 }` (4014: `"… enable the privileged intents in the Developer Portal or stop requesting them"`), `FatalClose { shard: u32, code: u16 }` (any other close code twilight does not reconnect on), `ShardEnded { shard: u32 }` (the stream ended without a close code) and `GatewayStopped` (every shard task stopped before startup finished). Each message names the cause and the fix, and none contains the token.
- **`App::run` order:** runtime ⇒ crypto ⇒ token (unchanged), then:
  1. `self.http.gateway().authed().await` then `.model().await`. `twilight_http::error::ErrorType::Unauthorized`, or `Response { status }` with 401, ⇒ `InvalidToken`; any other `Response { status }` ⇒ `GatewayInfo { status: Some(status.get()) }`; any other failure ⇒ `GatewayInfo { status: None }`. Each failure is logged at `error` with its `Display`; twilight never prints the token.
  2. `plan(..)`. When `misaligned`, log `tracing::warn!(target: "noctis::app", total, max_concurrency, "shard total is not a multiple of max_concurrency; identify buckets are uneven")`.
  3. The identify queue is the App's `identify_queue` port when the bot set one, and `Arc::new(MemoryIdentifyQueue::new(plan.max_concurrency))` when the slot took its default. Make `Ports::is_default` non-`cfg(test)`.
  4. `Gateway::start(GatewayOptions { token: token.expose().to_owned(), intents, gateway_url, event_types: EventTypeFlags::INTERACTION_CREATE, identify_queue, total, max_concurrency }, Arc::new(DiscardInbound))`. A `ShardConfigError` is logged at `error` and returned as `GatewayStopped`. `DiscardInbound` is core-private: it logs each item at `trace` and drops it, until 5.5 adds lanes.
  5. **Startup:** receive `ShardEvent`s until every shard number in `0..total` has sent `Ready` at least once. An `Ended` before that maps `close_code` as follows: `4004` ⇒ `InvalidToken`, `4013` ⇒ `InvalidIntents`, `4014` ⇒ `DisallowedIntents`, any other `Some(code)` ⇒ `FatalClose`, `None` ⇒ `ShardEnded`. A closed channel maps to `GatewayStopped`. Returning drops the `Gateway`, which aborts every shard task. On success, log `info` "all {total} shards ready".
  6. **After startup:** keep receiving. Log `Ended` at `error` with the shard and code, `Disconnected` at `warn`, and return `Ok(())` when the channel closes (every task has ended). Signal shutdown is Story 5.8.
- **noctis-gateway public API** (re-exported at the crate root, every item with rustdoc and a doctest):
  - `Inbound { pub shard: ShardId, pub received_at: tokio::time::Instant, pub kind: InboundKind }`, `#[non_exhaustive] enum InboundKind { Interaction(Box<Interaction>, RawJson), Event(Box<Event>) }` and `RawJson` (`Bytes`; `new(Bytes)`, `as_bytes() -> &[u8]`, `into_bytes() -> Bytes`; `Debug` prints only `len`; `Clone, PartialEq, Eq`).
  - `trait InboundSink: Send + Sync + 'static { fn push(&self, inbound: Inbound); }`. Its docs state that `push` must never block or await.
  - `#[non_exhaustive] enum ShardEvent { Ready { shard: u32 }, Resumed { shard: u32 }, Disconnected { shard: u32, close_code: Option<u16> }, Ended { shard: u32, close_code: Option<u16> } }` (`Debug, Clone, Copy, PartialEq, Eq`).
  - `GatewayOptions` (pub fields as in step 4: `token: String`, `intents`, `gateway_url: Option<String>`, `event_types: EventTypeFlags`, `identify_queue: Arc<dyn IdentifyQueue>`, `total: NonZeroU32`, `max_concurrency: NonZeroU16`); its manual `Debug` redacts the token.
  - `Gateway::start(options, sink: Arc<dyn InboundSink>) -> Result<(Gateway, mpsc::UnboundedReceiver<ShardEvent>), ShardConfigError>` builds one `Config` through `shard_config(&token, intents, gateway_url.as_deref(), PortQueue { .. })` and clones it for every shard. It spawns one task per `ShardId::new_checked(i, total)` into a `JoinSet` that it owns, gives each task a clone of the `UnboundedSender`, and holds no sender itself.
  - `Gateway::shard_total() -> u32`. Dropping the `Gateway` aborts every task.
  - Also re-export `twilight_gateway::EventTypeFlags`, so core needs no `twilight-gateway` dependency.
- **PortQueue (gateway, crate-private, `Clone + Debug`):** `Queue::enqueue(id)` creates a oneshot and `tokio::spawn`s a loop with bucket `id % max_concurrency`. Each iteration runs `select! { r = queue.acquire(bucket) => …, () = tx.closed() => return }`. On `Ok(lease)` it sends `()`, drops the lease and stops. On `Err(e)` it logs `warn` and sleeps a backoff of 1 s, doubling to a 30 s cap; that sleep also races `tx.closed()`. It never drops `tx` early, because twilight immediately re-enqueues a cancelled receiver and that would busy-loop.
- **Shard task:** runs in span `noctis.shard` with field `shard`. It loops `futures_util::StreamExt::next(&mut shard)` and reads each item as follows:
  - `Err(e)` ⇒ `warn`, continue.
  - `Message::Close(frame)` ⇒ remember `frame.map(|f| f.code)` as the last close code and send `Disconnected`.
  - `Message::Text(text)` ⇒ take `received_at = Instant::now()` first, then call `classify(text, wanted, shard, received_at) -> Classified { status: Option<ShardEvent>, inbound: Option<Inbound> }` (a pure, unit-tested function).
  - Stream end ⇒ send `Ended { close_code: last }` and stop.

  `classify` works like this:
  - `GatewayEventDeserializer::from_json(&text)` returns `None` ⇒ `warn` (no payload in the log).
  - op 0 with `t == READY` ⇒ `Ready`; op 0 with `t == RESUMED` ⇒ `Resumed`.
  - When `t == INTERACTION_CREATE` and `wanted` contains it, copy the raw `d` first, through `#[derive(Deserialize)] struct Envelope<'a> { #[serde(borrow)] d: &'a RawValue }`, into `Bytes::copy_from_slice(raw.get().as_bytes())`. A failure there ⇒ `warn`, no inbound.
  - Then call `twilight_gateway::parse(text, wanted)`. `Ok(Some(GatewayEvent::Dispatch(_, Event::InteractionCreate(ic))))` ⇒ `Interaction(Box::new(ic.0), raw)`; any other `Dispatch(_, event)` ⇒ `Event(Box::new(event))`; non-dispatch or `Ok(None)` ⇒ no inbound; `Err` ⇒ `warn` with the event type only.
  - The task pushes the inbound with `sink.push` and never awaits sink capacity or core.
- **Dependencies:**
  - `crates/noctis-gateway/Cargo.toml` adds `noctis-ports`, `bytes`, `futures-util`, `serde` (workspace, derive), `serde_json = { workspace = true, features = ["raw_value"] }`, `tracing`, `twilight-model`, and tokio features `macros, rt, sync, time`.
  - `crates/noctis-core/Cargo.toml` adds `noctis-gateway.workspace = true` and tokio feature `sync`.
  - The root `Cargo.toml` `[workspace.dependencies]` adds `tracing-subscriber = { version = "0.3.23", default-features = false }` (crates.io latest stable 0.3.23, MSRV 1.65, MIT).
  - `crates/noctis-testkit/Cargo.toml` `[dev-dependencies]` adds `noctis-ports.workspace = true`, `bytes.workspace = true` and `tracing-subscriber = { workspace = true, features = ["fmt", "std"] }`.
  - No other new crates.
- **Rules catalog:** remove the GW-01, GW-02 and GW-03 entries from `xtask/rules-pending.toml`, and name at least one test per id with `gw_01`, `gw_02` or `gw_03`. **No test name may contain `gw_05`**, because GW-05 stays pending for Story 5.6. In `docs/discord-rules.md`, change only the Notes cell of GW-02 and GW-03 to describe the new enforcement (`StartError::SessionStartLimit`; `shard_total(0)` is `BuildViolation::InvalidShardTotal` and a fixed total below ⌈recommended×2/5⌉ is `StartError::ShardTotalTooSmall`), and keep the Enforcement cells as they are.
- No `unwrap`/`expect` outside tests. Every cargo command runs with `--release`. There is no test-only branch in core: tests reach shards only through `http_proxy` + `gateway_url`.

**Never:**
- Shard Ranges (`.shards(..)`, `NOCTIS_SHARDS`; Story 12.1), lanes, drop policy or metrics (5.5, 5.7), handlers, event-type derivation or the GW-05 build check (5.6), `close_all`, signal shutdown or `ShardEvent` hooks and metrics (5.8, 13.5), resume through `SessionStore` (12.2), using `/gateway/bot`'s `url` (shards use `gateway_url` or twilight's default), facade re-exports of `noctis-gateway` types.
- Never `InMemoryQueue::default()`, `twilight_gateway::create_recommended` or `create_bucket` (it panics). Never block or `.await` inside `InboundSink::push`.
- Never log or format the token, an interaction or event payload, or a `RawJson`'s content.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Plan: recommended | (r, mc, rem, fixed) = (1,1,1000,None); (20,16,1000,None); (17,16,1000,None) | total 1/20/17, mc as given, `misaligned` false ×3 (recommended never warns) |
| 2 | Plan: bad info | (5,0,1000,None); (5,0,0,Some(3)); (0,1,1000,None) | `InvalidGatewayInfo` ×3 (mc checked first) |
| 3 | Plan: zero recommended, fixed | (0,1,1000,Some(3)) | total 3 (minimum 0) |
| 4 | Plan: GW-03 boundaries | r=1 f=1; r=2 f=1; r=3 f=1; r=3 f=2; r=5 f=2; r=10 f=3; r=10 f=4; r=2 f=100; r=u32::MAX f=1717986918 / 1717986919 (rem u32::MAX) | ok; ok; `TooSmall{1,2}`; ok; ok; `TooSmall{3,4}`; ok; ok; `TooSmall{1717986918,1717986919}` / ok (no overflow) |
| 5 | Plan: GW-02 | total 20 rem 19 reset 14_400_000; rem 20; total 1 rem 0; rem 0 + reset `u64::MAX` | `SessionStartLimit{20,19,4h}`; ok; `{1,0,..}`; `reset_after == Duration::from_millis(u64::MAX)`, no panic |
| 6 | Plan: order | (10,1,0,Some(1)) | `TooSmall{1,4}` (before the limit) |
| 7 | Plan: misaligned | fixed/mc 3/2; 4/2; 3/1; 16/16; 1/16; 17/16; 32/16 | true, false, false, false, true, true, false |
| 8 | Builder | `shard_total(5)`; unset; `0`; `0` then `5`; `u32::MAX`; `0` + timeout 0 + `gateway_url("http://x")` | `Some(5)`; `None`; `[InvalidShardTotal]`; `Some(5)`; `Some(u32::MAX)`; violations [InvalidHttpTimeout, InvalidGatewayUrl, InvalidShardTotal] in that order |
| 9 | Env | `NOCTIS_SHARD_TOTAL` absent / `""` / `" \t"` / `" 12 \n"` / `"007"` / `"4294967295"` / `"0"` / `"4294967296"` / `"-1"` / `"+5"` / `"1_000"` / `"1e3"` / `"abc"` / `"１２"` / `"12345678901"` / non-UTF-8 | None / None / None / 12 / 7 / u32::MAX / from_env ok, build `InvalidShardTotal` / `InvalidShardTotal{var}` ×8 (message lacks the value) / `NotUnicode{var:"NOCTIS_SHARD_TOTAL"}` |
| 10 | PortQueue | mc 16: `enqueue(17)`, `enqueue(16)`, `enqueue(u32::MAX)`; mc 1: `enqueue(5)` | port sees `acquire(1)`, `acquire(0)`, `acquire(15)`, `acquire(0)`; receivers resolve `Ok(())` |
| 11 | PortQueue errors | port fails 2× then grants (paused clock) | 3 acquires, granted at ≥ 1 s + 2 s; one `Ok(())`; no busy loop |
| 12 | PortQueue cancel | receiver dropped while acquire pending / during backoff | spawned task ends; no further acquire (counted) |
| 13 | classify: interaction | `{"op":0,"s":2,"t":"INTERACTION_CREATE","d": {"id":"5", …,"x":"ü🙂ü"} }` (spaces kept), wanted ⊇ INTERACTION_CREATE | `Interaction`, id 5; `raw.as_bytes()` equals the exact `d` slice byte for byte |
| 14 | classify: event | `MESSAGE_DELETE` dispatch, wanted all; wanted only INTERACTION_CREATE | `Event(MessageDelete)`; nothing |
| 15 | classify: status | READY dispatch (wanted none); RESUMED | status `Ready`/`Resumed`, inbound none |
| 16 | classify: non-dispatch | op 10 HELLO, op 11, op 9 `d:false`, op 7 | nothing |
| 17 | classify: bad input | `not json`; `""`; INTERACTION_CREATE with `"d":{}`; with `"d":null`; `t:"FOO_BAR"`; INTERACTION_CREATE not wanted | nothing ×6, no panic; parse failures logged without payload |
| 18 | Startup errors map | `Ended` code 4004 / 4013 / 4014 / 4010 / 4011 / 4012 / None; channel closed | `InvalidToken` / `InvalidIntents{s}` / `DisallowedIntents{s}` / `FatalClose{s,4010}` / `{s,4011}` / `{s,4012}` / `ShardEnded{s}`; `GatewayStopped` |
| 19 | E2E GW-01 default queue | HTTP fake `shards 20, mc 16`, gateway fake Ready; `App::run` spawned | 20 connections, IDENTIFY `d.shard` = {[i,20]}; shards 0-15 identify within 2 s of the first; each shard 16-19 ≥ 4.5 s after shard i-16; run still pending |
| 20 | E2E GW-01 port | same with `.identify_queue(Recording(MemoryIdentifyQueue::new(16)))` | recorded buckets == `shard % 16` for all 20; default never built |
| 21 | E2E single | defaults (`shards 1, mc 1`) | 1 connection, IDENTIFY `[0,1]`, token `Bot fake-token`, intents GUILDS; run pending 1 s after READY; one `GET /api/v10/gateway/bot` |
| 22 | E2E GW-03 | `.shard_total(4)`, recommended 20 | `Err(Start(ShardTotalTooSmall{4,8}))`; 0 gateway connections |
| 23 | E2E warn | separate binary with a global capturing subscriber: `.shard_total(3)`, mc 2 | warning logged with total 3 and max_concurrency 2; 3 shards identify |
| 24 | E2E GW-02 | `shards 20, remaining 19` | `SessionStartLimit{20,19,..}`; 0 connections |
| 25 | E2E HTTP | route `GET /gateway/bot` → 401; 500; 200 `{}`; delay 2 s + `http_timeout(100 ms)`; `max_concurrency 70000`; `shards 0`; `mc 0` | `InvalidToken`; `GatewayInfo{Some(500)}`; `{None}`; `{None}`; `{None}`; `InvalidGatewayInfo`; `InvalidGatewayInfo`; 0 connections each |
| 26 | E2E fatal at identify | `identify: Close(4014)`, `Close(4004)`, `Close(4013)`, `Close(4011)` | run returns within 10 s: `DisallowedIntents{0}`, `InvalidToken`, `InvalidIntents{0}`, `FatalClose{0,4011}`; exactly 1 connection each |
| 27 | E2E resumable | `identify: Close(4000)` flipped to `Ready` after the first IDENTIFY | startup succeeds (≥ 2 connections), run pending |
| 28 | E2E after startup | 2 shards ready, then `gw.close(4014, "")` | run returns `Ok(())` within 5 s |
| 29 | Missing token | no token | `MissingToken`, zero HTTP requests (existing behaviour) |
| 30 | Gateway direct | `Gateway::start` (total 1, all event types, `MemoryIdentifyQueue::new(1)`) with a channel sink; `gw.send_raw` INTERACTION_CREATE; `gw.dispatch("MESSAGE_DELETE", ..)` | `Ready{0}` event; Inbound `Interaction` with `shard == ShardId::ONE`, exact raw bytes, `received_at` between send and receive; Inbound `Event(MessageDelete)`; dropping `Gateway` closes the event channel |

</intent-contract>

## Code Map

- `crates/noctis-core/src/app.rs`: `run()` ~L218-241 (replace the warn stub; drop the `unused_async` allow); env consts L27-32 (add `SHARD_TOTAL_VAR`); the `App` struct L57 and `AppBuilder` L281 (add the `shard_total` fields); getters near `gateway_url()` L160; `from_lookup` L338; setter after `gateway_url` L447; `build` L607 (check after the gateway URL match, field into `App`); Debug impls L242 and L683; `ports()` L207 (`dead_code` allow may go).
- `crates/noctis-core/src/ports.rs`: `ports!` macro (`Ports.identify_queue`, the `defaults` set); `is_default` L94 is `#[cfg(test)]`, so make it crate-visible always.
- `crates/noctis-core/src/error.rs`: `BuildViolation` (add `InvalidShardTotal` after `InvalidGatewayUrl`), `StartError` (new variants), `EnvError` (add `InvalidShardTotal`).
- `crates/noctis-core/src/token.rs` L58: `pub(crate) fn expose()` is the only token read.
- `crates/noctis-core/src/startup.rs`: the existing checks, reused unchanged.
- `crates/noctis-core/src/app/tests.rs` L27-35: `lookup_var` panics on unknown variable names, so add `NOCTIS_SHARD_TOTAL`.
- `crates/noctis-core/src/lib.rs`: add `mod sharding;` and update the crate docs on `run`.
- `crates/noctis-gateway/src/{lib.rs,shard_config.rs}`: 5.3's `shard_config` (reuse it as is) and the crate docs; add the modules `inbound.rs`, `queue.rs` (PortQueue), `shard.rs` (task + `classify`) and `gateway.rs`.
- `crates/noctis-ports/src/{identify.rs,memory/identify.rs}`: the `IdentifyQueue::acquire(bucket) -> Lease` port, and `MemoryIdentifyQueue::new(NonZeroU16)` (cancel-safe, FIFO, `InvalidValue` when bucket ≥ mc).
- twilight (read-only, `~/.cargo/registry/src/*/`):
  - gateway 0.17.1 `shard.rs` L680-707: a cancelled identify receiver is re-enqueued immediately.
  - `lib.rs` L32-45: `parse` and the `Message::{Text, Close}` re-exports.
  - `config.rs` L41: `Config: Clone`.
  - gateway-queue 0.17.0 `lib.rs` L25: `trait Queue { fn enqueue(&self, id: u32) -> oneshot::Receiver<()>; }`.
  - model 0.17.1 `gateway/event/gateway.rs` L79-117: `GatewayEventDeserializer::{from_json, op, event_type}`.
  - `gateway/close_code.rs` L93: `can_reconnect`.
  - `gateway/connection_info/bot_connection_info.rs`: `{shards, session_start_limit{max_concurrency: u16, remaining, reset_after}}`.
  - http 0.17.1 `error.rs` L110: `ErrorType::{Response{status}, Unauthorized}`; `StatusCode::get()`.
- `crates/noctis-testkit/src/fake.rs` L67-140: `GatewayBot`/`SessionStartLimit` (`max_concurrency: u32`); `route(Method::GET, "/gateway/bot", ..)` overrides the canned answer; `FakeResponse::delay`.
- `crates/noctis-testkit/src/fake/gateway.rs`: `FakeGatewayConfig.identify` (`IdentifyReply::{Ready, Close(u16)}`), `set_config`, `dispatch`, `send_raw`, `close`, `frames()`, `connections()`, `wait_for_frame`.
- `crates/noctis-testkit/tests/{http_fake.rs,gateway_fake.rs}`: the `app(..)` helper and multi-thread test style to copy.
- `xtask/rules-pending.toml` L357-370: GW-01..03 entries to delete; `xtask/src/rules_catalog/mod.rs` L1-30 describes how evidence is matched (by the test-name snake token).
- `docs/discord-rules.md` L271-272: the GW-02 and GW-03 Notes cells.
- `examples/project-template/`:
  - `.claude/skills/noctis/references/app-and-sharding.md`: `run()` bullet ~L36, methods tables (move `.shard_total` to Implemented; `.shards(ShardRange)` stays [planned, Story 12.1]), Errors (new variants), §2 env (`NOCTIS_SHARD_TOTAL` implemented), §4 Sharding (fixed-total wording: 0 = BuildError, too small = `StartError::ShardTotalTooSmall` at run).
  - `SKILL.md` L21: the API hash.
  - `README.md` L58 and `.env.example` (add `# NOCTIS_SHARD_TOTAL=`).

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-core/src/{error.rs,sharding.rs,lib.rs}`: add the variants and `plan`; unit tests for Matrix 1-7 (names include `gw_02`/`gw_03`).
2. `crates/noctis-core/src/{app.rs,app/tests.rs,ports.rs}`: setter, getter, env, build check, Debug and `run` per Boundaries; core-private `DiscardInbound`; a pure `fn start_error(shard, close_code) -> StartError` for Matrix 18; tests for Matrix 8, 9 and 18.
3. `crates/noctis-gateway/{Cargo.toml,src/*}`: the API per Boundaries; unit tests for Matrix 10-12 (`#[tokio::test(start_paused = true)]` with a scripted port) and 13-17 (fixtures built with `serde_json`; first check that the interaction fixture deserializes into twilight's `Interaction`, adding required fields as needed).
4. `crates/noctis-core/Cargo.toml` and the root `Cargo.toml`: the dependencies.
5. `crates/noctis-testkit/{Cargo.toml,tests/sharding.rs,tests/sharding_warning.rs}`: Matrix 19-30 with `#[tokio::test(flavor = "multi_thread")]`. Run `App::run` in `tokio::spawn` and abort it at the end. Bound every wait with ≤ 15 s timeouts and poll the fakes' logs. Test names include `gw_01` (rows 19, 20), `gw_02` (24) and `gw_03` (22). `sharding_warning.rs` holds only Matrix 23 and installs a global `tracing_subscriber::fmt` subscriber whose `MakeWriter` appends to an `Arc<Mutex<Vec<u8>>>`.
6. `xtask/rules-pending.toml` and `docs/discord-rules.md`: per Boundaries.
7. `xtask/public-api.txt`: run `cargo run --release -p xtask -- api-snapshot --bless` and copy the printed hash to `SKILL.md` L21.
8. `examples/project-template/`: update the skill, README and `.env.example` per the Code Map, then run `skill-check`.
9. `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: already holds this story's planning decisions; append any implementation decisions.

**Acceptance Criteria:**
- Given `FakeDiscordHttp` answering 20 shards with `max_concurrency = 16`, when `App::run` starts without a fixed total, then 20 shards connect, each in its own task, identify through the `IdentifyQueue` port in buckets `shard_id % 16`, and the 16 first-bucket-round shards identify in parallel (Matrix 19, 20).
- Given a fixed total, when it is 0 then `build()` returns `InvalidShardTotal`; when it is below the GW-03 minimum then `run` returns `ShardTotalTooSmall` before any connection; when it is not a multiple of `max_concurrency > 1` then a warning is logged and startup continues.
- Given `remaining` lower than the shards to identify, when `run` starts, then it returns `SessionStartLimit` and opens no gateway connection.
- Given a 401 from `/gateway/bot` or a 4004/4014 close during startup, when `run` starts, then it returns `InvalidToken`/`DisallowedIntents { shard }` and never hangs or returns `Ok`.
- Given shard tasks receiving gateway messages, when they parse them with `twilight_gateway::parse`, then interactions become `Inbound { shard, received_at, kind: Interaction(_, RawJson) }` with the exact raw `d` bytes and other wanted events become `Inbound` `Event`s (Matrix 13, 30).
- Given the public API change, when `api-snapshot`, `skill-check`, `rules-catalog`, `deps-check` and `cargo deny check` run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 24 findings — high 0, medium 0, low 9, false 15, maybe-false 0
- findings:
  - `[low]` `[reject]` Startup can hang forever when a shard never reaches READY (unreachable gateway, repeated non-fatal closes); no startup timeout — real: `await_startup` has no bound and twilight reconnects indefinitely, but each failure is logged at `warn` by `drive`, retrying through a network outage at boot is defensible, a bad `gateway_url` is rare, and the fix needs a timeout plus a new public `StartError` variant.
  - `[low]` `[reject]` Fatal closes after startup end `run` with `Ok(())`, indistinguishable from a clean stop — real (`supervise` then `Ok(())`), but out of scope by intent: epics assign post-startup fatal-close reporting to Story 5.8 ("Graceful shutdown and fatal-close reporting") and the hook to 13.5; `run`'s return contract is settled there.
  - `[low]` `[reject]` One shard ending after startup goes unnoticed except for an `error` log — out of scope by intent: shard-state hooks and metrics are Story 13.5 / 5.7 / 5.8 per epics.
  - `[false]` `[reject]` Out-of-range `/gateway/bot` body maps to `GatewayInfo{None}` with a network-oriented message — the spec mandates `GatewayInfo{status: None}` for an undecodable body (Matrix 25), the message points at the HTTP proxy (the only realistic source of such a body), and the decode error is logged at `error` with its `Display`.
  - `[false]` `[reject]` Lease dropped before the identify is sent — `noctis_ports::Lease` is a plain value with no `Drop`; the port contract (identify.rs) defines the window as "at most one Lease per bucket per 5 s" at grant time, not at drop.
  - `[false]` `[reject]` GW-03 Enforcement cell still says "App-build" — the doc's convention treats the startup `/gateway/bot` fetch as App-build (GW-01's cell reads "App-build (fetch `GET /gateway/bot` at startup)"), and the spec deliberately keeps Enforcement cells for the rules-catalog classification.
  - `[false]` `[reject]` GW-01 row still promises "buckets in order" — that Limit cell restates Discord's rule; the Notes cell makes no ordering claim about noctis, and Discord does not reject out-of-order identifies within the 5 s per-bucket limit, which `MemoryIdentifyQueue` enforces.
  - `[false]` `[reject]` GW-01..03 enforcement evidence missing — `xtask/src/rules_catalog/mod.rs` matches `unit` evidence by the test-name snake token; `gw_01_*` (testkit sharding.rs, gateway queue.rs), `gw_02_*` and `gw_03_*` (core sharding.rs, testkit sharding.rs) exist.
  - `[low]` `[reject]` Very large totals allocate `vec![false; total]` and spawn `total` tasks — needs a proxy returning `remaining` ≥ a huge total (`plan` refuses with `SessionStartLimit` otherwise); not met in everyday use, and the fix is a new sanity ceiling.
  - `[false]` `[reject]` Unbounded shard-event channel can grow — its only consumer, `App::run`, drains it until close (`await_startup` then `supervise`); `noctis-gateway` is not re-exported through the facade, and events are tiny and paced by twilight's reconnect backoff.
  - `[false]` `[reject]` GW-01 E2E timing assertions flaky / too loose — the 2 s first-round window is ample for 16 loopback handshakes, and a serialized-bucket regression fails exactly that check; the second-round ≥ 4.5 s check pins the 5 s window.
  - `[false]` `[reject]` No test for re-READY during startup or `Ended` after READY — `startup_waits_for_every_ready_and_maps_closed_channel` sends duplicate `Ready`s and checks dedup; failing startup on any `Ended` before all shards are ready is the spec's protocol (Boundaries, run step 5).
  - `[false]` `[reject]` No test that a setter overrides `NOCTIS_SHARD_TOTAL` — `from_lookup` and `shard_total(..)` write the same `AppBuilder.shard_total` field, so the setter cannot fail to win; `0` rejected at build is documented in the skill and tested (`from_env_shard_total_zero_fails_at_build`).
  - `[low]` `[reject]` Permanent port error (bot-set `MemoryIdentifyQueue` smaller than `max_concurrency`) retries forever and startup hangs — real, but only for an explicit mis-sized port; each retry logs the `InvalidValue` at `warn`, and a fatal path needs a new error channel from the queue to core.
  - `[low]` `[reject]` A panicking `IdentifyQueue::acquire` drops `tx` and twilight re-enqueues in a hot loop — requires a buggy port (the shipped one never panics); the fix (`catch_unwind` plus backoff) adds complexity.
  - `[low]` `[reject]` `await_startup` has no startup timeout (edge) — same defect as the first row; rejected for the same reasons.
  - `[false]` `[reject]` `/gateway/bot`'s `url` is dropped — the spec's Never list forbids using it deliberately; shards use `gateway_url` or twilight's default Discord gateway.
  - `[low]` `[patch]` (gap) Shard-task logs that must never carry the payload are untested — pre-verified; patch: add `crates/noctis-testkit/tests/shard_log_redaction.rs` with a capturing subscriber that sends an undeserializable frame and an unparsable `INTERACTION_CREATE` carrying a sentinel and asserts the warnings appear without it.
  - `[low]` `[patch]` (gap) `InboundKind::Interaction`'s payload-free `Debug` is unasserted — pre-verified; patch: assert `!format!("{inbound:?}").contains("interaction-token")` in `interaction_keeps_exact_raw_bytes`.
  - `[false]` `[reject]` (intent D1) GW-03 uses ⌈recommended×2/5⌉ and fails at start, not a guild count at build — the spec settled it deliberately: no guild count exists before READY, the epic AC allows "`build`/start", and only `shard_total(0)` is knowable at build.
  - `[false]` `[reject]` (intent D2) After startup a 4014 ends `run` with `Ok(())` — the epic AC is scoped to "when startup runs"; post-startup fatal-close reporting belongs to Stories 5.8 and 13.5.
  - `[false]` `[reject]` (intent D3) `Inbound` production is checked at the `Gateway` surface, not via `App::run` — lanes are Story 5.5; the AC concerns shard tasks, which Matrix 30 exercises directly.
  - `[false]` `[reject]` (intent D4) GW-01 "buckets in order" and timing checked loosely — per-bucket FIFO and the 5 s window hold; shard-id ordering is a recorded decision and not a Discord-enforced limit; the timing test covers both readings AC1-(i)/(ii).
  - `[false]` `[reject]` (intent D5) GW-05 cited but its row is unchanged — GW-05's enforcement is the handler-derived intent check at `build()` (Story 5.6); this story implements the 4014 → `DisallowedIntents` mapping (Matrix 26) and the spec keeps GW-05 pending deliberately.

## Design Notes

- **Why a sink trait, not lanes:** AD-10 lanes and drop policy are Story 5.5. 5.4 fixes the shard-side contract (parse, then a non-blocking push). 5.5 implements `InboundSink` with its lanes, and in 5.4 core discards because no handler exists yet (5.6). Tests observe `Inbound` through `Gateway::start` with their own sink.
- **Why the GW-03 minimum is `ceil(recommended×2/5)`:** Discord does not return a guild count. It recommends about one shard per 1000 guilds, so `guilds ≤ recommended×1000`, and GW-03 allows 2500 guilds per shard. Discord's own 4011 close maps to `FatalClose` as a backstop. A recommended total never warns or fails this check.
- **Why startup errors are `StartError`, not `BuildError`:** `build()` makes no connection (5.1). Only `shard_total(0)` is knowable at build time.
- **The `Ended` and channel-close protocol:** only shard tasks hold senders, so channel closure means "every task ended", including a twilight panic. Core needs no `JoinSet` access.
- PortQueue sketch:
  ```rust
  loop { select! { r = q.acquire(bucket) => match r {
      Ok(_lease) => { let _ = tx.send(()); return; }
      Err(e) => { warn!(..); select! { () = sleep(backoff) => {}, () = tx.closed() => return } backoff = (backoff * 2).min(MAX); } },
    () = tx.closed() => return } }
  ```

## Verification

**Commands** (repo root; CI is paused, so local gates only; never a debug build):
- `cargo fmt --all --check`: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace`: exit 0. Run `-p noctis-testkit --test sharding` 3 times to check for flakes.
- `cargo hack test --release --workspace --each-feature`: exit 0.
- `cargo +1.91 check --release --workspace --all-targets`: exit 0.
- `cargo deny check`: exit 0.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog` (GW-01..03 now covered), `-- api-snapshot` and `-- skill-check`: each exits 0. api-snapshot needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0 (see AGENTS.md pitfalls).
- `cargo tree -i aws-lc-rs`: no match. `test ! -d target/debug`: true.

## Auto Run Result

- **Summary:** Automatic sharding in `App::run`: fetches `GET /gateway/bot`, plans the shard total (recommended, `NOCTIS_SHARD_TOTAL` or the `shard_total(..)` setter), checks session-start limits and the GW-03 minimum (ceil(recommended*2/5)), starts shards through `noctis-gateway` with identify paced by an `IdentifyQueue` port (`MemoryIdentifyQueue`: `max_concurrency` buckets, at most one lease per bucket per 5 s), waits for every READY, maps fatal closes (4004/4010/4011/4013/4014) to typed `StartError` variants, and parses frames into payload-free `Inbound` values pushed to an `InboundSink` (core discards them until Stories 5.5 and 5.6).
- **Files changed:**
  - `crates/noctis-core/src/app.rs`, `app/tests.rs`: run/startup orchestration, env/setter shard total, tests.
  - `crates/noctis-core/src/sharding.rs`: shard plan, GW-02/GW-03 checks.
  - `crates/noctis-core/src/error.rs`, `lib.rs`, `ports.rs`: `StartError` variants, exports.
  - `crates/noctis-gateway/src/{gateway,shard,queue,inbound,lib}.rs`: shard tasks, port-backed identify queue, `Inbound`/`InboundSink`.
  - `crates/noctis-testkit/tests/sharding.rs`, `sharding_warning.rs`, `shard_log_redaction.rs`: E2E sharding, GW-01 timing, warning and log-redaction tests.
  - `docs/discord-rules.md`, `xtask/rules-pending.toml`: GW-01..03 marked enforced.
  - `xtask/public-api.txt`, `examples/project-template/**` (skill, references, `.env.example`, README): API snapshot and Usage Skill updates.
  - `Cargo.toml`, `Cargo.lock`, crate `Cargo.toml`s: dependencies.
- **Review findings (24: high 0, medium 0, low 9, false 15):**
  - Patched (2, both low): a shard log-redaction test (`shard_log_redaction.rs`) and a payload-free `Debug` assertion for `InboundKind::Interaction`.
  - Deferred: none.
  - Rejected (22): no startup timeout (twice; retrying is logged at warn, and a fix needs a new public error); post-startup fatal close returns `Ok(())` and an unnoticed shard end (Stories 5.8, 5.7, 13.5); undecodable `/gateway/bot` maps to `GatewayInfo{None}` (as the spec requires); lease drop timing (`Lease` has no `Drop`); GW-03 Enforcement cell (follows the doc convention); GW-01 "buckets in order" wording (restates Discord's rule); missing GW evidence (tests exist); huge-total allocation (needs a hostile proxy); unbounded event channel (drained by `run`); E2E timing flakiness (windows are ample and precise); missing re-READY/`Ended` test (already covered); setter vs env precedence (same field); mis-sized port retrying forever (warn-logged, needs a new channel); panicking port hot loop (needs a buggy port); dropped `/gateway/bot` `url` (spec forbids using it); intent items D1 to D5 (settled by the spec or scoped to Stories 5.5, 5.6 and 5.8).
- **Follow-up review recommended:** false. Patched by verdict: high 0, medium 0, low 2.
- **Verification:** all local gates in `gates.sh` green: fmt, clippy `-D warnings`, workspace tests (release), cargo-hack each-feature, MSRV 1.91 check, cargo deny, xtask deps-check, rules-catalog, api-snapshot and skill-check. No `target/debug`. CI is paused by the maintainer.
- **Residual risks:** startup has no timeout (an unreachable gateway or a mis-sized identify port retries forever, logged at warn); a fatal close after startup is not yet surfaced (Story 5.8); no ceiling on the shard total when a proxy returns huge values.
