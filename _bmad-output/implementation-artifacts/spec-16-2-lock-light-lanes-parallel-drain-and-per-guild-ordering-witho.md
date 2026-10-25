---
title: 'Story 16.2: Lock-light lanes, parallel drain and per-guild ordering without head-of-line blocking'
type: 'refactor'
created: '2026-10-25'
status: 'in-progress'
baseline_revision: 'f7b3b161430323238c282e1959cdf16e80656d69'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-16-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Every shard push and every pop contends on one `std::sync::Mutex` (three gauge sets + `notify_one` each), droppable eviction scans the whole event lane, one drain task is the single consumer for all shards (two Registry lookups per event), and ordered handlers share 64 fixed hashed sub-queues, so one slow guild blocks every guild in its bucket. The 16.1 baseline shows it: push ns/item 258 at P = 1 vs 1249 at P = 16, (c) 64-guild p99 19 ms behind a 50 ms handler.

**Approach:** Partition the lanes by shard (`shard.number() % partitions`, one lock + one `Notify` per partition, global atomic depth counters for the bounds and gauges), make eviction O(1) with a two-FIFO event queue, run one drain task per partition (partitions = runtime worker count) that pops in batches, look the Registry up once per event, and replace the 64 sub-queues with on-demand per-guild FIFOs whose map only holds guilds with an ordered job in flight. Public API (`xtask/public-api.txt`) does not change.

## Boundaries & Constraints

**Always:**
- Shard tasks never await: `InboundSink::push` stays synchronous; no lock is held while calling the `DropObserver`, logging or touching metrics.
- One partition (`lanes(config)`) behaves exactly as today: every test in `crates/noctis-gateway/src/lanes/tests.rs` passes byte-for-byte unchanged, including its private access (`sender.shared.depths()`, `receiver.shared.lock().event.iter()`, `receiver.shared.lock().overflow.iter()`, the `LaneConfig { .. }` literal with exactly today's five fields).
- Order: items of one shard keep arrival order (interactions among themselves, events among themselves) through push, pop, batch, Discord-cache feed and dispatch; ordered handlers of one guild run in arrival order; interactions of a partition are popped before any of its events.
- Bounds are global across partitions: Σ event depths ≤ `event_capacity`, Σ overflow depths ≤ `overflow_capacity`, even under concurrent pushes (reserve with CAS on the atomic, under the partition lock).
- Drop policy, drop counter (`noctis_events_dropped_total{app, event_type}`), `DropObserver` call, log targets/levels/messages unchanged. Metric names and labels unchanged; `noctis_lane_depth{lane}` and `noctis_event_overflow_depth` report the sum across partitions.
- At most `handler_concurrency` events hold a permit (queued ordered jobs included); interactions take no permit.
- AD-21 order: `shutdown` → every partition drains until empty; `cancel` → queued items of all partitions discarded with exactly one `warn` "lanes discarded at shutdown" whose `interactions`/`events` fields are totals over all partitions (plus locally batched and permit-waiting items).
- No task exists per idle guild; the per-guild map is keyed `Option<Id<GuildMarker>>`, holds only guilds with a queued or running ordered job, carries a `// keyed-map: ` marker stating that bound (deps-check rule).
- `unhandled` dhat count stays 0; the drain's batch buffer is allocated once per drain task, never per event or batch.
- No new dependency (std `Mutex`/atomics/`HashMap`, tokio `JoinSet`/`Notify`, tokio 1.54.1 `Handle::metrics().num_workers()`).

**Never:**
- No public API change of the facade (`api-snapshot` passes without `--bless`; Usage Skill `API hash` untouched). `noctis-gateway`'s own items may grow (it is not in the snapshot).
- No lock-free queue crate, no `parking_lot`/`crossbeam`/`dashmap`; no unbounded guild-keyed map; no per-guild task that outlives its queued work.
- No change to `crates/noctis-core/benches/pipeline*` measurement logic (16.1 numbers must stay comparable); no bench in any gate; no `[pipeline]` budget table.
- Do not touch Story 16.3 items (metric-handle caching, clone-before-filter, `join_all`/`Vec` removal, span fields, locale `Arc` reuse).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Partition index | `lanes_with(cfg, n)`, push on `ShardId::new(s, t)` | goes to partition `s % n` (n = 1 → always 0; s = u32::MAX, n = 64 → 63) | — |
| Partition count | runtime workers w | `lane_partitions()` = clamp(w, 1, `MAX_LANE_PARTITIONS` = 64); current-thread runtime → 1; w = 200 → 64 | — |
| `lanes_with` n = 1 | any | identical to `lanes(cfg)` | — |
| Per-shard FIFO | n = 4, shards 0..8 interleave m(1..=800) | each shard's pops are in its push order; every item popped exactly once | — |
| Global bound, concurrent | n = 4, cap 4, overflow 2, 8 threads × 1000 non-droppables | Σ event depth never > 4, Σ overflow never > 2, popped + `dropped_total` = 8000 | drops counted + observer/error log |
| Full, droppable, own partition has droppable | cap reached, push T on partition k holding T/P | oldest droppable **of partition k** evicted in O(1), counted, debug log; survivors' FIFO unchanged | — |
| Full, droppable, own partition has none | cap reached by other partitions' items | the pushed droppable is dropped (`DroppedDroppable`), counted | debug log |
| Full, non-droppable | Σ overflow < cap | to partition k's overflow; popped after k's event queue items | — |
| Overflow refill | pop event in k, k's overflow non-empty | k's oldest overflow item moves to k's event queue with a fresh entry sequence (same order as today) | — |
| Event queue empty, overflow not | k's event queue empty, k's overflow holds items | pop takes k's overflow front (no stuck items) | — |
| Overflow cap 0 | non-droppable on full lane | dropped, counted, observer or `error` | as today |
| Eviction complexity | 200 000 items, droppables only at the back, 100 000 droppable pushes | completes < 2 s in release (old scan: ~10^10 steps) | test timeout = fail |
| Eviction memory | consumer stalled, droppables pushed forever | queue length stays ≤ capacity (no tombstone growth) | — |
| Batch pop | partition holds 3 interactions + 100 events, batch max 32 | one lock: 3 interactions then 29 events, gauges set once (3 `set` calls) | — |
| Batch, empty | nothing queued, senders alive | waits on the partition `Notify`; cancel-safe | — |
| Closed | all senders dropped, partition empty | `None`/closed; drain task returns | — |
| Push after receiver gone | the unsplit receiver, or the target partition's `LanePartition`, dropped | discarded silently, no count, no gauge change (all receivers gone → gauges 0) | — |
| Unsplit receiver, n > 1 | `recv`/`try_recv` | interactions of any partition first, then events; per-partition FIFO kept; wakes on a push to any partition | — |
| Interaction under flood | 10 000 events then I then E on one shard | I handed to its pipeline before E's handlers start; I takes no permit | — |
| Registry miss | unhandled event | no permit, one Registry lookup, trace log, zero allocations | — |
| Ordered, two guilds | guild A handler blocked, B pushed after (ids colliding under the old hash) | B's ordered events run; A's queue waits | — |
| Ordered, same guild | events 1,2,3 of guild G | ordered handlers start 1,2,3, each after the previous ended | panics caught, next runs |
| Ordered, guildless | `USER_UPDATE` ×2 | key `None`, sequential in arrival order | — |
| Ordered map bound | idle after work | map empty, `Workers::subqueues()` = 0, no runner task alive | — |
| Ordered map, saturated | N = `handler_concurrency` guilds blocked | map entries ≤ N; further events wait for a permit in the drain | — |
| Ordered + cancel | job queued behind a blocked one, cancel fires | queued job reported `Cancelled` once (as today) | sink report |
| Shutdown, n = 4 | items on 4 shards, shutdown fired | every item of every partition dispatched; drain returns | — |
| Cancel, n = 4 | items queued on 4 partitions + batched + 1 waiting | one `warn` with totals; no per-partition warn; nothing runs | — |
| Supervisor aborted | `AbortOnDrop` dropped | every partition drain task aborted (JoinSet) | — |
| Concurrency, n = 4 | `handler_concurrency(2)`, 10 events on 4 shards | ≤ 2 in handler phase at once | — |

</intent-contract>

## Code Map

- `crates/noctis-gateway/src/lanes.rs` -- today one `Shared { queues: Mutex<Queues>, notify, config, gauges }`; `LaneSender::enqueue` L322 (O(n) `position` scan L338), `push` L363 (gauges + `notify_one` per push), `Clone`/`Drop` count senders under the lock, `LaneReceiver::{recv, recv_interaction, try_recv, depths, pop, pop_interaction}`, `Drop` discards. Rewrite here; keep public names.
- `crates/noctis-gateway/src/lanes/tests.rs` -- read-only contract: private access listed in Always; `config()` literal; row21 (gauges after each push/pop), row23 (8 threads), observer tests (shard `ShardId::new(2, 4)`).
- `crates/noctis-gateway/src/lib.rs` -- re-exports lanes items; add `lanes_with`, `LanePartition`, `MAX_LANE_PARTITIONS`.
- `crates/noctis-core/src/worker.rs` -- `SUBQUEUES`/`subqueue` L33-44, `Workers { ordered: Vec<mpsc::UnboundedSender<Job>> }` L84, `Workers::new` spawns 64 tasks L101, `dispatch` L184 (second Registry lookup), `drain` L268 (single consumer, select on `recv`/`recv_interaction`/permit), `discard` L326, `run_subqueue` L363, `run_concurrent`, `run_handler` (unchanged).
- `crates/noctis-core/src/worker/tests.rs` -- `Harness`/`Running` call `drain(receiver, app.workers(..))` with `lanes(app.lane_config())` (1 partition, current-thread runtime); `subqueue_hash` L169-179 and L313 reference the removed hash; `zero_grace_discards_lanes` L1612, `permit_wait_is_discarded` L1633, `guild_isolation` L275.
- `crates/noctis-core/src/registry/tests.rs` L41, L57 -- `workers(..).subqueues() == 0` (keep `#[cfg(test)] subqueues()` = live per-guild entries).
- `crates/noctis-core/src/registry.rs` L276 `handlers(kind) -> Option<&Arc<Handlers>>`, L323 doc "the sub-queues exist only then" (reword).
- `crates/noctis-core/src/app.rs` L754 `lane_config`, L1094 `run_started` (`lanes(..)`, tap, `drain` spawn), L934 doc "One core task drains the lanes" (reword), `wind_down`/`settle` L1649-1700 (unchanged contract: waits drain then tracker).
- `crates/noctis/src/lib.rs` L1255-1262 -- `EventHandler` derive doc describes 64 sub-queues and the hang hazard.
- `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md` L360-369 and `references/app-and-sharding.md` L497-503 -- Usage Skill text on sub-queues and "one core task".
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` L430-462 -- AD-10 (append Amended note).
- `crates/noctis-core/benches/PIPELINE.md`, `crates/noctis-core/tests/pipeline_alloc.rs` (`RECORDED`) -- 16.1 baseline to compare and update.
- `crates/noctis-core/benches/pipeline/fixtures.rs` L40 -- the bench's own copy of the old hash (keeps `SLOW_GUILD`; leave as is).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-gateway/src/lanes.rs` -- implement per Design Notes "Lanes": `pub const MAX_LANE_PARTITIONS: usize = 64`; `pub fn lanes_with(config, partitions: NonZeroUsize) -> (LaneSender, LaneReceiver)` (clamps to 64; `lanes(c)` = `lanes_with(c, 1)`); per-partition `Mutex<Queues>` + `Notify`; global `AtomicUsize` depths and sender count; `EventQueue` with O(1) `evict_oldest_droppable`; one gauge `set` per push (the lane it touched); `LaneReceiver::into_partitions(self) -> Vec<LanePartition>`; `LanePartition::{recv_batch, try_recv_batch, recv_interaction, depths}` (dropping a `LanePartition` discards its queues and closes that partition); unsplit receiver works across partitions; doc tests for each new pub item -- AC 1-2.
- `crates/noctis-gateway/src/lanes/partition_tests.rs` (new, `#[cfg(test)] mod partition_tests;` beside `mod tests;`) -- tests for every lanes Matrix row (partition index, per-shard FIFO, global bound under 8 threads, per-partition eviction, eviction complexity/memory, refill/overflow-only pop, batch + gauge-set count via a counting `metrics::Recorder`, unsplit n > 1, closed/receiver gone).
- `crates/noctis-gateway/src/lib.rs` -- re-export `lanes_with`, `LanePartition`, `MAX_LANE_PARTITIONS`.
- `crates/noctis-core/src/worker.rs` -- per Design Notes "Drain" and "Ordered": remove `SUBQUEUES`, `subqueue`, `run_subqueue`, the mpsc vector; add `OrderedQueues`; `Workers` shared by `Arc`; `dispatch` takes the `Arc<Handlers>` found by the drain; `drain(receiver, workers)` keeps its signature (1 partition runs inline, n > 1 in a `JoinSet`), single summary warn; `#[cfg(test)] subqueues()` returns live per-guild entries -- AC 3-5.
- `crates/noctis-core/src/worker/tests.rs` -- delete `subqueue_hash` (Matrix 38, the removed hash) and replace L313's `assert_eq!(subqueue(..), 0)` with `assert_eq!(user_update(1).guild_id(), None)`; change nothing else in existing tests; add tests (multi-thread runtime where parallelism matters, `lanes_with(app.lane_config(), 4)`) for: guild isolation with ids colliding under the old hash, same-guild order across 4 partitions, map empty after work and ≤ N while saturated, interaction-before-later-event under a 10 000-event flood, concurrency bound with 4 partitions, shutdown drains all partitions, cancel's single warn totals across partitions, aborting the supervisor aborts partition drains -- covers the core Matrix rows.
- `crates/noctis-core/src/app.rs` -- `pub(crate) fn lane_partitions() -> NonZeroUsize` (workers of `Handle::try_current()`, else 1, clamped 1..=64); `run_started` uses `lanes_with(self.lane_config(), lane_partitions())`; reword the L934 doc.
- `crates/noctis-core/src/registry.rs` -- reword the `has_ordered` doc (no sub-queue tasks).
- `crates/noctis/src/lib.rs` -- `EventHandler` doc: ordered = per-guild FIFO, created on demand; a hung ordered handler blocks only its own guild (guildless events share one queue), but events queued behind it still hold permits.
- `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md`, `references/app-and-sharding.md` -- same wording change (behavior docs only; API hash unchanged); `grep -rn "sub-queue" examples/ crates/noctis/src` returns nothing afterwards.
- `crates/noctis-core/tests/pipeline_alloc.rs` -- rerun; set `RECORDED` to measured counts (`unhandled` must stay 0); an increase gets a `(decision)` memlog line with its cause.
- `crates/noctis-core/benches/PIPELINE.md` -- keep the baseline; add "Story 16.2" section (same Machine format, full commit hash, every scenario table) and a "Baseline vs 16.2" table per scenario × P: events/s (both, ratio), p99 (both), int. p99 (both, (a)/(d)), push ns/item (both, and P16/P1 ratio); dhat before/after; state partitions = runtime workers (4 here) and the per-target verdicts.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` -- append to AD-10 "- **Amended (Story 16.2):**" describing partitions by shard, global bounds, per-partition eviction, one drain per partition with batched pops, per-guild on-demand FIFOs bounded by `handler_concurrency`, metrics summed.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions D1-D7 already appended; add implementation decisions and any unmet target (measured value + reason).

**Acceptance Criteria:**
- Given P ∈ {1, 16} producers in the 16.1 bench on the same machine, when it runs, then saturation (a) push ns/item at P = 16 ≤ 1.5× P = 1 and events/s at P = 16 ≥ 2× the baseline 88 393, fixed-rate (a) p99 ≤ 1 ms (NFR-3), and (d) int. p99 at every P ≤ its baseline (8 820.9 / 9 949.1 / 12 535.8 µs); a missed target is recorded in PIPELINE.md and the memlog with measured value and cause (D7).
- Given a full event lane, when a droppable arrives on a partition holding a droppable, then that partition's oldest droppable is evicted without scanning (eviction-complexity test passes) and all existing `lanes` tests pass unchanged.
- Given 4 partitions, when events arrive faster than one task drains, then 4 drain tasks run concurrently, each pop takes up to `DRAIN_BATCH` = 32 items under one lock with one gauge update per batch, each event costs one Registry lookup, and an interaction queued on a shard before event E starts before E.
- Given guild A's ordered handler sleeps 50 ms, when the (c) bench runs with 64 guilds, then the fast guilds' p99 ≤ 1 ms (baseline 19 ms) with 0 order violations.
- Given `handler_concurrency = N`, when the pipeline runs, then ≤ N events hold permits, `shutdown` drains all partitions and `cancel` logs one summary warn over all partitions.
- Given the gates, when the story is done, then `api-snapshot` passes without `--bless`, deps-check (incl. `keyed-map`) passes, all existing worker, lane, shutdown and testkit tests pass (only the two hash references above edited), and AD-10 carries the amendment.

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions** (memlog):
- D1: Partition key = shard (`number % n`) so one shard's whole stream, including READY before GUILD_CREATE for the Discord cache, stays ordered; n = runtime workers clamped 1..=64, independent of the shard count (single-shard bots get one busy partition, idle drains just park).
- D2: Partition count reaches the lanes via `lanes_with`, not a new `LaneConfig` field (the existing tests build `LaneConfig` by literal).
- D3: Eviction is per partition: a droppable on a full lane evicts its own partition's oldest droppable, else is dropped; bounds stay global via atomics.
- D4: Drain = one task per partition (in a `JoinSet` inside `drain`; n = 1 runs inline so existing current-thread tests keep their timing) with batched pops of `DRAIN_BATCH` = 32.
- D5: Ordered = `Mutex<HashMap<Option<Id<GuildMarker>>, VecDeque<Job>>>`; bound = permits held by queued/running jobs ≤ `handler_concurrency`.
- D6: `subqueue_hash` and one `subqueue(..)` assert are the removed mechanism itself; "existing tests pass unchanged" is read as every behavioral test; `subqueues()` keeps its name and returns live entries (0 when idle, as both registry tests expect).
- D7: Bench targets are not gates (epic context); a target missed on this 4-CPU shared host after the specified design is recorded, not a blocker.

**Lanes.** Per partition: `Queues { interaction: VecDeque<Inbound>, event: EventQueue, overflow: VecDeque<Inbound>, receiver_alive: bool }`. `EventQueue` = two `VecDeque<(u64, Inbound)>` (droppable / other) plus a next-sequence counter; the sequence is assigned when an item enters the event queue (push or refill); `pop_front` takes the lower front sequence; `evict_oldest_droppable` = droppable `pop_front`; `len` and `iter()` (merged, FIFO) exist for the tests. `Shared` keeps `fn lock()` (partition 0) and `fn depths()` (sums) for the existing tests. Global `AtomicUsize`: interaction, event, overflow totals and senders. Full = overflow total > 0 || event total ≥ capacity (as today). Pop: interaction front; else event front (+ refill from own overflow); else overflow front. `notify_one` on the partition's `Notify` after each push; last sender drop notifies every partition. Unsplit `recv` with n > 1 enables every partition's `Notified` before re-checking (no lost wakeup), returns interactions from any partition before events.

**Drain.** Per partition: `recv_batch(&mut local)` (buffer `VecDeque::with_capacity(DRAIN_BATCH)` once), then per item exactly today's steps (locales.observe, discord cache, one `registry.handlers` lookup → `None` continue, permit select with `recv_interaction` of its partition and `cancel`, `dispatch(shard, event, handlers, permit)`). Shutdown: finish `local`, `try_recv_batch` until empty. Cancel: return `(interactions, events)` = partition depths + `local` + waiting; `drain` sums all and calls the existing warn once.

**Ordered.** dispatch: lock map; key present → `push_back(job)`; else insert empty `VecDeque` and spawn a runner (tracker) with the job. Runner: run ordered part (today's `run_subqueue` body), then lock → `pop_front` → next, or remove key and exit. Never hold the lock across `.await`.

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-gateway` -- all lanes tests (old and new) pass
- `cargo test --release -p noctis-core --lib worker` and `--lib registry` -- pass
- `cargo test --release -p noctis-core --test pipeline_alloc` -- pass with updated `RECORDED`, `unhandled` 0
- `cargo bench -p noctis-core --bench pipeline -- --test` -- exit 0; then `tools/story-loop/bg.sh cargo bench -p noctis-core --bench pipeline` -- numbers into PIPELINE.md
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- deps-check`, `rules-catalog`, `api-snapshot` (no `--bless`), `skill-check`, `tw-grammar --check`; `git diff --exit-code xtask/public-api.txt` -- ok
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run by the gate script only.
