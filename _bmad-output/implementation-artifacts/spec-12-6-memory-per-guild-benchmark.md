---
title: 'Story 12.6: Memory-per-guild benchmark'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '94952ba0b09759a4e9b1688de93c69f90f7d09ff'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-12-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred:
  - summary: >-
      noctis-core's new versioned dev-dependency on the `noctis` facade forms a dev-dependency cycle that may block `cargo publish -p noctis-core` before `noctis` is on crates.io.
    evidence: |-
      Unverified (maybe-false). The workspace entry carries `version = "0.1.0"`, so the dev-dependency is kept in the published manifest; noctis-testkit already has the same cycle (`noctis.workspace = true` under dev-dependencies). Settle with `cargo publish --dry-run --workspace` (multi-package publish) or `cargo publish --dry-run -p noctis-core` in the Epic 14 publish story; if it fails, strip the version or publish with a staged order.
    location: >-
      crates/noctis-core/Cargo.toml:43
    severity: medium (unverified)
  - summary: >-
      The NFR-1 1 KiB framework-per-guild budget is checked by no executed gate until Story 14.3 turns `gate = true` on.
    evidence: |-
      `budgets.toml` has `gate = false` (report only, the intent and a recorded maintainer decision); the smoke test S4 only bounds per-guild growth at 16384 bytes and the bench runs only in the disabled CI workflow, so a 1-16 KiB per-guild regression passes every local gate. Story 14.3 owns gating; tightening S4 toward the budget is the cheap interim option if 200-guild noise allows.
    location: >-
      crates/noctis-core/benches/budgets.toml:7
    severity: medium
---

<intent-contract>

## Intent

**Problem:** NFR-1 promises a documented framework memory budget per idle guild (initial goal ≤ 1 KiB, Discord cache excluded) measured in CI, with Discord cache and Renderer cache memory reported separately, but nothing measures it: there is no `noctis-core/benches`, no `budgets.toml`, `main.yml`'s `benches` job is an echo stub, and nothing stops a future unbounded guild/user/channel/message/interaction-keyed framework map (AD-25). NFR-1, UX-DR8; AD-25, AD-28.

**Approach:** A `harness = false` bench `memory_per_guild` in `crates/noctis-core/benches/` runs one App against `FakeGateway` four times under the dhat counting allocator (warm-up, empty baseline, 10k idle guilds with `DiscordCacheConfig::none()`, 10k with `DiscordCacheConfig::default()`), derives framework bytes per guild, Discord cache bytes and Renderer cache bytes, and reports them against a new `crates/noctis-core/benches/budgets.toml` (report only; Story 14.3 gates). The per-guild framework state becomes an explicit fixed-size `GuildRecord`, and a new `deps-check` rule `keyed-map` rejects Discord-id-keyed framework maps unless a line marker states their bound. `main.yml` runs the bench on every push to `main`.

## Boundaries & Constraints

**Always:**
- **Budgets file** `crates/noctis-core/benches/budgets.toml` (shared by every Noctis bench; Stories 10.11 and 14.3 add their own top-level sections, so the bench reads only `[memory_per_guild]` and ignores other top-level tables). Exact content:
  ```toml
  # Noctis performance budgets (AD-25, AD-28). One top-level table per bench.
  # `gate = false` reports only; Story 14.3 confirms the numbers (PRD Q3) and turns gating on.

  [memory_per_guild]
  nfr = "NFR-1"
  guilds = 10000
  gate = false

  [memory_per_guild.framework_bytes_per_guild]
  max = 1024
  note = "NFR-1 initial goal: at most 1 KiB of framework memory per idle guild, excluding the Discord cache"

  [memory_per_guild.discord_cache_bytes_per_guild]
  note = "reported only: NFR-1 sets no Discord cache target; default configuration GUILD | CHANNEL | ROLE (no members, no messages)"

  [memory_per_guild.render_cache_bytes]
  max = 67108864
  note = "MemoryRenderCache::DEFAULT_LIMITS byte budget (64 MiB); idle guilds render nothing"
  ```
  Inside `[memory_per_guild]` and each metric table unknown keys are errors (`#[serde(deny_unknown_fields)]`); `nfr` (string), `guilds` (integer), `gate` (bool) and all three metric tables are required; `max` is an optional `u64`, `note` a required string.
- **Bench layout:** `crates/noctis-core/Cargo.toml` sets `autobenches = false` and `[[bench]] name = "memory_per_guild"`, `path = "benches/memory_per_guild/main.rs"`, `harness = false`; dev-dependencies add `noctis = { workspace = true, features = ["testkit"] }` (a dev edge to the facade is already allowed by deps-check, `xtask/src/deps_check.rs:767`; same precedent as `noctis-testkit`'s dev-dep) and `toml.workspace = true` (toml 1.1.8 already in `Cargo.lock`). dhat comes through `noctis::testkit::dhat` (dhat 0.3.3); no new external crate. Root `Cargo.toml` gets no profile (the `cfg` gate forbids it).
  - `benches/memory_per_guild/budget.rs` -- pure, no I/O except `load(path)`: `MemoryBudgets` (serde), `Measurement { guilds: u32, baseline: i64, none: i64, default: i64, render_cache_bytes: u64 }` (the three `i64` are live-heap deltas of the runs), `evaluate(&MemoryBudgets, &Measurement) -> Result<Report, BudgetError>`, `Report::markdown(&self) -> String`, `Report::failed(&self) -> bool` (= `gate && any metric Over`). Metrics: framework bytes per guild = `(none − baseline) / guilds` (f64); Discord cache bytes = `default − none` (i64, total) and per guild = that `/ guilds`; render cache bytes = `render_cache_bytes`. Status per metric: `Ok` (has `max`, value ≤ max), `Over` (value > max), `Reported` (no `max`). A negative value is shown as is (never clamped), compares as within budget, and the row's note gains `(below measurement noise)`. `BudgetError` (thiserror) variants: `Read { path, source }`, `Parse { path, source }` (toml error), `ZeroGuilds`, `TooManyGuilds { guilds }` (> 100 000).
  - `benches/memory_per_guild/measure.rs` -- the App runs (below); `pub(crate) async fn measure(guilds: u32) -> Result<Measurement, MeasureError>`.
  - `benches/memory_per_guild/main.rs` -- `#[global_allocator] static ALLOC: noctis::testkit::dhat::Alloc`; parses args (ignores cargo's `--bench`; `--guilds N` overrides the file's `guilds`; a non-integer N, 0 or more than 100 000 → exit 2 with a message); loads `concat!(env!("CARGO_MANIFEST_DIR"), "/benches/budgets.toml")`; builds a multi-thread tokio runtime (2 workers, `enable_all`) after starting the profiler; runs `measure`; prints `Report::markdown()` to stdout and appends it to the file named by `GITHUB_STEP_SUMMARY` when that variable is set (a write failure only prints a warning to stderr). Exit: 0 after a report unless `Report::failed()` (exit 1); 2 for a budget/argument/measurement error (message on stderr, nothing claimed).
- **Measurement** (`measure.rs`): `pub(crate) fn start_profiler() -> Result<dhat::Profiler, MeasureError>` starts `noctis::testkit::dhat::Profiler::builder().testing().build()` and returns `MeasureError::AllocatorMissing` when a `black_box`ed probe `Box` leaves `HeapStats::get().total_blocks == 0` (as `crates/noctis-testkit/src/memory.rs:100-105`); `main` calls it once before building the runtime and keeps the profiler alive until the report is printed (dhat allows one profiler per process). `MeasureError` (thiserror): `AllocatorMissing`, `Timeout { what: &'static str }`, `NoSession`, `Test(#[from] TestError)`. Live bytes = `HeapStats::get().curr_bytes` as `i64` (saturating). Run order: warm-up (100 guilds, default cache; discarded, initializes lazy globals), baseline (0 guilds, `none()`), none (N, `none()`), default (N, `default()`). One run:
  1. Create `let render = Arc::new(MemoryRenderCache::new())` and a counter `Arc<AtomicUsize>`, then settle and read `before` (settle = sleep 300 ms, then the median of 5 reads 50 ms apart).
  2. Build the App through `noctis::testkit::TestApp::build(&mut builder)` (it starts `FakeDiscordHttp` and `FakeGateway`, sets the test runtime, token, proxy, gateway URL and `publisher(false)`, runs the App and waits for the shard's session; `crates/noctis-testkit/src/app.rs:149-237`). `builder` = `App::builder()` with `.state(Arc::clone(&counter))`, `.render_cache(Arc::clone(&render))` (the `Arc<P>` impl at `crates/noctis-ports/src/store.rs:220`), `.discord_cache(<run config>)`, `.module(Module::new("bench").add::<CountGuilds>())`; `CountGuilds` is a `#[derive(EventHandler)]` unit struct whose `#[handler] async fn handle(self, ctx: EventCtx, _ev: GuildCreate) -> HandlerResult<()>` increments the counter (`ctx.state::<Arc<AtomicUsize>>()`). The TestApp's own allocations are identical in every run, so they cancel in the differences.
  3. Dispatch `GUILD_CREATE` with `test_app.gateway_fake().dispatch("GUILD_CREATE", json)` (public, `#[doc(hidden)]`; the type is not named, so no facade re-export is needed) for every guild in batches of 250, waiting after each batch until the counter reaches the batch end (30 s per batch, else `MeasureError::Timeout`; a `dispatch` returning ≠ 1 is `MeasureError::NoSession`). `TestError`s map to `MeasureError::Test(TestError)`.
  4. Settle, read `after`, read `render.usage().bytes`; delta = `after − before`.
  5. `test_app.stop().await`, drop everything, sleep 300 ms.
  Guild fixture (deterministic, i = 0..N): guild id `1_000_000 + i`, name `guild-{i}`, `preferred_locale` cycling `en-US`, `tr`, `de`, `ja`; roles: `@everyone` (id = guild id) and `member` (id `guild_id * 10 + 1`); channels: category `guild_id * 10 + 2` (type 4, "Text"), text `guild_id * 10 + 3` (type 0, "general"), voice `guild_id * 10 + 4` (type 2, "Voice"), each with `guild_id`; every other field as the testkit's `GUILD_CREATE` JSON (`crates/noctis-testkit/src/seed.rs:86-147`: `members`, `presences`, `voice_states`, `threads`, `emojis`, `stickers` empty). The JSON for a guild is built just before its dispatch, never stored.
- **Report** (`Report::markdown`), exact shape (numbers with one decimal for per-guild values, integers otherwise, no thousands separators):
  ```text
  ## Memory per idle guild (NFR-1)

  10000 idle guilds through FakeGateway; Discord cache: GUILD | CHANNEL | ROLE (no members, no messages); gate: off (reporting until Story 14.3)

  | Metric | Measured | Budget | Status |
  |---|---|---|---|
  | Framework bytes per guild | 87.3 | <= 1024 | ok |
  | Discord cache bytes per guild | 2311.6 | - | reported |
  | Discord cache bytes (total) | 23116000 | - | reported |
  | Renderer cache bytes | 0 | <= 67108864 | ok |

  Notes:
  - Framework bytes per guild: <framework note>
  - Discord cache bytes per guild: <discord note>
  - Renderer cache bytes: <render note>
  ```
  The numbers above illustrate the shape only. The `Discord cache bytes (total)` row has no budget and no note line. `gate: on (over-budget rows fail the run)` when `gate = true`; status `over` for `Over`; the first number is `Measurement.guilds`; the output ends with one newline. The cache line names `DiscordCacheConfig::default().resources()` through its `Display` (12.5), so it follows the real default.
- **Per-guild framework state** (AC 2): new crate-private `crates/noctis-core/src/guild_record.rs` (declared in `lib.rs`): `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub(crate) struct GuildRecord { pub(crate) locale: Locale }` documented as the one per-guild framework record (Story 13.3 adds the Module bitset as another `Copy` field); `pub(crate) const GUILD_RECORD_MAX_BYTES: usize = 64;` with `const _: () = assert!(size_of::<GuildRecord>() <= GUILD_RECORD_MAX_BYTES);`; `pub(crate) type GuildRecords = HashMap<Id<GuildMarker>, GuildRecord>; // keyed-map: bounded by the App's guild membership (GUILD_CREATE inserts, GUILD_DELETE removes, interactions never insert)`. `AppLocales.guilds` (`crates/noctis-core/src/locale.rs:206`) becomes `RwLock<GuildRecords>` and its helpers (`:282-333`) use the alias and `record.locale`; behavior is unchanged (unparseable `preferred_locale` still removes the record; `refresh` still never inserts).
- **deps-check rule `keyed-map`** (AD-25) in `xtask/src/deps_check.rs`: new `Rule::KeyedMap` (id `keyed-map`), a `keyed_map_sites` scan like `scan_session_sites` (`:693`) over `src/**/*.rs` of every internal crate except `noctis-testkit` (test infrastructure), files named `tests.rs` or under a `tests` directory exempt, and a pure predicate `keys_by_discord_id(line: &str) -> bool`: false when the trimmed line starts with `//`; false when the line contains `// keyed-map: ` followed by at least one non-space character; otherwise true when, after one of the openers `HashMap<`, `BTreeMap<`, `HashSet<`, `BTreeSet<`, `DashMap<`, `IndexMap<`, `IndexSet<`, `Cache<`, `LruCache<` and any spaces and `(` characters, the text starts with `Id<` and the text up to the first following `>` ends with `GuildMarker`, `UserMarker`, `ChannelMarker`, `MessageMarker` or `InteractionMarker`. Violation reason: ``{location} keys a framework map by a Discord id; bound it (AD-25) or mark the line `// keyed-map: <why it is bounded>` ``. Update the module docs with the rule.
- **Docs/records:** spine AD-25 (`ARCHITECTURE-SPINE.md:1217-1245`) gets an `Amended (2026-10-24, Story 12.6)` line: bench `memory_per_guild` (four runs, dhat deltas), `budgets.toml` schema (one table per bench, `gate`, metric `max`/`note`), `GuildRecord`, the `keyed-map` deps-check rule. The Usage Skill `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` §4 "Clusters" gains a short "Sizing memory" paragraph with the numbers the verification bench run printed (framework bytes per guild, Discord cache bytes per guild with the default configuration), rounded up to the next 10 bytes, how to reproduce (`cargo bench -p noctis-core --bench memory_per_guild`), and that `none()` drops the Discord cache share. Memlog `(decision)` lines exist (planner wrote them; add one for the measured numbers).
- **CI** (`.github/workflows-disabled/main.yml`): `on: push: branches: [main]` plus `workflow_dispatch`; job `benches` (name `Benches`): `actions/checkout@v7`, `rustup toolchain install`, `Swatinem/rust-cache@v2`, `cargo bench -p noctis-core --bench memory_per_guild` (keep a comment that Stories 10.11 and 14.3 add their benches; gating starts in 14.3). Leave `latest-takumi` as is.
- **README** (Epic 12 completes with this story; AGENTS "after every third finished epic"): Epic 12 row `✅ Done`, stories-done badge +1 (alt text and URL), the "Clusters and scale" bullet loses "(in progress)" and gains bullets for the configurable Discord cache with `ctx.cache()` and the memory-per-guild bench; check the status note and API-preview names still match.
- `rust-best-practices` applies (`references/testing.md`, `performance.md`, `error-handling.md`, `serde.md`).

**Never:** no public API change (no `xtask/public-api.txt` change, no Usage Skill API hash change); no criterion for this bench (it measures heap, not time); no gating (exit 1 only when a budget says `gate = true`); no network beyond the local fakes; no debug build (`cargo bench` uses the release-inheriting bench profile); no `dhat-heap.json` left in the repository; no clamping of negative deltas; no change to locale resolution behavior; no new framework state per guild beyond `GuildRecord`.

## I/O & Edge-Case Matrix

Budget (`crates/noctis-core/tests/memory_bench_budget.rs`, `#[path = "../benches/memory_per_guild/budget.rs"] mod budget;`):

| # | Input | Expected |
|---|---|---|
| U1 | the real `benches/budgets.toml` | parses; guilds 10000, gate false, framework max 1024, discord max none, render max 67108864 = `MemoryRenderCache::DEFAULT_LIMITS.max_bytes()` |
| U2 | file with extra top-level `[reference_card]` table | parses (ignored) |
| U3 | unknown key in `[memory_per_guild]` or a metric table (`maxx = 1`) | `BudgetError::Parse`, message names the key |
| U4 | missing `gate`, missing a metric table, `max = -1`, `max = 1.5`, `guilds = "x"` | `BudgetError::Parse` each |
| U5 | missing file path | `BudgetError::Read` naming the path |
| U6 | `guilds = 0` (file) or `evaluate` with `Measurement.guilds = 0` | `ZeroGuilds` |
| U7 | guilds 100 000 / 100 001 | accepted / `TooManyGuilds { guilds: 100001 }` |
| U8 | N 10000, baseline 1 000 000, none 1 500 000, default 24 616 000, render 0 | framework 50.0 ok; discord total 23116000, per guild 2311.6 reported; render 0 ok; `failed()` false |
| U9 | framework exactly 1024.0 (none − baseline = 10 240 000) | `ok` |
| U10 | framework 1024.1 with gate false / gate true | `over`, `failed()` false / true |
| U11 | none < baseline (negative framework) | value shown negative (e.g. `-3.2`), status ok, note has `(below measurement noise)` |
| U12 | default < none | discord values negative, `reported`, noise note |
| U13 | render 67108865 with gate true | `over`, `failed()` true |
| U14 | note with non-ASCII text (`"bütçe ≤ 1 KiB"`) | printed verbatim in Notes |
| U15 | `markdown()` for U8 with the real file's notes | golden string: the template above with 10000, 50.0, 2311.6, 23116000, 0 and the three notes |
| U16 | deltas at `i64::MAX` / `i64::MIN` | no overflow panic (compute in i128 or f64) |

Measurement smoke (`crates/noctis-core/tests/memory_bench_smoke.rs`: own binary, dhat global allocator, one `#[test]` building its own 2-worker runtime, `#[path = "../benches/memory_per_guild/measure.rs"] mod measure;`):

| # | Scenario | Expected |
|---|---|---|
| S1 | `measure(200)` | `Ok`; every run handled exactly its guilds (counter == N) |
| S2 | same | `default − none > 0` (the default cache holds 200 guilds' channels and roles) |
| S3 | same | `render_cache_bytes == 0` |
| S4 | same | `(none − baseline) / 200 < 16384` (gross-leak sanity only; the budget is checked by the bench) |

S0: the budget test binary (no dhat allocator) also includes `measure.rs` by path and asserts `measure::start_profiler()` is `Err(MeasureError::AllocatorMissing)`.

GuildRecord (`crates/noctis-core/src/guild_record.rs` `#[cfg(test)]`): G1 `GuildRecord: Copy` (static assert fn) and `size_of::<GuildRecord>() <= 64`; G2 existing `locale/tests.rs` cases (set, update, delete, unavailable, refresh never inserts, 10 000 records then 0) pass unchanged.

keyed-map (`xtask/src/deps_check.rs` tests, predicate and scan):

| # | Line | Hit |
|---|---|---|
| K1 | `guilds: RwLock<HashMap<Id<GuildMarker>, Locale>>,` | yes |
| K2 | `BTreeMap<Id<UserMarker>, X>`, `HashSet<Id<ChannelMarker>>`, `DashMap<Id<MessageMarker>, X>`, `moka::sync::Cache<Id<InteractionMarker>, X>` | yes each |
| K3 | `HashMap<(Id<GuildMarker>, Id<UserMarker>), X>`, `HashMap< Id<GuildMarker>, X>` | yes |
| K4 | `HashMap<Id<twilight_model::id::marker::GuildMarker>, X>` | yes |
| K5 | `HashMap<Id<RoleMarker>, X>`, `HashMap<&'static str, X>`, `HashMap<String, Id<GuildMarker>>`, `Vec<Id<GuildMarker>>`, `HashMap<u64, X>` | no |
| K6 | K1 + ` // keyed-map: bounded by guild membership` | no |
| K7 | K1 + ` // keyed-map:` or ` // keyed-map:   ` (no reason) | yes |
| K8 | `/// HashMap<Id<GuildMarker>, _>` and `// HashMap<Id<UserMarker>, _>` | no |
| K9 | scan: a hit in `src/foo/tests.rs`, `src/tests/x.rs`, or in `noctis-testkit` | not reported |
| K10 | scan: a hit in `noctis-core/src/x.rs` | one violation for `noctis-core` naming `crates/noctis-core/src/x.rs:<line>` (relative to the workspace root, as `session-store` prints) and `[keyed-map]` |
| K11 | real workspace after the change | `deps-check` passes (the only line with such a map is the marked `GuildRecords` alias) |

</intent-contract>

## Code Map

- `crates/noctis-core/Cargo.toml` -- add `autobenches = false`, `[[bench]]`, dev-deps `noctis` (features `testkit`) and `toml`.
- `crates/noctis-core/src/locale.rs:196-345` -- `AppLocales.guilds` map and helpers (`guilds_read/_write`, `guild`, `recorded`, `set_guild`, `observe`, `refresh`) to switch to `GuildRecords`; `locale/tests.rs:180-230` existing cases must stay green.
- `crates/noctis-core/src/lib.rs` -- declare `mod guild_record;`.
- `crates/noctis-core/src/worker.rs:286` -- drain calls `locales.observe` then the cache feed (12.5), then dispatch: why counting in a `GuildCreate` handler means locale record and cache are already updated.
- `crates/noctis-core/src/app.rs` -- `AppBuilder::discord_cache` (12.5), `render_cache` port setter (`:2665`), `run_until`; `Runtime::for_test` (`runtime.rs:142`).
- `crates/noctis-core/src/discord_cache.rs` -- `DiscordCacheConfig::{default, none, resources}`; `DiscordCacheResources` `Display` in `crates/noctis-ports/src/memory/discord.rs`.
- `crates/noctis-ports/src/memory/stores.rs:167-168` -- `MemoryRenderCache` 64 MiB default, `usage().bytes` (weigher bytes); `store.rs:220` `Arc<P>` impls.
- `crates/noctis-testkit/src/app.rs:149-237,314-350` -- `TestApp::build`, `stop`, `gateway_fake()`; facade re-exports at `crates/noctis/src/lib.rs:3324-3410` (`noctis::testkit::{TestApp, TestError, dhat}`; the facade does not re-export `FakeGateway`, which is why the bench goes through `TestApp`).
- `crates/noctis-testkit/src/fake/gateway.rs:567-600` -- `dispatch` returns the sessions reached; dispatched frames are not retained (only frames received from the client are logged), so the fake adds no per-guild bytes.
- `crates/noctis-testkit/tests/testapp_memory.rs` and `src/memory.rs:48-130` -- dhat binary pattern and allocator probe.
- `crates/noctis-testkit/src/seed.rs:86-147` -- `GUILD_CREATE` JSON that twilight 0.17 `Guild` accepts (role `colors` object is required).
- `xtask/src/deps_check.rs` -- module docs `:1-40`, `Rule` `:425-455`, `check` `:496-520`, `scan_session_sites`/`scan_lines`/`collect_rs_files` `:693-750` to reuse, tests from `:1000`.
- `.github/workflows-disabled/main.yml` -- stub to fill; `ci.yml` step style.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md:1217-1245` -- AD-25.
- `README.md:18,250,334-338` -- badge, Epic 12 row, Clusters bullet.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/guild_record.rs`, `lib.rs`, `locale.rs` -- `GuildRecord`, size assert, `GuildRecords` alias with the `keyed-map` marker; switch `AppLocales`; G1-G2.
- `xtask/src/deps_check.rs` -- `keyed-map` rule, predicate, scan, docs; K1-K11.
- `crates/noctis-core/benches/budgets.toml`, `benches/memory_per_guild/{main.rs,budget.rs,measure.rs}`, `Cargo.toml` -- the bench.
- `crates/noctis-core/tests/memory_bench_budget.rs`, `tests/memory_bench_smoke.rs` -- U1-U16, S1-S4, allocator-missing check. Unused items of a `#[path]` module get `#[expect(dead_code, reason = "...")]` only where the compiler flags them.
- Run `cargo bench -p noctis-core --bench memory_per_guild`; record its report in the Auto Run Result and a memlog line.
- `.github/workflows-disabled/main.yml` -- push-to-main trigger and the bench job.
- Spine AD-25 amendment; `app-and-sharding.md` sizing paragraph; README refresh.

**Acceptance Criteria:**
- Given `cargo bench -p noctis-core --bench memory_per_guild`, when it runs with the stated default cache configuration, then it loads 10 000 idle guilds through `FakeGateway` and prints framework bytes per guild, Discord cache bytes (total and per guild) and Renderer cache bytes, each against its `budgets.toml` entry, and exits 0 while `gate = false`.
- Given the four runs, when the report is computed, then Discord cache bytes = default-run delta − `none()`-run delta, framework bytes = (`none()`-run delta − empty-baseline delta) / guilds, and Renderer cache bytes = the App's `MemoryRenderCache` weigher bytes.
- Given the framework sources, when `cargo run --release -p xtask -- deps-check` runs, then no framework map is keyed by a guild, user, channel, message or interaction id without a stated bound, and the only per-guild record is the `Copy`, ≤ 64-byte `GuildRecord`.
- Given `.github/workflows-disabled/main.yml`, when it is re-enabled, then every push to `main` runs the bench and reports without gating.
- Given the story is done, when every local gate runs, then all pass and `xtask/public-api.txt` is unchanged.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 25 findings — high 0, medium 3, low 10, false 11, maybe-false 1
- findings:
  - `[maybe-false]` `[defer]` (blind) dev-dependency `noctis-core -> noctis` creates a publish cycle — the spec chose this edge citing the noctis-testkit precedent, which has the same cycle; settle with `cargo publish --dry-run --workspace` in Epic 14 (deferred, medium unverified).
  - `[false]` `[reject]` (blind) `keyed-map` omits `RoleMarker`, `EmojiMarker` and other per-guild markers — the intent/AC name exactly guild, user, channel, message and interaction ids and K5 specifies `Id<RoleMarker>` as no hit; no such map exists.
  - `[low]` `[patch]` (blind) `keys_by_discord_id` misses turbofish, aliased/inferred keys and unlisted map types, while only raw `u64` and multi-line keys are documented — grouped with the undocumented-blind-spot entry; patch: list these limits in the `deps_check.rs` module docs and the spine AD-25 amendment.
  - `[false]` `[reject]` (blind) inline `#[cfg(test)]` modules are scanned although "test files" are exempt — the module docs state precisely "files named `tests.rs` and files under a `tests` directory", which is the implemented behavior; an inline fixture fails loudly and takes a marker.
  - `[medium]` `[patch]` (blind) `parse_args` in the bench's `main.rs` has no tests — same root as the verification-gap entry; patch: move `parse_args` into `budget.rs` and add argument cases to `memory_bench_budget.rs`.
  - `[false]` `[reject]` (blind) the Renderer cache metric is always 0 — idle guilds render nothing, which the spec settles (S3, the row's note "idle guilds render nothing"); the intent asks for the weigher bytes of idle guilds, and 0 is the correct value.
  - `[false]` `[reject]` (blind) framework bytes include testkit per-guild retention and miss real-shard state — `FakeGateway::dispatch` (`crates/noctis-testkit/src/fake/gateway.rs:567-590`) sends each frame and keeps nothing; the twilight shard keeps no per-guild set outside the Discord cache, which is measured separately.
  - `[low]` `[reject]` (blind) per-guild framework bytes step with `HashMap` capacity doubling — real but the swing (about 30 B per guild at most) is under 1% of the per-guild total the Usage Skill reports; not worth extra report or Skill text.
  - `[false]` `[reject]` (blind) README marks Epic 12 done while the spec is `in-progress` and sprint status is not updated — the story loop's finalize sets `status: done` and `tools/story-loop/finish.sh` updates `sprint-status.yaml` in the same finishing commit as the README refresh.
  - `[false]` `[reject]` (blind) README API-preview names were not checked — spot check: `AppBuilder::ref_store`, `App::from_env`, `ActionRow::buttons`, `HandlerError`, `Responded`, `CommandCtx`, `ComponentCtx`, `PayloadEnum` all exist in `xtask/public-api.txt` or the sources.
  - `[low]` `[patch]` (blind) `GUILD_RECORD_MAX_BYTES`'s doc claims "map overhead included" though the const assert bounds only `size_of::<GuildRecord>()` — patch: reword the doc to say it bounds the record size only (map overhead is measured by the bench).
  - `[false]` `[reject]` (blind) the CI bench job cannot fail and keeps no artifact — report-only until Story 14.3 is the intent and a maintainer decision; the step summary is the specified report sink and `budgets.toml`'s content is spec-mandated.
  - `[low]` `[patch]` (edge) turbofish `HashMap::<Id<GuildMarker>, _>::new()` bypasses `keyed-map` — grouped with the undocumented-blind-spot entry; documented as a known limit (the predicate is spec-exact).
  - `[low]` `[patch]` (edge) a path-qualified key `HashMap<twilight_model::id::Id<GuildMarker>, X>` bypasses `keyed-map` (`strip_prefix("Id<")` fails) — grouped with the undocumented-blind-spot entry; documented as a known limit.
  - `[low]` `[reject]` (edge) a per-guild value in (max, max + 0.05) prints `1024.0 | <= 1024 | over` — real but a 0.05-byte window on a value measured near 30 B; the spec defines `Over` as value > max, so changing the comparison or format is not a direct correction.
  - `[low]` `[reject]` (edge) the `budgets.toml` Discord-cache note hardcodes `GUILD | CHANNEL | ROLE` and can drift from the real default — only on a future default change, the report header follows the real default, and the file content is spec-exact.
  - `[low]` `[reject]` (edge) nothing checks the counter equals N after the last batch (S1 "exactly") — each dispatch is checked to reach exactly one session (`!= 1` is `NoSession`), so extra handling needs a fake-gateway bug; a post-check adds a branch and error path for an unlikely case.
  - `[medium]` `[patch]` (vgap) `parse_args` (`--guilds N`/`--guilds=N`, ignoring cargo's `--bench`) is compiled but never executed by any gate — pre-verified; patch: move it into `budget.rs` and add a test case covering `--bench`, both override forms, 0, 100001, a non-integer and a trailing `--guilds`.
  - `[medium]` `[defer]` (vgap) the 1 KiB budget is enforced by no executed gate — report-only until Story 14.3 is in the intent; deferred to 14.3 with the S4-tightening option.
  - `[false]` `[reject]` (intent) `GuildRecord` has no Module bitset — the spec deliberately settles that Story 13.3 (per-guild Module enablement, not yet implemented) adds the field; there is no bitset to store today.
  - `[low]` `[patch]` (intent) `keyed-map` is a line heuristic with blind spots and the marker is unverified — the blind-spot part groups with the documentation patch; the marked `GuildRecords` bound is covered by existing `locale/tests.rs` cases (delete removes, refresh never inserts, 10 000 records then 0).
  - `[false]` `[reject]` (intent) the tests never run the 10k bench against the budget — the spec settles that pure logic and a 200-guild smoke run are tested through `#[path]` and the bench reports (gating in 14.3); the budget-gap part is the deferred vgap entry.
  - `[false]` `[reject]` (intent) "reports on every push" lives in a disabled workflow — the maintainer's CI pause (AGENTS.md) puts every workflow in `.github/workflows-disabled/`; `actions/checkout@v7` matches `ci.yml`.
  - `[false]` `[reject]` (intent) the Renderer cache measurement is trivially zero — same refutation as the blind-layer entry: 0 is the correct value for idle guilds.
  - `[low]` `[reject]` (intent) the default cache configuration is stated in the header, the `budgets.toml` note and the Usage Skill, which can drift — same as the edge entry; the Skill numbers are measured values that any later re-measure updates.

## Design Notes

- **Four runs in one process** (decision): dhat allows one profiler per process, so the runs are sequential and each measures its own `after − before`; a warm-up run first absorbs lazy globals (rustls provider, tracing/metrics statics, OnceLocks) that would otherwise inflate the baseline. Batches of 250 with a wait keep lane queues small, so retained `VecDeque` capacity from a burst does not pose as per-guild memory.
- **Report, not gate** (decision): `gate = false` until Story 14.3 records the first numbers (PRD Q3); exit 1 exists only for `gate = true` so 14.3 flips one value.
- **`GuildRecord` now, bitset later** (decision): today the only per-guild framework state is the guild locale; naming the record and bounding its size gives Story 13.3 a home for the Module bitset without a second guild-keyed map.
- **`keyed-map` in deps-check** (decision): the tool already hosts source rules (`session-store`, `http-client`); a line marker keeps bounded maps explicit. Known limit: maps keyed by raw `u64` ids are not detected (documented in the rule docs).
- **Bench-local code tested through `#[path]`**: a `harness = false` bench runs no `#[test]`s, so the pure budget logic and a 200-guild smoke run live in `tests/` and include the bench modules by path; the workspace test gate covers them.

## Verification

**Commands:**
- `cargo fmt --all` -- no diff.
- `cargo test --release -p noctis-core --test memory_bench_budget --test memory_bench_smoke`; `cargo test --release -p noctis-core --lib locale guild_record`; `cargo test --release -p xtask deps_check` -- pass.
- `cargo bench -p noctis-core --bench memory_per_guild` -- exit 0, report printed with all four rows; `git status` shows no `dhat-heap.json`.
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean.
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass.
- `cargo run --release -p xtask -- deps-check`, `-- tw-grammar --check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- pass.
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace`; `bash .github/scripts/check-build-config.sh` -- pass.

## Auto Run Result

`cargo bench -p noctis-core --bench memory_per_guild` (2026-10-24, exit 0, about 27 s, no `dhat-heap.json` written; a rerun printed 29.0 and 9731.4 for the per-guild rows):

```text
## Memory per idle guild (NFR-1)

10000 idle guilds through FakeGateway; Discord cache: GUILD | CHANNEL | ROLE (no members, no messages); gate: off (reporting until Story 14.3)

| Metric | Measured | Budget | Status |
|---|---|---|---|
| Framework bytes per guild | 28.5 | <= 1024 | ok |
| Discord cache bytes per guild | 9731.8 | - | reported |
| Discord cache bytes (total) | 97318118 | - | reported |
| Renderer cache bytes | 0 | <= 67108864 | ok |

Notes:
- Framework bytes per guild: NFR-1 initial goal: at most 1 KiB of framework memory per idle guild, excluding the Discord cache
- Discord cache bytes per guild: reported only: NFR-1 sets no Discord cache target; default configuration GUILD | CHANNEL | ROLE (no members, no messages)
- Renderer cache bytes: MemoryRenderCache::DEFAULT_LIMITS byte budget (64 MiB); idle guilds render nothing
```

## Auto Run Result

- Summary: added the `memory_per_guild` dhat bench in noctis-core (10 000 idle guilds through FakeGateway, four sequential runs after a warm-up) reporting framework bytes per guild (28.5 B vs 1 KiB NFR-1 goal), Discord cache bytes (reported only, ~9.7 KB/guild with GUILD|CHANNEL|ROLE) and Renderer cache bytes (0 for idle guilds); `benches/budgets.toml` with `gate = false` until Story 14.3; `GuildRecord` with a const-asserted size bound as the home for Story 13.3's Module bitset; new `keyed-map` deps-check rule flagging maps keyed by Discord ids without a bound marker; report-only CI bench job in `.github/workflows-disabled/main.yml`; spine AD-25 amendment; Usage Skill and README (Epic 12) updates.
- Files changed: `crates/noctis-core/benches/memory_per_guild/{main,budget,measure}.rs` (bench, budget logic, measurement); `crates/noctis-core/benches/budgets.toml` (budgets); `crates/noctis-core/src/guild_record.rs`, `locale.rs`, `lib.rs` (GuildRecord, bounded guild locale map); `crates/noctis-core/tests/memory_bench_budget.rs`, `memory_bench_smoke.rs` (budget/arg tests, 200-guild smoke); `crates/noctis-core/Cargo.toml`, `Cargo.lock` (dhat, bench target); `xtask/src/deps_check.rs` (keyed-map rule); `ARCHITECTURE-SPINE.md`, `.memlog.md` (AD-25 amendment); Usage Skill `app-and-sharding.md`; `README.md`; workflow file.
- Review findings: 25 total (high 0, medium 3, low 10, false 11, maybe-false 1). Patched: 2 medium (`parse_args` untested -> moved into `budget.rs` with argument tests), 4 low grouped into keyed-map blind-spot documentation (turbofish, path-qualified, aliased keys, raw u64) plus the `GUILD_RECORD_MAX_BYTES` doc rewording. Deferred: publish cycle from the `noctis-core -> noctis` dev-dependency (verify with `cargo publish --dry-run --workspace` in Epic 14); 1 KiB budget not gated (Story 14.3). Rejected: missing per-guild markers, inline test modules, Renderer metric 0, testkit retention, HashMap capacity steps, README/sprint status timing, API-preview names, CI job non-failing, 0.05-byte Over window, hardcoded cache note drift, post-batch counter check, missing Module bitset (13.3), 10k bench untested, disabled workflow, default-config drift — reasons in the Review Triage Log.
- Follow-up review recommended: false (patched by verdict: high 0, medium 2, low 5; first pass, no high patched; the two medium patches share one root and are covered by new tests).
- Verification: local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, deps-check, tw-grammar, tw_crosscheck, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91); bench run exit 0 with no `dhat-heap.json` left; CI paused by maintainer.
- Residual risks: possible crates.io publish cycle via the dev-dependency (Epic 14); NFR-1 budget is report-only until 14.3; `keyed-map` is a line heuristic with documented blind spots.
