---
title: 'Story 3.2: Bounded in-memory store defaults'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: 'd716808b45ba4554b36f4931ee20f5d2262cf2ef'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-1-port-traits-and-the-shared-op-vocabulary.md'
warnings: ['oversized']
deferred:
  - summary: >-
      MemoryDiscordCache message memory grows with every channel ever seen, because twilight-cache-inmemory never purges a channel's message list on CHANNEL_DELETE or GUILD_DELETE, and DM channels are never deleted.
    evidence: |-
      twilight-cache-inmemory 0.17.1 touches `channel_messages` only in event/message.rs (create/delete/update) and in `clear()` (lib.rs l.291); no channel or guild delete handler removes it. The per-channel cap (AC5) holds, but total messages = cap x channels ever seen. Upstream behaviour exposed, not caused, by Story 3.2; AC5 bounds the cache by resource set plus the per-channel cap. Address with Story 12.5 (DiscordCacheConfig), e.g. purge on delete or cap channels with cached messages.
    location: >-
      crates/noctis-ports/src/memory/discord.rs (MemoryDiscordCache::update / with_message_cap)
    severity: medium
---

<intent-contract>

## Intent

**Problem:** Story 3.1 added the port traits but no implementations. A bot cannot run out of the box, and nothing proves the op contracts (atomicity, TTL, bounds) on a real store (FR-38, NFR-1 bounded, UX-DR8; AD-1, AD-14, AD-25).

**Approach:** Add `noctis_ports::memory` with:
- a byte-budgeted, TTL-capped moka store behind the seven byte-store ports;
- trivial `LocaleSource`/`GuildAccessList` defaults;
- `MemoryDiscordCache`, which wraps `twilight-cache-inmemory` restricted to chosen resources and returns owned `twilight_model` values.

Re-export it all as `noctis::ports::memory`. Reconcile the template's `bounded_cache.rs` by compiling it in a facade test, and document the defaults in the Usage Skill. `IdentifyQueue`'s default is Story 3.3.

## Boundaries & Constraints

**Always:**

Dependencies:
- noctis-ports `[dependencies]` become exactly bytes, moka, postcard, serde, thiserror, twilight-cache-inmemory and twilight-model.
- Add these workspace entries:
  - root `moka = "0.12"`, which becomes `moka = { version = "0.12", default-features = false }`; the member enables `features = ["sync"]`. Verified on crates.io: 0.12.16 is the latest, MSRV 1.71.1, default features are empty.
  - `twilight-cache-inmemory = "0.17.1"` (already in the root, latest); the member uses `default-features = false`, so the `permission-calculator` feature stays off.
- noctis-ports dev-deps: `serde_json.workspace = true` only. There is no tokio in noctis-ports: concurrency tests use `std::thread::scope` with 100 threads and a `Barrier`, and they poll ready futures once with `std::task::Waker::noop()`.

`MemoryLimits` (`Copy`, `Debug`, `Eq`; private fields):
- `const fn new(max_bytes: u64, max_ttl: Option<Duration>)` panics if `max_bytes == 0` or `max_ttl == Some(Duration::ZERO)`. In a `const` context that panic is a compile error. `fn try_new(..) -> Option<Self>` returns `None` for the same inputs.
- Getters `max_bytes()` and `max_ttl()`.
- `pub const ENTRY_OVERHEAD: u64 = 64`.
- An entry weighs `key.len() + value.len() + ENTRY_OVERHEAD` in UTF-8 bytes. The moka weigher returns that weight as `u32::try_from(w).unwrap_or(u32::MAX)`.

The seven byte stores are `MemoryRefStore`, `MemorySessionStore`, `MemoryModuleStateStore`, `MemoryRateLimitStore`, `MemoryCacheStore`, `MemoryRenderCache` and `MemoryAssetStore`:
- Each implements only its port.
- Each has `pub const DEFAULT_LIMITS: MemoryLimits`, `new()` (= `DEFAULT_LIMITS`), `with_limits(MemoryLimits)`, `limits()`, `Default`, and `usage() -> MemoryUsage { pub entries: u64, pub bytes: u64 }`. `usage()` calls moka `run_pending_tasks()` first.
- Each has a manual `Debug` that prints the limits, never keys or values.
- All seven are thin wrappers over one private `ByteStore` (`moka::sync::Cache<String, Slot>`, `Slot { value: Bytes, deadline: Option<Instant> }`).
- The moka cache uses `EvictionPolicy::lru()`, because TinyLFU admission could silently drop fresh Refs. It has `max_capacity(max_bytes)`, the weigher, and an `Expiry` whose create/update hooks return `deadline - at` (saturating).
- Never use moka's `time_to_live`.

Defaults (documented in rustdoc and the skill):

| Store | max_bytes | max_ttl |
|---|---|---|
| Ref | 32 MiB | 7 days |
| Session | 1 MiB | none |
| ModuleState | 16 MiB | none |
| RateLimit | 16 MiB | 1 day |
| Cache | 32 MiB | none |
| RenderCache | 64 MiB | 1 day |
| AssetStore | 32 MiB | 1 day |

Op semantics on `ByteStore`:
- Every mutation (put, put_if_absent, take, delete, touch, incr) goes through `cache.entry_by_ref(key).and_compute_with(..)`. moka serializes those calls per key, so they are atomic with each other. `get` uses `cache.get`.
- Every read and compute re-checks liveness itself: `deadline.is_none_or(|d| d > Instant::now())`. A dead slot is treated as absent.
- Deadline: `now.checked_add(min(ttl, max_ttl))`, where a missing side is ignored. `None` and `None` give no deadline, and an overflow also gives no deadline.
- `incr_with_ttl` on a live counter keeps the existing deadline.
- `touch` sets a new deadline (clamped to `max_ttl`) with the same value.
- A write whose weight exceeds `max_bytes` returns `PortError::invalid_value(key, "entry larger than the store's byte budget")` and changes nothing. A weight equal to `max_bytes` is accepted.
- Futures are `Box::pin(async move { … })`. The work runs on the first poll and never awaits.
- `MemoryModuleStateStore::version()` reads an `AtomicU64` (starting at 0) that is `fetch_add(1)` after every successful `put`. A failed put and an eviction leave it unchanged.

Locale and guild-access defaults:
- `NoLocaleOverlay` (unit struct, `Default`): `load` returns `Ok(vec![])`.
- `AllowAllGuilds` (unit struct, `Default`): `allows` returns `true`.

`DiscordCacheResources` (`Copy`, `Eq`, `Hash`, `Debug`, `Default` = empty; private `u8` bits):
- Consts `GUILD`, `CHANNEL`, `ROLE`, `MEMBER`, `USER`, `MESSAGE`.
- Const fns `empty()`, `all()`, `union()`, `contains()` and `is_empty()`, plus `BitOr`.
- It never exposes twilight-cache-inmemory's `ResourceType`.

`MemoryDiscordCache`:
- Constructors: `new(resources)`, with a message cap of `DEFAULT_MESSAGE_CAP = 100` (twilight's own default), and `with_message_cap(resources, cap: usize)`. A `cap == 0` removes `MESSAGE` from the set, because twilight treats 0 like 1. Getters `resources()` and `message_cap()`.
- Resource mapping: each resource maps to the same-named `ResourceType`. `MEMBER` also turns on `USER` internally (a Member needs its User), but `resources()` reports what the caller asked for.
- Private `NoctisModels: CacheableModels` uses `DefaultCacheModels`' types, except `Message = MessageRecord(Message)`. `MessageRecord` is a private newtype that implements `CacheableMessage` (reactions methods over `Message.reactions`), `From<Message>`, `PartialEq<Message>`, `PartialEq`, `Clone` and `Debug`, so messages stay full fidelity.
- `update(&Event)` calls `InMemoryCache::update`.
- `channel`, `role` (`.resource()`), `user` and `message` return clones of the stored value.
- `guild` converts `CachedGuild` to `Guild` through its getters. The list fields (`channels`, `roles`, `members`, `emojis`, `stickers`, `presences`, `voice_states`, `threads`, `stage_instances`) are empty, `guild_scheduled_events` is copied, and `approximate_member_count`/`approximate_presence_count` are `None`.
- `member` converts `CachedMember` together with the cached `User`. `deaf`/`mute` are `unwrap_or(false)`, and `avatar_decoration_data`/`banner` are `None` (no getters exist). If the user is not cached, the result is `None`.
- `*_ids` read twilight's per-guild sets (`guild_channels`, `guild_roles`, `guild_members`) and return them sorted ascending. A missing set gives `None`.
- No twilight guard is held past the synchronous block. The rustdoc lists every lossy field.

Facade, template and docs:
- The facade adds `pub mod memory` inside `noctis::ports`, with an explicit `pub use noctis_ports::memory::{…}` list (no glob).
- Public API changes bless `xtask/public-api.txt` and update the template skill, the `SKILL.md` API hash and the template `AGENTS.md` in the same commit.

**Never:**
- No `IdentifyQueue` default (Story 3.3).
- No `Namespaced`/`Shared` wrappers, App builder setters, `ctx.kv()`/`ctx.cache()`, `DiscordCacheConfig` or `CacheDisabled` (Stories 7.x–12.5).
- No testkit conformance suites.
- No tokio, futures-util or dashmap as a direct noctis-ports dependency.
- No `std::thread`, `std::time::SystemTime` I/O or `unsafe` in `src/`.
- No twilight-cache-inmemory type in any public signature.
- No new rule id, no `docs/discord-rules.md` change, no CI job.

## I/O & Edge-Case Matrix

The byte-store matrix lives in `crates/noctis-ports/tests/memory_stores.rs`. TTL rows use real sleeps with at least a 2× margin (TTL 100 ms, sleep 250 ms). L(b, t) means `MemoryLimits::new(b, t)`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Limits valid edges | `L(1, None)`, `L(u64::MAX, Some(1ns))` | the getters echo the inputs | — |
| Limits zero | `try_new(0, None)`, `try_new(1, Some(ZERO))` | `None` | `new` panics (`#[should_panic]`) |
| Every default | `X::DEFAULT_LIMITS` for all 7 stores | equal the table above; `X::new().limits() == X::DEFAULT_LIMITS` | — |
| Missing key | `get("nope")` on a fresh store | `None`; `usage() == {0, 0}` | — |
| Empty key / empty value | `put("", Bytes::new(), None)` | `get("") == Some(empty)` (not `None`) | — |
| Unicode key | `"ğüş😀\u{0}"` | round-trip; weight counts UTF-8 bytes (`usage().bytes == key.len()+v.len()+64`) | — |
| Overwrite | `put(k,a,Some(100ms))`, then `put(k,b,None)`, sleep 250 ms | `get == Some(b)` (TTL reset to none) | — |
| Budget exact | `L(100,None)`, key `"k"`, value of 35 bytes (weight 100) | Ok; readable | — |
| Budget +1 | value of 36 bytes | `PortError::InvalidValue`; the prior value under `k` remains | applies to put, put_if_absent and incr (on a 29-byte key with `L(100,None)`: 29+8+64=101) |
| Bounded | `L(10*(64+4+8), None)`, put 1000 keys `"k%03d"` with 8-byte values | `usage().bytes <= max_bytes` and `entries <= 10` | — |
| TTL expiry | `put(k,v,Some(100ms))`, sleep 250 ms | `get == None`, `take == None`, `touch == false` | — |
| Zero TTL | `put(k,v,Some(ZERO))` | `get == None` immediately | — |
| Huge TTL | `put(k,v,Some(Duration::MAX))` with `L(_, None)` | no panic; live | — |
| max_ttl clamp | `L(_,Some(100ms))`, `put(k,v,None)` and `put(k2,v,Some(1h))`, sleep 250 ms | both `None` | — |
| put_if_absent | live key | `false`, value unchanged; on an expired key `true` with the new value | — |
| put_if_absent race | 100 threads, same key, distinct values | exactly one `true`; `get` = the winner's value | — |
| take race | 100 threads `take` on one live key | exactly one `Some`; then `get == None` | — |
| delete | missing key; live key | Ok both; then `get == None` | — |
| touch extends | `put(k,v,Some(150ms))`, sleep 80 ms, `touch(k,1s)`, sleep 150 ms | `touch == true`; `get == Some(v)` | — |
| touch never creates | `touch("missing",1s)` | `false`; `get == None` | — |
| touch clamp | `L(_,Some(100ms))`, `touch(k,1h)`, sleep 250 ms | expired | — |
| incr create | `incr(k,0,None)` | `0`; raw == `0u64.to_be_bytes()` | — |
| incr race | 100 threads `incr(k,1,None)` | final 100; returned values = {1..=100} | — |
| incr TTL on create only | `incr(k,1,Some(150ms))`, sleep 80 ms, `incr(k,1,Some(10s))`, sleep 120 ms | the second call returns 2; `get == None` (the first deadline is kept) | — |
| incr saturate | stored `u64::MAX-1`, `incr(k,5,None)` twice | `u64::MAX` both times | — |
| incr non-counter | values of 7 bytes, 9 bytes and empty | `PortError::InvalidValue { key: k, .. }`; value unchanged | — |
| incr after expiry | counter expired | recreated as `by` | — |
| version | fresh `MemoryModuleStateStore` | `0`; after one put `1`; after an oversized put still `1`; after 100 concurrent puts `101` | — |
| Swap via dyn | each store as `Arc<dyn Port>` in a caller fn | compiles and behaves as above | — |
| Trivial defaults | `NoLocaleOverlay.load("en-US")`, `load("")`; `AllowAllGuilds.allows(Id::new(1))` | `Ok(vec![])`; `true` | — |

The `MemoryDiscordCache` matrix lives in `crates/noctis-ports/tests/discord_cache.rs`, over recorded gateway frames in `tests/fixtures/gateway/*.json` (`{"op":0,"s":n,"t":"…","d":{…}}`). Each frame is parsed with `GatewayEventDeserializer::from_json(..)` + `DeserializeSeed` over `serde_json::Deserializer`, then `Event::from(GatewayEvent)`. Fixture ids: guild 1, channels 10 and 11, role 1 (@everyone) and role 20, user 30 (member with role 20), message 40 in channel 10.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Empty cache | `all()`, no events | every getter `None`; id lists `None` | — |
| GUILD_CREATE, all | `all()` | `guild(1)` has the name from the fixture and empty list fields; `channel(10)`; `role(20)`; `member(1,30)` with `user.id == 30` and `roles == [20]`; `user(30)`; `channel_ids(1) == [10,11]`, `role_ids(1) == [1,20]` and `member_ids(1) == [30]` (sorted) | — |
| Owned clone | mutate the returned `Guild`, read again | the cache is unchanged | — |
| Restricted | `CHANNEL` only | `channel(10)` Some; `guild`, `role`, `member`, `user` `None`; `channel_ids(1)` Some; `role_ids`/`member_ids` `None` | — |
| MEMBER without USER | `GUILD | MEMBER` | `member(1,30)` Some (USER is on internally); `resources()` reports `GUILD | MEMBER` | — |
| MESSAGE_CREATE | `all()` | `message(40)` equals the parsed `Message` (author, content) | — |
| Message cap | cap 2, three MESSAGE_CREATE in channel 10 (ids 40, 41, 42) | 40 → `None`; 41 and 42 Some | — |
| Cap 0 | `with_message_cap(all(), 0)` | `resources()` lacks `MESSAGE`; `message(40) == None` | — |
| Updates | CHANNEL_UPDATE rename, GUILD_ROLE_DELETE 20, GUILD_MEMBER_REMOVE 30, CHANNEL_DELETE 11, MESSAGE_DELETE 40, GUILD_DELETE 1 (each applied in order) | each is reflected: new name; `role(20)` None; `member` None; `channel_ids == [10]`; `message` None; `guild(1)` None | — |
| Resource set ops | `GUILD | ROLE` | `contains(GUILD)`; `!contains(USER)`; `all().contains(x)` for each const; `empty().is_empty()`; `Default == empty()` | — |

</intent-contract>

## Code Map

Ports crate:
- `crates/noctis-ports/Cargo.toml` -- add `moka = { workspace = true, features = ["sync"] }`, `twilight-cache-inmemory = { workspace = true, default-features = false }` and dev-dep `serde_json`.
- `crates/noctis-ports/src/lib.rs` -- add `pub mod memory;` and a crate-doc paragraph on the defaults. Existing flat re-exports are unchanged.
- `crates/noctis-ports/src/store.rs` -- the `op!` macro contracts that the memory stores must honour, verbatim.
- `crates/noctis-ports/src/error.rs` -- `PortError::invalid_value(key, why)` is used for non-counter and oversized values.
- `crates/noctis-ports/src/discord_cache.rs` -- the trait to implement (10 methods).
- `crates/noctis-ports/tests/purity.rs` l.21-22 -- `ALLOWED_DEPENDENCIES` must gain `moka` and `twilight-cache-inmemory`, kept sorted. Its parser only reads `[dependencies]` lines, and the dev-dep is fine.

moka 0.12.16 (registry source):
- `sync::Cache::entry_by_ref(&str).and_compute_with(FnOnce(Option<Entry>) -> ops::compute::Op<V>) -> CompResult`. It serializes per key and passes `None` for expired entries (`value_initializer.rs` l.179-290).
- `run_pending_tasks`, `entry_count`, `weighted_size`.
- `Expiry::{expire_after_create, expire_after_update}` take `std::time::Instant`.
- `CacheBuilder::{max_capacity, weigher, eviction_policy(EvictionPolicy::lru()), expire_after}`.

twilight-cache-inmemory 0.17.1:
- `InMemoryCache::builder().resource_types(ResourceType).message_cache_size(n).build()`, `update(&impl UpdateCache)` (`Event` implements it), getters returning `Reference` (`.value()`), `GuildResource::resource()`.
- `CacheableModels` (traits.rs l.55) and `CacheableMessage` (l.235); `DefaultCacheModels` is at lib.rs l.238.
- On GUILD_CREATE, `cache_guild` (event/guild.rs l.12) inserts an empty per-guild set for each wanted resource, so a guild with no channels gives `Some([])`.
- `CachedMember` has no `banner`/`avatar_decoration_data` getters.

twilight-model 0.17.1:
- `Guild` and `Member` are not `non_exhaustive` (struct literals work).
- `gateway::event::{GatewayEventDeserializer, GatewayEvent, Event}`, with `From<GatewayEvent> for Event`.

Facade, template and tooling:
- `Cargo.toml` (root) -- the `moka` workspace entry becomes `{ version = "0.12", default-features = false }`. `twilight-cache-inmemory` already exists (and is not locked yet).
- `crates/noctis/src/lib.rs` l.151-196 -- `pub mod ports`. Add a nested `pub mod memory` with docs and a doctest that builds `Arc<dyn CacheStore>` from `MemoryCacheStore::new()` and asserts a `DEFAULT_LIMITS` getter.
- `crates/noctis/Cargo.toml` -- dev-dep `tokio = { workspace = true, features = ["macros", "rt"] }`, needed by the included template file's `#[tokio::test]`.
- `examples/project-template/src/adapters/bounded_cache.rs` -- its signatures already match. Replace the `// NOTE` line with: "Compiles against the real `noctis::ports` API (checked by noctis-rs `crates/noctis/tests/template_adapter.rs`); only `noctis::testkit::conformance` is still planned." Fix only what that compile surfaces.
- `examples/project-template/.claude/skills/noctis/references/adapters.md` -- §1 ("configure them with … [planned]") becomes the real defaults section; §4 already uses `bounded_cache.rs` as the custom-Adapter example.
- `examples/project-template/.claude/skills/noctis/SKILL.md` l.21 -- the API hash.
- `examples/project-template/AGENTS.md` -- add an Adapters convention line. This also closes 3.1's deferred finding.
- `xtask/src/skill_check/mention.rs` -- every new snapshot item's last segment (`ENTRY_OVERHEAD`, `with_message_cap`, `GUILD`, …) must appear in the skill `*.md`.
- `.github/workflows-disabled/ci.yml` -- unchanged; the existing jobs cover this story.

## Tasks & Acceptance

**Execution:**
- `Cargo.toml` -- adjust the moka entry -- no default features.
- `crates/noctis-ports/Cargo.toml` and `tests/purity.rs` -- the deps and the allow-list -- AC1.
- `crates/noctis-ports/src/memory/mod.rs` -- module docs (the defaults table, the eviction note "the defaults are caches: capacity may evict any entry, swap a persistent adapter for durable data"), `MemoryLimits` and `MemoryUsage`, and the re-exports.
- `crates/noctis-ports/src/memory/byte_store.rs` -- the private `ByteStore`, `Slot`, the deadline helper and the moka `Expiry`. Add unit tests for the deadline helper: None/None, clamp, overflow and ZERO.
- `crates/noctis-ports/src/memory/stores.rs` -- the 7 wrappers. Generate the shared inherent API and the `Debug`/`Default` impls with one `macro_rules!`, and implement each port by delegating.
- `crates/noctis-ports/src/memory/trivial.rs` -- `NoLocaleOverlay` and `AllowAllGuilds`.
- `crates/noctis-ports/src/memory/discord.rs` -- `DiscordCacheResources`, `MemoryDiscordCache`, `NoctisModels`, `MessageRecord` and the conversions.
- `crates/noctis-ports/tests/memory_stores.rs` -- the byte-store matrix.
- `crates/noctis-ports/tests/discord_cache.rs` and `tests/fixtures/gateway/*.json` -- the cache matrix.
- `crates/noctis/Cargo.toml` and `src/lib.rs` -- `noctis::ports::memory` and the tokio dev-dep.
- `crates/noctis/tests/template_adapter.rs` -- include the template file with `#[path = "../../../examples/project-template/src/adapters/bounded_cache.rs"] mod bounded_cache;` (narrow `#[allow]` with a reason only if a workspace lint fires on template code). Drive `BoundedCacheStore::new(2)` through `Arc<dyn CacheStore>`: put/get, `put_if_absent` false on a live key, delete, `incr_with_ttl` 5 then 10, and an eviction keeping `get` hits ≤ 2 of 3 keys.
- `examples/project-template/src/adapters/bounded_cache.rs`, `.claude/skills/noctis/references/adapters.md`, `SKILL.md` and `AGENTS.md`:
  - real defaults section: the table, `MemoryLimits`, `usage`, `DiscordCacheResources`, `MemoryDiscordCache` with its lossy fields, `NoLocaleOverlay` and `AllowAllGuilds`;
  - mention every new item;
  - keep `ctx.kv()`, builder setters and conformance marked [planned];
  - update the hash.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the planning decisions are logged. Add implementation decisions only if any arise.

**Acceptance Criteria:**
- Given `noctis_ports::memory`, when each default store is constructed with `new()` or `with_limits`, then it reports its byte budget and max TTL, its defaults equal the documented table, and `usage()` never exceeds `max_bytes` after any sequence of writes. `MemoryDiscordCache` is bounded by its resource set and its per-channel message cap.
- Given 100 concurrent callers, when they race `put_if_absent`/`take` on one key or `incr_with_ttl` on one counter, then exactly one wins, no increment is lost, and the TTL is set only on create.
- Given an entry past its TTL, when it is read, then it is a miss; `touch` extends a live entry and `delete` removes it.
- Given `MemoryModuleStateStore`, when `put` succeeds, then `version()` grows by one, starting from 0.
- Given `MemoryDiscordCache` built for a set of resources, when recorded gateway events are applied, then only those resources are cached (the twilight message cap applies), and every getter returns an owned `twilight_model` value.
- Given the template's `bounded_cache.rs`, when `cargo test --release -p noctis` runs, then it compiles unchanged in its signatures against `noctis::ports` and passes its contract checks. The Usage Skill documents custom Adapters with that file as the example, and `skill-check` and `api-snapshot` pass.

## Spec Change Log

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 36 findings — high 0, medium 5, low 14, false 17, maybe-false 0
- findings:
  - `low` `patch` blind: `resources()` rustdoc ("as requested") and skill ("what you asked for") contradict `with_message_cap(.., 0)` removing MESSAGE — doc-only wording fix in discord.rs and adapters.md §1.1.
  - `low` `reject` blind: weigher clamps to u32::MAX while `check_fits` compares u64 — real only for single entries over 4 GiB, which the spec's Design Notes already call approximate; a guard adds a branch for a case nobody meets.
  - `low` `reject` blind: ENTRY_OVERHEAD = 64 understates resident memory — the bound still holds within a constant factor and the constant is fixed by the spec; changing it means editing the spec.
  - `false` `reject` blind: skill overstates "usage() never exceeds max_bytes" — `usage()` runs `run_pending_tasks()` before reading, so the reported value is within budget; `usage_stays_within_the_budget` asserts it.
  - `low` `reject` blind: `usage()` runs maintenance synchronously — the spec mandates `run_pending_tasks()` in `usage()`; the work is bounded by pending writes and no async caller exists yet.
  - `false` `reject` blind: `version()` not linearised with `put` — the bump happens after the write, so any reader that sees the new version re-reads the new value; eviction leaving the version unchanged is spec-mandated.
  - `low` `reject` blind: module state can be silently evicted from the 16 MiB LRU — covered by the documented "defaults are caches" note in rustdoc and the skill; needs 16 MiB of module-state entries first.
  - `false` `reject` blind: `put_if_absent` returns InvalidValue for an oversized value on a live key — the spec matrix ("Budget +1 ... applies to put, put_if_absent and incr") mandates the check first.
  - `low` `patch` blind: timing tests `touch_extends_a_live_entry` / `incr_sets_the_ttl_on_create_only` leave 70 ms margin, below the spec's own 2x rule — widen the initial TTL to 400 ms and the final incr sleep to 500 ms.
  - `low` `reject` blind: `member()` does two non-atomic lookups — a transient `None` only when a concurrent update removes the user mid-call, which is also what the next read returns; atomicity needs holding twilight guards, which the spec forbids.
  - `low` `reject` blind: `DiscordCacheResources::all()` is a literal and `Debug` prints raw bits — all six constants are in `all()` (tested by `resource_set_ops`); a flag-name `Debug` adds a manual impl for cosmetic gain.
  - `false` `reject` blind: no default resource set for the App's DiscordCache — App wiring and `DiscordCacheConfig` belong to Story 12.5 per epics; nothing constructs a default App cache yet.
  - `medium` `patch` blind: discord_cache tests leave Guild/Member field mapping and other cases unasserted — grouped with the verification-gap Guild/Member finding; patched there (full struct equality against the payload).
  - `low` `reject` blind: defaults table written three times with no drift check — the spec requires the table in rustdoc and the skill; generating it adds tooling for a table that changes rarely.
  - `low` `reject` blind: `xtask/public-api.txt` lists trait impls twice — cargo-public-api output for traits reachable by two paths; `api-snapshot` passes and the duplication is stable, fixing it means changing the generator.
  - `low` `reject` edge: `check_fits` admits entries over u32::MAX that the weigher clamps — same root cause as the blind weigher-width row; rejected with it.
  - `false` `reject` edge: `max_bytes` below ENTRY_OVERHEAD makes every write fail — that is a loud typed `InvalidValue`, and `L(1, None)` is a spec-listed valid edge.
  - `false` `reject` edge: `put_if_absent`/`incr` with ttl ZERO insert a dead entry — the port contract treats an expired entry as absent and the spec matrix requires zero TTL to be dead immediately; this is the specified behaviour.
  - `low` `reject` edge: resident memory transiently exceeds `max_bytes` before moka housekeeping — bounded by moka's write-log flush and invisible through `usage()`; forcing maintenance per write adds cost and a branch.
  - `medium` `defer` edge: message memory grows with every channel ever seen (twilight never purges `channel_messages` on channel/guild delete) — verified in twilight-cache-inmemory 0.17.1 sources; upstream behaviour within AC5's bound; deferred to Story 12.5.
  - `medium` `defer` edge: claim that AC1 "bounded" fails for MemoryDiscordCache — same root cause as the previous row; deferred with it.
  - `false` `reject` edge: MEMBER caches users so `user()` answers without USER — spec-mandated ("MEMBER also turns on USER internally, but resources() reports what the caller asked for").
  - `low` `reject` edge: claim that the u32 weigher clamp breaks AC1 — same root cause as the blind weigher-width row; rejected with it.
  - `medium` `patch` vgap: LRU admission of fresh writes under a full budget is untested — add `full_budget_admits_the_newest_write` to memory_stores.rs.
  - `low` `patch` vgap: expired entries leaving `usage()` is untested — add `expired_entries_leave_usage` to memory_stores.rs.
  - `medium` `patch` vgap: Guild and Member conversion fields are mostly unasserted — add full-struct equality against the GUILD_CREATE payload with the documented lossy fields cleared.
  - `false` `reject` intent: "runs out of the box" vs port-only surface — epics assign App builder setters and ctx wiring to Stories 7.x-12.5; the spec settled this.
  - `false` `reject` intent: MemoryDiscordCache has no byte/entry bound — AC5 defines its bound as the resource set plus twilight's per-channel cap; the concrete leak is the deferred edge row.
  - `false` `reject` intent: three stores have `max_ttl` none — "none" is a documented default, as the spec table settles.
  - `false` `reject` intent: 100 OS threads instead of 100 tasks — the store futures are ready on first poll; the spec deliberately chose threads for real parallelism.
  - `false` `reject` intent: `version()` counts puts, not state changes — the port contract says "greater after every put" and the spec settled eviction behaviour.
  - `false` `reject` intent: Guild/Member are lossy reconstructions — spec-mandated and documented; the missing field assertions are the patched vgap row.
  - `false` `reject` intent: fixtures are synthetic, not captured traffic — the spec defines the recorded-frame fixtures and their shape.
  - `false` `reject` intent: template reconciled via facade test, not by building the template crate — the spec settled this (template_adapter.rs with `#[path]`).
  - `false` `reject` intent: skill sections relabelled [planned] and workspace default-features changed — the relabels describe APIs that do not exist yet, and the Cargo entries are spec-mandated.
  - `false` `reject` intent: byte-only capacity configuration — the AC allows bytes and the spec's design note chose bytes.

## Design Notes

Decisions (logged in the memlog):
- **The budget is in bytes for every store.** The AC allows entries or bytes; one weigher keeps a single code path, and NFR-1 needs RenderCache bytes (Story 12.6 reads `usage().bytes`).
- **Oversized writes are a typed error, not a silent drop.** A silently dropped Ref would surface later as an "expired" interaction.
- **`max_ttl` is a clamp computed by noctis.** We compute it ourselves rather than use moka's `time_to_live`, so `incr` keep-deadline and `touch` stay exact. Deadlines live in the value, and moka's `Expiry` only mirrors them.
- **The eviction policy is LRU.** TinyLFU may reject newly inserted entries.
- **All mutations go through `and_compute_with`.** It is the only moka path that serializes per key.
- **`MemoryDiscordCache` mixes conversion and a newtype.** It converts Guild and Member (cheap, lossy fields documented) and keeps a full `Message` newtype, because `CachedMessage` drops author and mention users. `MEMBER` implies `USER` internally.
- **Own resource bitset.** It keeps twilight-cache-inmemory out of the public API (AD-2). Story 12.5's `DiscordCacheConfig` builds on it.
- **Trivial `NoLocaleOverlay` and `AllowAllGuilds`.** They complete "each port has a default" (FR-38).
- **No tokio in noctis-ports tests.** Store futures are ready, and std threads give real parallelism.

A weight above `u32::MAX` is clamped by the weigher, so the accounting is approximate only for single entries over 4 GiB.

```rust
self.cache.entry_by_ref(key).and_compute_with(|cur| match cur.filter(|e| e.value().is_live(now)) {
    Some(e) => { taken = Some(e.into_value().value); Op::Remove }
    None => Op::Nop,
});
```

## Verification

**Commands** (repo root; CI is paused, so these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0, including `memory_stores`, `discord_cache`, `purity`, the facade doctests and `template_adapter`.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0 (moka, crossbeam, uuid, dashmap licenses are MIT/Apache; duplicate versions only warn).
- `cargo +1.91 check --release --workspace --all-targets` -- expected: exit 0.
- `cargo tree --release -p noctis-ports -e normal --depth 1` -- expected: exactly the 7 deps.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot` and `-- skill-check` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.

## Auto Run Result

- **Summary:** noctis-ports now ships bounded in-memory defaults for every port: moka-backed byte-budgeted LRU stores (`max_bytes` weigher with `ENTRY_OVERHEAD`, optional `max_ttl` clamp computed by noctis, deadlines in the value, all mutations through `and_compute_with`), oversized writes rejected as typed `InvalidValue`, `usage()` running pending maintenance, a `MemoryDiscordCache` over twilight-cache-inmemory gated by noctis's own `DiscordCacheResources` bitset (Guild/Member converted with documented lossy fields, `Message` newtype, MEMBER implies USER internally), plus trivial `NoLocaleOverlay` and `AllowAllGuilds`. The facade re-exports them, and the project template, its AGENTS.md and the noctis skill (adapters.md §1.1 defaults table) are updated.
- **Files changed:**
  - `crates/noctis-ports/src/memory/{mod,byte_store,stores,discord,trivial}.rs`: the byte-budgeted store core, the per-port stores, the Discord cache and the trivial defaults.
  - `crates/noctis-ports/src/lib.rs`, `crates/noctis-ports/Cargo.toml`, `Cargo.toml`, `Cargo.lock`: module wiring and the moka/twilight-cache-inmemory deps.
  - `crates/noctis-ports/tests/{memory_stores,discord_cache,purity}.rs` and `tests/fixtures/gateway/*.json`: contract, budget, TTL, concurrency and gateway-fixture tests.
  - `crates/noctis/src/lib.rs`, `crates/noctis/Cargo.toml`, `crates/noctis/tests/template_adapter.rs`: facade re-exports and the template reconciliation test.
  - `examples/project-template/{AGENTS.md,src/adapters/bounded_cache.rs}` and `.claude/skills/noctis/{SKILL.md,references/adapters.md}`: template and usage skill.
  - `xtask/public-api.txt`: API snapshot; `.memlog.md`: design decisions.
- **Review findings (36: high 0, medium 5, low 14, false 17):**
  - Patched (6; at entry verdict medium 2, low 4, plus 1 medium grouped into a patched row): `resources()` doc wording; widened TTL margins in two timing tests; new `full_budget_admits_the_newest_write`; new `expired_entries_leave_usage`; full-struct Guild/Member equality against the GUILD_CREATE payload (absorbs the blind discord_cache coverage row).
  - Deferred (2 medium, one root cause): twilight never purges `channel_messages` on channel/guild delete, so message memory grows with every channel seen; deferred to Story 12.5 (`DiscordCacheConfig`).
  - Rejected (28): u32 weigher clamp for >4 GiB entries (approximate by design, 3 rows); ENTRY_OVERHEAD understates resident memory (spec-fixed constant); synchronous maintenance in `usage()` (spec-mandated); module state evictable from the LRU (documented "defaults are caches"); non-atomic `member()` lookup (transient, guards forbidden); `all()` literal and raw-bit Debug (tested, cosmetic); defaults table not drift-checked (rare change); duplicate trait impls in public-api.txt (generator behaviour, stable); transient resident overshoot before moka housekeeping (invisible via `usage()`); and 17 `false` rows that the spec already settles (usage within budget, version semantics, `put_if_absent` size check order, zero-TTL dead entries, `max_bytes` below overhead, MEMBER implies USER, App wiring belongs to Stories 7.x-12.5, byte-only budget, `max_ttl` none, threads instead of tasks, lossy Guild/Member, synthetic fixtures, facade-based template check, [planned] relabels and Cargo entries).
- **Follow-up review recommended: true.** First pass patched 2 medium entries (0 high). Named risk: the new Guild/Member full-struct equality test and the LRU full-budget admission test were written during review and have not been independently re-reviewed; if the test's "lossy fields cleared" list is wrong, it could mask a conversion bug, and the admission test relies on moka eviction timing.
- **Verification:** local gates via the scratchpad `gates.sh` all green (`cargo fmt --check`, `cargo clippy --release --workspace --all-targets --all-features -D warnings`, `cargo test --release --workspace`, `cargo hack --each-feature`, `cargo deny check`, MSRV 1.91 check, `noctis-ports` dep-count check, xtask deps-check/rules-catalog/api-snapshot/skill-check, build-config check, no `target/debug`). CI is paused by the maintainer, so the local gates are the whole check.
- **Residual risks:** unbounded per-channel message growth in `MemoryDiscordCache` until Story 12.5; byte accounting is approximate (fixed per-entry overhead, clamp above 4 GiB, transient overshoot before moka maintenance); timing-based TTL tests may still flake on a heavily loaded runner.
