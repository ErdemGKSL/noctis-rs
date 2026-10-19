# Adapters (ports)

Covers: every port, its in-memory default, how to swap one, the typed `ctx.kv()`
key-value API, the Discord resource cache (`ctx.cache()`), namespacing and sharing
across Apps, writing your own Adapter, conformance tests, the render contract.
FR-38..40, FR-71; AD-1, AD-14. Rule: anything that stores, caches, queues or resumes
data that must outlive one call goes through a port, never a `static` map.

## 1. The ports (`noctis::ports`)

| Port | Stores | Ops it uses | Default | Swap with [planned] |
| --- | --- | --- | --- | --- |
| `RefStore` | Payloads behind Refs (by_ref/spill Routes) | `put_if_absent`, `get`, `take`, `delete`, `touch` | bounded memory, TTL | `.ref_store(..)` |
| `SessionStore` | shard `(session_id, seq, resume_url)` for resume | `get`, `put`, `delete` | memory (resume only within one process life) | `.session_store(..)` |
| `IdentifyQueue` | identify coordination per bucket (Cluster-wide) | `acquire(bucket) -> Lease` | in-process queue with real `max_concurrency` | `.identify_queue(..)` |
| `ModuleStateStore` | Module enabled/disabled, global and per guild | `get`, `put`, `version() -> u64` (polled, default 5 s) | memory | `.module_state_store(..)` |
| `RateLimitStore` | your declared rate-limit counters | `incr_with_ttl`, `get`, `put` | memory | `.rate_limit_store(..)` |
| `CacheStore` | general key-value data with TTL for bots and Modules (`ctx.kv()`) | `get`, `put`, `put_if_absent`, `delete`, `incr_with_ttl` | bounded memory | `.cache_store(..)` |
| `DiscordCache` | Discord resources: guilds, channels, roles, members, users, messages (`ctx.cache()`) | `update(&Event)`, typed owned getters | wraps `twilight-cache-inmemory`, only the resources the App needs | `.discord_cache_backend(..)` |
| `RenderCache` | encoded images by render key | `get`, `put` | byte-bounded memory | `.render_cache(..)` |
| `AssetStore` | fetched remote images (avatars, icons, emoji) | `get`, `put` | byte-bounded memory | `.asset_store(..)` |
| `LocaleSource` | runtime overlays of content strings | `load(locale)` | none | `.locale_source(..)` |
| `GuildAccessList` | per-App guild allow/deny lists (white-label) | `allows(guild)` | allow all | `.guild_access(..)` |
| `Renderer` (driven port, not a store) | turns a `RenderJob` into `RenderOutput` | `render(job)` | `noctis-image` local pool | `.renderer(..)` |

### 1.1 The in-memory defaults (`noctis::ports::memory`)

Every port except `IdentifyQueue` (its default is [planned]) has a bounded in-memory
default in `noctis::ports::memory`. An App that swaps nothing runs on these.

| Default | Port | `max_bytes` | `max_ttl` |
| --- | --- | --- | --- |
| `MemoryRefStore` | `RefStore` | 32 MiB | 7 days |
| `MemorySessionStore` | `SessionStore` | 1 MiB | none |
| `MemoryModuleStateStore` | `ModuleStateStore` | 16 MiB | none |
| `MemoryRateLimitStore` | `RateLimitStore` | 16 MiB | 1 day |
| `MemoryCacheStore` | `CacheStore` | 32 MiB | none |
| `MemoryRenderCache` | `RenderCache` | 64 MiB | 1 day |
| `MemoryAssetStore` | `AssetStore` | 32 MiB | 1 day |
| `MemoryDiscordCache` | `DiscordCache` | the chosen resources + 100 messages per channel | — |
| `NoLocaleOverlay` | `LocaleSource` | `load` returns `Ok(vec![])` (no overlay) | — |
| `AllowAllGuilds` | `GuildAccessList` | `allows` returns `true` | — |

Each byte store has the same inherent API:

```rust
use std::time::Duration;
use noctis::ports::memory::{MemoryCacheStore, MemoryLimits, MemoryUsage};

MemoryCacheStore::DEFAULT_LIMITS;                 // the row above, as MemoryLimits
let store = MemoryCacheStore::new();              // = with_limits(DEFAULT_LIMITS); also Default
const LIMITS: MemoryLimits = MemoryLimits::new(8 * 1024 * 1024, Some(Duration::from_secs(3600)));
let store = MemoryCacheStore::with_limits(LIMITS);
store.limits().max_bytes();                       // 8 MiB
store.limits().max_ttl();                         // Some(1h); None = uncapped
let MemoryUsage { entries, bytes } = store.usage();  // current count and weight
```

- `MemoryLimits` (`Copy`, `Eq`): `const fn new(max_bytes: u64, max_ttl: Option<Duration>)`
  panics on `max_bytes == 0` or `Some(Duration::ZERO)` (a compile error in a `const`);
  `MemoryLimits::try_new(..) -> Option<MemoryLimits>` returns `None` instead; getters
  `max_bytes()` and `max_ttl()`.
- **Weight:** an entry weighs `key.len() + value.len() + MemoryLimits::ENTRY_OVERHEAD`
  (64) bytes, in UTF-8 bytes. `usage()` returns `MemoryUsage { entries, bytes }` (public
  `u64` fields) and never exceeds `max_bytes`.
- **Eviction:** when the budget is full, the least recently used entry is evicted. A
  single write heavier than `max_bytes` fails with `PortError::InvalidValue` and changes
  nothing.
- **TTL:** every TTL is clamped to `max_ttl`; with no TTL and a `max_ttl`, the entry
  expires after `max_ttl`. `incr_with_ttl` on a live counter keeps its expiry.
- `MemoryModuleStateStore`'s `version()` starts at 0 and grows by one on every
  successful `put`.
- `Debug` prints only the limits, never keys or values.
- **The defaults are caches:** capacity may evict any entry; swap a persistent adapter
  for durable data (and for anything a Cluster must share, see §7).

`MemoryDiscordCache` wraps twilight's in-memory cache, restricted to a resource set:

```rust
use noctis::ports::memory::{DiscordCacheResources as R, MemoryDiscordCache};

let cache = MemoryDiscordCache::new(R::GUILD | R::CHANNEL | R::ROLE);   // 100 messages/channel
let cache = MemoryDiscordCache::with_message_cap(R::all(), 50);        // cap 0 = no MESSAGE
cache.resources();     // the requested set, without MESSAGE when the cap is 0
cache.message_cap();   // MemoryDiscordCache::DEFAULT_MESSAGE_CAP = 100 for new()
```

- `DiscordCacheResources` (`Copy`, `Eq`, `Hash`, `Default` = empty): consts `GUILD`,
  `CHANNEL`, `ROLE`, `MEMBER`, `USER`, `MESSAGE`; const fns `empty()`, `all()`,
  `union(other)`, `contains(other)`, `is_empty()`; and `|`. `MEMBER` also caches each
  member's user internally, but `resources()` reports only what you asked for.
- Getters return owned clones. What is lossy: a `Guild` has empty `channels`, `roles`,
  `members`, `emojis`, `stickers`, `presences`, `voice_states`, `threads` and
  `stage_instances` (use `channel_ids`/`role_ids`/`member_ids`, sorted ascending) and
  `None` approximate counts; a `Member` has `avatar_decoration_data` and `banner` `None`
  and `deaf`/`mute` `false` when unknown, and is `None` when its user is not cached.
  `Channel`, `Role`, `User` and `Message` are stored whole.

Wiring a default or your own Adapter into the App (`.cache_store(..)` etc. in the table
above) is [planned].

### 1.2 The Discord resource cache

The Discord resource cache is its own port, `DiscordCache`, separate from
`CacheStore`. Read it through `ctx.cache()` [planned] (`ctx.cache().guild(id).await?`,
`.roles(guild)`, `.member(guild, user)`, `.user(id)`, …); choose which resources are
cached with `.discord_cache(DiscordCacheConfig::…)` [planned] (`none()` disables it),
and replace its storage with `.discord_cache_backend(impl DiscordCache)` [planned].
Until then, `MemoryDiscordCache` (§1.1) is the default. Reads return owned values;
no guard can be held across `.await`. It is typed, not bytes: an out-of-process
implementation serializes the twilight-model values itself.

## 2. Port reference (`noctis::ports`)

Every port is `Send + Sync + 'static` and dyn-compatible (no generic methods, no
associated types or consts), so the App holds it as `Arc<dyn Port>`. Async methods
return `noctis::ports::BoxFuture<'a, Result<T, PortError>>`, defined as
`Pin<Box<dyn Future<Output = T> + Send + 'a>>`: the same type as
`futures_util::future::BoxFuture`, so `Box::pin(async move { … })` and
`futures_util`'s `.boxed()` both fit.

### 2.1 The op vocabulary

Keys are `&str`, values are `noctis::bytes::Bytes`, TTL is `Option<Duration>`
(`None` = no expiry; the entry may still be evicted by capacity).

| Op | Signature tail | Contract |
| --- | --- | --- |
| `get` | `(key) -> Option<Bytes>` | An expired or missing entry is `None`. |
| `put` | `(key, value: Bytes, ttl: Option<Duration>) -> ()` | Overwrites the value and resets the TTL. |
| `put_if_absent` | `(key, value, ttl) -> bool` | `true` only for the **one** caller that inserted; an expired entry counts as absent. |
| `take` | `(key) -> Option<Bytes>` | Atomic get + delete; exactly one concurrent caller gets `Some`. |
| `delete` | `(key) -> ()` | Removes the entry; a missing key is not an error. |
| `touch` | `(key, ttl: Duration) -> bool` | `true` if a live entry existed, whose expiry becomes now + ttl; a missing or expired entry gives `false` and nothing is created. |
| `incr_with_ttl` | `(key, by: u64, ttl: Option<Duration>) -> u64` | Atomic, no lost increments, returns the new value. TTL is set only when the call creates the entry (`by = 0` creates 0). Stored as an 8-byte big-endian `u64`; the add saturates at `u64::MAX`. A non-counter value gives `PortError::InvalidValue`. |

Every op borrows `&'a self` and `key: &'a str` and returns
`BoxFuture<'a, Result<_, PortError>>`.

### 2.2 Byte-store ports (exactly these methods)

```rust
pub trait RefStore: Send + Sync + 'static {         // put_if_absent, get, take, delete, touch
    fn put_if_absent<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>) -> BoxFuture<'a, Result<bool, PortError>>;
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>>;
    fn touch<'a>(&'a self, key: &'a str, ttl: Duration) -> BoxFuture<'a, Result<bool, PortError>>;
}
pub trait SessionStore: Send + Sync + 'static { /* get, put, delete */ }
pub trait ModuleStateStore: Send + Sync + 'static {
    /* get, put */
    fn version(&self) -> BoxFuture<'_, Result<u64, PortError>>;  // 0 when empty, grows on every put
}
pub trait RateLimitStore: Send + Sync + 'static { /* incr_with_ttl, get, put */ }
pub trait CacheStore: Send + Sync + 'static {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    fn put<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>) -> BoxFuture<'a, Result<(), PortError>>;
    fn put_if_absent<'a>(&'a self, key: &'a str, value: Bytes, ttl: Option<Duration>) -> BoxFuture<'a, Result<bool, PortError>>;
    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>>;
    fn incr_with_ttl<'a>(&'a self, key: &'a str, by: u64, ttl: Option<Duration>) -> BoxFuture<'a, Result<u64, PortError>>;
}
pub trait RenderCache: Send + Sync + 'static { /* get, put */ }
pub trait AssetStore: Send + Sync + 'static { /* get, put */ }
```

### 2.3 The other ports

```rust
pub trait IdentifyQueue: Send + Sync + 'static {
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>>;
}
// #[must_use], Debug + PartialEq + Eq, not Clone.
impl Lease { pub const fn new(bucket: u32) -> Lease; pub const fn bucket(&self) -> u32; }

pub trait LocaleSource: Send + Sync + 'static {
    // `locale` is a Discord code such as "en-US"; pairs are (message key, value);
    // an empty Vec means no overlay.
    fn load<'a>(&'a self, locale: &'a str) -> BoxFuture<'a, Result<Vec<(String, String)>, PortError>>;
}

pub trait GuildAccessList: Send + Sync + 'static {
    fn allows(&self, guild: Id<GuildMarker>) -> bool;   // sync
}

pub trait DiscordCache: Send + Sync + 'static {
    fn update<'a>(&'a self, event: &'a Event) -> BoxFuture<'a, Result<(), PortError>>;
    fn guild(&self, guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Guild>, PortError>>;
    fn channel(&self, channel: Id<ChannelMarker>) -> BoxFuture<'_, Result<Option<Channel>, PortError>>;
    fn role(&self, role: Id<RoleMarker>) -> BoxFuture<'_, Result<Option<Role>, PortError>>;
    fn member(&self, guild: Id<GuildMarker>, user: Id<UserMarker>) -> BoxFuture<'_, Result<Option<Member>, PortError>>;
    fn user(&self, user: Id<UserMarker>) -> BoxFuture<'_, Result<Option<User>, PortError>>;
    fn message(&self, message: Id<MessageMarker>) -> BoxFuture<'_, Result<Option<Message>, PortError>>;
    fn channel_ids(&self, guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Vec<Id<ChannelMarker>>>, PortError>>;
    fn role_ids(&self, guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Vec<Id<RoleMarker>>>, PortError>>;
    fn member_ids(&self, guild: Id<GuildMarker>) -> BoxFuture<'_, Result<Option<Vec<Id<UserMarker>>>, PortError>>;
}
```

- A `Lease` grants one identify in its bucket (`shard_id % max_concurrency`). A Cluster
  implementation of `IdentifyQueue` must serialize per bucket across processes.
- `GuildAccessList::allows` is **synchronous**: it runs on a pipeline stage that does
  no port I/O. Never block in it; a remote-backed implementation keeps a local snapshot
  and refreshes it in the background.
- `DiscordCache` uses only `twilight_model` types (`gateway::event::Event`,
  `guild::{Guild, Member, Role}`, `channel::{Channel, Message}`, `user::User`). Getters
  return owned values; `None` means not cached. `channel_ids`, `role_ids` and
  `member_ids` return ids only (`None` = guild not cached), so a large guild is never
  cloned per call.
- `Id`, the `*Marker` types, `Event` and the cached resource types come from the
  `twilight-model` crate; `noctis` does not re-export it yet [planned]. Until it does,
  add `twilight-model = "0.17"` to your bot's `Cargo.toml` (the same version noctis
  uses), or your `GuildAccessList`/`DiscordCache` impl will not type-check.

### 2.4 Errors

```rust
#[non_exhaustive]
pub enum PortError {
    Backend(Box<dyn Error + Send + Sync>),                // source() is the backend error
    InvalidValue { key: String, why: Cow<'static, str> },
    Unavailable { why: Cow<'static, str> },
    Value(ValueError),                                    // From<ValueError>
}
PortError::backend(err)            // impl Into<Box<dyn Error + Send + Sync>>; use as map_err(PortError::backend)
PortError::invalid_value(key, why) // why: impl Into<Cow<'static, str>>
PortError::unavailable(why)

#[non_exhaustive]  // Copy + Eq
pub enum ValueError { Serialize, Empty, Malformed, Trailing { extra: usize } }
```

Core turns a `PortError` into a handler error / Error Sink event.

### 2.5 Typed values

```rust
use noctis::ports::{decode_value, encode_value};

let raw: Bytes = encode_value(1, &cfg)?;                  // [schema] ++ postcard(cfg)
let cfg: Option<GuildConfig> = decode_value(1, &raw)?;    // other schema byte → Ok(None)
```

`encode_value<T: Serialize + ?Sized>(schema: u8, value: &T) -> Result<Bytes, ValueError>`
fails with `ValueError::Serialize` (e.g. a sequence of unknown length).
`decode_value<T: DeserializeOwned>(schema: u8, raw: &[u8]) -> Result<Option<T>, ValueError>`:
empty input is `ValueError::Empty`, a body that does not decode is
`ValueError::Malformed`, leftover bytes are `ValueError::Trailing { extra }`, and a
different schema byte is a miss (`Ok(None)`). Bump the schema byte when the type
changes incompatibly; old entries then read as misses.

### 2.6 Rules every Adapter follows

- **Keys are opaque and stored verbatim.** Core's `Namespaced` wrapper [planned]
  already prefixed them with `<app>:<port>:`; never add your own prefix.
- **Values are opaque `Bytes`** (`noctis::bytes::Bytes`). Counters are 8-byte
  big-endian `u64`.
- `put_if_absent` and `take` must have **exactly one winner** under concurrency;
  `incr_with_ttl` is atomic, sets TTL only on create, and never loses increments.
- An expired entry behaves exactly like a missing one.
- Adapters never receive secrets: no method takes the bot token, the payload key or
  any other credential.

## 2b. Render contract (`Renderer`)

The seam between core and the image renderer, in `noctis::ports` with no image
dependency. You normally write Templates with the image API (Epic 10, [planned]);
implement `Renderer` only to replace the renderer.

```rust
pub trait Template: Serialize + DeserializeOwned + Send + Sync + 'static {
    const ID: &'static str;
    const VERSION: u32;                       // bump when fields change
    const SIZE: Size;
    const FORMAT: Format;
    const ANIMATION: Option<AnimationSpec>;   // None = still image
    fn view() -> ErasedView;
}

pub trait Renderer: Send + Sync + 'static {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>>;
    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError>;
}
```

| Type | Shape |
| --- | --- |
| `ErasedView` | `ErasedView::new(view)`, `downcast_ref::<T>() -> Option<&T>`; `Clone` shares the value; `Debug` hides it |
| `TemplateDescriptor` | `{ id: &'static str, version: u32, view: ErasedView }`, `TemplateDescriptor::of::<T>()` |
| `RenderJob` | `{ template_id: &'static str, template_version: u32, data: Bytes, size: Size, format: Format, animation: Option<AnimationSpec> }`; `RenderJob::of(&template) -> Result<RenderJob, ValueError>`; `data` is plain postcard, no schema byte |
| `decode_template` | `decode_template::<T>(data: &[u8]) -> Result<T, ValueError>` (`Malformed`, `Trailing`) |
| `Size` | `Size::new(width, height)` (const; a zero side panics, a compile error in a `const`), `Size::try_new(..) -> Option<Size>`, `width()`, `height()` |
| `Format` | `Png`, `Jpeg { quality: u8 }`, `Webp { quality: u8 }`, `WebpLossless` |
| `AnimationSpec` | `{ duration: Duration, fps: u16, format: AnimatedFormat }`, const `AnimationSpec::new(duration, fps, format)` |
| `AnimatedFormat` | `Webp { quality: u8 }`, `Gif`, `Apng` |
| `Frame` | `{ index: u32, count: u32 }`; `Frame::STILL` = `{ index: 0, count: 1 }` |
| `OutputFormat` | `Png`, `Jpeg`, `Webp`, `Gif`, `Apng`; `extension()` → `"png"`, `"jpg"`, `"webp"`, `"gif"`, `"png"` (Discord shows APNG as `.png`); `From<Format>`, `From<AnimatedFormat>` |
| `RenderOutput` | `{ bytes: Bytes, digest: [u8; 32], format: OutputFormat, settings_used: EncodeSettings }` |
| `EncodeSettings` | `{ size: Size, quality: Option<u8>, fps: Option<u16>, frames: u32 }` |
| `RenderError` | `#[non_exhaustive]`: `UnknownTemplate { id, version }`, `Unsupported { id, why }`, `QueueFull`, `Failed(source)`; `RenderError::failed(err)` |

The render enums are closed (exhaustive `match` is fine); the render structs have
public fields, except `Size`, which validates.

## 3. Using ports from handlers [planned]

You never call port traits directly; contexts give typed, namespaced handles:

```rust
let kv = ctx.kv();                                               // CacheStore, namespaced
let cfg: Option<GuildConfig> = kv.get(&config_key(guild)).await?;   // T: DeserializeOwned
kv.put(&config_key(guild), &cfg, None).await?;                   // T: Serialize
let first = kv.put_if_absent(&cooldown, &(), Some(Duration::from_secs(60))).await?;
let n: u64 = kv.incr(&xp_key(guild, user), 1, None).await?;
let n: u64 = kv.counter(&xp_key(guild, user)).await?;            // 0 when missing
kv.delete(&key).await?;

let guild = ctx.cache().guild(guild_id).await?;                  // DiscordCache, owned value

ctx.modules().set_enabled("profile", ModuleScope::Guild(guild), false).await?;  // ModuleStateStore
let on = ctx.modules().is_enabled("profile", Some(guild));       // cached snapshot, sync
```

Rules of thumb: build keys with a short type prefix (`"profile:{guild}:{user}"`);
`CacheStore` is a cache (it may evict): durable business data belongs in your own
database, injected as `State<Db>` or wrapped in your own port.

## 4. Implementing your own Adapter

The template's `src/adapters/bounded_cache.rs` is a complete `CacheStore`; it compiles
against the real `noctis::ports` API today. The essentials:

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

Wire it [planned]: `App::from_env()?.cache_store(RedisCache::new(pool))`. Handlers don't change.

Checklist for any Adapter: bounded (or backed by bounded storage), atomic ops really
atomic, TTL honoured on read, verbatim keys, no secrets, no blocking calls in async
(use async clients or `spawn_blocking`), passes conformance.

## 5. Conformance tests (FR-39) [planned]

```rust
use noctis::testkit::conformance;

#[tokio::test]
async fn bounded_cache_store_conforms() {
    conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}
```

One suite per port: `ref_store`, `session_store`, `identify_queue`,
`module_state_store`, `rate_limit_store`, `cache_store`, `render_cache`, `asset_store`,
`discord_cache`.
They test TTL, overwrite, delete, missing keys, concurrent callers (exactly one
`take`/`put_if_absent` winner, no lost increments) and App namespacing. The factory is
called per case, so return a fresh, empty store (for Redis: a fresh key prefix or DB).

## 6. Namespacing and sharing (FR-71) [planned]

Two Apps sharing one Adapter instance never see each other's keys (`<app>:<port>:`).
Opt one port into cross-App sharing with `Shared::new(adapter)` → prefix
`_shared:<port>:` (typical: `CacheStore` for data both white-label Apps read).

## 7. Clusters

In a Cluster every process must share: `RefStore` (a Ref minted by process A is clicked
on process B), `RateLimitStore`, `ModuleStateStore`, `IdentifyQueue`, and usually
`CacheStore`/`RenderCache`/`AssetStore`. `SessionStore` must persist across restarts to
resume. Official Redis/SQL adapters are planned for v1.x; until then write them as
above and run the conformance suites.
