---
title: 'Story 14.1: Adapter conformance suites'
type: 'feature'
created: '2026-10-25'
status: 'in-progress'
baseline_revision: '2590b2fa386b4b5f7705047ce6323f8d4c16f066'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Third-party adapter authors (Redis, SQL) have no reusable way to prove their port implementations keep the semantics core relies on (FR-39, AD-14). The template's `tests/adapters.rs` and the Usage Skill already call `noctis::testkit::conformance::cache_store(..)`, which does not exist yet (marked `[planned, Story 14.1]`).

**Approach:** Add a `conformance` module to `noctis-testkit` with one async suite per store port, `IdentifyQueue` and `DiscordCache`, re-export it as `noctis::testkit::conformance`, prove every in-memory default passes and that deliberately broken stores fail (meta-tests), run it against the template's `BoundedCacheStore`, and document it in the Usage Skill.

## Boundaries & Constraints

**Always:**
- Public API (exact): in `noctis_testkit::conformance`, re-exported by the facade as `noctis::testkit::conformance::{..}`:
  - `pub async fn ref_store<P: RefStore, F: Fn() -> P>(factory: F)`; same shape for `session_store` (`SessionStore`), `module_state_store` (`ModuleStateStore`), `rate_limit_store` (`RateLimitStore`), `cache_store` (`CacheStore`), `render_cache` (`RenderCache`), `asset_store` (`AssetStore`), `identify_queue` (`IdentifyQueue`).
  - `pub async fn discord_cache<C: DiscordCache, F: Fn(DiscordCacheResources) -> C>(factory: F)`.
  - Suites return `()` and panic on the first failing case with the message `conformance::<suite>: case `<case>` failed: <detail>` (`<suite>` = function name). An unexpected `Err(PortError)` is a failure whose detail includes the error's `Display`. A panic inside a spawned task is re-raised with the same prefix (never a bare `JoinError`).
- The factory is called once per case and must return a fresh, empty store (Redis: a fresh key prefix or DB); cases use fixed keys and may rely on emptiness.
- Case order per byte-store suite is fixed: basic cases → `race_*` cases → `ttl_*` cases. All `ttl_*` cases share ONE expiry wait per suite: write all short-TTL entries, `tokio::time::sleep(EXPIRY_WAIT)` once, then check. Constants (private, documented in rustdoc): `SHORT_TTL = 1 s`, `EXPIRY_WAIT = 2.5 s` (tolerates second-resolution backends), `LONG_TTL = 1 h` (below every memory default's `max_ttl`). Clock is real time; rustdoc says the suites must not run on a paused tokio clock and need a runtime with the time driver (`#[tokio::test]`, either flavor).
- Concurrency: `RACE_CALLERS = 32` tasks spawned with `tokio::spawn` on one `Arc<P>`, released together by a `tokio::sync::Barrier`; increments use 32 tasks × 16 calls of `by = 1`.
- Namespacing cases wrap ONE inner `Arc<dyn Port>` (unsized coercion of `Arc<P>`) in `noctis_core::Namespaced::new(&AppId::new("conformance-a"), ..)`, `Namespaced::new(&AppId::new("conformance-b"), ..)` and `Namespaced::shared(..)`.
- Suites write at most 64 distinct keys per instance (bounded adapters such as `BoundedCacheStore::new(10_000)` never evict during a run).
- `crates/noctis-testkit` stays free of `noctis-voice`/`noctis-image`; new normal dependency only `serde.workspace = true` (for `DeserializeSeed` when parsing fixtures). deps-check must pass unchanged.
- Every public API change in the same commit: template code + `AGENTS.md` + Usage Skill, `xtask/public-api.txt` re-blessed, `SKILL.md` `API hash` line updated.

**Never:**
- No change to port traits, memory defaults or `Namespaced` behaviour; if a memory default fails a case that the port contract (rustdoc in `crates/noctis-ports/src/store.rs`, `identify.rs`, `discord_cache.rs`) requires, fix the default, not the suite, and log a `(decision)` line.
- No suites for `LocaleSource`, `GuildAccessList` or `Renderer` (not store ports; out of the story's list).
- No paused-clock tricks, no `cargo hack`, no new CI gate (`.github/workflows-disabled/ci.yml` unchanged: the suites run inside `cargo test --release --workspace`).
- Do not remove the template's other "planned API" notes beyond the two adapter files (Story 14.2 owns that).

## I/O & Edge-Case Matrix

Byte-store cases (a row applies to every suite whose port has the op; "seed" uses `put` where the port has it, else `put_if_absent`). Value set `V` = `b""`, `b"v"`, all bytes `0..=255`, 64 KiB of `0xAB`.

| Scenario (case name) | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| `missing_key` | fresh store, key `k` | `get`/`take` → `None`; `delete` → `Ok(())` twice; `touch(k, LONG)` → `false` and `get` stays `None` (not created) | any `Err` fails |
| `roundtrip_values` | seed each value of `V` under its own key, no TTL | `get` returns the exact bytes; `b""` is `Some(empty)`, never `None` | — |
| `key_shapes` | keys `"ключ:🔑:%25"`, 512-byte ASCII key, `"K"` vs `"k"`, `"a"` vs `"ab"` | each key reads only its own value (verbatim, case- and prefix-distinct) | — |
| `overwrite` (put) | `put(k,"a")`, `put(k,"b")` | `get` → `"b"` | — |
| `put_if_absent_live` | seed `"a"`, then `put_if_absent(k,"b")` | `false`, value stays `"a"`; on absent key → `true` and stored | — |
| `take_once` | seed `"a"`; `take` twice | `Some("a")` then `None`; `get` → `None` | — |
| `delete_live` | seed, `delete`, `delete` | `get` → `None`; both `Ok` | — |
| `touch_live` | seed no TTL; `touch(k, LONG)` | `true`; value unchanged | — |
| `incr_basic` | missing `n`: `incr(n,5,None)`, `incr(n,0,None)`, `incr(z,0,None)` | `5`, `5`; `z` → `0` and `get(z)` = 8 zero bytes; `get(n)` = `5u64.to_be_bytes()` | — |
| `incr_saturates` | `put(n, (u64::MAX-1).to_be_bytes())`, `incr(n,5)`, `incr(n,1)` | `u64::MAX`, `u64::MAX` | — |
| `incr_non_counter` | `put` of `b""`, `b"abc"`, 9 bytes; `incr(k,1)` | `Err(PortError::InvalidValue{..})`; stored value unchanged | `Ok` fails |
| `version_monotonic` | fresh `ModuleStateStore` | `version()` = 0; two reads without `put` are non-decreasing; after each of 3 `put`s strictly greater than before; 8 concurrent `put`s → each caller's following read > pre-race value | — |
| `namespacing` | write `k` via app A (each op the port has; `incr` for counters) | app B and shared → miss / independent counter; inner `get("conformance-a:<segment>:k")` → A's value; shared writes land at `_shared:<segment>:k` | — |
| `race_put_if_absent` | 32 callers, distinct values, same key | exactly one `true`; stored value = winner's | — |
| `race_take` | seed, 32 callers `take` | exactly one `Some` | — |
| `race_incr` | 32×16 `incr(n,1,None)` | final `get` = 512; the 512 returned values are exactly `1..=512` | — |
| `ttl_live_before_expiry` | seed with `SHORT_TTL` | immediately `get` → `Some` | — |
| `ttl_get_expires` | seed `SHORT_TTL`, wait | `get` → `None` | — |
| `ttl_put_resets` | `put(SHORT)` then `put(None)`; and `put(None)` then `put(SHORT)` | after wait: first live, second gone | — |
| `ttl_put_if_absent_expired` | `put_if_absent(SHORT)`, wait, `put_if_absent(LONG)` | `true`, new value stored | — |
| `ttl_take_expired` | seed `SHORT`, wait | `take` → `None` | — |
| `ttl_touch` | `SHORT` key touched with `LONG`; `LONG` key touched with `SHORT`; `SHORT` key after wait | after wait: first live, second gone; touch on expired → `false` | — |
| `ttl_incr_create_only` | `incr(n,1,SHORT)`, `incr(n,1,LONG)`, wait, `incr(n,1,None)` | `1`, `2`, then `1` (expired counter recreated; second call did not reset TTL) | — |

`identify_queue` (factory must give ≥ 2 buckets and no Lease in the last 5 s):

| Scenario | Input / State | Expected | Error Handling |
|---|---|---|---|
| `lease_bucket` | `acquire(0)`, `acquire(1)` | `lease.bucket()` = 0 / 1; first grant within 1 s | `Err` fails |
| `bucket_spacing` | after `acquire(0)` returned at `t1`, `acquire(0)` again | granted at `t2` with `4.9 s ≤ t2 - t1 ≤ 8 s` | — |
| `buckets_parallel` | fresh instance: `acquire(0)`, then concurrently a second `acquire(0)` and `acquire(1)` (same run as `bucket_spacing`) | `acquire(1)` granted within 1 s while the second `acquire(0)` still waits | — |

`discord_cache` (fixtures: the 8 JSON frames of `crates/noctis-ports/tests/fixtures/gateway/`):

| Scenario | Input / State | Expected | Error Handling |
|---|---|---|---|
| `empty` | `factory(all())`, no events | every getter on fixture ids → `Ok(None)` | — |
| `guild_create_all` | `all()`, `guild_create` + `message_create` | guild `"Noctis Test Guild"`; channel 10 `"general"`; role 20 `"Moderator"`; member (1,30) with roles `[20]`; user 30; message 40 == payload; `channel_ids` `[10,11]`, `role_ids` `[1,20]`, `member_ids` `[30]` (compare sorted) | — |
| `unknown_ids` | after `guild_create_all` | ids 999 of every kind → `None` | — |
| `owned_reads` | mutate a returned `Guild` | next read unchanged | — |
| `updates_in_order` | `channel_update`, `guild_role_delete`, `guild_member_remove`, `channel_delete`, `message_delete`, `guild_delete` | channel 10 named `"renamed"`; role 20, member, message 40, guild 1 gone; `channel_ids` `[10]` before `guild_delete` | — |
| `disabled_<R>` | for each R in GUILD..MESSAGE: `factory(all().without(R))`, same events | R's getter → `None`; every other primary getter → `Some` | — |
| `channel_only` | `factory(CHANNEL)` | channel 10 `Some`; guild, role, member, user, message `None` | — |

Meta (broken stores in the test file) — the suite must panic, message containing the case: TTL-ignoring `CacheStore` → `case \`ttl_`; `RefStore` whose `take` does `get`, `tokio::task::yield_now().await`, `remove` → `case \`race_take\``; `RateLimitStore` whose `incr` reads, yields, writes → `case \`race_incr\``.

</intent-contract>

## Code Map

- `crates/noctis-ports/src/store.rs` -- the op contracts (rustdoc of `op!`) the byte suites test; `ModuleStateStore::version` contract. Read-only.
- `crates/noctis-ports/src/identify.rs`, `crates/noctis-ports/src/memory/identify.rs` -- `IdentifyQueue`/`Lease`; memory queue: 5 s per bucket, bucket ≥ `max_concurrency` is an error, `tokio::time` clock. Read-only.
- `crates/noctis-ports/src/discord_cache.rs`, `crates/noctis-ports/src/memory/discord.rs` -- `DiscordCache`, `DiscordCacheResources` (`all()`, `without`, consts `GUILD..MESSAGE`). Read-only.
- `crates/noctis-ports/tests/discord_cache.rs` -- reuse its `parse()` (GatewayEventDeserializer + `DeserializeSeed`) and expected values; fixtures in `crates/noctis-ports/tests/fixtures/gateway/*.json` (copy, don't move).
- `crates/noctis-ports/src/memory/{stores.rs,byte_store.rs,mod.rs}` -- memory defaults (`Memory*Store::new()`), std `Instant` clock, `max_ttl` clamps (≥ 1 day where set).
- `crates/noctis-core/src/namespace.rs` -- `Namespaced<dyn Port>`, `AppId`, segments `ref`, `session`, `module_state`, `rate_limit`, `cache`, `render_cache`, `asset`; exported at `noctis_core::{Namespaced, AppId}` (lib.rs:331).
- `crates/noctis-testkit/src/lib.rs` -- add `pub mod conformance;` and a crate-doc bullet.
- `crates/noctis-testkit/Cargo.toml` -- add `serde.workspace = true` to `[dependencies]`; tokio already has `rt`, `sync`, `time`.
- `crates/noctis/src/lib.rs` ~3810-3906 -- `pub mod testkit`; add `pub mod conformance { //! docs; pub use noctis_testkit::conformance::{..}; }` following the `pub mod fake` pattern, plus a module-doc bullet.
- `examples/project-template/src/adapters/bounded_cache.rs` -- passes the CacheStore suite as written (Mutex map, TTL, atomic ops); only its line-1 NOTE changes.
- `examples/project-template/tests/adapters.rs` -- already calls `conformance::cache_store(|| BoundedCacheStore::new(10_000)).await`.
- `crates/noctis/tests/template_adapter.rs` -- precedent for `#[path = "../../../examples/..."]` inclusion of the template adapter.
- Skill: `examples/project-template/.claude/skills/noctis/references/testing.md` (§ intro line 7, §7), `references/adapters.md` (§5 "Conformance tests (FR-39) [planned]", line ~657-672), `SKILL.md` line 23 (`API hash`); `examples/project-template/AGENTS.md` line 26.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-testkit/src/conformance.rs` + `crates/noctis-testkit/src/conformance/{bytes.rs,identify.rs,discord.rs}` -- implement the suites per the matrix: shared case helpers per op (generic over a small private trait or closures), the `fail(suite, case, detail) -> !` helper producing the exact panic format, the race harness (spawn + Barrier, join, re-panic with prefix). Module rustdoc: usage example (`no_run` or a real doctest against `MemoryCacheStore`), the factory contract, the time budget (~3 s per byte suite, ~5 s for `identify_queue`), real-clock requirement, case list.
- `crates/noctis-testkit/src/conformance/fixtures/*.json` -- copies of the 8 gateway fixtures, embedded with `include_str!`.
- `crates/noctis-testkit/Cargo.toml` -- add `serde.workspace = true`.
- `crates/noctis-testkit/src/lib.rs` -- `pub mod conformance;` + doc bullet.
- `crates/noctis/src/lib.rs` -- facade `testkit::conformance` re-export module listing all 9 functions + docs; testkit module-doc bullet.
- `crates/noctis-testkit/tests/conformance.rs` -- (a) one `#[tokio::test(flavor = "multi_thread")]` per memory default passing its suite (`MemoryRefStore`, `MemorySessionStore`, `MemoryModuleStateStore`, `MemoryRateLimitStore`, `MemoryCacheStore`, `MemoryRenderCache`, `MemoryAssetStore`, `MemoryIdentifyQueue::new(NonZeroU16::new(2))`, `MemoryDiscordCache::new`); (b) the 3 meta-tests on `#[tokio::test]` (current-thread, deterministic) using `futures_util::FutureExt::catch_unwind(AssertUnwindSafe(..))`, asserting `Err` and the case substring; a suite that returns normally fails the meta-test; (c) `#[path = "../../../examples/project-template/src/adapters/bounded_cache.rs"] mod bounded_cache;` and a test running `cache_store(|| BoundedCacheStore::new(10_000))`.
- `examples/project-template/src/adapters/bounded_cache.rs` -- line 1 NOTE: drop "only `noctis::testkit::conformance` is still planned"; mention the suite now runs in noctis-rs `crates/noctis-testkit/tests/conformance.rs`.
- `examples/project-template/tests/adapters.rs` -- keep the call; update its doc comment to list what the suite checks (TTL, overwrite, delete, missing key, races, namespacing).
- `examples/project-template/.claude/skills/noctis/references/testing.md` -- intro: conformance no longer "planned"; §7 drop `[planned, Story 14.1]`, show usage, failure message format, timing, real clock.
- `examples/project-template/.claude/skills/noctis/references/adapters.md` -- §5 drop `[planned]`; signatures of all 9 suites (`discord_cache` factory takes `DiscordCacheResources`; `identify_queue` needs ≥ 2 buckets), the case list, meta-test note, fresh-store rule; mention every new public item so `skill-check` passes.
- `examples/project-template/AGENTS.md` -- line 26: name `noctis::testkit::conformance::<port>`.
- `xtask/public-api.txt` + `SKILL.md` -- `cargo run --release -p xtask -- api-snapshot --bless`, paste the printed hash into `SKILL.md`'s `API hash` line.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- `(decision)` lines for D1-D6 below and any memory-default fix.

**Acceptance Criteria:**
- Given a crate with `noctis` `features = ["testkit"]`, when it calls `noctis::testkit::conformance::cache_store(|| MyStore::new()).await` (and likewise the other 8 suites), then it compiles and runs every matrix case for that port.
- Given each in-memory default, when `cargo test --release -p noctis-testkit --test conformance` runs, then all 9 suites pass, and so does `BoundedCacheStore`.
- Given the three broken stores, when they run their suites, then each suite panics with a message starting `conformance::<suite>: case ` and naming the `ttl_*`, `race_take` and `race_incr` case respectively.
- Given the new public API, when `api-snapshot` and `skill-check` run, then both pass with the re-blessed snapshot and updated hash, and the skill no longer marks conformance `[planned]`.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (log each in the memlog):
- D1 Scope: 7 byte stores + `IdentifyQueue` + `DiscordCache` (the story's list); `LocaleSource`/`GuildAccessList` have no store semantics.
- D2 API is the shape the template and skill already use (`factory` closure, `()` + panic), extended to `Fn(DiscordCacheResources) -> C` because the "disabled resources" check needs several configurations; core passes the configured set to custom backends (`AppBuilder::discord_cache_backend` docs).
- D3 Disabled resources are tested as `all().without(R)` (dependencies stay on, so a row isolates R) plus `CHANNEL` only (matches `restricted_to_channels`). If `MemoryDiscordCache` misses a `Some` expectation only because twilight derives that resource through the disabled one (e.g. user via member), drop that single getter from that row and log it; never weaken the `None` side.
- D4 One expiry wait per suite with `SHORT_TTL` 1 s / `EXPIRY_WAIT` 2.5 s keeps the suite usable against second-resolution SQL TTLs and the whole run under ~25 s in parallel.
- D5 `version_monotonic` asserts 0 on a fresh store (contract text) and strict growth per `put`, not `+1`.
- D6 Meta-tests add a lost-increment store beyond the story's two (FR-39 "no lost increments").

Race harness sketch:
```rust
let store = Arc::new(factory());
let gate = Arc::new(Barrier::new(RACE_CALLERS));
let tasks: Vec<_> = (0..RACE_CALLERS).map(|i| {
    let (store, gate) = (Arc::clone(&store), Arc::clone(&gate));
    tokio::spawn(async move { gate.wait().await; store.take("k").await })
}).collect();
// join each; JoinError::is_panic -> fail(suite, "race_take", panic text)
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-testkit --test conformance` -- 13 tests pass
- `cargo test --release -p noctis-testkit -p noctis` -- pass (doctests included)
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` (via `tools/story-loop/bg.sh`) -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- deps-check`, `-- api-snapshot`, `-- skill-check`, `-- rules-catalog`, `-- tw-grammar --check` -- ok
- `cargo deny check` and `cargo +1.91 check --release --workspace` -- ok
- `cargo test --release --manifest-path examples/project-template/Cargo.toml --test adapters` -- best effort: passes; if it fails for a reason outside the adapter files, record it in `deferred` for Story 14.2.
