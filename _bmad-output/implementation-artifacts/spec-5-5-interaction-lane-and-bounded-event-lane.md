---
title: 'Story 5.5: Interaction Lane and bounded event lane'
type: 'feature'
created: '2026-10-20'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Core still discards every `Inbound` through `DiscardInbound` (5.4). FR-5 and AD-10 need two lanes between the shard tasks and core. The interaction lane has priority, is never dropped and has no bound. The event lane is bounded and has a per-type drop policy and a metered overflow. Every drop is counted, and every lane's depth is exposed as a metric with the `app` label (NFR-5, AD-29). Shard tasks must never wait on either lane.

**Approach:** Add a `lanes` module to `noctis-gateway`. It holds a `LaneSender`, which implements the existing sync `InboundSink`, and a single-consumer `LaneReceiver` that always drains interactions first. One `std::sync::Mutex` over three `VecDeque`s and a `tokio::sync::Notify` implement them. Metric handles are registered once, through the `metrics` 0.24 facade. `App::run` replaces `DiscardInbound` with the lanes and spawns a core drain task. Until Story 5.6 adds handlers, that task only traces and drops each item.

## Boundaries & Constraints

**Always:**
- **Gateway API (new `crates/noctis-gateway/src/lanes.rs`, re-exported at the crate root, each item with rustdoc and a doctest):**
  - `pub const DROPPABLE_EVENTS: EventTypeFlags = EventTypeFlags::PRESENCE_UPDATE.union(EventTypeFlags::TYPING_START);`
  - `#[derive(Debug, Clone)] pub struct LaneConfig { pub app: Arc<str>, pub event_capacity: NonZeroUsize, pub overflow_capacity: usize, pub droppable: EventTypeFlags }`. Its `Default` is `app "default"`, `event_capacity 4096`, `overflow_capacity 16384` and `droppable DROPPABLE_EVENTS`. These defaults are also `pub const DEFAULT_EVENT_CAPACITY: NonZeroUsize` and `DEFAULT_OVERFLOW_CAPACITY: usize`.
  - `pub fn lanes(config: LaneConfig) -> (LaneSender, LaneReceiver)`. It registers and caches three gauge handles, each set to 0: `noctis_lane_depth{app, lane="interaction"}`, `noctis_lane_depth{app, lane="event"}` and `noctis_event_overflow_depth{app}`. The label values come from `config.app`, unvalidated (Story 7.4 validates App ids). No queue is pre-allocated.
  - `#[derive(Clone)] LaneSender`. It implements `InboundSink` and has a manual `Debug` that prints only the depths. A live-sender count rises on `Clone` and falls on `Drop`.
  - `LaneReceiver` (single consumer, not `Clone`, `Send`) has a manual `Debug` that prints only the depths. Its methods:
    - `pub async fn recv(&mut self) -> Option<Inbound>`. It is cancel-safe. Once the last sender has dropped, it returns the remaining items and then `None`.
    - `pub fn try_recv(&mut self) -> Option<Inbound>`
    - `pub fn depths(&self) -> LaneDepths`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub struct LaneDepths { pub interaction: usize, pub event: usize, pub overflow: usize }`.
- **Push (`InboundSink::push`)** is sync and O(1), except for the eviction scan, which is O(`event_capacity`) and only runs when the lane is full. The `std::sync::Mutex` is locked with `unwrap_or_else(PoisonError::into_inner)` and is never held across an `.await`. Metric and log calls are made after the guard is released. The rules:
  1. If the receiver has dropped, discard the item. Nothing is counted or logged.
  2. `InboundKind::Interaction` is pushed to the back of the interaction queue. It is never dropped and has no bound.
  3. For `InboundKind::Event(e)`, let `t = e.kind()`. The event is *droppable* iff `config.droppable.intersects(EventTypeFlags::from(t))`. The lane is *full* iff `overflow` is non-empty or `event.len() == event_capacity`. Invariant: a non-empty overflow implies `event.len() == event_capacity`.
     - The lane is not full: push to the back of `event`.
     - The lane is full and the event is droppable: remove the oldest droppable event queued in `event` and count that event's type, then push the new event to the back. If no droppable event is queued, drop the new event and count `t`. Either drop is logged at `debug` with `event_type` only.
     - The lane is full and the event is not droppable: if `overflow.len() < overflow_capacity`, push it to the back of `overflow`. Otherwise drop it, count `t`, and log `tracing::error!(target: "noctis::lanes", event_type, overflow_capacity, "event overflow is full; event dropped")`.
     - Every drop increments `metrics::counter!("noctis_events_dropped_total", "app" => .., "event_type" => t.name().unwrap_or("UNKNOWN"))` by 1.
  4. After an item is enqueued, update the affected depth gauge and call `notify_one()`. A stored permit means a wake-up is never lost.
- **Pop (`recv`/`try_recv`):** the first non-empty source wins, in this order:
  1. The front of `interaction`.
  2. The front of `event`. If `overflow` is non-empty, its front then moves to the back of `event` (refill keeps FIFO order within the non-droppable stream).
  3. Nothing: `None` when no sender is left (check this after the queues, under the lock), else `recv` awaits `Notify::notified()` and loops.

  Each pop updates the gauges. Dropping the `LaneReceiver` marks it gone and clears the queues.
- **Gauges** take `depth as f64`. Allow `clippy::cast_precision_loss` with a narrowly scoped `#[expect(.., reason)]`.
- **Core (`crates/noctis-core/src/app.rs`, `run`):** delete `DiscardInbound`. Before `Gateway::start`, call `let (sender, receiver) = noctis_gateway::lanes(LaneConfig::default())`, so the `app` label is `"default"` until Story 7.4. Spawn `drain(receiver)` with `tokio::spawn` and keep its handle in a core-private `AbortOnDrop(JoinHandle<()>)` that aborts on every return path. Pass `Arc::new(sender)` as the sink. `drain` is `while let Some(inbound) = receiver.recv().await { dispatch(inbound) }`. In 5.5, `dispatch` is a sync fn that logs `trace!(target: "noctis::app", shard, kind = ?inbound.kind, "no handler registered")` and drops the item. It never awaits.
- **Dependencies:**
  - Root `[workspace.dependencies]`: add `metrics-util = { version = "0.20.4", default-features = false }` (crates.io latest stable, MSRV 1.71.1, MIT). `metrics = "0.24"` already resolves to 0.24.6 (MSRV 1.71.1, MIT, only dependency `rapidhash` 4.x, MIT OR Apache-2.0).
  - `crates/noctis-gateway/Cargo.toml`: `[dependencies] metrics.workspace = true`; `[dev-dependencies]` `metrics-util = { workspace = true, features = ["debugging"] }`, `tracing-subscriber = { workspace = true, features = ["fmt", "std"] }`.
  - `crates/noctis-testkit/Cargo.toml` `[dev-dependencies]`: `metrics.workspace = true`, `metrics-util = { workspace = true, features = ["debugging"] }`.
  - No other new crates. `noctis-core` gets no `metrics` dependency in 5.5.
- No `unwrap`/`expect` outside tests. Never log a payload: logs carry `event_type`, `shard` and capacities only. Every cargo command runs with `--release`.

**Never:**
- The worker set, semaphores, per-guild ordering, `catch_unwind` or handlers (Story 5.6).
- Author-facing lane setters or env variables, or a facade re-export of the lane types (Story 5.6 adds `.event_lane(..)`; see Design Notes).
- Error Sinks, spans `noctis.event`/`noctis.interaction`, the `metrics` recorder in `Runtime` (Story 5.7). Shutdown draining (Story 5.8). `AppId` (Story 7.4).
- Awaiting, blocking or `tokio::sync::Mutex` inside `push`. A `tokio::sync::mpsc` channel for the event lane (a channel cannot evict from the middle). Any change to the public API snapshot.

## I/O & Edge-Case Matrix

Notation: `C` is `event_capacity` and `O` is `overflow_capacity`. `M` is a non-droppable event (`MESSAGE_DELETE`), `T` is `TYPING_START`, `P` is `PRESENCE_UPDATE` and `I` is an interaction. The default droppable set applies unless a row says otherwise. "cnt{X}" is `noctis_events_dropped_total{app, event_type="X"}`.

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Empty | new lanes; `try_recv` | `None`; depths `{0,0,0}`; three gauges registered at 0 with exact labels `{app,lane}` / `{app}` |
| 2 | FIFO | C=3: push M1 M2 M3 | depths `{0,3,0}`; pops M1, M2, M3 |
| 3 | Priority | push M1 M2 I1 M3 I2 | pops I1, I2, M1, M2, M3 |
| 4 | Evict oldest droppable | C=3, lane [T1 M1 P1]; push T2 | lane [M1 P1 T2]; cnt{TYPING_START}=1; overflow 0 |
| 5 | Evict a different type | C=3, lane [M1 P1 T1]; push T2 | P1 evicted; cnt{PRESENCE_UPDATE}=1; lane [M1 T1 T2] |
| 6 | No droppable queued | C=3, lane [M1 M2 M3]; push T1 | T1 dropped; cnt{TYPING_START}=1; lane unchanged; no `error` log |
| 7 | Overflow then cap | C=3, O=2, lane [M1 M2 M3]; push M4 M5 M6 | overflow [M4 M5]; overflow gauge 2; M6 dropped; cnt{MESSAGE_DELETE}=1; exactly one `error` log with `event_type` and no payload sentinel |
| 8 | Refill order | after #7, pop ×5 | M1..M5 in order; after the 1st pop depths `{0,3,1}`; final `{0,0,0}` |
| 9 | Droppable while overflow non-empty | C=2, O=2, lane [T1 M1], overflow [M2]; push T2 | T1 evicted, lane [M1 T2], overflow [M2]; pops M1, T2, M2 |
| 10 | Full of droppables, non-droppable arrives | C=2, O=1, lane [T1 T2]; push M1 | M1 goes to overflow (non-droppables never evict) |
| 11 | O = 0 | C=1, lane [M1]; push M2 | M2 dropped with an `error` log; cnt=1 |
| 12 | C = 1 | push M1, then M2 | M1 in the lane, M2 in overflow |
| 13 | Empty droppable set | `droppable = empty`, C=1, O=1, lane [T1]; push T2, T3 | T2 to overflow; T3 dropped with an `error` log; cnt{TYPING_START}=1 |
| 14 | Custom droppable | `droppable = MESSAGE_DELETE`, C=1, lane [M1]; push M2 | M1 evicted; cnt{MESSAGE_DELETE}=1 |
| 15 | Non-dispatch kind | C=1, O=0, lane full; push `Event::GatewayHeartbeatAck` | dropped; counted under `event_type="UNKNOWN"`; no panic |
| 16 | Interactions are never dropped | C=1, O=1, both full; push 10 000 I | interaction depth 10 000; no counter increments; all 10 000 popped before M1 |
| 17 | Close with items | push I1 M1, drop every sender | pops I1, M1, then `None` |
| 18 | Close while waiting | `recv` pending, drop the last sender clone (2 clones) | stays pending after the first drop; `None` after the last |
| 19 | Wake | `recv` pending in a spawned task; push M1 | the task gets M1 (no lost wake-up, bounded by a 1 s timeout) |
| 20 | Receiver gone | drop the receiver, then push I1 and M1 | no panic; nothing counted; gauges unchanged |
| 21 | Gauges track | push I1 M1 M2 (C=1, O=1), pop I1 | interaction 1→0; event 1; overflow 1→0 after M1 is popped |
| 22 | Unicode label | `app = "bot-ü🙂"` | gauges and counters carry the label byte for byte |
| 23 | Concurrency | 8 threads × 1000 mixed I/M/T pushes, C=64, O=64, consumer draining | popped + Σcnt = 8000; every pushed interaction popped; no deadlock (10 s timeout) |
| 24 | FR-21 under full lanes | paused clock; C=3, O=2, filled with M; a consumer spawns one task per item, and each M task sleeps 10 s; then push I with `received_at = Instant::now()` | the I task starts with `received_at.elapsed() <= 2500 ms` (0 under the paused clock), before any M task ends |
| 25 | Debug redaction | `format!("{sender:?}")`, `format!("{receiver:?}")` with a sentinel payload queued | depths only, no sentinel |
| 26 | Heartbeats unaffected (E2E) | `Gateway::start` (1 shard, all event types) with a `LaneSender` (C=16, O=16), `FakeGateway` heartbeat 100 ms; the consumer takes one item and then sleeps 10 s; `gw.dispatch("MESSAGE_DELETE", ..)` ×2000 | over the next 1.5 s, ≥ 8 heartbeats with every gap ≤ 300 ms; 1 connection (no reconnect); `receiver.depths()` = `{0,16,16}` |
| 27 | App metrics (E2E) | separate test binary with a global `DebuggingRecorder`; `App::run` against the fakes; after READY, `gw.send_raw(INTERACTION_CREATE)` | the snapshot holds `noctis_lane_depth{app="default",lane="interaction"}` and `{lane="event"}` and `noctis_event_overflow_depth{app="default"}`; the interaction gauge reads 0 within 5 s (drained) |
| 28 | Drain ends | core `drain` over lanes whose sender is dropped after I1 M1 | returns; `AbortOnDrop` dropped while pending aborts the task |

</intent-contract>

## Code Map

- `crates/noctis-gateway/src/inbound.rs`: `Inbound`, `InboundKind` (`#[non_exhaustive]`; the lanes match `Interaction(..)` and `Event(..)` and treat any future variant as an event that is not droppable), `InboundSink` (its docs already require "never block or await").
- `crates/noctis-gateway/src/lib.rs`: add `mod lanes;` and the re-exports, plus a `# Lanes` crate-docs section.
- `crates/noctis-gateway/src/gateway.rs`: `Gateway::start(options, sink)` is unchanged. Each task holds an `Arc<dyn InboundSink>` clone, so the lanes close when every task ends or the `Gateway` is dropped.
- `crates/noctis-gateway/src/shard.rs` L40-120: `classify`, which READY and other wanted dispatches pass through as `Event`s. It is unchanged.
- `crates/noctis-core/src/app.rs`:
  - L12: the gateway imports.
  - L259-303: `run`. Replace `Arc::new(DiscardInbound)` at L291.
  - L329-341: `DiscardInbound`, which is deleted.
  - Add `drain`, `dispatch` and `AbortOnDrop` near `supervise` (L404).
- `crates/noctis-core/src/app/tests.rs`: add the Matrix 28 tests there (`#[tokio::test]`).
- twilight (read-only, `~/.cargo/registry/src/*/`):
  - gateway 0.17.1 `event.rs`: `EventTypeFlags` bitflags (`PRESENCE_UPDATE` L148, `TYPING_START` L187, `From<EventType>` L338).
  - model 0.17.1 `gateway/event/mod.rs` L271: `Event::kind()`.
  - `gateway/event/kind.rs` L98: `EventType::name() -> Option<&'static str>`.
- metrics 0.24.6 (read-only):
  - `src/recorder/mod.rs` L206/L215: `set_default_local_recorder` and `with_local_recorder`. Both are thread-local, so unit tests build the lanes and push inside the scope.
  - `src/common.rs` L18: `SharedString = Cow<'static, str>`, which has `From<Arc<str>>`.
- `crates/noctis-testkit/tests/sharding.rs` L460-500: the direct `Gateway::start` pattern (build an App first to install rustls), `recv` helpers and `WAIT`.
- `crates/noctis-testkit/tests/gateway_fake.rs` L345-385: `heartbeat_timing` with `on_heartbeat` and `HeartbeatSeen::since_previous`.
- `crates/noctis-testkit/tests/sharding_warning.rs`: the pattern for a separate binary with global state (Matrix 27).
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md`:
  - §4, L249-252: lane wording.
  - §11 Observability, L403-410.

## Tasks & Acceptance

**Execution:**
1. `Cargo.toml` (root), `crates/noctis-gateway/Cargo.toml`, `crates/noctis-testkit/Cargo.toml`: add the dependencies per Boundaries.
2. `crates/noctis-gateway/src/lanes.rs` and `lib.rs`: implement the API and rules. Add unit tests for Matrix 1-25. Use `metrics_util::debugging::DebuggingRecorder` with `metrics::set_default_local_recorder` on current-thread tests, and `tracing::subscriber::with_default` with a buffer writer for the log rows. Row 24 uses `#[tokio::test(start_paused = true)]`, and row 23 uses `std::thread::scope`.
3. `crates/noctis-core/src/app.rs` and `app/tests.rs`: wire the lanes into `run` with `drain`, `dispatch` and `AbortOnDrop`. Add tests for Matrix 28. Update `run`'s rustdoc and the crate docs (`lib.rs`) to say that inbound items flow through the lanes.
4. `crates/noctis-testkit/tests/lanes.rs`: Matrix 26 (`#[tokio::test(flavor = "multi_thread")]`, every wait ≤ 15 s). `crates/noctis-testkit/tests/lane_metrics.rs` holds only Matrix 27 and installs `metrics::set_global_recorder(DebuggingRecorder)` once.
5. `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md`: in §4, state the default bounds (event lane 4096, overflow 16384) and the default droppable set (presence, typing), and say that interactions are always taken first. In §11, list `noctis_lane_depth{app,lane}` (`lane` = `interaction` | `event`), `noctis_event_overflow_depth{app}` and `noctis_events_dropped_total{app,event_type}` as live, and note that `app` is `"default"` until `.app_id(..)` [planned, Story 7.4]. Do not change the API hash: the facade API is unchanged. Run `skill-check`.
6. `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: already holds this story's planning decisions. Append any implementation decisions.

**Acceptance Criteria:**
- Given shard tasks pushing into a `LaneSender`, when the consumer is stalled behind a 10 s handler and 2000 events flood in, then heartbeats keep their interval, the shard never reconnects, and the excess is dropped or overflowed without awaiting (Matrix 26).
- Given a full event lane, when a droppable event arrives, then the oldest queued droppable event is dropped and `noctis_events_dropped_total{event_type}` increments; when a non-droppable event arrives, then it goes to the overflow (`noctis_event_overflow_depth`) and is dropped with an `error` log only beyond the hard cap (Matrix 4-15).
- Given full event and overflow lanes, when an interaction arrives, then it is enqueued, popped before every event, and its handler starts within the 2500 ms FR-21 deadline under a paused clock (Matrix 16, 24).
- Given a running App, when metrics are scraped, then `noctis_lane_depth{lane}` reports each lane with `app="default"` (Matrix 27).
- Given the change, when `api-snapshot` runs, then `xtask/public-api.txt` is unchanged, and every gate below passes.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why a mutex queue, not `mpsc`:** AD-10 needs "drop the oldest *droppable*" eviction from the middle of the queue, and FIFO refill from the overflow. A `try_send` channel cannot do either. `push` holds a `std::sync::Mutex` for a bounded, non-awaiting critical section. That is the AD-10 "only `try_send`" contract: never await capacity or a handler. One lock keeps the priority check across the lanes atomic.
- **Why no author setters yet:** in 5.5, `App::run` parses only `INTERACTION_CREATE`, so the event lane stays empty until Story 5.6 derives event types from handlers. 5.6 adds `AppBuilder::event_lane(capacity, overflow)` (and droppable marking on `#[event]`) when events actually flow. That also keeps 5.5 free of facade API changes.
- **Interaction lane unbounded:** AD-10 exempts it from the bound. Its depth is metered, and Discord's 3 s deadline limits how long useful items stay valuable.
- **`app = "default"`:** no `AppId` exists before Story 7.4, which replaces it. Two Apps in one process share label values until then.
- **Counter registration at drop time:** the `noctis_events_dropped_total` handle is looked up per drop, which only happens on the overload path. The depth gauges are cached handles, so the hot path does no registry lookups (AD-25).

## Verification

**Commands** (repo root; CI is paused, so local gates only; never a debug build):
- `cargo fmt --all --check`: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace`: exit 0. Run `-p noctis-testkit --test lanes` 3 times to check for flakes.
- `cargo hack test --release --workspace --each-feature`: exit 0.
- `cargo +1.91 check --release --workspace --all-targets`: exit 0.
- `cargo deny check`: exit 0.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot` (unchanged) and `-- skill-check`: each exits 0. `api-snapshot` needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0 (see the AGENTS.md pitfalls).
- `git diff --exit-code xtask/public-api.txt`: exit 0. `test ! -d target/debug`: true.
