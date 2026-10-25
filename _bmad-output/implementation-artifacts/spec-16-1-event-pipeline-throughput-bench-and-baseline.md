---
title: 'Story 16.1: Event pipeline throughput bench and baseline'
type: 'chore'
created: '2026-10-25'
status: 'ready-for-dev'
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

**Problem:** Epic 16 must prove every event-pipeline optimization with numbers, but nothing measures the path from a shard's parsed `Inbound` to a handler start: no throughput, latency, push-cost or allocation figures exist, and the testkit has no way to inject pre-parsed `Inbound` values into a running App's lanes (AD-10 promises one).

**Approach:** Add a hidden lane tap (core `__testing::tap_lanes`, testkit `TestApp::lane_sender`) so a real `TestApp` (on `Runtime::for_test()`) exposes the `LaneSender` its shard tasks push into; build the criterion bench `crates/noctis-core/benches/pipeline.rs` with scenarios (a)-(d) driven by P ∈ {1, 4, 16} producer tasks, a dhat test `crates/noctis-core/tests/pipeline_alloc.rs` asserting recorded allocation counts, and record one full run as the baseline in `crates/noctis-core/benches/PIPELINE.md`. Pipeline behavior does not change.

## Boundaries & Constraints

**Always:**
- The timed loop does no JSON parsing: every `Box<Event>` and `(Box<Interaction>, RawJson)` is built before timing; a producer only stamps `received_at = tokio::time::Instant::now()`, records the same instant for latency, and calls `InboundSink::push` on the tapped `LaneSender` (never awaits lane capacity).
- Latency = handler-start instant − push instant; the handler takes `Instant::now()` as its **first** statement, before any state lookup or sleep.
- Both seams are `#[doc(hidden)]` and test-only: with no tap installed the App behaves exactly as before; `xtask/public-api.txt` and the Usage Skill `API hash` stay byte-identical (no `--bless`).
- Existing worker, lane, shutdown and testkit tests pass unchanged; no file under `crates/noctis-core/src/worker*` or `crates/noctis-gateway/src/lanes*` changes.
- Bench and alloc-test code obey the workspace lints (`unwrap_used`/`expect_used`/`print_stdout` need a scoped `#[expect]`/`#![allow]` with a reason, as in `memory_per_guild` and `crates/noctis/tests/alloc_dispatch.rs`); no map keyed by a Discord `Id` (index `Vec`s by number instead).

**Never:**
- No gate runs the bench: do not add it to `tools/story-loop/gates.sh`, `.github/workflows-disabled/ci.yml` or `main.yml`; no `[pipeline]` table in `benches/budgets.toml` (Story 14.3 sets budgets).
- No optimization of lanes/drain/worker (Stories 16.2/16.3), no public API change, no change to `examples/project-template/`.
- No new dependency besides `criterion` (workspace pin `0.8`, crates.io max stable 0.8.2, MSRV 1.86); no hdrhistogram (sort a `Vec<u64>` for percentiles).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Tap absent | App built without `tap_lanes` | `run_started` creates lanes and starts the gateway exactly as today | none |
| Tap installed | `tap_lanes(builder, f)` then `run_until` | `f` called exactly once per run with a clone of the run's `LaneSender`, after `lanes(..)` and before `Gateway::start` | none |
| Tap set twice | two `tap_lanes` calls | last one wins (same as `observe_runs`) | none |
| `lane_sender()` after `TestApp::build` | build returned Ok | `Some(sender)`; an `Inbound` pushed on it reaches the App's handlers | none |
| Push after `stop()` | retained sender, App stopped | push is discarded silently (lane `Closed` path), no panic, no drop count | none |
| Seq → ids | item seq `s` (0-based, `s < items`) | message/interaction id `= s + 1` (never `Id::new(0)`), interaction id `1_000_000_000_000 + s`, token `bench-{s}` | — |
| Handler sees unknown seq | seq ≥ table length | counted as `unexpected`; run reported failed | bench exits 2 naming scenario |
| Duplicate start | same (seq, handler) recorded twice | counted as `duplicate`; run failed | exit 2 |
| Missing start | run timeout 30 s reached before all expected starts | run failed with `missing = n` | exit 2 naming scenario/P |
| Percentile n = 0 | no samples | `n/a` for p50/p99/p99.9 | none |
| Percentile small n | n < 100 → p99 `n/a`; n < 1000 → p99.9 `n/a`; n = 1 → p50 = the sample | nearest rank in integer math: index `(q‰·n + 999) / 1000 − 1` on sorted samples (q‰ = 500, 990, 999; no floats) | none |
| Percentile exact | n = 1000, samples 1..=1000 µs | p50 = 500, p99 = 990, p99.9 = 999 | self-check at bench start |
| Fixed rate pacing | rate 10 000/s, P = 16 (625/s each, not integral per ms) | producer i pushes item k at `t0 + k·P/rate` (deadline-based `sleep_until`, pushes every overdue item); reports achieved rate and max lag | lag reported, not fatal |
| Saturation, no drops | N items ≤ lane capacity (`event_lane(262_144, 16_384)`) | every expected handler start recorded, zero drops | missing start → exit 2 |
| Producer tail | any producer sequence | its last item is a handled `MESSAGE_DELETE` (sentinel), so the run end covers trailing unhandled items | — |
| (c) slow guild id | 16.1 hash `(id·0x9E37_79B9_7F4A_7C15) >> 58` | `SLOW_GUILD` = smallest id > 10 000 whose bucket equals guild 1's; never inside 1..=G | self-check at start |
| (c) guild order | ordered handler per guild | per-guild seqs start in increasing order; `order_violations` reported, must be 0 | non-zero → exit 2 |
| Smoke | `cargo bench … -- --test` (criterion test mode) | every scenario runs once with tiny counts (saturation 200 items, fixed rate 0.2 s, no warm-up) and prints the table | exit 2 on failure |
| Fixture invalid | interaction JSON does not deserialize or no handler starts in the pre-run smoke push | setup error message | exit 2 before timing |
| dhat allocator missing | test binary without `dhat::Alloc` | fail: "dhat counts no allocations" | panic |
| dhat count differs | measured min ≠ recorded constant | fail: `case <name>: recorded R, measured M (a removed allocation updates RECORDED in the same commit)` | panic |

</intent-contract>

## Code Map

- `crates/noctis-core/src/app.rs` -- `run_started` L1094 creates `noctis_gateway::lanes(self.lane_config())`, spawns `drain`, then `Gateway::start(options, Arc::new(sender))`: call the tap here. `set_run_observer` L3196 and `run_observer` copy at L3142 are the pattern for the new hidden field. `event_lane` L2515 (capacity 1..=1 048 576, overflow 0..=4 194 304).
- `crates/noctis-core/src/handler.rs` L257/L270 -- `AppShared::run_observer` + `RunObserver` type; add `lane_tap: Option<LaneTap>` beside it.
- `crates/noctis-core/src/lib.rs` L373+ -- hidden `pub mod __testing` (`observe_runs`, `responded`, `AppProbe`): add `tap_lanes` and `pub use noctis_gateway::{Inbound, InboundKind, InboundSink, LaneSender, RawJson};`.
- `crates/noctis-gateway/src/lanes.rs` -- read-only: `LaneSender` (`Clone` adds a sender, `InboundSink::push`, closed lane discards), `LaneReceiver`; one `Mutex` for all queues (the baseline being measured).
- `crates/noctis-gateway/src/inbound.rs` -- `Inbound { shard: ShardId, received_at: tokio::time::Instant, kind }`, `InboundKind::{Interaction(Box<Interaction>, RawJson), Event(Box<Event>)}`, `RawJson::new(Bytes)`.
- `crates/noctis-core/src/worker.rs` -- read-only: `drain` (locales.observe, discord cache, `registry.handlers` miss → `continue`, permit, `dispatch`), `subqueue` hash L37-43 (copied into the bench for `SLOW_GUILD`), `run_concurrent` (`join_all`), `run_handler`.
- `crates/noctis-testkit/src/app.rs` -- `TestApp::build` L152 (requires a multi-thread runtime; installs `observe_runs`; builds and runs the App); add the tap install + `lane_sender` field/method. `http_fake()` L347 (`clear_requests()` in `fake.rs` L736).
- `crates/noctis-testkit/src/app/wire.rs` L572 `interaction_json` -- reference for the hand-built interaction fixtures (guild branch: `application_id`, `type`, `version`, `authorizing_integration_owners`, `entitlements`, `locale`, `app_permissions`, `channel`, `channel_id`, `guild_id`, `guild_locale`, `member`, `context`); stray-token callbacks are answered `204` (`answer` L192).
- `crates/noctis-core/benches/memory_per_guild/{main.rs,measure.rs}` -- pattern for a `harness = false` bench using `noctis::testkit::{TestApp, dhat}`, `#[derive(EventHandler)]`, exit codes 0/2, `#[expect(clippy::print_stdout)]`.
- `crates/noctis/tests/alloc_dispatch.rs` -- dhat counting-test pattern (one profiler per process, min-of-N "quiet" deltas).
- `crates/noctis-core/Cargo.toml` -- `autobenches = false`, existing `[[bench]] memory_per_guild`, dev-deps (`noctis` with `testkit`, tokio `rt-multi-thread`).
- `Cargo.toml` (workspace) -- `criterion = "0.8"` (unused so far), `dhat = "0.3"`.
- `examples/project-template/.claude/skills/noctis/SKILL.md` + `references/modules-middleware-ratelimits.md` -- derive syntax (`#[derive(EventHandler)] #[event(ordered)]`, `#[derive(Button)]`, `#[derive(SlashCommand)]`).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/handler.rs` -- add `pub(crate) type LaneTap = Arc<dyn Fn(noctis_gateway::LaneSender) + Send + Sync>;` and `AppShared::lane_tap: Option<LaneTap>` (doc: testkit seam, `None` in production) -- carries the tap to the run.
- `crates/noctis-core/src/app.rs` -- builder field `lane_tap` + `pub(crate) fn set_lane_tap`, copy into `AppShared` like `run_observer`; in `run_started` right after `lanes(..)`: `if let Some(tap) = &self.shared.lane_tap { tap(sender.clone()); }` -- the AD-10 injection point.
- `crates/noctis-core/src/lib.rs` -- in `__testing`: `pub fn tap_lanes(builder: &mut AppBuilder, tap: Arc<dyn Fn(LaneSender) + Send + Sync>)` (doc: called once per run with a clone of the shard tasks' sender; last call wins; only observes) and the `noctis_gateway` re-exports listed in the Code Map.
- `crates/noctis-testkit/src/app.rs` -- `TestApp` field `lanes: Arc<Mutex<Option<LaneSender>>>`; `build` installs `tap_lanes` storing the sender (replacing any previous); `#[doc(hidden)] pub fn lane_sender(&self) -> Option<noctis_core::__testing::LaneSender>` returns a clone -- the testkit injection path the bench uses.
- `crates/noctis-testkit/tests/lane_injection.rs` -- new test (multi-thread tokio): App with a `MESSAGE_DELETE` handler counting into state; push one `Inbound` via `lane_sender()` → handler runs within 5 s with the pushed message id; after `stop()`, a push on the retained sender returns without panic -- covers Matrix rows 2, 4, 5.
- `Cargo.toml` (workspace) -- `criterion = { version = "0.8.2", default-features = false }` -- keeps plotters/rayon out.
- `crates/noctis-core/Cargo.toml` -- dev-dep `criterion = { workspace = true, features = ["cargo_bench_support"] }`, dev-dep `bytes.workspace = true` if not reachable; `[[bench]] name = "pipeline" path = "benches/pipeline.rs" harness = false`; `[[test]] name = "pipeline_alloc" path = "tests/pipeline_alloc.rs" harness = false`.
- `crates/noctis-core/benches/pipeline.rs` (+ optional submodules in `benches/pipeline/`) -- the bench per Design Notes: custom `fn main() -> ExitCode` running `self_check()`, criterion saturation groups, fixed-rate runs, then one markdown table on stdout -- AC 1-3.
- `crates/noctis-core/tests/pipeline_alloc.rs` -- the dhat test per Design Notes -- AC 4.
- `crates/noctis-core/benches/PIPELINE.md` -- the baseline per Design Notes, filled from one full `cargo bench -p noctis-core --bench pipeline` run on this machine -- AC 5.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- `(decision)` lines for D1-D6 (already appended by planning; add implementation decisions if any).

**Acceptance Criteria:**
- Given the bench, when `cargo bench -p noctis-core --bench pipeline` runs, then a real App built by `TestApp::build` (on `Runtime::for_test()`) receives pre-parsed `Inbound` values from P ∈ {1, 4, 16} producer tasks through `TestApp::lane_sender()`, and for every scenario × P the table reports items, sustained events/s, p50/p99/p99.9 µs (received_at → handler start), and push ns/item (saturation runs).
- Given the scenarios, when the bench runs, then it covers (a) the AD-25 mix at a fixed 10 000 events/s and at saturation, (b) `MESSAGE_DELETE` with 1 and 4 concurrent no-op handlers, (c) ordered handlers over 1, 64 and 10 000 guilds with the slow guild sleeping 50 ms, (d) a saturation flood with 1 % interactions whose interaction p99 is a separate column.
- Given `-- --test`, when the bench runs, then every scenario completes once with smoke counts and exits 0.
- Given `cargo test --release -p noctis-core --test pipeline_alloc`, when it runs, then it prints blocks and bytes for `unhandled`, `one_concurrent`, `two_concurrent`, `ordered` and passes because each block count equals its recorded constant.
- Given the finished run, when `PIPELINE.md` is read, then it names CPU count and model, rustc version, the full commit hash measured, the command, every scenario's numbers, the four dhat counts, and states that no number gates CI (Story 14.3).
- Given the gates, when the story is done, then clippy `--all-targets` compiles bench and test with `-D warnings`, `api-snapshot` passes with no re-bless, and `git diff` shows no change to `xtask/public-api.txt`, `examples/project-template/`, `gates.sh` or the workflow files.

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions** (logged in the memlog):
- D1: `cargo bench --release` is not a cargo flag (`cargo bench --help` has only `--profile`); the command is `cargo bench -p noctis-core --bench pipeline` (bench profile inherits release, as `main.yml` notes).
- D2: Injection = hidden `__testing::tap_lanes` + hidden `TestApp::lane_sender` (rustdoc omits `#[doc(hidden)]` items, as with `__new`/`gateway_fake`, so the snapshot is unchanged).
- D3: criterion measures the saturation runs (`iter_custom` + `Throughput::Elements(n)`, `sample_size(10)`, `warm_up_time(1 s)`, `measurement_time(5 s)`); fixed-rate runs and all percentiles are custom code reported after criterion.
- D4: Saturation Apps use `event_lane(262_144, 16_384)` so a run of N ≤ 20 000 items never drops; fixed-rate runs keep the default lane.
- D5: Interaction no-op handlers ack with the cheapest real response (component `defer_update`, command `defer_reply`); the fake answers `204`; the bench calls `http_fake().clear_requests()` after each run to bound memory. The testkit's run observer stays installed (a small, constant overhead; noted in PIPELINE.md).
- D6: `pipeline_alloc` uses `TestApp` (needs a multi-thread runtime) on `new_multi_thread().worker_threads(1)` — one worker, so all async work runs on one thread — with `DiscordCacheConfig::none()`, no subscriber, no recorder; `harness = false` so no other test runs in the process.

**Bench layout.** One `TestApp` per scenario (reused across P and iterations), stopped before the next. Producers are `tokio::spawn`ed on the bench's multi-thread runtime (default workers = cores), producer i uses `ShardId::new(i, P)`. A `Recorder` in App state (or a bench `static`) holds pre-sized `Vec<AtomicU64>` push-nanos and start-nanos per (seq, handler) relative to one epoch `Instant`; reset per run. Run ends when all expected starts are recorded (poll every 1 ms, timeout 30 s). Events/s = items / (last start − first push). Push ns/item = Σ producer time inside its push loop / items.

**Scenario items.** Unhandled `MESSAGE_CREATE` (one `Message` built from JSON once, cloned per item before timing); handled `MESSAGE_DELETE { id: s+1, channel_id, guild_id }`; component `#[derive(Button)] struct Tap { seq: u64 }` (custom_id read from `Tap { seq }.build()`, e.g. by serializing the built component with `serde_json`; interaction `type` 3, `data { custom_id, component_type: 2 }`), command `#[derive(SlashCommand)] #[command("seq")] struct Seq { seq: i64 }` (plus whatever description/option metadata the derive requires to build; its interaction JSON carries `data { id, name: "seq", type: 1, options: [{ name: "seq", type: 4, value: s }] }`). Mix (a): per 20 items 12 MESSAGE_CREATE, 5 MESSAGE_DELETE, 2 Tap, 1 Seq (60/25/10/5), interleaved, saturation N = 20 000 per iteration, fixed rate 10 000/s for 5 s after 1 s warm-up. (b): all `MESSAGE_DELETE`, 1 or 4 handler types, saturation. (c): fixed 10 000/s for 5 s, item k → guild `(k mod G) + 1`, every 1000th item → `SLOW_GUILD`; one `#[event(ordered)]` handler that sleeps 50 ms when its guild is `SLOW_GUILD`; latencies of fast guilds only. (d): saturation, 70 % MESSAGE_CREATE, 29 % MESSAGE_DELETE, 1 % Tap; event and interaction percentiles in separate columns.

**Alloc test.** One App: `MessageDelete` → 1 concurrent handler; `ReactionRemoveAll` → 2 concurrent handlers; `ChannelPinsUpdate` → 1 `#[event(ordered)]` handler; `MessageCreate` unhandled. Handlers signal through a `static` `AtomicUsize` the main task waits on (a `static` `tokio::sync::Notify` if `Notify::const_new` compiles under the workspace tokio features, else polling with `tokio::task::yield_now`); the signal path must not allocate. Window: `dhat::HeapStats::get()` before `push` → await the signal → one `yield_now` → stats after; the `Inbound` is built before the window. `unhandled` = window(push MESSAGE_CREATE then MESSAGE_DELETE) − window(MESSAGE_DELETE). Each case: 3 warm-ups, then min of 7 windows. Constants `RECORDED: [(&str, u64); 4]`; procedure: run once, copy the printed counts, rerun green.

**PIPELINE.md.** Sections: Machine (`nproc`, `/proc/cpuinfo` model, rustc `--version`, OS), Commit (`git rev-parse HEAD` full hash + "plus Story 16.1's bench files; pipeline code unchanged"), Command, one table per scenario (P, items, events/s, p50/p99/p99.9 µs, push ns/item, notes), dhat counts table, Method notes (D3-D6, pacing, slow-guild choice), "Not a gate: Story 14.3 sets budgets; Stories 16.2/16.3 compare against these numbers".

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release -p noctis-testkit --test lane_injection` -- pass
- `cargo test --release -p noctis-core --test pipeline_alloc` -- pass, prints four counts
- `cargo bench -p noctis-core --bench pipeline -- --test` -- every scenario runs, exit 0
- `tools/story-loop/bg.sh cargo bench -p noctis-core --bench pipeline` -- full run; its table goes into `PIPELINE.md`
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- deps-check`, `rules-catalog`, `api-snapshot` (no `--bless`), `skill-check`, `tw-grammar --check` -- ok; `git diff --exit-code xtask/public-api.txt` -- no diff
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run by the gate script only, never by the implementer.
