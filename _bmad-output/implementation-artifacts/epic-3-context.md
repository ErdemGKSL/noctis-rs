# Epic 3 Context: Pluggable storage ports

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Every storage-like concern in noctis (anything that stores, caches, queues or resumes data that must outlive one call) becomes a small, dyn-compatible port trait in `noctis-ports`, each with a bounded in-memory default in `noctis_ports::memory`. Bot authors can then swap in Redis, SQL or any other backend by handing an `Arc<dyn Port>` to the App, with no change to handler code. This lets later epics (Refs, sessions, modules, rate limits, rendering, localization, clusters) build on one uniform, swappable storage layer, and keeps framework memory bounded.

## Stories

- Story 3.1: Port traits and the shared op vocabulary
- Story 3.2: Bounded in-memory store defaults
- Story 3.3: In-memory IdentifyQueue with identify buckets

## Requirements & Constraints

- v1 ports: `RefStore`, `SessionStore`, `IdentifyQueue`, `ModuleStateStore`, `RateLimitStore`, `CacheStore`, `DiscordCache`, `RenderCache`, `AssetStore`, `LocaleSource`, `GuildAccessList`. `Renderer` is a driven port in the same crate, not a store. A new concern adds a new port; never extend an unrelated one.
- Each port has an in-memory default; replacing it needs no handler change.
- Defaults are bounded by configurable capacity (entries or bytes, moka weighers by bytes) and TTL, with documented defaults. No unbounded map keyed by guild, user, channel, message or interaction.
- Discord cache memory is reported separately from framework memory (framework target: about 1 KiB per idle guild, excluding the Discord cache).
- Security: ports accept only opaque keys and `Bytes` values; no method takes a token, payload key or other secret.
- Atomicity: under concurrent callers exactly one `put_if_absent`/`take` wins, no increment is lost, and `incr_with_ttl` sets TTL only on create. Expired entries read as misses; `touch` extends; `delete` removes.
- Public traits are re-exported via `noctis::ports`; the Project Template's Usage Skill gains an Adapters reference (each trait and op semantics, plus how to write a custom Adapter); `api-snapshot` and `skill-check` must pass.

## Technical Decisions

- Port traits are `Send + Sync + 'static`, dyn-compatible; async methods return `BoxFuture<'_, Result<T, PortError>>` (noctis-ports' own alias, identical to futures_util's; no futures-util dependency). Deps: bytes, postcard (`alloc` only), serde, thiserror, twilight-model; a purity test forbids I/O and secret-named parameters. The App holds `Arc<dyn Port>`.
- Shared op vocabulary: `get`, `put(ttl)`, `put_if_absent(ttl) -> bool`, `take` (atomic get+delete), `delete`, `touch`, `incr_with_ttl(key, by, ttl) -> u64`. Subsets: `RefStore` = put_if_absent, get, take, delete, touch; `RateLimitStore` = incr_with_ttl, get, put; `CacheStore` = get, put, put_if_absent, delete, incr_with_ttl; `ModuleStateStore` adds `version() -> u64` (monotonic, polled by core).
- `IdentifyQueue` is a coordination port: `acquire(bucket) -> Lease`.
- `DiscordCache` is typed, not a byte store: `update(&Event)` plus owned getters (`guild`, `channel`, `role`, `member`, `user`, `message`, per-guild lists) returning `Option` of twilight-model values; no references or guards cross the port. Memory default wraps `twilight-cache-inmemory` restricted to configured resource types and twilight's per-channel message cap. Not namespaced; each App owns its instance.
- `CacheStore` is the general KV for bots/Modules (later reached via `ctx.kv()`); it never backs the Discord cache.
- Typed values: postcard with a leading schema byte; schema mismatch reads as a miss.
- Key namespacing (`<app>:<port>:`, `_shared:<port>:`) is done only by core's `Namespaced<P>`/`Shared<P>` wrappers; adapters store keys verbatim and never prefix.
- Render contract lives in `noctis-ports` with no dependency on takumi or `noctis-image`: `Template` trait (`ID`, `VERSION`, `SIZE`, `FORMAT`, `ANIMATION`, `view() -> ErasedView`), `RenderJob` (postcard `data: Bytes`), `Format`, `AnimationSpec`, `Frame` (`Frame::STILL`), `TemplateDescriptor`, `RenderOutput` with `OutputFormat::extension()`, and `trait Renderer` (`render`, `register`).
- Exempt from ports: twilight-http rate-limiter state, per-connection gateway state, request-scoped in-flight coordination.
- Op contracts: `touch(key, ttl) -> bool` never creates; `incr_with_ttl` stores an 8-byte big-endian counter saturating at `u64::MAX`. `ModuleStateStore` = get, put, `version()` (0 when empty, grows on every put); `SessionStore` = get, put, delete; `RenderCache`/`AssetStore` = get, put. `IdentifyQueue::acquire(bucket: u32) -> Lease` (non-Clone, `Lease::new`, `bucket()`). `LocaleSource::load(code) -> Vec<(key, value)>`. `GuildAccessList::allows(guild) -> bool` is synchronous.
- `DiscordCache` per-guild lists return ids (`channel_ids`, `role_ids`, `member_ids`); getters never return twilight-cache-inmemory `Cached*` types, so 3.2 converts them or uses custom cacheable models.
- `PortError` is non_exhaustive (`Backend`, `InvalidValue{key, why}`, `Unavailable{why}`, `Value(ValueError)`) with `backend`/`invalid_value`/`unavailable` constructors. Typed values via `encode_value(schema, &T)` / `decode_value(schema, raw) -> Option<T>`: schema mismatch = miss; empty, malformed or trailing bytes = `ValueError`; postcard errors are not exposed.
- Render types live in `noctis_ports::render`: closed enums, non-zero validated `Size`, `OutputFormat::extension()` (APNG -> "png", JPEG -> "jpg"), `EncodeSettings` as `settings_used`, non_exhaustive `RenderError`, no serde; template data is plain postcard (`RenderJob::of`, `decode_template`).
- Facade exposes `noctis::ports` (flat) and `noctis::bytes::Bytes`.
- Memory byte stores (`noctis_ports::memory`: Ref, Session, ModuleState, RateLimit, Cache, RenderCache, AssetStore) wrap one private moka 0.12 `sync::Cache` (feature `sync` only, default features off in the workspace entry), LRU, weigher `key+value+64`, budgeted by `MemoryLimits { max_bytes, max_ttl }` with documented `DEFAULT_LIMITS` and `usage()`. Deadlines live in the value (max_ttl clamps); every mutation goes through `and_compute_with`; a write heavier than the budget is `PortError::InvalidValue`. `NoLocaleOverlay` and `AllowAllGuilds` are the trivial defaults.
- `MemoryDiscordCache` wraps twilight-cache-inmemory 0.17.1 (default features off) via private `CacheableModels` (full `Message` newtype; `CachedGuild`/`CachedMember` converted to owned types, missing fields None/false); resources chosen with noctis's own `DiscordCacheResources` bitset (no twilight type exposed); `MEMBER` implies `USER`; message cap 0 disables messages; id lists sorted. The template's `bounded_cache.rs` is compiled by a facade integration test via `#[path]`.
- `noctis-ports` is layer 1 (wire and ports): no I/O beyond memory defaults.

## Cross-Story Dependencies

- 3.2 and 3.3 implement the traits defined in 3.1.
- 3.2 reconciles the Project Template's `adapters/bounded_cache.rs` custom `CacheStore` with the real signature.
- 3.3: buckets are `shard_id % max_concurrency`; each bucket grants at most one lease per 5 s, different buckets run concurrently (test with paused tokio clock). Docs must state a Cluster implementation must serialize per bucket across processes. Consumed by the gateway (Epic 5) and clusters (Epic 12).
- Render contract types are built on exactly by Stories 10.2, 10.3, 10.6; conformance suites for ports come later in `noctis-testkit` and the memory defaults must pass them.
