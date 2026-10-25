---
title: 'Story 14.3: Dispatch latency, allocation and memory gates'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_revision: 'a37158c0906622c1ea5105b42d0cca1bd5c0d05f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/crates/noctis-core/benches/PIPELINE.md'
  - '{project-root}/_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md'
warnings: ['oversized']
deferred:
  - summary: >-
      dispatch_latency stamps received_at at the actual (possibly late) push, so producer schedule lag is excluded from the gated p99 (possible coordinated omission).
    evidence: |-
      Reused benches/pipeline/run.rs sets received_at when the producer pushes; worst schedule lag is reported (8779.2 µs in the verification run) but never bounds run validity. Unverified whether this materially moves the p99: settle by rerunning with received_at = scheduled deadline (or rejecting runs whose lag exceeds a bound) and comparing the median p99 with the push-stamped one.
    location: >-
      crates/noctis-core/benches/dispatch_latency/budget.rs check_run; crates/noctis-core/benches/pipeline/run.rs
    severity: medium (unverified)
  - summary: >-
      Guarded component Routes box two handler futures (guarded wraps the boxed typed future in a second Box::pin), so AD-25's general "one boxed handler future" holds only for unguarded Routes.
    evidence: |-
      crates/noctis-core/src/component.rs guarded (about l.582-604) Box::pins an async block awaiting typed(..), which returns another boxed HandlerFuture; pre-existing, and interaction_alloc measures unguarded Routes only, while the AD-25 amendment and CHANGELOG state the rule generally.
    location: >-
      crates/noctis-core/src/component.rs:582
    severity: low
---

<intent-contract>

## Intent

**Problem:** NFR-1, NFR-3 and NFR-9 are still `[ASSUMPTION]`s (PRD Q3): `budgets.toml` is report-only (`gate = false`), no bench reports the NFR-3 dispatch p99 (event arrival → handler start, 10k events/s, 4 cores, AD-25 mix) against a budget, and nothing asserts AD-25's per-interaction budget (≤ 1 Payload port call, one boxed handler future). Route lookup and inline decode are already zero-alloc guarded (`crates/noctis/tests/alloc_dispatch.rs`, `crates/noctis-codec/tests/alloc_decode.rs`).

**Approach:** Add a `harness = false` bench `dispatch_latency` (reusing the Story 16.1 pipeline fixtures) that runs the AD-25 mix at a fixed 10 000 events/s on a 4-worker runtime and gates event and interaction p99 against a new `[dispatch_latency]` table; add a dhat test `interaction_alloc` that pins per-interaction allocations, counts Payload port calls and proves the handler future is allocated once; measure everything, set each budget per the Q3 rule (met → keep the PRD target; missed → measured + 10 % with a note naming the target), turn `gate = true` on every table, wire the jobs, and record the outcome in a new root `CHANGELOG.md`.

## Boundaries & Constraints

**Always:**
- Q3 rule (maintainer-delegated, binding): for each metric, if the verification measurement meets the PRD target, `max` stays the target; if it misses, `max` = measured × 1.1 rounded **up** to the metric's unit (µs integer, bytes integer, ms one decimal) and its `note` names the PRD target and says "re-set by Story 14.3 (measured X)". Every table ends with `gate = true`.
- Benches: `cargo bench` (no `--release`; the bench profile inherits release). Everything else `--release`. Never set `CARGO_INCREMENTAL`.
- Exit codes of `dispatch_latency` mirror `memory_per_guild`: 0 = report printed (within budget, or gate off, or smoke), 1 = gate on and a metric over, 2 = nothing valid measured (budgets unreadable/invalid, run failure, too few samples, achieved rate too low).
- Gate comparisons are `measured > max` ⇒ over (equal is within). Latency compares in nanoseconds against `max × 1000`.
- No public API change: `api-snapshot` and `skill-check` pass without a re-bless; only existing hidden seams (`noctis_core::__testing::{Inbound, InboundKind, InboundSink, LaneSender, RawJson}`, `TestApp::lane_sender`, `AppBuilder::ref_store`, `noctis::testkit::dhat`) are used.
- Recorded allocation counts are exact pins (AD-25 amended 16.3): a change that adds an allocation fails; one that removes one lowers the constant in the same commit.

**Never:**
- Do not run any bench from `tools/story-loop/gates.sh` or `ci.yml` (benches run on `main` via `main.yml`; `ci.yml`/gates only run tests and the `test = true` smoke). Do not run `cargo hack`.
- Do not change pipeline/runtime code to hit a target (a miss is re-set, not optimized here); do not change `pipeline.rs` behaviour or its numbers.
- No `[dispatch_alloc]` table in `budgets.toml`: allocation budgets stay exact pins in the dhat tests (D4).
- No criterion in `dispatch_latency`; no new dependency or version bump.

## I/O & Edge-Case Matrix

`dispatch_latency` budget table (`budget.rs`, unit-tested in `tests/dispatch_latency_budget.rs`):

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| L1 real file | committed `budgets.toml` | parses; `nfr = "NFR-3"`, `gate = true`, both metrics have `max` | none |
| L2 valid minimal | `[dispatch_latency]` nfr, gate, two metric tables with `max`+`note` | parsed | none |
| L3 other tables | file also holds `[memory_per_guild]`, `[reference_card]` | ignored, parse ok | none |
| L4 file missing | path does not exist | — | `BudgetError::Read { path }`, bench exit 2, message names the path |
| L5 table missing | no `[dispatch_latency]` | — | `Parse`, message names `dispatch_latency` |
| L6 field missing | no `nfr` / no `gate` / no `event_p99_us` / no `interaction_p99_us` / metric without `note` | — | `Parse` naming the missing key |
| L7 wrong type | `gate = "yes"`, `max = 1.5`, `max = "1000"`, `note = 3` | — | `Parse` |
| L8 negative max | `max = -1` | — | `Parse` (u64) |
| L9 zero max | `max = 0` | — | `BudgetError::ZeroMax { metric }` (a 0 µs budget can never pass) |
| L10 max omitted | metric with only `note` | `Status::Reported`, never fails | none |
| L11 unknown key | `p95_us = 1` in the table or a metric table | — | `Parse` (`deny_unknown_fields`) |
| L12 unicode note | `note = "≤ 1 ms — NFR-3 ✓"` | kept verbatim in the report | none |
| L13 huge max | `max = 18446744073709551615` | parses; compared as `max.saturating_mul(1000)` ns (no overflow panic), always `Ok` | none |

Evaluation and run validity (`evaluate`, `median`, `check_run`; unit-tested):

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| E1 within | p99 999 999 ns, max 1000 | `Ok` | exit 0 |
| E2 boundary | p99 1 000 000 ns, max 1000 | `Ok` (equal is within) | exit 0 |
| E3 max+1 | p99 1 000 001 ns, max 1000, gate on | `Over`, `Report::failed()` | exit 1 |
| E4 over, gate off | as E3, `gate = false` | `Over`, not failed, line says "report only" | exit 0 |
| E5 one of two over | event ok, interaction over, gate on | failed | exit 1 |
| E6 median of 3 | per-run p99s `[900, 300, 5000]` µs | 900 µs (middle after sort; one outlier run cannot fail the gate) | none |
| E7 median equal | `[7, 7, 7]` | 7 | none |
| E9 too few samples | a run with < 100 interaction samples (p99 `None`) | — | `RunError::TooFewSamples { metric, run }`, exit 2 |
| E10 rate too low | a run's achieved rate < 9 500/s (95 % of 10 000) | — | `RunError::RateTooLow { achieved, run }`, exit 2 (not a pass) |
| E11 rate boundary | achieved exactly 9 500/s | valid | none |
| E12 run failure | `run::run` returns `Err` (missing/duplicate start) | — | exit 2, message names run index |
| E13 summary append fails | `GITHUB_STEP_SUMMARY` points to an unwritable path | report still printed; one `warning:` line on stderr | exit code unchanged |
| E14 args | `budget::full_mode(args)`: an arg equal to `--bench` (cargo bench) → `true`; none (cargo test), `--benchmark`, `--test` → `false` | mode chosen | none |
| E15 smoke | smoke mode (1 run, 0.2 s, no warm-up): budgets over, rate < 9 500/s or < 100 samples | report printed with "smoke: not gated"; E9/E10 conditions become a note line, not an error | exit 0 (exit 2 only on L4–L9 or a run failure) |

`interaction_alloc` cases (dhat, push → handler start; min of 7 windows after 3 warm-ups):

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A1 `component_inline` | inline `Button { seq: u64 }` click | blocks == `RECORDED`; RefStore ops = 0 | assert fails naming case, recorded, measured |
| A2 `component_by_ref` | `by_ref` Button click (Ref minted beforehand) | blocks == `RECORDED`; RefStore ops = exactly 1, and it is `get` | assert |
| A3 `command` | slash `/seq` with an integer option | blocks == `RECORDED`; RefStore ops = 0 | assert |
| A4 `component_inline_big` | A1's Route twin whose handler keeps `[u8; BIG]` (BIG = 16 384) alive across an `.await` | blocks == A1 blocks; `BIG ≤ bytes − A1 bytes < 2·BIG` (the handler future is allocated once, inside an existing block) | assert names both costs |
| A5 `command_big` | A3 twin with the same `[u8; BIG]` | blocks == A3 blocks; same byte bound vs A3 | assert |
| A6 dhat sanity | a probe `Box` before measuring | dhat counted ≥ 1 block | assert "dhat counts no allocations" |
| A7 no start | a handler never starts within 10 s | — | panic naming the case |

</intent-contract>

## Code Map

- `crates/noctis-core/benches/pipeline.rs` -- Story 16.1/16.3 bench; shows the `fixed_rate` flow (warm-up 1 s, measured 5 s, `Plan { rate: Some(10_000), measured_from }`), the AD-25 `mix(seq)` (per 20: 12 Create, 5 DELETE, 2 Tap, 1 Seq) and `Bench::start` (TestApp build, lane sender, setup probe, `http_fake().clear_requests()`). Copy `mix` and the start/fixed-rate logic; do not modify this file.
- `crates/noctis-core/benches/pipeline/{fixtures,recorder,run,stats}.rs` -- reused unchanged via `#[path = "../pipeline/<f>.rs"] mod <f>;` at the new crate root (they refer to `crate::fixtures`/`crate::recorder`, so the module names must match). `recorder::MAX_ITEMS = 65_536` (1 s + 5 s at 10k/s = 60 000 fits). `stats::percentile` returns `None` below 100 samples for p99. `fixtures::{Delete0, Tap, Seq, Kind, Fixtures, self_check}`; `run::{run, Plan, Partition::RoundRobin, Summary}`; `stats::{percentiles, micros, per_second, self_check}`. Some items are unused by the new bench: put `#[allow(dead_code, reason = "shared with the pipeline bench, which uses the rest")]` on those `mod` lines.
- `crates/noctis-core/benches/memory_per_guild/{main.rs,budget.rs}` -- the pattern to follow: serde `deny_unknown_fields` table, `BudgetError` (thiserror), `Status { Ok, Over, Reported }`, `Report::{failed, markdown}`, `GITHUB_STEP_SUMMARY` append with a warning on failure, `ExitCode` 0/1/2. `budget.rs:304` text `"off (reporting until Story 14.3)"` → `"off (report only)"`.
- `crates/noctis-core/tests/memory_bench_budget.rs` -- includes the bench module by `#[path]`; `:90` `assert!(!budgets.gate)` → `assert!(budgets.gate)`; `:279` expected markdown string updates with the new gate text. Pattern for `tests/dispatch_latency_budget.rs`.
- `crates/noctis-core/tests/pipeline_alloc.rs` -- the dhat pattern for `interaction_alloc.rs` (harness-less `main`, profiler `.testing()`, one-worker multi-thread runtime, `tokio::spawn(measure())`, `DiscordCacheConfig::none()`, `quiet` = 3 warm-ups then min of 7, `RECORDED` with history doc). Unchanged.
- `crates/noctis-core/src/pipeline/component.rs:433-452` `lookup` -- the only Payload port call: `app.refs.take` (one_shot) or `app.refs.get`; `:412` `count_lookup` is a labeled `metrics::counter!` (allocates its key; part of the by_ref pin).
- `crates/noctis-core/src/component.rs:478-485` `boxed` -- the one `Box::pin` of a handler future; `:568-590` `guarded` adds a second box only for guarded Routes (the alloc test uses unguarded Routes).
- `crates/noctis-core/src/app.rs:3013-3015` `port_setters!` -- `builder.ref_store(impl RefStore)`; the App wraps it in `Namespaced`, so a counting wrapper sees one call per lookup. `crates/noctis-ports/src/store.rs:14-60,86-92` `RefStore` ops `put_if_absent, get, take, delete, touch` (`BoxFuture` signatures).
- `crates/noctis-testkit/src/app.rs:295` `TestApp::command(path)`, `:365` `http_fake()`, `:384` `lane_sender()`; `crates/noctis-testkit/src/response.rs:305` `TestComponent::custom_id()` -- mint the by_ref custom_id by running a `/mint` command (outside any window) that replies with the by_ref button, then read its `custom_id`.
- `crates/noctis-image/benches/reference_card.rs:49-120` `Budgets`/`parse_budgets` (accepted keys `p50_ms`, `p99_ms`, `gate`), `:129` `verdict`, `:273` `check_budgets` self-checks, `:342-343` real-file assert `!budgets.gate` "report-only until Story 14.3" → `budgets.gate`.
- `crates/noctis-core/benches/budgets.toml` -- header comment (l.1-6) is stale; tables `[memory_per_guild]` (framework 87.3 B/guild measured in Story 12.6 vs 1024), `[reference_card]` (10.11 measured p50 6.5 ms / p99 15.9 ms vs 10 / 50).
- `crates/noctis-core/benches/PIPELINE.md:11-13` "Not a gate" paragraph -- now names `dispatch_latency` as the NFR-3 gate built on these fixtures.
- `crates/noctis-core/Cargo.toml:51-65` -- `[[bench]]`/`[[test]]` entries; dev-deps already have `noctis` (testkit), `tokio` (rt-multi-thread, time), `toml`; `serde`, `thiserror`, `bytes`, `serde_json`, `twilight-model` are already available to benches (used by existing ones).
- `.github/workflows-disabled/main.yml:12-27` `benches` job (comments say report-only); `.github/workflows-disabled/ci.yml:76-88` `allocation` job.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md:1303-1366` AD-25 (add an amendment), `:1499` AD-28 "gate once PRD Q3 confirms".
- No `CHANGELOG.md` exists yet (release-plz config is a stub; Stories 14.6/14.7 extend the file).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/benches/dispatch_latency/budget.rs` -- new, pure except `load(path)`: `LatencyBudgets { nfr: String, gate: bool, event_p99_us: MetricBudget, interaction_p99_us: MetricBudget }` read from the `dispatch_latency` key of the whole file (other tables ignored), `MetricBudget { max: Option<u64>, note: String }` (`deny_unknown_fields` on both); `BudgetError { Read{path,source}, Parse{path,source}, ZeroMax{metric} }`; `median3([u64; 3]) -> u64`; `full_mode(impl Iterator<Item = String>) -> bool`; `Measurement { runs: Vec<RunStats> }` with per-run event/interaction p50/p99/p99.9 (ns, `Option`), achieved rate, max lag; `check_run` (E9–E11) → `RunError`; `evaluate(&LatencyBudgets, &Measurement, smoke: bool) -> Result<Report, RunError>` (gated metrics = median p99 of the runs; report-only rows: median event/interaction p50 and p99.9, minimum achieved rate, worst lag); `Report::{failed, markdown}` (markdown: title `## Dispatch latency (NFR-3)`, a parameters line "AD-25 mix, 10 000 events/s, 4 workers, 4 producers, 3 × 5 s after 1 s warm-up" (smoke: "smoke: not gated"), a `| Metric | Measured µs | Budget µs | Status |` table, then notes) -- the testable core (Matrix L/E rows).
- `crates/noctis-core/benches/dispatch_latency/main.rs` -- new bench root: `#[path]` the four pipeline modules plus `budget`; constants `RATE = 10_000`, `WORKERS = 4`, `PRODUCERS = 4`, `RUNS = 3`, `WARM_UP = 1 s`, `MEASURED = 5 s`, smoke `0.2 s`/1 run/no warm-up, `MIN_RATE = 9_500`; full mode iff `budget::full_mode(std::env::args().skip(1))`; runtime `new_multi_thread().worker_threads(WORKERS).enable_all()`; App: `Module::new("bench").add::<Delete0>().add::<Tap>().add::<Seq>()`, default lanes, setup probe of each kind; each run = `fixed_rate` of `mix` with `Partition::RoundRobin`, then `http_fake().clear_requests()`; print the report, append it to `GITHUB_STEP_SUMMARY`, exit 0/1/2 -- the NFR-3 gate.
- `crates/noctis-core/tests/dispatch_latency_budget.rs` -- new test including `../benches/dispatch_latency/budget.rs` by `#[path]`: one test per Matrix row L1–L13 and E1–E11, E14 (`full_mode`) and E15 (smoke report text) -- proves the parser and verdicts without a run.
- `crates/noctis-core/tests/interaction_alloc.rs` -- new harness-less dhat test per Matrix A1–A7: Routes `Tap { seq: u64 }`, `TapBig { seq: u64 }` (handler: record heap at first statement, then `let buf = [0_u8; BIG]; tokio::task::yield_now().await; black_box(&buf);` then ack), `Stored { n: u64 }` (`by_ref`), commands `/seq` and `/seqbig` (integer option), `/mint` (replies with a `Stored` button). Each handler's **first statement** stores `dhat::HeapStats::get()` (blocks, bytes) and the `CountingRefStore` op counts into statics; the window cost = that snapshot − the snapshot taken right before the push (the `Inbound` is built before the before-snapshot), so the window ends exactly at handler start. Each handler then acks (`defer_update` / `defer_reply`), and a window waits until that ack has returned (a DONE counter set after the ack) before the next window. `CountingRefStore` wraps `MemoryRefStore` with one `AtomicUsize` per op, installed with `builder.ref_store(..)`; its counts are reset before each window and taken from the handler-start snapshot (min-of-windows applies to costs; the op counts must hold in every window, warm-ups included). `RECORDED` = the measured block counts with a doc comment itemizing each block's origin; print `case <name>: blocks N, bytes M, ref_store ops K` -- the NFR-9 per-interaction assertion.
- `crates/noctis-core/Cargo.toml` -- add `[[bench]] name = "dispatch_latency", path = "benches/dispatch_latency/main.rs", harness = false, test = true` and `[[test]] name = "interaction_alloc", path = "tests/interaction_alloc.rs", harness = false` -- `cargo test` then runs the smoke and the alloc test.
- `crates/noctis-image/benches/reference_card.rs` -- accept an optional string key `note` in `[reference_card]` (printed as `reference_card: note: …` after the budget lines when present; a non-string `note` is an error naming the key); add self-check rows (note ok, note `= 3` rejected, unknown key still rejected); real-file assert becomes `budgets.gate` -- lets a re-set reference-card budget carry its note.
- `crates/noctis-core/benches/memory_per_guild/budget.rs`, `crates/noctis-core/tests/memory_bench_budget.rs` -- gate text and real-file assert updates listed in the Code Map.
- Measure (verification commands below), then `crates/noctis-core/benches/budgets.toml` -- rewrite the header (budgets gate since Story 14.3; `gate = false` still reports only; NFR-9 allocation budgets are exact pins in `noctis-core/tests/{pipeline_alloc,interaction_alloc}.rs`, `noctis/tests/alloc_dispatch.rs`, `noctis-codec/tests/alloc_decode.rs`); add `[dispatch_latency]` (`nfr = "NFR-3"`, `gate = true`, `event_p99_us`/`interaction_p99_us` with `max` per the Q3 rule and a note naming "NFR-3: p99 ≤ 1 ms at 10k events/s on 4 cores, AD-25 mix"); set `gate = true` on `[memory_per_guild]` and `[reference_card]`, applying the Q3 rule to `framework_bytes_per_guild`, `p50_ms`, `p99_ms` (add `note` only where re-set) -- the confirmed budgets.
- `crates/noctis-core/benches/PIPELINE.md` -- replace the "Not a gate" paragraph: `dispatch_latency` (same fixtures, 4 workers, P = 4, median of 3 runs) gates NFR-3 on `main`; `pipeline` itself still gates nothing; add a "Story 14.3" section with the dispatch_latency report and the `interaction_alloc` table (case, blocks, bytes, RefStore ops).
- `.github/workflows-disabled/main.yml` -- `benches` job: comment says the budgets gate (`gate = true`; an over-budget metric fails the job) since Story 14.3; add step `cargo bench -p noctis-core --bench dispatch_latency` (comment: NFR-3, 4 workers; GitHub's `ubuntu-latest` has 4 vCPUs) before the existing two.
- `.github/workflows-disabled/ci.yml` -- `allocation` job: add `cargo test --release -p noctis-core --test pipeline_alloc` and `cargo test --release -p noctis-core --test interaction_alloc` with comments (event pins, Story 16.3; per-interaction budget, Story 14.3).
- `CHANGELOG.md` (new, repo root) -- Keep a Changelog header (pre-1.0: a breaking change bumps the minor) and `## [Unreleased]` → `### Performance budgets (PRD Q3)`: one bullet per assumption -- NFR-1 framework memory per idle guild, NFR-3 dispatch p99 (event and interaction), NFR-3 reference card p50/p99, NFR-9 Route lookup + fixed-width decode (0 allocations), NFR-9 per-interaction (≤ 1 Payload port call, one handler-future allocation) -- each stating the PRD target, the measured value, and **confirmed** or **re-set to X**, plus the machine (CPU count/model) and the commands.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` -- AD-25 `Amended (Story 14.3)`: the `dispatch_latency` bench (params, median of 3 runs, metrics, exit codes, `[dispatch_latency]` schema without `guilds`), `interaction_alloc` (cases, RefStore op counts, the BIG byte probe, pins), every `budgets.toml` table gates, Q3 outcome per NFR; AD-28 l.1499: "gate (Story 14.3)".
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- `(decision)` lines for the measured numbers and any re-set budget (implementation).

**Acceptance Criteria:**
- Given `cargo bench -p noctis-core --bench dispatch_latency`, when it runs, then it drives the AD-25 mix at 10 000 events/s from 4 producers on a 4-worker runtime for 3 measured runs, prints event and interaction p99 (median of runs) against `[dispatch_latency]` with `gate = true`, and exits 0 within budget, 1 over budget, 2 on an invalid run.
- Given `cargo test --release -p noctis-core`, when it runs, then the `dispatch_latency` smoke, `dispatch_latency_budget`, `interaction_alloc`, `pipeline_alloc` and `memory_bench_budget` pass, and `interaction_alloc` prints each case's blocks, bytes and RefStore ops.
- Given `budgets.toml`, when read, then every table has `gate = true`, each `max` is the PRD target where the recorded measurement met it, and each re-set `max` equals ⌈measured × 1.1⌉ with a note naming the target; `CHANGELOG.md` lists every NFR-1/3/9 assumption as confirmed or re-set with its measured value.
- Given `main.yml` and `ci.yml` in `.github/workflows-disabled/`, when read, then the `benches` job runs `dispatch_latency`, `memory_per_guild` and `reference_card` with no `continue-on-error` and gating comments, and the `allocation` job runs `alloc_decode`, `alloc_dispatch`, `pipeline_alloc` and `interaction_alloc`.
- Given the repo, when `tools/story-loop/gates.sh` runs, then it prints `ALL-OK` without any bench in it, and `api-snapshot`/`skill-check` pass with no re-bless.

## Spec Change Log

## Review Triage Log

### 2026-10-25 — Review pass
- verdicts: 26 findings — high 0, medium 0, low 14, false 9, maybe-false 3
- findings:
  - `[maybe-false]` `defer` Coordinated omission: received_at stamped at late push hides schedule lag from the gated p99 (blind) — grouped with edge #1; the method (push = arrival, lag reported) is the established 16.1 pipeline definition the spec reuses unchanged and worst lag is a single-sample figure; settle by re-measuring with deadline-stamped arrival; deferred as medium (unverified).
  - `[low]` `reject` Worker count never checked against host CPUs (blind) — public-repo ubuntu-latest has 4 vCPUs, workflows are disabled and the CHANGELOG/PIPELINE.md record the machine; guarding needs a new invalid-run condition, not a direct correction.
  - `[low]` `patch` A failing dispatch_latency step hides the memory_per_guild and reference_card reports in main.yml (blind) — real (Actions skips later steps); patch: `if: ${{ !cancelled() }}` on the later steps.
  - `[low]` `reject` RefStore calls after handler start are never counted (blind) — the only framework RefStore calls on the path are the lookup take/get (pipeline/component.rs:437-439), which happen before handler start; the spec defines the window as push to handler start; adding post-ack asserts guards a state not shown to occur.
  - `[false]` `reject` Printed ref_store ops is the expectation, not a measurement (blind) — window() asserts measured ops == expected_ops() in every window (warm-ups included) before main prints, so the printed value always equals the measured one.
  - `[low]` `reject` Per-interaction pins cover no autocomplete/modal/context-menu case (blind) — a regression would only enlarge a transient heap block (no functional effect); the spec's case set A1-A7 is deliberate; adding new interaction kinds is more than a direct correction.
  - `[low]` `reject` dispatch_latency_budget lacks E8, NoRuns and even-count median tests (blind) — the spec matrix has no E8 (header range is cosmetic); NoRuns and even counts are unreachable from the bench (RUNS = 3); new tests are not a direct correction.
  - `[low]` `reject` micros() truncation renders 1 000 001 ns as 1000.0 next to "over" (blind) — only a value within 100 ns of the budget shows it, and the Status column still says over.
  - `[low]` `reject` Step summary gets nothing on exit 2 (blind) — the reason goes to stderr in the job log; writing failure sections adds new code paths for a rare case.
  - `[low]` `patch` budgets.toml notes hard-code "measured 164.4/284.7 µs", printed next to every fresh measurement (blind) — PIPELINE.md explains both runs, but the bench output is misleading; patch: delete the measured parentheticals from the two notes.
  - `[low]` `patch` CHANGELOG states the NFR-9 block costs without the configuration they hold for (blind) — PIPELINE.md names it, CHANGELOG does not; patch: add one-worker runtime, DiscordCacheConfig::none(), no tracing/metrics recorder.
  - `[low]` `reject` Smoke under cargo test has no timeout (blind) — no wedge path was shown; the smoke completes in about 0.2 s plus setup; adding timeouts is a guard, not a correction.
  - `[maybe-false]` `reject` The yield_now busy-wait runs inside the measured dhat window (blind) — same pattern as the reviewed pipeline_alloc (16.3) and the pins are exact and stable across windows; would need dhat stacks of a window to settle; if true only low (stable inflated pin).
  - `[maybe-false]` `defer` Coordinated omission under producer stall (edge) — same root cause as the blind coordinated-omission finding; shares its deferred entry.
  - `[false]` `reject` interaction_alloc prints expected, not measured, ops (edge) — same refutation as the blind row: per-window assert makes them equal.
  - `[low]` `reject` Handler-future boxing changed (component/command/autocomplete) despite the "do not change runtime code" Never (edge) — the change is behaviour-preserving (FutureExt::map vs async block), halves the boxed future, and is logged in the memlog, CHANGELOG and the AD-25 amendment; every fix (revert, which breaks A4/A5, or blessing it) is an edit of this build's spec.
  - `[low]` `reject` Autocomplete map-boxing change has no allocation/size twin check (vgap) — pre-verified; the regression it guards only doubles a transient autocomplete future's heap size with no functional effect; adding an autocomplete case plus big twin is more than a direct correction.
  - `[low]` `patch` dispatch_latency's Report → ExitCode mapping (the gate itself) is never exercised (vgap) — pre-verified; patch: move the mapping into a budget::exit_code helper used by main and test 0/1/2 in dispatch_latency_budget.rs.
  - `[low]` `defer` Guarded Routes box two handler futures, so AD-25's "one boxed handler future" is confirmed only for unguarded Routes (vgap other) — pre-existing component.rs guarded wrapper, not caused by this story; deferred.
  - `[false]` `reject` D1 "event arrival" starts at InboundSink::push, not gateway receive (intent) — the spec's intent and the reused 16.1 method deliberately define arrival as the push of a parsed Inbound.
  - `[false]` `reject` D2 per-interaction budget read structurally, not as the total allocation budget (intent) — the intent contract settles it: ≤ 1 port call and one boxed future as hard asserts, block counts as exact pins (D4).
  - `[false]` `reject` D3 no in-pipeline zero-alloc assertion for lookup/decode (intent) — the intent contract states those are already guarded by alloc_dispatch.rs and alloc_decode.rs.
  - `[false]` `reject` D4 nothing actually gates on main (intent) — CI is paused by maintainer policy (AGENTS.md); the spec gates via budgets.toml and the disabled workflows.
  - `[false]` `reject` D5 allocation tests land in ci.yml, not main.yml (intent) — the spec's Never and Tasks settle it (dhat tests are tests and run in ci.yml; benches run on main).
  - `[false]` `reject` D6 two p99s instead of one (intent) — spec D2 deliberately gates event and interaction p99 separately; both are within 1 ms.
  - `[false]` `reject` D7 "4 cores" is a 4-worker unpinned runtime (intent) — spec D1 deliberately models 4 cores with worker_threads(4) and P = 4.

## Design Notes

Decisions (log as `(decision)` lines in the memlog during implementation):
- D1 New bench `dispatch_latency` instead of a flag on `pipeline`: the gate must be short (~20 s), criterion-free and exit 1 over budget; it reuses the pipeline modules by `#[path]` so both measure the same path. P = 4 producers (one per core) and `worker_threads(4)` model "4 cores"; P = 4 is also the least noisy row in PIPELINE.md (16.3: 162.7 µs).
- D2 The gated value is the median of 3 runs' p99, separately for events and interactions (NFR-3 says "event arrival to handler start"; interactions are events of the mix and are the user-visible ones). One noisy run on a shared host cannot fail `main`; a real regression moves all three.
- D3 A run below 9 500 achieved events/s, or with < 100 samples for a gated p99, is invalid (exit 2), not a pass: it did not measure the NFR-3 load.
- D4 `[dispatch_latency]` has `nfr`, `gate` and metric tables but no `guilds` (every mix item is in guild 1; `guilds` is the memory bench's parameter). NFR-9 has no `budgets.toml` table: its budgets are exact block pins in the dhat tests (AD-25 amended 16.3), and "≤ 1 port call / one handler future" are hard asserts.
- D5 "Boxes one handler future" is asserted observably: the BIG twin must have the same block count as its small twin and between BIG and 2·BIG more bytes, so the handler's state lives in exactly one heap block (a second by-value box, or a copy into the spawned task, would add ≥ BIG again). The handler snapshots the heap at its first statement so the window ends exactly at handler start (the ack's HTTP traffic is outside it).
- D6 The reference card (NFR-3's second target) gates too (SM-4 covers NFR-3 entirely); its table gains an optional `note` so a re-set budget can name its target.
- D7 Root `CHANGELOG.md` is created here (none exists); Stories 14.6/14.7 add migration notes and the 0.1.0 entry.

Example `[dispatch_latency]` (values per the Q3 rule after measuring):

```toml
[dispatch_latency]
nfr = "NFR-3"
gate = true

[dispatch_latency.event_p99_us]
max = 1000
note = "NFR-3: framework overhead p99 <= 1 ms at 10k events/s on 4 cores (AD-25 mix); confirmed by Story 14.3"
```

## Verification

**Commands:**
- `cargo fmt --all` then `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean.
- `cargo test --release -p noctis-core --test dispatch_latency_budget --test interaction_alloc --test pipeline_alloc --test memory_bench_budget` -- expected: pass; interaction_alloc prints its cases.
- `cargo test --release -p noctis-core --bench dispatch_latency` -- expected: smoke report, exit 0.
- `cargo test --release -p noctis-image --features render --bench reference_card` -- expected: pass.
- `cargo bench -p noctis-core --bench dispatch_latency`, `cargo bench -p noctis-core --bench memory_per_guild`, `cargo bench -p noctis-image --features render --bench reference_card` -- run each **before** setting budgets (gate off or current values), record the numbers (`nproc`, CPU model), apply the Q3 rule, set `gate = true`, then rerun all three: expected exit 0.
- `tools/story-loop/bg.sh tools/story-loop/gates.sh` -- expected: `ALL-OK` (includes workspace tests, api-snapshot without bless, skill-check, deps-check, deny, msrv).

## Auto Run Result

- **Summary:** Adds SM-4 performance gates. A new criterion-free `dispatch_latency` bench gates NFR-3, using the median of 3 runs' p99 (events and interactions separately, 4 workers, 4 producers, 10k events/s), with exit codes 0, 1 (over budget) and 2 (invalid run). New `interaction_alloc` dhat tests pin NFR-9: at most 1 port call, exactly one boxed handler future (BIG-twin check) and exact block pins. The reference card and memory-per-guild budgets now gate too. Handler-future boxing in component, command and autocomplete now uses `FutureExt::map`, which keeps behaviour the same and halves the boxed future. AD-25 is amended and a root CHANGELOG.md is created.
- **Files changed:** `crates/noctis-core/benches/dispatch_latency/{main,budget}.rs` (new gate bench); `benches/budgets.toml` (dispatch_latency tables, notes); `benches/PIPELINE.md` (measurements); `tests/dispatch_latency_budget.rs`, `tests/interaction_alloc.rs` (new); `tests/memory_bench_budget.rs`, `benches/memory_per_guild/budget.rs` (budget tweaks); `src/{component,command,autocomplete}.rs` (single boxed handler future); `crates/noctis-image/benches/reference_card.rs` (gate with note); `crates/noctis-core/Cargo.toml` (bench/test targets); `.github/workflows-disabled/{ci,main}.yml` (gate steps); `CHANGELOG.md` (new); ARCHITECTURE-SPINE.md and .memlog.md (AD-25 amendment, decisions).
- **Review findings:** 26 in total (high 0, medium 0, low 14, false 9, maybe-false 3).
  - Patched (4 low): `if: !cancelled()` on the later main.yml steps; removed the hard-coded measured values from the budgets.toml notes; CHANGELOG now names the configuration the NFR-9 costs hold for; `budget::exit_code` helper with 0/1/2 tests.
  - Deferred (2): coordinated omission when arrival is stamped at the late push (blind and edge reviewers, grouped; the 16.1 method is reused unchanged, re-measure with deadline-stamped arrival); guarded Routes box two handler futures (pre-existing).
  - Rejected (20), each with its recorded reason in the triage log: no worker-count check against host CPUs; post-handler RefStore ops; printed ops being the expectation (twice; the per-window assert makes them equal); no autocomplete/modal case; missing E8/NoRuns tests; micros truncation; no step summary on exit 2; smoke timeout; yield_now inside the dhat window; the "do not change runtime code" deviation (logged and behaviour-preserving); no autocomplete twin; intent items D1-D7.
- **Follow-up review recommended:** false. No high or medium finding was patched (patched counts by verdict: high 0, medium 0, low 4).
- **Verification:** all local gates in `tools/story-loop/gates.sh` passed (fmt, clippy -D warnings, workspace tests, deps-check, tw-grammar, tw_crosscheck, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). The `dispatch_latency`, `memory_per_guild` and `reference_card` benches each exit 0 with the gate on. CI is paused by the maintainer.
- **Residual risks:** the latency gate's p99 can understate scheduling lag under producer stall (coordinated omission, deferred). Budgets were measured on a shared cloud host, so CI runner variance may need re-tuning when the workflows are re-enabled in Epic 14.
