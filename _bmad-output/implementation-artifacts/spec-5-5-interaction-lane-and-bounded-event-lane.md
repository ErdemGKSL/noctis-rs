---
title: 'Story 5.5: Interaction Lane and bounded event lane'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: '5b827b4678bae5c3f9dd187a13c026d360097756'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Story 5.5's diff bundles an unrelated AGENTS.md maintainer-workflow rule (refresh README.md after every third finished epic).
    evidence: |-
      The AGENTS.md hunk has nothing to do with lanes; blind and intent-alignment reviewers flagged it as out-of-story scope. The fix edits an agent-context file, so it is deferred: either confirm the rule as a maintainer-requested change in its own commit or move it out of the story's change set.
    location: >-
      AGENTS.md (Maintainer workflow section)
    severity: low
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

### 2026-10-20 — Review pass
- verdicts: 36 findings — high 0, medium 4, low 18, false 13, maybe-false 1
- findings:
  - `[false]` `[reject]` (blind) The core `dispatch` trace logs the interaction token through `kind = ?inbound.kind` — `InboundKind`'s manual `Debug` (inbound.rs) prints only `Interaction(RawJson { len })` or `Event(<type>)`, never the payload or token.
  - `[low]` `[reject]` (blind) Depth gauges written after the lock can be reordered and stay stale once the lanes go idle — real but cosmetic (a gauge off by a few items on an idle bot, needing an exact push/pop interleaving); the spec requires metric calls after the guard is released, so a fix needs a sequence guard (added state), not a direct correction.
  - `[low]` `[reject]` (blind) One `error` line per event dropped past the overflow cap, without rate limiting — reached only after 4096 + 16384 queued non-droppables (FR-5: "the App is overloaded"); AD-10/FR-5 and Matrix 7 prescribe the error log, the counter carries the volume, and rate limiting adds state.
  - `[low]` `[reject]` (blind) Droppable push scans the full event lane under the mutex — the spec bounds it explicitly (O(`event_capacity`), only when full); about 4096 pointer checks per push only under overload; a droppable counter adds state.
  - `[low]` `[reject]` (blind) A droppable arrival can overtake older non-droppables waiting in the overflow — Matrix 9 settles this order deliberately; FR-5/AD-10 require no cross-type order and 5.5 has no ordered consumer; changing it rewrites the spec's eviction policy.
  - `[low]` `[reject]` (blind) Interactions are discarded silently after the receiver drops — this happens only when `App::run` is already returning (drain aborted); shutdown draining is excluded by the intent contract (Story 5.8).
  - `[medium]` `[patch]` (blind) Matrix 27 never shows the interaction went through the lane — grouped with the verification-gap entry; see that row for the patch.
  - `[low]` `[reject]` (blind) Matrix 26 does not assert `noctis_events_dropped_total` — the counting code is the same `count_drop` path that unit rows 7, 11 and 13-15 assert exactly; Matrix 26 does not require it.
  - `[maybe-false]` `[reject]` (blind) Matrix 26 timing bounds may flake on loaded runners — no subscriber is installed in that binary, so the drop errors are never formatted, and the bounds leave wide slack (8 of ~15 beats, gaps up to 3x the interval); settling it needs repeated runs on a loaded runner, and if true it would only be `low`.
  - `[low]` `[reject]` (blind) Row 24 passes whether or not interactions are prioritised (paused clock) — true, but rows 3 and 16 already assert the priority; tightening it needs extra pop-order tracking, not a direct correction.
  - `[low]` `[reject]` (blind) `try_recv` cannot tell an empty lane from a closed one — the spec fixes the `Option<Inbound>` signature; no caller needs the distinction (core uses `recv`), and `noctis-gateway` is internal and outside the facade snapshot.
  - `[low]` `[reject]` (blind) Each drop looks the counter up in the recorder again — a deliberate trade-off in the spec's Design Notes (drops happen only on the overload path); caching per type adds a map.
  - `[false]` `[reject]` (blind) Public API snapshot and Usage Skill hash not updated — `api-snapshot` records only the facade (xtask/src/api_snapshot.rs header), and the facade does not re-export `noctis_gateway`, so the snapshot does not change.
  - `[low]` `[defer]` (blind) An unrelated `AGENTS.md` README-refresh rule is bundled with the story — real scope creep (Epic 15 exists in epics.md, so the epic list is correct); the fix edits an agent-context file, so it is deferred.
  - `[low]` `[reject]` (blind) No high-water warning for the unbounded interaction lane — AD-10 makes the lane unbounded and metered by design, and the depth gauge is live; a threshold warning adds state.
  - `[low]` `[patch]` (blind) Small `app.rs` points: an overlong `App::run` doc line, a redundant `drop(inbound)`, and an undocumented abort-on-shutdown — the doc line overruns the paragraph's wrap; the explicit drop and the 5.8-owned shutdown behaviour do no harm. Patch: rewrap the `run` rustdoc paragraph.
  - `[low]` `[reject]` (edge) Push snapshot written after a concurrent pop leaves a stale gauge when idle — same root cause as the blind gauge-race row; rejected there.
  - `[low]` `[reject]` (edge) A push that enqueued before `LaneReceiver::drop` writes non-zero gauges after the reset — same root cause; the window exists only while `App::run` returns, and the gauges belong to an App that has ended.
  - `[low]` `[reject]` (edge) O(capacity) eviction scan when the full lane holds no droppable — same as the blind scan row; rejected there.
  - `[medium]` `[patch]` (edge) The interaction gauge is 0 from creation, so Matrix 27 passes without any push or drain — grouped with the verification-gap entry.
  - `[false]` `[reject]` (edge) With `EventTypeFlags::all` the first item may be `GatewayHello`, not READY — `classify` turns only `GatewayEvent::Dispatch` into an `Inbound`, so HELLO never reaches the lane and the first item is the READY dispatch.
  - `[medium]` `[patch]` (edge) The Matrix 27 claim "drained" is not shown — grouped with the verification-gap entry.
  - `[false]` `[reject]` (edge) Matrix 27 waits for IDENTIFY, not READY, before sending INTERACTION_CREATE — the fake answers IDENTIFY with READY on the same ordered socket, and the shard parses INTERACTION_CREATE whether or not READY came first; no outcome changes.
  - `[false]` `[reject]` (edge) Matrix 26 stalls the consumer by not calling `recv` instead of sleeping behind a 10 s handler — a consumer that stops taking items looks the same to `push` as one sleeping 10 s; the memlog logs the choice, and it lets the test assert the final depths.
  - `[medium]` `[patch]` (vgap) App-level lane wiring is unverified: `app_reports_lane_depths` stays green if the drain is not spawned, the guard is dropped at once, or the sink discards — pre-verified (gauges start at 0, and `DebuggingRecorder` resets them on read). Patch: the test captures the core drain's `no handler registered` trace for the `Interaction` item.
  - `[low]` `[reject]` (vgap other) Receiver-drop reset can be overwritten by an in-flight push, leaving the gauges stuck — same root cause as the gauge-race rows; rejected there.
  - `[false]` `[reject]` (intent 1) AC1 "handler sleeping 10 s" versus a stalled receiver — no handlers exist before Story 5.6; for the shard path, a stalled consumer looks the same, and the spec (Matrix 26) settles this.
  - `[false]` `[reject]` (intent 2) A literal `try_send` versus a non-awaiting mutex push — the spec Design Notes and memlog settle it (AD-10's point is never awaiting capacity; `mpsc` cannot evict from the middle).
  - `[false]` `[reject]` (intent 3) AC3 is checked at the lane, not at an App handler — handlers and the worker set are Story 5.6 (epics FR-5 split); the spec settles Matrix 24 at the lane level.
  - `[false]` `[reject]` (intent 4) The event-lane policy cannot be reached through the App — 5.5 parses only INTERACTION_CREATE, and Story 5.6 derives event types from handlers (spec Design Notes).
  - `[false]` `[reject]` (intent 5) No author-facing droppable marking — the intent contract excludes author setters (Story 5.6 adds `.event_lane(..)` and `#[event]` marking).
  - `[false]` `[reject]` (intent 6) Policy extensions beyond the AC text (self-drop, full when the overflow is non-empty, counting overflow drops) — the spec settles each one (Matrices 6 and 9, rule 3), consistent with FR-5's "nothing is dropped silently".
  - `[low]` `[patch]` (intent 7) AC4 is checked only for metric presence through the App — the same weak Matrix 27 test; grouped with the verification-gap entry (row 21 covers non-zero depth tracking; `noctis_event_overflow_depth` is the spine's own metric name).
  - `[false]` `[reject]` (intent 8) The FR-5 worker set is not implemented — epics assign it to Story 5.6, and the intent contract excludes it.
  - `[low]` `[defer]` (intent 9) An out-of-intent `AGENTS.md` change — same root cause as the blind AGENTS.md row; deferred.
  - `[false]` `[reject]` (intent 10) Partial-delivery markers and no API re-bless — `in-progress` is the workflow status before review, and the snapshot covers only the facade, which does not change.

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

## Auto Run Result

- **Summary:** Added the `lanes` module to `noctis-gateway`. `LaneSender` implements the sync `InboundSink`, and the single-consumer `LaneReceiver` always drains interactions first. Both run on one `std::sync::Mutex` over three `VecDeque`s plus a `tokio::sync::Notify`. The interaction lane has priority, is never dropped and has no bound. The event lane is bounded, evicts the oldest droppable event when full and sends the rest to a metered overflow. A drop past the overflow cap is counted and logged at `error`. Depth gauges and `noctis_events_dropped_total` use the `metrics` 0.24 facade with `app="default"`. `App::run` now uses the lanes in place of `DiscardInbound` and spawns a core drain task, which traces and drops each item until Story 5.6 adds handlers. A shard task never awaits a lane.
- **Files changed:**
  - `crates/noctis-gateway/src/lanes.rs`, `lanes/tests.rs`: the lane implementation and its unit matrix.
  - `crates/noctis-gateway/src/lib.rs`, `Cargo.toml`: export the module; add `metrics`.
  - `crates/noctis-core/src/app.rs`, `app/tests.rs`, `lib.rs`, `Cargo.toml`: wire the lanes and the drain task into `App::run`.
  - `crates/noctis-testkit/tests/lanes.rs`, `tests/lane_metrics.rs`, `Cargo.toml`: integration tests for Matrix 26 (shard never blocks while a consumer stalls) and Matrix 27 (App-level lane depths and drain).
  - `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md`: document lane behaviour in the Usage Skill.
  - `Cargo.lock`, `Cargo.toml`: dependency updates. Architecture `.memlog.md`: decision lines.
  - `AGENTS.md`: maintainer README-refresh rule, outside the story's scope (deferred).
- **Review findings:** 36 in total: high 0, medium 4, low 18, false 13, maybe-false 1.
  - **Patches** (patched counts by verdict: high 0, medium 4, low 2):
    - Medium (blind, edge x2, vgap): Matrix 27 now captures the core drain's `no handler registered` trace for the `Interaction` item. This shows the item went through the lane and was drained.
    - Low (intent 7): grouped with that same Matrix 27 fix.
    - Low (blind): rewrapped the `App::run` rustdoc paragraph.
  - **Defers** (low): the unrelated `AGENTS.md` README-refresh rule (blind and intent 9). It needs maintainer confirmation in its own commit.
  - **Rejects** (each with the reason recorded in the Review Triage Log):
    - Gauge staleness races (blind/edge/vgap): cosmetic, and a fix would need a sequence guard.
    - Unrate-limited overflow error logs: prescribed by AD-10/FR-5.
    - O(capacity) eviction scan: spec-bounded and only on overload.
    - A droppable overtaking overflow items: Matrix 9 sets this order deliberately.
    - Silent discard after the receiver drops: shutdown belongs to Story 5.8.
    - Matrix 26 lacks a drop-counter assert: covered by unit rows.
    - Matrix 26 timing flake (maybe-false): wide slack and no subscriber.
    - Row 24 passes under a paused clock: rows 3 and 16 cover priority.
    - `try_recv` ambiguity: the signature is fixed by the spec.
    - Per-drop counter lookup: a Design Notes trade-off.
    - No high-water warning: AD-10 makes the lane unbounded and metered.
    - All 13 false findings: no snapshot change, since the facade does not re-export the gateway; handlers, worker set, `#[event]` marking and author setters belong to Story 5.6; READY/HELLO ordering and `try_send` semantics are settled by the spec.
- **Follow-up review recommended:** false. The four medium findings shared one root cause and were fixed by a single test patch, and no high finding was patched.
- **Verification:**
  - All local gates passed via `gates.sh`: fmt, clippy `-D warnings` (release), `cargo test --release --workspace`, `cargo hack --each-feature`, the MSRV 1.91 check, `cargo deny`, and xtask deps-check, rules-catalog, api-snapshot (unchanged) and skill-check.
  - `xtask/public-api.txt` is unchanged, and no `target/debug` exists.
  - CI is paused by the maintainer, so only local gates ran.
- **Residual risks:**
  - Depth gauges can stay a few items stale on an idle bot (push/pop reorder race).
  - Matrix 26 timing has not been proven on a loaded runner.
  - The interaction lane has no bound and no high-water alert.
  - `app="default"` labels collide when two Apps run in one process (until Story 7.4).
  - The `AGENTS.md` scope item is still deferred.
