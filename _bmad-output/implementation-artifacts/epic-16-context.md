# Epic 16 Context: Event pipeline throughput

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Make the path from a shard's parsed `Inbound` to a handler start scale with cores and shards: no single-consumer drain ceiling, no global lock every shard contends on, no cross-guild head-of-line blocking for ordered handlers, and no avoidable per-event allocation. Every existing runtime guarantee (interaction priority, bounded event lane and drop policy, per-guild order, `handler_concurrency` bound, shutdown phases) must hold unchanged. Each optimization is proven by a repeatable bench rather than assumed. Added at the maintainer's request; runs after Epics 12, 13, 15 and before Epic 14 so launch budgets are set against the optimized pipeline.

## Stories

- Story 16.1: Event pipeline throughput bench and baseline
- Story 16.2: Lock-light lanes, parallel drain and per-guild ordering without head-of-line blocking
- Story 16.3: Per-event and per-handler overhead

## Requirements & Constraints

- Concurrent event dispatch with backpressure: shard socket reads and heartbeats never wait on handler queues; interactions are never dropped and never queue behind events; when the event bound is reached, droppable types drop the oldest queued droppable event, others go to a metered overflow with a hard cap (beyond it: dropped with an error log); every drop is counted per event type; queue depth metric per lane; handlers can opt into per-guild ordered dispatch.
- Latency target: framework overhead arrival-to-handler-start p99 <= 1 ms at 10k events/s on 4 cores under the standard mix (60% `MESSAGE_CREATE` unhandled, 25% handled guild events, 10% component interactions with inline Payloads, 5% command interactions, no-op handlers).
- Allocation: the dispatch path has a documented, benchmark-measured allocation budget; target for 16.3 is zero core allocations for an unhandled event and at most 3 for one handled event with one concurrent no-op handler.
- Observability: spans for every event, metrics for lane depths and drops; metric names and labels must stay unchanged (`noctis_lane_depth` reports the sum across partitions).
- Reliability: handler panics become logged errors; panic/cancel handling and Middleware order unchanged.
- Bench numbers do not gate CI yet (Epic 14 sets budgets); no gate runs the bench. Public API should not change; a target unreachable without an API change is recorded as a `(decision)` memlog line with measured value and reason. If the API does change: update Project Template, its `AGENTS.md`, the Usage Skill, re-bless `xtask/public-api.txt` and the Skill's `API hash` in the same commit.
- Existing worker, lane, shutdown and testkit tests must pass unchanged.

## Technical Decisions

- Shard task only reads, filters, parses and enqueues with `try_send`; it never awaits lane capacity or a handler.
- `noctis-gateway` owns the lanes and `Inbound { shard, received_at, kind: Interaction(Box<Interaction>, RawJson) | Event(Box<Event>) }`; `noctis-testkit` injects `Inbound` into the same lanes (the bench uses this path, no JSON parse in the timed loop).
- Interaction lane has priority, is never dropped, takes no concurrency permit; core owns the semaphore-bounded worker set.
- Starting point (to be replaced): one `std::sync::Mutex` guarding all queues of all shards, three gauge updates and `notify_one` per push/pop, O(capacity) eviction scan; one drain task with two Registry lookups per event and a `locales.observe` write lock; 64 fixed hashed ordered sub-queues; per-run span with seven fields, new locale `Arc`, Middleware `Around`, full `Event` clone per extra handler, `Vec` + `join_all`, labeled `metrics::` macros per call.
- 16.2 direction: partition lanes (e.g. per shard, preserving guild order) or lock-free queue; O(1) amortized droppable eviction with unchanged FIFO of survivors; several drain tasks (<= runtime worker count) or batched pops; one Registry lookup per event; gauges at most once per batch; per-guild on-demand FIFOs whose map holds only guilds with queued work (bounded by `handler_concurrency`, no unbounded guild-keyed map, no task per idle guild). AD-10 gets an "Amended (Story 16.2)" note in the architecture spine.
- 16.3 direction: cache metric handles (register once per label set); filter/payload-type check before any clone; single handler without `join_all`/`Vec`; skip Middleware state and unused span fields when unused; reuse locale `Arc` when unchanged.
- Allocation discipline: every framework queue/map has a documented budget; no unbounded map keyed by guild/user/channel/message/interaction. Benches live in `noctis-core/benches` (criterion, workspace pin, `harness = false`); dhat counting-allocator tests assert recorded allocation counts.
- Shutdown order: `shutdown` drains every partition until empty; `cancel` discards queued items with one summary `warn` totaling all partitions.

## Cross-Story Dependencies

- 16.1 creates `crates/noctis-core/benches/pipeline.rs`, `benches/PIPELINE.md` (baseline) and `tests/pipeline_alloc.rs`; 16.2 and 16.3 compare against and update them. Story 14.3 later extends these benches instead of creating them.
- 16.2 must reach >= 2x saturation throughput at P = 16, push cost at P = 16 <= 1.5x P = 1, interaction p99 under flood no worse than baseline, guild isolation in the ordered scenario.
- Bench injection seam (hidden, no public API change): `noctis_core::__testing::tap_lanes` plus `#[doc(hidden)] TestApp::lane_sender()`. Recorded dhat baseline block counts: unhandled 0, one concurrent 7, two concurrent 12, ordered 4.
- 16.3 measures scenario (b) against the end-of-16.2 numbers (>= 1.3x) and lowers the recorded dhat counts.
- Touches the same drain/dispatch code as Stories 12.5, 13.3 and 13.5, which must already be in place.
