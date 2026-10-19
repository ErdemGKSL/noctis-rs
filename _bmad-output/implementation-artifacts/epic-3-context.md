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

- Port traits are `Send + Sync + 'static`, dyn-compatible; async methods return `futures_util::future::BoxFuture<'_, Result<T, PortError>>`. The App holds `Arc<dyn Port>`.
- Shared op vocabulary: `get`, `put(ttl)`, `put_if_absent(ttl) -> bool`, `take` (atomic get+delete), `delete`, `touch`, `incr_with_ttl(key, by, ttl) -> u64`. Subsets: `RefStore` = put_if_absent, get, take, delete, touch; `RateLimitStore` = incr_with_ttl, get, put; `CacheStore` = get, put, put_if_absent, delete, incr_with_ttl; `ModuleStateStore` adds `version() -> u64` (monotonic, polled by core).
- `IdentifyQueue` is a coordination port: `acquire(bucket) -> Lease`.
- `DiscordCache` is typed, not a byte store: `update(&Event)` plus owned getters (`guild`, `channel`, `role`, `member`, `user`, `message`, per-guild lists) returning `Option` of twilight-model values; no references or guards cross the port. Memory default wraps `twilight-cache-inmemory` restricted to configured resource types and twilight's per-channel message cap. Not namespaced; each App owns its instance.
- `CacheStore` is the general KV for bots/Modules (later reached via `ctx.kv()`); it never backs the Discord cache.
- Typed values: postcard with a leading schema byte; schema mismatch reads as a miss.
- Key namespacing (`<app>:<port>:`, `_shared:<port>:`) is done only by core's `Namespaced<P>`/`Shared<P>` wrappers; adapters store keys verbatim and never prefix.
- Render contract lives in `noctis-ports` with no dependency on takumi or `noctis-image`: `Template` trait (`ID`, `VERSION`, `SIZE`, `FORMAT`, `ANIMATION`, `view() -> ErasedView`), `RenderJob` (postcard `data: Bytes`), `Format`, `AnimationSpec`, `Frame` (`Frame::STILL`), `TemplateDescriptor`, `RenderOutput` with `OutputFormat::extension()`, and `trait Renderer` (`render`, `register`).
- Exempt from ports: twilight-http rate-limiter state, per-connection gateway state, request-scoped in-flight coordination.
- `noctis-ports` is layer 1 (wire and ports): no I/O beyond memory defaults.

## Cross-Story Dependencies

- 3.2 and 3.3 implement the traits defined in 3.1.
- 3.2 reconciles the Project Template's `adapters/bounded_cache.rs` custom `CacheStore` with the real signature.
- 3.3: buckets are `shard_id % max_concurrency`; each bucket grants at most one lease per 5 s, different buckets run concurrently (test with paused tokio clock). Docs must state a Cluster implementation must serialize per bucket across processes. Consumed by the gateway (Epic 5) and clusters (Epic 12).
- Render contract types are built on exactly by Stories 10.2, 10.3, 10.6; conformance suites for ports come later in `noctis-testkit` and the memory defaults must pass them.
