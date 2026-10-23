---
title: 'Story 12.1: Shard Ranges and Cluster-wide identify'
type: 'feature'
created: '2026-10-23'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Every process starts all shards `0..total`, so a bot cannot be split across a Cluster: there is no Shard Range, the startup checks (session start limit, readiness, shard gauges) assume the whole total, and one `IdentifyQueue` instance cannot be handed to two in-process Apps (no `Arc<P>` impl), so Cluster-wide identify serialization is untestable.

**Approach:** Add a configured Shard Range (`AppBuilder::shard_range(start..end)` / `NOCTIS_SHARD_RANGE=start..end`, half-open, requires a fixed `shard_total`, validated at `build()`), pass it through `GatewayOptions` so the gateway spawns only those shards, scope the run-time plan/readiness/gauges to the range, warn on a range not aligned to `max_concurrency`, implement `IdentifyQueue` for `Arc<P>`, and prove per-bucket identify concurrency across two in-process Apps sharing one queue plus the designated-publisher rule. FR-3, FR-13, AD-21, AD-14, GW-01.

## Boundaries & Constraints

**Always:**
- Range notation is Rust's half-open `start..end` in both the builder (`std::ops::Range<u32>`) and the env var; shard ids are global (`ShardId::new(id, total)` with `id` in the range, `total` the fixed total), so buckets stay `shard_id % max_concurrency` (GW-01) across processes.
- A Shard Range needs a fixed shard total (every process must agree on the total; Discord's recommendation can differ between process starts). All range validation that needs no Discord data happens at `build()` (compile → build → runtime order); only alignment needs `max_concurrency`, so it is a run-time warning.
- GW-02 session-start check compares `session_start_limit.remaining` with the number of shards **this process identifies** (range length), not the total. GW-03 (`ShardTotalTooSmall`) still checks the total.
- Identify always goes through the `IdentifyQueue` port (`PortQueue`); never `InMemoryQueue::default()`. A non-default port gets every bucket unchanged.
- The existing publisher behaviour (`publish_at_startup`, before `GET /gateway/bot`, never on READY) is kept; this story only adds the Cluster test and docs.
- Messages never contain the token. Every new public item has rustdoc with a doctest (crate convention).
- Public API change ⇒ same commit updates `examples/project-template/` (README env table, `.env.example`, `AGENTS.md`, the Usage Skill), re-blesses `xtask/public-api.txt`, and the Skill's `API hash` line.
- `rust-best-practices` applies (read `references/api-design.md`, `error-handling.md`, `async-and-concurrency.md`, `testing.md`).

**Never:**
- No `NOCTIS_SHARDS` variable (the Skill's old planned name); no inclusive `..=` notation, no `a-b` notation, no total embedded in the range string.
- No session resume, SessionStore, `--no-resume`, HTTP-proxy warning or cache work (Stories 12.2–12.5). No FR-53 presence routing.
- No new crates or dependency versions. No change to `IdentifyQueue`'s trait methods or `Lease`.
- Do not change the `Enforcement` column of any `docs/discord-rules.md` row (only the How text of GW-01/GW-02/GW-03); no new rule ids.
- No `Namespaced`/`Shared` wrapping of `IdentifyQueue` (AD-14); `Shared::new(queue)` stays a compile error.

## I/O & Edge-Case Matrix

Env parsing (`NOCTIS_SHARD_RANGE`, via `read_var`: trimmed of ASCII whitespace, empty = unset, read after `NOCTIS_SHARD_TOTAL` and before `NOCTIS_SHUTDOWN_GRACE`). Grammar: `<digits>..<digits>`, each side 1-10 ASCII digits fitting `u32` (leading zeros allowed), nothing else.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| E1 | valid | `32..64` | `builder.shard_range(32..64)` | none |
| E2 | trimmed / leading zeros | `"  0007..0010\n"` | `7..10` | none |
| E3 | unset / empty / whitespace | absent, `""`, `"   "` | no range | none |
| E4 | max | `4294967294..4294967295` | parsed; `build()` then judges bounds | none |
| E5 | over u32 / 11 digits | `0..4294967296`, `00000000001..2` | — | `EnvError::InvalidShardRange { var: "NOCTIS_SHARD_RANGE" }` |
| E6 | wrong notation | `32..=63`, `32-63`, `32..`, `..64`, `..`, `32`, `32...64`, `32..64..96`, `32 .. 64`, `+1..2`, `-1..2`, `0x1..2`, `1_0..20`, `32..64/128` | — | `EnvError::InvalidShardRange` |
| E7 | unicode digits / non-Unicode | `٣..٤` (Arabic-Indic), `３..４` (full-width); raw non-UTF-8 bytes | — | `InvalidShardRange`; non-UTF-8 → `EnvError::NotUnicode` |
| E8 | empty range text accepted | `5..5`, `9..3` | parsed, passed on | `build()` reports `EmptyShardRange` (B2) |
| E9 | message | any invalid value `SECRET..x` | Display names the var and shows the format `start..end`, e.g. `0..16`; never the value | — |

Builder validation in `build()` (`shard_range` last call wins; violations collected like the other `BuildViolation`s):

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| B1 | valid | `shard_total(128).shard_range(32..64)` | `app.shard_range() == Some(32..64)`, `app.shard_total() == Some(128)` | none |
| B2 | empty | `0..0`, `5..5`, `9..3` (with total 16) | — | `BuildViolation::EmptyShardRange { start, end }` only |
| B3 | no total | `shard_range(0..4)`, no `shard_total` | — | `BuildViolation::ShardRangeWithoutTotal { start: 0, end: 4 }` |
| B4 | end = total (boundary) | total 4, `2..4`; total 4, `0..4` | accepted; `0..4` behaves exactly like no range | none |
| B5 | end = total + 1 / start ≥ total | total 4, `2..5`; total 4, `4..5`; `u32::MAX-1..u32::MAX` with total 4 | — | `BuildViolation::ShardRangeOutOfBounds { start, end, total }` |
| B6 | single shard | total 1, `0..1`; total 128, `127..128` | accepted | none |
| B7 | max | `shard_total(u32::MAX).shard_range(0..u32::MAX)` | accepted (ids `0..=u32::MAX-1`) | none |
| B8 | total 0 + range | `shard_total(0).shard_range(0..1)` | — | `InvalidShardTotal` only (no range violation) |
| B9 | empty + no total | `shard_range(3..3)` | — | `EmptyShardRange` only |
| B10 | last call wins | `.shard_range(0..2).shard_range(2..4)` | `2..4` | — |
| B11 | no range | default | `app.shard_range() == None`; all shards `0..total` | — |

Run-time plan (`sharding::plan`, after `GET /gateway/bot`; `mc` = `max_concurrency`):

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| P1 | aligned range | total 128, `32..64`, mc 16 | starts shards 32..64 only; no range warning | none |
| P2 | misaligned start | total 4, `1..3`, mc 2 | starts 1,2; one `warn` (target `noctis::app`, fields `start=1 end=3 max_concurrency=2`, message `shard range is not aligned to max_concurrency; identify buckets are split across processes`) | continues |
| P3 | misaligned end | total 8, `0..3`, mc 2 | warns | continues |
| P4 | last range ends at an unaligned total | total 3, `2..3`, mc 2 | no range warning (end == total); the existing total warning still fires | continues |
| P5 | mc = 1 | any range | never a range warning | — |
| P6 | mc > range length | total 32, `0..4`, mc 16 | warns (end 4 % 16 ≠ 0, end ≠ total) | continues |
| P7 | GW-02 by range | total 4, `2..4`, remaining 2 | starts | none |
| P8 | GW-02 by range, short | total 4, `2..4`, remaining 1 | no connection | `StartError::SessionStartLimit { required: 2, remaining: 1, .. }` |
| P9 | GW-03 still on total | recommended 10, total 3, `0..1` | no connection | `StartError::ShardTotalTooSmall { total: 3, minimum: 4 }` (checked before GW-02) |
| P10 | no range | as today | plan shards = `0..total`; existing tests unchanged | — |

Gateway / readiness / identify:

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| G1 | only own shards | `GatewayOptions { shards: 2..4, total: 4 }` | 2 tasks; IDENTIFY `d.shard` = `[2,4]`, `[3,4]` only; `Gateway::shard_range() == 2..4`, `shard_total() == 4` | — |
| G2 | invalid range to the gateway | `shards` empty or `end > total` | no task started | `ShardConfigError::InvalidShardRange` |
| G3 | startup readiness | range `2..4` | startup done when shards 2 and 3 sent READY; log `all 2 shards ready` | event for a shard outside the range is ignored |
| G4 | gauges | range `2..4` | `noctis_shard_status{shard}` exists only for 2 and 3 | — |
| G5 | all ended | both range shards end after startup | `RunError::AllShardsEnded` | — |
| G6 | shared queue, paused clock | two `PortQueue`s (two "processes") over one `Arc<MemoryIdentifyQueue::new(2)>`; A enqueues 0,1, B enqueues 2,3 at t0 | grants: one of {0,2} and one of {1,3} at t0, the others at exactly t0+5 s; per bucket never two grants < 5 s apart | — |
| G7 | three processes, one bucket | three `PortQueue`s over one `Arc<MemoryIdentifyQueue::new(1)>`, shards 0, 1, 2 | grants at t0, t0+5 s, t0+10 s | — |
| G8 | `Arc<P>` port | `Arc<MemoryIdentifyQueue>`, `Arc<dyn IdentifyQueue>` | implement `IdentifyQueue`, forward `acquire` unchanged | errors forwarded unchanged |
| G9 | two Apps, shared queue (real clock) | total 4, mc 2, App A `0..2`, App B `2..4`, both `.identify_queue(Arc::clone(&q))`, separate fakes per App | gw A sees only shards 0,1; gw B only 2,3; each bucket's two identifies (0/2 and 1/3) ≥ 4.5 s apart; first identify of each bucket within 2 s of the first overall | — |
| G10 | control: no shared queue | same, each App default queue | shards 0 and 2 identify within 2 s of each other | — |

Publisher:

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|----------|--------------|---------------------------|----------------|
| C1 | one publisher | App A `publisher(true)` range `0..1` of 2, App B `publisher(false)` range `1..2`, one command module, a `FakeDiscordHttp` each | http A: `GET {APP}` and `PUT {GLOBAL}` before `GET /gateway/bot`; http B: no request containing `/applications`, before or after READY | — |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/identify.rs` -- `IdentifyQueue` trait + `Lease`. Add `impl<P: IdentifyQueue + ?Sized> IdentifyQueue for std::sync::Arc<P>` forwarding `acquire` (mirror `store.rs:215-230` macro for byte stores). Update `lib.rs:51` doc line to say identify queues too.
- `crates/noctis-ports/tests/ports.rs` -- add Arc forwarding test (G8) next to the `Stub` impl (`:272`, `:395`).
- `crates/noctis/tests/types/shared_identify_queue.stderr` -- the help block lists implementors; re-bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui` after the Arc impl (the test must still fail to compile).
- `crates/noctis-gateway/src/gateway.rs` -- `GatewayOptions` (pub fields, Debug at `:89`): add `pub shards: std::ops::Range<u32>` (doc: "the shard ids this process runs, within `0..total`"); `Gateway::start` (`:180-224`) validates (G2) then spawns `ShardId::new_checked(id, total)` for `id in shards`; store the range; add `pub fn shard_range(&self) -> Range<u32>`; Debug includes it. Update every doctest constructing `GatewayOptions` (`shards: 0..1`, or `0..4` where total 4).
- `crates/noctis-gateway/src/shard_config.rs` -- `ShardConfigError`: add `InvalidShardRange` variant (`#[error("invalid shard range: it must be non-empty and lie within the shard total")]`).
- `crates/noctis-gateway/src/queue.rs` -- `PortQueue` (pub(crate)); add G6/G7 paused-clock tests (`#[tokio::test(start_paused = true)]`) in its `tests` module using `MemoryIdentifyQueue` behind an `Arc` shared by several `PortQueue::new(port.clone(), nz(..))`; record grant `tokio::time::Instant`s.
- `crates/noctis-gateway/src/lib.rs` -- crate docs `# Gateway`: mention `GatewayOptions::shards`.
- `crates/noctis-core/src/sharding.rs` -- `plan(info, fixed)` → `plan(info, fixed, range: Option<Range<u32>>)`; `ShardPlan` gains `shards: Range<u32>` (= range or `0..total`) and `range_misaligned: bool` (rename `misaligned` → `total_misaligned`); GW-02 uses `shards.len()`; order: InvalidGatewayInfo → GW-03 → GW-02. Unit tests P1-P10 here.
- `crates/noctis-core/src/app.rs` -- consts (`:55-73`): add `SHARD_RANGE_VAR = "NOCTIS_SHARD_RANGE"`; `App` field `shard_range: Option<Range<u32>>` + getter `pub fn shard_range(&self) -> Option<Range<u32>>` next to `shard_total` (`:445`); `from_env` doc (`:170-205`) and `from_lookup` (`:1433`, insert after `SHARD_TOTAL_VAR` block) with a `parse_shard_range` fn beside `parse_shard_total` (`:2455`); `AppBuilder` field + setter `pub fn shard_range(&mut self, shards: Range<u32>) -> &mut Self` after `shard_total` (`:1829`, doc: half-open, needs `shard_total`, violations, alignment warning, env var, last call wins); `build()` (`:2155`) validates B2-B9; `run_with` (`:774-820`) passes the range to `plan`, logs the range warning (P2) beside the existing total warning (`:776-783`), sets `GatewayOptions::shards`, logs `info` `starting shards {start}..{end} of {total}`; `ShardStatus` (`:959`) gets `shards: Range<u32>` beside `total` (init/update/Drop only for ids in `shards`; `set_shard_status` still gets `total`); `await_startup` (`:996`) and `supervise` (`:1054`) size their flag vectors by `shards.len()` and index by `shard - shards.start` (out-of-range ignored); `all {n} shards ready` uses the range length. Both `Debug` impls (`:1223`, `:2379`) add `shard_range`. `run_until` rustdoc (`:668-720`) mentions the range.
- `crates/noctis-core/src/observe.rs:261` -- `set_shard_status(app, shard, total, up)` bound check stays; callers pass the total.
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (near `InvalidShardTotal` `:218`): add `EmptyShardRange { start: u32, end: u32 }` ("shard range {start}..{end} is empty: use start..end with start below end, e.g. 0..16"), `ShardRangeWithoutTotal { start, end }` ("shard range {start}..{end} needs a fixed shard total: set shard_total (NOCTIS_SHARD_TOTAL) to the same value on every process"), `ShardRangeOutOfBounds { start, end, total }` ("shard range {start}..{end} exceeds the shard total {total}: shard ids run from 0 to {total}-1"). `EnvError` (near `InvalidShardTotal` `:760`): `InvalidShardRange { var }` ("environment variable `{var}` is not a shard range: set start..end, e.g. 0..16"). Rustdoc + doctests on each.
- `crates/noctis-core/src/app/tests.rs` -- builder/env tests (`shard_total_builder` `:1715` style; `from_lookup` tests); fix `status(total)` helper (`:1951`) and `:3346`, `:2505` (`GatewayOptions`).
- `crates/noctis-core/src/observe/tests.rs:300` -- `ShardStatus` literal gets `shards`.
- `crates/noctis-testkit/tests/{lanes.rs:49,shard_log_redaction.rs:82,sharding.rs:473,gateway_close.rs:58}` -- add `shards: 0..total` to `GatewayOptions` literals.
- `crates/noctis-testkit/tests/sharding.rs` -- reuse `Fakes`, `spawn`, `shard_of`, `wait_identifies`, `Recording` (`:35-200`) for G1, G3, G5, P7, P8.
- `crates/noctis-testkit/tests/cluster.rs` (new) -- G9, G10 with two `Fakes` (separate `FakeDiscordHttp` + `FakeGateway` per App, `GatewayBot { shards: 4, max_concurrency: 2 }`), compare `RecordedFrame::received_at` (std `Instant`) across both gateways.
- `crates/noctis-testkit/tests/shard_range_warning.rs` (new, own binary: global capturing subscriber like `sharding_warning.rs`) -- P2 then P1/P4 in one test function, sequential Apps, asserting the warning line fields and its absence.
- `crates/noctis-testkit/tests/publish.rs` -- C1 next to Matrix 29/30 (`:741-790`), reusing `fake()`, `start_run`, `requests`, `APP`, `GLOBAL`.
- `docs/discord-rules.md:292-294` -- How text only: GW-01 names `AppBuilder::shard_range`/`NOCTIS_SHARD_RANGE` and the alignment rule (start and end multiples of `max_concurrency`, end = total allowed); GW-02 "below the number of shards this process identifies (its Shard Range)"; GW-03 adds the three range `BuildViolation`s.
- `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` -- getters list (`:86-93`, add `app.shard_range()`), builder table (`:112`, row `.shard_range(start..end)`), BuildViolation list (`:167`), EnvError list, env table (`:281-285`: new `NOCTIS_SHARD_RANGE` row, fix the "read after" chain), §4 Sharding bullets, `### Clusters` example (`:465-479`: `NOCTIS_SHARD_RANGE=0..32`, shared `IdentifyQueue` via `.identify_queue(..)` with `Arc` for in-process Apps, alignment rule, exactly one publisher).
- `examples/project-template/.claude/skills/noctis/SKILL.md:23` -- `API hash` line ← hash printed by `api-snapshot --bless`.
- `examples/project-template/{README.md:77-80,.env.example:15,AGENTS.md:33}` -- env table row, commented `# NOCTIS_SHARD_RANGE=` with a one-line explanation, one Cluster line in AGENTS.md (range + same total everywhere + one publisher). `src/main.rs` already says "shard range"; no code change needed unless `skill-check`/template tests require it.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions already appended; implementation decisions go there too.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ports/src/identify.rs`, `src/lib.rs`, `tests/ports.rs` -- `Arc<P>` impl + test (G8) -- lets one queue serve several Apps.
- `crates/noctis/tests/types/shared_identify_queue.stderr` -- re-bless -- implementor list changed.
- `crates/noctis-gateway/src/{gateway.rs,shard_config.rs,lib.rs}` -- `shards` field, validation, `shard_range()`, doctests (G1, G2).
- `crates/noctis-gateway/src/queue.rs` -- paused-clock cross-process tests (G6, G7).
- `crates/noctis-core/src/sharding.rs` -- range-aware plan + unit tests (P1-P10).
- `crates/noctis-core/src/{app.rs,error.rs}` -- setter, env, getter, violations, run wiring, ShardStatus/readiness scoping (E1-E9, B1-B11, G3-G5).
- `crates/noctis-core/src/app/tests.rs`, `src/observe/tests.rs` -- env/builder unit tests for every E/B row; fix literals.
- `crates/noctis-testkit/tests/{sharding.rs,cluster.rs,shard_range_warning.rs,publish.rs,lanes.rs,shard_log_redaction.rs,gateway_close.rs}` -- G1, G3, G5, G9, G10, P2/P4, P7, P8, C1; literal fixes.
- `docs/discord-rules.md` -- GW-01/02/03 How text.
- `examples/project-template/**` -- Skill reference, SKILL.md hash, README, `.env.example`, AGENTS.md.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`.

**Acceptance Criteria:**
- Given `shard_total(128).shard_range(32..64)` (or `NOCTIS_SHARD_TOTAL=128`, `NOCTIS_SHARD_RANGE=32..64`), when the process starts, then it opens exactly 32 gateway connections identifying shards `[32..64, 128]` and startup completes when those 32 are READY; a range not aligned to `max_concurrency` starts the same way and logs one warning.
- Given two in-process Apps with ranges `0..2` and `2..4` of 4 sharing one `IdentifyQueue` (`Arc`), when both boot, then no bucket identifies twice within 5 s across both Apps (paused-clock queue test G6/G7 and real-clock App test G9).
- Given a Cluster where exactly one App sets `publisher(true)`, when all start, then only that App sends Publish requests, before its `GET /gateway/bot`; the others send no `/applications` request.
- Given the new API, when the story is done, then the Usage Skill's app-and-sharding reference documents Shard Ranges, `NOCTIS_SHARD_RANGE`, the shared `IdentifyQueue` and the publisher flag; `skill-check`, `api-snapshot` and `rules-catalog` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Half-open notation** (decision): `start..end` matches Rust's `Range<u32>` and the Skill's existing Cluster example; "shards 32–63 of 128" is `32..64` with total 128. One notation for builder and env avoids off-by-one translation.
- **Range requires a fixed total** (decision): a range is meaningless without an agreed total, and the recommended count can change between process restarts, which would make processes' ranges overlap or leave gaps. Making it a `build()` error keeps the check before any network call.
- **Alignment** (decision): aligned ⇔ `mc == 1` or (`start % mc == 0` and (`end % mc == 0` or `end == total`)). The last range of an uneven total is not double-warned; the existing total warning covers it.
- **Paused clock vs two Apps** (decision): `App::run` requires a multi-thread runtime (AD-21) and tokio's paused clock only runs on `current_thread`, so a paused-clock test cannot run real Apps. The AC is met by two tests: the deterministic paused-clock test over several `PortQueue`s (each one process's twilight `Queue`) sharing one `Arc<MemoryIdentifyQueue>`, and a real-clock two-App test against the fakes with the 4.5 s tolerance already used by `sharding.rs`.
- Example (unit test shape for G6):

```rust
let port: Arc<dyn IdentifyQueue> = Arc::new(MemoryIdentifyQueue::new(nz(2)));
let (a, b) = (PortQueue::new(port.clone(), nz(2)), PortQueue::new(port, nz(2)));
let start = Instant::now();
let grants = join_all([a.enqueue(0), a.enqueue(1), b.enqueue(2), b.enqueue(3)]
    .map(|rx| async move { rx.await.unwrap(); start.elapsed() })).await;
// buckets 0 = {0,2}, 1 = {1,3}: sorted per bucket == [0 s, 5 s]
```

## Verification

Run from `/home/claude/lanes/c/noctis-rs`, release only:

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis-gateway` queue tests, `noctis-core` sharding/app tests, testkit `cluster`, `shard_range_warning`, `sharding`, `publish`
- `TRYBUILD=overwrite cargo test --release -p noctis --test ui` once after the Arc impl, then `cargo test --release -p noctis --test ui` -- expected: pass, `shared_identify_queue` still a compile error
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0
- `cargo run --release -p xtask -- deps-check` / `-- rules-catalog` / `-- api-snapshot --bless` then `-- api-snapshot` / `-- skill-check` -- expected: exit 0; SKILL.md `API hash` equals the printed hash
- `cargo deny check` -- expected: exit 0
- `cargo +1.91 check --release --workspace` -- expected: exit 0
