# Adapters (ports)

Covers: every port, its in-memory default, how to swap one, the typed `ctx.cache()`
API, namespacing and sharing across Apps, writing your own Adapter, conformance tests.
FR-38..40, FR-71; AD-1, AD-14. Rule: anything that stores, caches, queues or resumes
data that must outlive one call goes through a port, never a `static` map.

## 1. The ports (`noctis::ports`)

| Port | Stores | Ops it uses | Default | Swap with |
| --- | --- | --- | --- | --- |
| `RefStore` | Payloads behind Refs (by_ref/spill Routes) | `put_if_absent`, `get`, `take`, `delete`, `touch` | bounded memory, TTL | `.ref_store(..)` |
| `SessionStore` | shard `(session_id, seq, resume_url)` for resume | `get`, `put`, `delete` | memory (resume only within one process life) | `.session_store(..)` |
| `IdentifyQueue` | identify coordination per bucket (Cluster-wide) | `acquire(bucket) -> Lease` | in-process queue with real `max_concurrency` | `.identify_queue(..)` |
| `ModuleStateStore` | Module enabled/disabled, global and per guild | `get`, `put`, `version() -> u64` (polled, default 5 s) | memory | `.module_state_store(..)` |
| `RateLimitStore` | your declared rate-limit counters | `incr_with_ttl`, `get`, `put` | memory | `.rate_limit_store(..)` |
| `CacheStore` | shared KV with TTL for bots and Modules (`ctx.cache()`); also the backing store of the Discord resource cache | `get`, `put`, `put_if_absent`, `delete`, `incr_with_ttl` | bounded memory | `.cache_store(..)` |
| `RenderCache` | encoded images by render key | `get`, `put` | byte-bounded memory | `.render_cache(..)` |
| `AssetStore` | fetched remote images (avatars, icons, emoji) | `get`, `put` | byte-bounded memory | `.asset_store(..)` |
| `LocaleSource` | runtime overlays of content strings | `load(locale)` | none | `.locale_source(..)` |
| `GuildAccessList` | per-App guild allow/deny lists (white-label) | `allows(guild)` | allow all | `.guild_access(..)` |
| `Renderer` (driven port, not a store) | turns a `RenderJob` into `RenderOutput` | `render(job)` | `noctis-image` local pool | `.renderer(..)` |

All defaults are bounded by entries or bytes with documented capacities and TTLs;
configure them with `noctis::ports::memory::MemoryRefStore::with_capacity(..)` etc.
[planned].

The Discord resource cache (guilds, roles, channels, members) is read through
`ctx.discord()`; choose which resources are cached with
`.discord_cache(DiscordCacheConfig::…)`. Reads return owned values; no guard can be held
across `.await`. Size your `CacheStore` with the Discord cache in mind when you swap it.

## 2. Port shape (what every Adapter looks like)

```rust
pub trait CacheStore: Send + Sync + 'static {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    fn put<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>) -> BoxFuture<'a, Result<(), PortError>>;
    fn put_if_absent<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>) -> BoxFuture<'a, Result<bool, PortError>>;
    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>>;
    fn incr_with_ttl<'a>(&'a self, key: &'a str, by: u64, ttl: Option<Duration>) -> BoxFuture<'a, Result<u64, PortError>>;
}
```

Contract [planned exact signatures; semantics fixed by AD-14]:
- `Send + Sync + 'static`, dyn-compatible; the App holds `Arc<dyn Port>`.
- Methods return `noctis::ports::BoxFuture` (re-export of `futures_util`'s).
- **Keys are opaque and stored verbatim.** Core's `Namespaced` wrapper already prefixed
  them with `<app>:<port>:`; never add your own prefix.
- **Values are opaque `Bytes`** (`noctis::bytes::Bytes`). Typed values are postcard with
  a leading schema byte; a schema mismatch reads as a miss. Counters are 8-byte
  big-endian `u64`.
- `ttl = None` means no expiry (the entry may still be evicted by capacity).
- `put_if_absent` and `take` must have **exactly one winner** under concurrency;
  `incr_with_ttl` is atomic, sets TTL only on create, and never loses increments.
- An expired entry behaves exactly like a missing one.
- Adapters never receive secrets (the token and payload key stay in core).
- Errors: `PortError::backend(err)`, `PortError::invalid_value(key, why)`,
  `PortError::unavailable(..)`; core turns them into handler errors / Error Sink events.

## 3. Using ports from handlers

You never call port traits directly; contexts give typed, namespaced handles:

```rust
let cache = ctx.cache();                                         // CacheStore
let cfg: Option<GuildConfig> = cache.get(&config_key(guild)).await?;   // T: DeserializeOwned
cache.put(&config_key(guild), &cfg, None).await?;                // T: Serialize
let first = cache.put_if_absent(&cooldown, &(), Some(Duration::from_secs(60))).await?;
let n: u64 = cache.incr(&xp_key(guild, user), 1, None).await?;
let n: u64 = cache.counter(&xp_key(guild, user)).await?;         // 0 when missing
cache.delete(&key).await?;

ctx.modules().set_enabled("profile", ModuleScope::Guild(guild), false).await?;  // ModuleStateStore
let on = ctx.modules().is_enabled("profile", Some(guild));       // cached snapshot, sync
```

Rules of thumb: build keys with a short type prefix (`"profile:{guild}:{user}"`);
`CacheStore` is a cache (it may evict): durable business data belongs in your own
database, injected as `State<Db>` or wrapped in your own port.

## 4. Implementing your own Adapter

The template's `src/adapters/bounded_cache.rs` is a complete `CacheStore`. The
essentials:

```rust
use std::future::ready;
use noctis::bytes::Bytes;
use noctis::ports::{BoxFuture, CacheStore, PortError};

pub struct BoundedCacheStore { capacity: usize, entries: Mutex<HashMap<String, Entry>> }

impl CacheStore for BoundedCacheStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        let now = Instant::now();
        let value = self.lock().get(key).filter(|e| e.is_live(now)).map(|e| e.value.clone());
        Box::pin(ready(Ok(value)))
    }
    // put / put_if_absent / delete / incr_with_ttl: one lock each, so they are atomic.
}
```

A networked Adapter (Redis sketch):

```rust
pub struct RedisCache { pool: deadpool_redis::Pool }

impl CacheStore for RedisCache {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        Box::pin(async move {
            let mut c = self.pool.get().await.map_err(PortError::backend)?;
            let v: Option<Vec<u8>> = redis::cmd("GET").arg(key).query_async(&mut c).await.map_err(PortError::backend)?;
            Ok(v.map(Bytes::from))
        })
    }
    fn put_if_absent<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>)
        -> BoxFuture<'a, Result<bool, PortError>>
    {
        Box::pin(async move {
            let mut c = self.pool.get().await.map_err(PortError::backend)?;
            let mut cmd = redis::cmd("SET");
            cmd.arg(key).arg(value.as_ref()).arg("NX");
            if let Some(ttl) = ttl { cmd.arg("PX").arg(ttl.as_millis() as u64); }
            let ok: Option<String> = cmd.query_async(&mut c).await.map_err(PortError::backend)?;
            Ok(ok.is_some())
        })
    }
    // incr_with_ttl: a Lua script doing INCRBY then PEXPIRE only when the result == by.
    // …
}
```

Wire it: `App::from_env()?.cache_store(RedisCache::new(pool))`. Handlers don't change.

Checklist for any Adapter: bounded (or backed by bounded storage), atomic ops really
atomic, TTL honoured on read, verbatim keys, no secrets, no blocking calls in async
(use async clients or `spawn_blocking`), passes conformance.

## 5. Conformance tests (FR-39)

```rust
use noctis::testkit::conformance;

#[tokio::test]
async fn bounded_cache_store_conforms() {
    conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}
```

One suite per port: `ref_store`, `session_store`, `identify_queue`,
`module_state_store`, `rate_limit_store`, `cache_store`, `render_cache`, `asset_store`.
They test TTL, overwrite, delete, missing keys, concurrent callers (exactly one
`take`/`put_if_absent` winner, no lost increments) and App namespacing. The factory is
called per case, so return a fresh, empty store (for Redis: a fresh key prefix or DB).

## 6. Namespacing and sharing (FR-71)

Two Apps sharing one Adapter instance never see each other's keys (`<app>:<port>:`).
Opt one port into cross-App sharing with `Shared::new(adapter)` → prefix
`_shared:<port>:` (typical: `CacheStore` for data both white-label Apps read).

## 7. Clusters

In a Cluster every process must share: `RefStore` (a Ref minted by process A is clicked
on process B), `RateLimitStore`, `ModuleStateStore`, `IdentifyQueue`, and usually
`CacheStore`/`RenderCache`/`AssetStore`. `SessionStore` must persist across restarts to
resume. Official Redis/SQL adapters are planned for v1.x; until then write them as
above and run the conformance suites.
