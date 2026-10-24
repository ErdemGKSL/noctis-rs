# Epic 12 Context: Clusters and scale

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author scale from one process to a Cluster by configuration alone: each process runs a Shard Range, identifies are serialized per bucket across the whole Cluster, sessions resume across restarts and graceful shutdowns, all Discord HTTP traffic can go through a shared rate-limit proxy, and the Discord resource cache is configurable (resources, backend, or off). Framework memory per idle guild is measured continuously. Handler code never changes when moving from one process to many.

## Stories

- Story 12.1: Shard Ranges and Cluster-wide identify
- Story 12.2: Session resume through the SessionStore
- Story 12.3: Resumable shutdown
- Story 12.4: Shared HTTP rate limiting through a proxy
- Story 12.5: Discord cache wiring, `ctx.cache()`, configuration and minimal intents
- Story 12.6: Memory-per-guild benchmark

## Requirements & Constraints

- Shard Range is half-open `start..end`, set via `AppBuilder::shard_range(Range<u32>)` or `NOCTIS_SHARD_RANGE=32..64` (no `NOCTIS_SHARDS`); it requires a fixed `shard_total` and is validated at `build()` (empty, without total, out of bounds are typed violations). Process runs only its shards; readiness and shard-status gauges cover only the range. Aligned iff `max_concurrency == 1` or start and end (or end == total) are multiples of it; misaligned only warns. `IdentifyQueue` is implemented for `Arc<P>` so in-process Apps can share one queue.
- Identify concurrency: one identify per bucket per 5 s, held across processes sharing an `IdentifyQueue` (test with two in-process Apps, paused clock).
- Exactly one process with `publisher = true` runs Publish on start; Publish never runs on Shard ready.
- Resume within Discord's window must not consume identify budget; a rejected stored session falls back to fresh identify and clears the stale entry, without operator action.
- Graceful shutdown defaults to resumable; `--no-resume` (Story 6.9 CLI parser) closes fresh and clears stored sessions. Events dropped under back-pressure are not replayed (at-most-once, documented).
- Proxy via builder or `NOCTIS_HTTP_PROXY`; no request may bypass it (raw REST, webhooks, interaction callbacks included). 429 with `X-RateLimit-Scope: shared` is a typed, counted error, never a hard-coded limit; 401/403/429 counts feed an invalid-request metric. Cluster without proxy logs a warning naming the global-limit risk. Request timeout applies to every request, default 10 s.
- Default cache holds only resources needed by enabled features; default intents are the minimum for registered handlers and cache resources (`Intents::GUILDS` for interaction-only). A missing intent is a typed `App::build` error naming resource and intent (GW-05). Disabled cache getters return typed `CacheDisabled`.
- Memory budget: initial goal at most 1 KiB per idle guild excluding the Discord cache; bench over 10k idle guilds reports framework, Discord cache and Renderer cache bytes separately against `budgets.toml`; reports on every push to `main` (gating comes in Story 14.3).
- Every public API change updates the Usage Skill (app-and-sharding, proxy mode, `ctx.cache()`), the Project Template (`.env.example`, README, cache reads), and passes `skill-check`.

## Technical Decisions

- `noctis-gateway` alone calls `SessionStore` (passed via `GatewayOptions::session_store`; core never calls it; deps-check rule `session-store` flags op calls elsewhere, `tests.rs` exempt). Key `shard:{id}:{total}` (namespaced), value `encode_value(1, StoredSession { session_id, sequence, resume_url })`, no TTL; stored seq is the last received.
- Each shard loads its entry before connecting (`get` bounded 5 s; error/timeout identifies), validates it (schema 1, session_id 1..=256 B, resume_url 1..=2048 B plus `check_gateway_url`, since twilight panics on bad URLs) and resumes via `ConfigBuilder::session`/`resume_url`. Unreadable/invalid entry, op 9 `d:false`, or READY while a stored session is pending deletes it once, inline before the next `shard.next()`. RESUMED counts as startup-ready.
- Write ships in 12.2: on `CloseMode::Resumable` for an identified shard, `put` after the close frame is observed, within the same `close_timeout`; `Fresh` deletes; no write on ack timeout, READY, reconnect or crash. 12.3 adds `--no-resume`, start-time clearing and the shutdown-order proof.
- Gateway reports `ShardEvent`s on a channel; only core turns them into Lifecycle Hooks. Core owns shutdown order: `ShutdownStarted` -> `gateway.close_all(Resumable | Fresh)` -> drain lanes until empty or grace ends (Responder and HTTP live) -> await `gateway.closed()` -> drop ports.
- Startup reads `GET /gateway/bot`; identify uses `IdentifyQueue` buckets from `max_concurrency` (never `InMemoryQueue::default()`); boot refuses when `session_start_limit.remaining` can't cover the shards.
- Ports: `Send + Sync + 'static`, dyn-compatible, `BoxFuture<'_, Result<T, PortError>>`, held as `Arc<dyn Port>`. `IdentifyQueue::acquire(bucket) -> Lease`. `DiscordCache` is typed: `update(&Event)` plus owned getters returning twilight-model values; no reference or guard crosses the port (no guard holdable across `.await`; enforce via lint or compile-fail fixture). Not wrapped in `Namespaced<P>`; a shared backend keys by `AppId`, each process writes only its own shards' guilds.
- Core feeds the cache before handlers see the event; those event types join the gateway event filter. Builder: `.discord_cache(DiscordCacheConfig::...)` / `none()`, `.discord_cache_backend(impl DiscordCache)`.
- HTTP: core builds the App's single `Arc<twilight_http::Client>` (no other crate builds one), always pointed at the loopback Discord edge; a configured Cluster proxy becomes the edge's upstream (they compose), with `ratelimiter(None)`.
- Memory: per-guild framework state is a fixed-size record (Module bitset, locale); no unbounded map keyed by guild/user/channel/message/interaction. Discord cache bytes = dhat live-heap difference between default-config and `none()` runs; framework bytes = `none()` run minus empty-App baseline; Renderer bytes from the memory `RenderCache` weigher. Bench lives in `noctis-core/benches`.

## Cross-Story Dependencies

- Builds on Stories 3.1/3.2 (`DiscordCache`, `MemoryDiscordCache`), 5.6 (event filter), 5.9 (Discord edge), 6.9 (CLI parser), and `FakeGateway`/fake HTTP from the testkit.
- 12.3 depends on 12.2's SessionStore persistence; 12.6 depends on 12.5's cache configuration (`none()`).
- 12.6 gating is enabled later in Story 14.3.
