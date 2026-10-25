---
title: 'Story 14.1: Adapter conformance suites'
type: 'feature'
created: '2026-10-25'
status: 'ready-for-dev'
baseline_revision: 'dc12a2804bb83f1b7c95f7638f5edfe4fde0c51e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-14-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-39/AD-14 promise a reusable conformance suite per port so Redis/SQL adapter authors can prove the semantics core relies on (TTL, overwrite, delete, missing key, atomicity, `version()` monotonicity, App namespacing). Nothing ships today: the template's `tests/adapters.rs` and the Usage Skill reference a planned `noctis::testkit::conformance` that does not exist.

**Approach:** Add `noctis_testkit::conformance` (re-exported as `noctis::testkit::conformance`) with one async, panic-on-failure suite function per byte-store port, `IdentifyQueue` and `DiscordCache`. Each takes a factory, runs a fixed ordered list of named checks against fresh instances, and panics with `conformance::<suite>: <check>: <detail>`. Every in-memory default passes; seven deliberately broken adapters fail at the expected check (meta-tests). The template test, the Usage Skill and the API snapshot are reconciled.

## Boundaries & Constraints

**Always:**
- Public API (exact signatures, all in `noctis_testkit::conformance`, re-exported by the facade inside `pub mod testkit` as `pub use noctis_testkit::conformance;`):
  `pub async fn ref_store<P: RefStore>(make: impl Fn() -> P)`, and likewise `session_store` (`SessionStore`), `module_state_store` (`ModuleStateStore`), `rate_limit_store` (`RateLimitStore`), `cache_store` (`CacheStore`), `render_cache` (`RenderCache`), `asset_store` (`AssetStore`); `pub async fn identify_queue<Q: IdentifyQueue>(make: impl Fn(NonZeroU16) -> Q)`; `pub async fn discord_cache<C: DiscordCache>(make: impl Fn(DiscordCacheResources) -> C)`. Nothing else becomes public.
- The factory is called once per check (fresh instance per check) and must return an empty store; the suite never assumes two factory results share state.
- Failure = `panic!` whose message starts `conformance::<suite>: <check>: ` (suite = function name, check = id from the tables below), followed by what was expected and observed. An unexpected `Err(PortError)` from any op panics under the current check with the error's `Display`. A panic or failure inside a spawned caller task is re-raised under the check that spawned it.
- Runtime guard (check id `runtime`, evaluated first by every suite): if `tokio::runtime::Handle::try_current()` fails or its `runtime_flavor()` is `CurrentThread`, panic `conformance::<suite>: runtime: needs a multi-thread tokio runtime (#[tokio::test(flavor = "multi_thread")])` (same precedent as `TestApp`, `crates/noctis-testkit/src/app.rs:151`).
- Concurrency uses `tokio::spawn` tasks released together by a `tokio::sync::Barrier` (real multi-thread parallelism); constants: `CALLERS = 64` contenders, `INCR_TASKS = 32` × `INCR_PER_TASK = 32`.
- Time: all suite waits and measurements use `tokio::time` (`sleep`, `Instant`). Byte-store TTL constants: `SHORT_TTL = 500 ms`, `LONG_TTL = 60 s`; all short-TTL setup of one `ttl` check happens first, then ONE `sleep(SHORT_TTL + 1 s)`, then all verifications (bounded wall time; no assertion depends on finishing within `SHORT_TTL` except reads issued immediately after setup).
- Byte-store check order (a suite runs only the checks whose ops its port has): `missing`, `round_trip`, `overwrite`, `delete`, `take`, `incr`, `version`, `concurrent_put`, `atomic_put_if_absent`, `atomic_take`, `atomic_incr`, `concurrent_version`, `namespacing`, `ttl`. Coverage: ref = missing, round_trip, overwrite, delete, take, atomic_put_if_absent, atomic_take, namespacing, ttl; session = missing, round_trip, overwrite, delete, concurrent_put, namespacing, ttl; module_state = missing, round_trip, overwrite, version, concurrent_put, concurrent_version, namespacing, ttl; rate_limit = missing, round_trip, overwrite, incr, concurrent_put, atomic_incr, namespacing, ttl; cache = missing, round_trip, overwrite, delete, incr, concurrent_put, atomic_put_if_absent, atomic_incr, namespacing, ttl; render_cache and asset_store = missing, round_trip, overwrite, concurrent_put, namespacing, ttl. Ports without `put` write through `put_if_absent`.
- Identify check order: `first_lease`, `parallel_buckets`, `interval`. DiscordCache order: `empty`, `owned_reads`, `event_updates`, `disabled_resources`, `unknown_targets`.
- One shared internal byte-store checker drives all seven ports (e.g. a private op-table trait implemented per port with optional ops); no seven copy-pasted check bodies.
- Namespacing goes through core's real wrapper: `noctis_core::Namespaced::new(&AppId::new("conformance-a"), inner)`, `Namespaced::new(&AppId::new("conformance-b"), inner)` and `Namespaced::shared(inner)` over ONE `Arc<dyn Port>` built from one factory call.
- DiscordCache fixtures are the suite's own copies under `crates/noctis-testkit/src/conformance/fixtures/` (copied from `crates/noctis-ports/tests/fixtures/gateway/`, the `channel_update` name changed to `yeniden-adlandırıldı 😀`), embedded with `include_str!` and parsed with `GatewayEventDeserializer` + `serde_json` exactly like `crates/noctis-ports/tests/discord_cache.rs:39-48`.
- Rustdoc on every public fn: what it checks (check ids), the multi-thread requirement, "factory returns a fresh empty instance", "never run under a paused tokio clock (`start_paused`) for byte stores", and how to do async setup in a sync factory (`tokio::task::block_in_place(|| Handle::current().block_on(connect()))`). A `no_run` example per module doc.
- Library code in the suite has no `unwrap`/`expect` on adapter output: failures go through the suite's panic helper with the check id.

**Never:**
- No change to port traits, `PortError`, memory defaults or `Namespaced` semantics; no new external crate (testkit gains only workspace deps `noctis-ports` and `serde`).
- No `Result`-returning suite API, no config struct, no async factory, no FIFO/cancel-safety/out-of-range-bucket checks for `IdentifyQueue`, no message-cap or eviction/capacity checks, no checks on order of id lists (compare sorted).
- No paused-clock tricks, no `std::thread::sleep` inside async suites, no `cargo hack` run, no debug builds.
- Do not move the template `adapters` test into a gate (Story 14.2 does that) and do not touch `.github/workflows-disabled/`.

## I/O & Edge-Case Matrix

Keys used by `round_trip`: `"plain"`, `"with:colon"`, `"100%"`, `"ğüş😀"`, a 200-byte ASCII key (`"k".repeat(200)`); no two share their last `:` segment. Values: empty, all 256 byte values `0..=255` (invalid UTF-8 included), 64 KiB of `0xAB`.

| Scenario (suite: check) | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| any: runtime | current-thread runtime or no runtime | nothing else runs | panic `...: runtime: needs a multi-thread tokio runtime ...` |
| byte: missing | fresh store, key never written | `get`→`None`; `take`→`None`; `delete`→`Ok`; `touch(LONG_TTL)`→`false` and a following `get` is `None` (not created) | any `Some`/`true`/`Err` → panic `missing` |
| byte: round_trip | each key × each value written (`put`, or `put_if_absent`→`true` on ref) | `get` returns the identical bytes for every pair; keys independent | mismatch → panic naming key/value len |
| byte: overwrite | write `a` then `put` `b` (ports with `put`); `put_if_absent(c)` on live key | `get`=`b`; `put_if_absent`→`false`, value still `b` (ref: still `a`) | panic `overwrite` |
| byte: delete | write, `delete`, `delete` again | `get`→`None`; second delete `Ok`; then `put_if_absent`→`true` (ports that have it) | panic `delete` |
| ref: take | write `v`, `take`, `take` | first `Some(v)`, second `None`, `get`→`None` | panic `take` |
| byte: incr | fresh key `incr(5)`; `incr(5)`; fresh `incr(0)`; fresh `incr(u64::MAX)`; `put` BE(`u64::MAX-1`) then `incr(5)` then `incr(1)`; `put` BE(41) then `incr(1)` | `5`,`10`; `0` and `get`=`[0;8]`; `u64::MAX`; `u64::MAX`,`u64::MAX`; `42`; after `incr(5)`/`incr(5)` `get` = `10u64.to_be_bytes()` | panic `incr` |
| byte: incr non-counter | `put` values of len 0, 7, 9 (`b"abcdefg"`, `b"abcdefghi"`) then `incr(1)` | `Err(PortError::InvalidValue{..})` for each, stored value unchanged | `Ok` or other variant → panic `incr` |
| module_state: version | fresh; `get`; `put` k1; `put` k1 same value; `put` k2 | `0`; still `0` after `get`; strictly increasing after each `put` (`v1>0`, `v2>v1`, `v3>v2`) | panic `version` |
| byte: concurrent_put | `CALLERS` tasks `put` distinct 8-byte values to one key at once | final `get` is exactly one of the written values | torn/other/`None` → panic `concurrent_put` |
| byte: atomic_put_if_absent | `CALLERS` tasks `put_if_absent` distinct values to one fresh key | exactly one `true`; `get` = that winner's value | 0 or ≥2 winners → panic with count |
| ref: atomic_take | key holds `v`; `CALLERS` tasks `take` | exactly one `Some(v)`, rest `None`; `get`→`None` | 0 or ≥2 → panic `atomic_take` with count |
| byte: atomic_incr | `INCR_TASKS`×`INCR_PER_TASK` `incr(1)` on one fresh key | final `get` = BE(1024); the 1024 returned values are exactly `1..=1024` (no duplicates, none lost) | panic `atomic_incr` with lost/duplicate count |
| module_state: concurrent_version | `v0=version()`; `CALLERS` tasks each `put` own key then read `version()`; one poller reads `version()` in a loop until all done | every task's read `> v0`; final `> v0`; poller's sequence non-decreasing | panic `concurrent_version` |
| byte: namespacing | A=`Namespaced::new(conformance-a)`, B=`conformance-b`, S1,S2=`Namespaced::shared` over one inner; logical keys `"k"`, `""`, `"ğ:ü"` | A write invisible via B and S1 (`None`); inner `get("conformance-a:<seg>:k")` = value (verbatim, `<seg>` = `NamespacedPort::SEGMENT`); inner write of `"conformance-b:<seg>:k"` visible via B `"k"`; S1 write visible via S2, invisible via A; A `delete` (when present) leaves B's same logical key intact | panic `namespacing` naming the leaking pair |
| byte: ttl | short-TTL write; `None`-TTL write; LONG_TTL write; short write then `put` without TTL (overwrite resets); ref: short `put_if_absent` then `touch(LONG_TTL)`→`true`; ref: short entry, after sleep `touch`→`false`; incr: `incr(1, SHORT)` then `incr(1, LONG)` | immediately: short entry readable. After one sleep: short entry `None` (and `take`→`None` on ref); `None`/LONG entries live; overwritten entry live; touched entry live; expired-then-touched stays `None` (not resurrected); `put_if_absent` on expired key → `true` (ports with `put_if_absent`); incr counter expired (TTL from create kept) and a new `incr(1)` returns `1` | panic `ttl` naming the sub-case |
| identify: first_lease | `make(2)`; `acquire(0)` | granted within 1 s; `lease.bucket()==0` | late/wrong bucket → panic `first_lease` |
| identify: parallel_buckets | same fresh queue: `acquire(1)` concurrently with a second `acquire(0)` | bucket 1 granted within 1 s, `bucket()==1` | panic `parallel_buckets` |
| identify: interval | the second `acquire(0)` from above | not granted before 4.9 s after the first bucket-0 grant; granted within 10 s | early → panic `interval` (elapsed shown); timeout → panic `interval` |
| discord: empty | `make(all())`, no update | all six getters and three id lists `Ok(None)` | panic `empty` |
| discord: owned_reads | `make(all())`, `GUILD_CREATE` | guild id 1 name `Noctis Test Guild`; channel 10 `general`; role 20 `Moderator`; member (1,30) user 30 roles `[20]`; user 30 `ayla`; sorted `channel_ids`=[10,11], `role_ids`=[1,20], `member_ids`=[30]; mutating a returned `Guild`'s name does not change the next read | panic `owned_reads` |
| discord: event_updates | then `MESSAGE_CREATE`, `CHANNEL_UPDATE`, `GUILD_ROLE_DELETE`, `GUILD_MEMBER_REMOVE`, `CHANNEL_DELETE`, `MESSAGE_DELETE`, `GUILD_DELETE` in order | message 40 (id, channel 10, author 30, content `hello noctis`); channel 10 renamed `yeniden-adlandırıldı 😀`; role 20 `None`; member (1,30) `None`; channel 11 `None` and sorted `channel_ids`=[10]; message 40 `None`; guild 1 `None` | panic `event_updates` naming the event |
| discord: disabled_resources | `make(CHANNEL)` and `make(empty())`, apply `GUILD_CREATE` + `MESSAGE_CREATE` | every `update` `Ok`; CHANNEL set: channel 10 `Some`, guild/role/member/user/message `None`, `role_ids`/`member_ids` `None` or `Some(empty)`; empty set: every getter `None`, every id list `None` or `Some(empty)` | panic `disabled_resources` |
| discord: unknown_targets | `make(all())`, no `GUILD_CREATE`; apply `MESSAGE_DELETE`, `GUILD_ROLE_DELETE`, `GUILD_MEMBER_REMOVE`, `CHANNEL_DELETE`, `GUILD_DELETE` | every `update` `Ok`; guild 1 `None` | `Err` → panic `unknown_targets` |
| meta: broken adapters | see task list | each suite panics; message starts with the expected `conformance::<suite>: <check>:` | wrong check or no panic → test fails |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/store.rs` -- the seven byte-store traits and the op contracts the checks encode (read-only).
- `crates/noctis-ports/src/lib.rs:28-41` -- op vocabulary table (incr: BE `u64`, saturating, TTL on create, `InvalidValue`) (read-only).
- `crates/noctis-ports/src/identify.rs`, `crates/noctis-ports/src/discord_cache.rs` -- `IdentifyQueue`/`Lease`, `DiscordCache` (read-only).
- `crates/noctis-ports/src/memory/{stores,identify,discord}.rs` -- defaults under test: `Memory*Store::new()`, `MemoryIdentifyQueue::new(NonZeroU16)`, `MemoryDiscordCache::new(DiscordCacheResources)`; byte stores use `std::time::Instant` (real clock), identify uses `tokio::time`.
- `crates/noctis-ports/tests/discord_cache.rs` + `tests/fixtures/gateway/*.json` -- event parsing helper and fixtures to copy; `restricted_to_channels` shows memory's disabled-resource behavior.
- `crates/noctis-ports/tests/memory_stores.rs` -- existing per-store contract tests (style reference; keep).
- `crates/noctis-core/src/namespace.rs` -- `Namespaced::new/shared`, `NamespacedPort::SEGMENT`, `AppId::new`; `Namespaced<dyn Port>` implements each port (needs `Arc<dyn Port>`).
- `crates/noctis-testkit/src/lib.rs`, `Cargo.toml` -- add `pub mod conformance;`, doc bullet, deps `noctis-ports.workspace = true`, `serde.workspace = true` (edge `noctis-testkit -> noctis-ports` already allowed in `xtask/src/deps_check.rs:216`).
- `crates/noctis-testkit/src/app.rs:19,151` -- runtime-flavor guard precedent.
- `crates/noctis/src/lib.rs:3410-3495` -- facade `pub mod testkit` (doc bullets + re-exports).
- `crates/noctis/tests/template_adapter.rs` -- includes the template's `bounded_cache.rs` by `#[path]`; the in-workspace proof that `BoundedCacheStore` conforms.
- `examples/project-template/tests/adapters.rs`, `src/adapters/bounded_cache.rs`, `AGENTS.md:26` -- template side (template is its own workspace; not built by gates until 14.2).
- `examples/project-template/.claude/skills/noctis/references/adapters.md` §5 (line ~565), `references/testing.md` line 6 and §7 (line ~263), `SKILL.md:23` (API hash) -- Usage Skill.
- `xtask/public-api.txt` -- snapshot to re-bless; `xtask/src/skill_check/` requires every new public item mentioned in the skill.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-testkit/Cargo.toml` -- add `noctis-ports.workspace = true` and `serde.workspace = true` to `[dependencies]` -- the suite names port types and uses `DeserializeSeed`.
- `crates/noctis-testkit/src/conformance.rs` (+ `conformance/bytes.rs`, `conformance/identify.rs`, `conformance/discord.rs`, `conformance/fixtures/*.json`) -- implement the nine suites, the runtime guard, the panic helper and the shared byte-store checker per Boundaries and Matrix -- FR-39, AD-14.
- `crates/noctis-testkit/src/lib.rs` -- `pub mod conformance;` and a crate-doc bullet -- exposure.
- `crates/noctis-testkit/tests/conformance.rs` -- (a) one `#[tokio::test(flavor = "multi_thread")]` per suite running it on its memory default (`MemoryRefStore::new`, …, `MemoryIdentifyQueue::new`, `MemoryDiscordCache::new`) plus `cache_store(|| Arc::new(MemoryCacheStore::new()))`; (b) a `#[test]` asserting the `runtime` panic on a current-thread runtime; (c) meta-tests, each building its own multi-thread runtime and `catch_unwind(AssertUnwindSafe(|| rt.block_on(suite)))`, downcasting the payload to `String`, asserting the prefix: ignoring-TTL `CacheStore` (wraps memory, passes `ttl = None`) → `cache_store: ttl`; non-atomic-take `RefStore` (`get`, `tokio::task::yield_now`, `delete`) → `ref_store: atomic_take`; lost-increment `RateLimitStore` (`get`, yield, `put` of the saturating sum, TTL only when the key was missing, `InvalidValue` for non-8-byte values, so only atomicity is broken) → `rate_limit_store: atomic_incr`; constant-version `ModuleStateStore` (always `0`) → `module_state_store: version`; last-segment-keyed `SessionStore` (stores under `key.rsplit(':').next()`) → `session_store: namespacing`; never-waiting `IdentifyQueue` → `identify_queue: interval`; delete-ignoring `DiscordCache` (forwards to memory but drops every `*Delete`/`GuildMemberRemove` event) → `discord_cache: event_updates` -- AC 2 and 3.
- `crates/noctis/src/lib.rs` -- in `pub mod testkit`: `pub use noctis_testkit::conformance;` and a doc bullet naming the nine suites -- facade path `noctis::testkit::conformance::<port>`.
- `crates/noctis/tests/template_adapter.rs` -- add `#[cfg(feature = "testkit")] #[tokio::test(flavor = "multi_thread")] async fn bounded_cache_store_passes_the_conformance_suite()` calling `noctis::testkit::conformance::cache_store(|| BoundedCacheStore::new(10_000)).await` -- in-workspace proof of the template adapter; gate the imports it alone uses with the same `cfg` so non-`testkit` builds stay warning-free.
- `examples/project-template/tests/adapters.rs` -- drop the "planned API" NOTE line, use `#[tokio::test(flavor = "multi_thread")]`, keep the `conformance::cache_store(|| BoundedCacheStore::new(10_000))` call; `src/adapters/bounded_cache.rs` -- reword line-1 NOTE to say the conformance suite runs against it (no "planned"); `AGENTS.md` line 26 -- "`adapters.rs` (`noctis::testkit::conformance::cache_store` against `BoundedCacheStore`)".
- `examples/project-template/.claude/skills/noctis/references/adapters.md` §5 and `references/testing.md` (line 6 and §7) -- drop `[planned]` for conformance; document: signatures of all nine suites and their factory shapes, check ids per suite, the panic message format, multi-thread requirement, fresh-empty-instance factory, no paused clock, `block_in_place` pattern for async setup, wall time (~2-3 s per byte store, ~5 s identify), that `DiscordCache` gets the configured resources and must return `None` for disabled ones, and that `IdentifyQueue`/`DiscordCache` are not namespaced -- Usage Skill (AC 4).
- `xtask/public-api.txt` + `SKILL.md` `**API hash:**` -- run `cargo run --release -p xtask -- api-snapshot --bless`, put the printed hash into `SKILL.md`, then `skill-check` must pass.

**Acceptance Criteria:**
- Given an adapter author's test `#[tokio::test(flavor = "multi_thread")] async fn t() { noctis::testkit::conformance::cache_store(|| MyStore::new()).await }` with `noctis` feature `testkit`, when it runs against a correct adapter, then it passes; when the adapter breaks any matrix row, then it fails with a panic starting `conformance::cache_store: <check>:`.
- Given every in-memory default (seven byte stores, `MemoryIdentifyQueue`, `MemoryDiscordCache`), when `cargo test --release -p noctis-testkit --test conformance` runs, then all nine suites pass.
- Given the seven broken adapters, when the meta-tests run, then each suite panics at exactly the expected `<suite>: <check>`.
- Given the template's `BoundedCacheStore`, when `cargo test --release -p noctis --all-features --test template_adapter` runs, then the conformance test passes; and `cargo test --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template --test adapters` compiles and passes.
- Given the new public API, when `api-snapshot` and `skill-check` run, then both pass with the re-blessed snapshot, the matching `API hash`, and every new `noctis::testkit::conformance::*` item mentioned in the skill.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Panic, not `Result`:** the suites are assertions like `assert_memory_flat`'s callers use; the template calls `.await;` with no `?`, and a `#[must_use]` `Result` would trip clippy there. Check ids in the message make meta-tests precise.
- **Sync factory:** matches the skill/template shape already published; async setup works via `block_in_place` because the suite requires a multi-thread runtime, which it also needs for real parallel contention.
- **Identify factory takes `NonZeroU16`:** the suite needs two buckets; memory takes `max_concurrency`, a Redis queue can ignore it. `MemoryIdentifyQueue::new` fits directly.
- **DiscordCache factory takes `DiscordCacheResources`:** core builds the default from the configured resources (`crates/noctis-core/src/app.rs:3344-3365`), and "disabled resources" are only testable if the suite chooses the set. Disabled item getters must be `None` (a backend caches only what is configured, UD-8 bounded memory); id lists of disabled resources may be `None` or `Some(empty)` because memory returns `channel_ids` even without `GUILD`.
- **4.9 s interval floor:** 100 ms tolerance for cross-process clock skew; memory waits exactly 5 s on the tokio clock.
- **Golden meta-test shape:**
  ```rust
  fn fails_at(suite: impl Future<Output = ()>, prefix: &str) {
      let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap();
      let err = std::panic::catch_unwind(AssertUnwindSafe(|| rt.block_on(suite))).unwrap_err();
      let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
      assert!(msg.starts_with(prefix), "{msg}");
  }
  ```

## Verification

Run from `/home/claude/lanes/d/noctis-rs`, release only (wrap workspace-wide runs in `tools/story-loop/bg.sh`):

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo test --release -p noctis-testkit --test conformance` -- expected: pass (defaults + meta-tests + runtime guard)
- `cargo test --release -p noctis --all-features --test template_adapter` -- expected: pass
- `cargo test --release -p noctis-testkit --doc` and `cargo test --release -p noctis --all-features --doc` -- expected: pass
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, printed hash equals `SKILL.md` `API hash`
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- expected: exit 0
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` and `cargo +1.91 check --release --workspace` -- expected: exit 0
- `cargo test --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template --test adapters` -- expected: pass (delete `target/template` afterwards if disk is tight)
- `cargo hack` -- not run here (gate script, epic-last story only)

**Manual checks:**
- If `api-snapshot` lacks its toolchain, install it per AGENTS.md Known pitfalls (`nightly-2026-10-15`, `cargo-public-api` 0.52.0).
- If the template's own crate fails to compile for reasons unrelated to this story, record a `deferred` entry (Story 14.2 owns the template build); `crates/noctis/tests/template_adapter.rs` stays the binding proof that `BoundedCacheStore` conforms.
