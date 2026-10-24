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
| `SessionStore` | shard `(session_id, seq, resume_url)` for resume, under `shard:<id>:<total>`; only the gateway calls it (read at shard start, written after an acknowledged resumable close, deleted on rejection or a fresh close); values are opaque bytes, return them verbatim | `get`, `put`, `delete` | memory (resumes only when the same instance reaches the next App) | `.session_store(..)` |
| `IdentifyQueue` | identify coordination per bucket (Cluster-wide) | `acquire(bucket) -> Lease` | `MemoryIdentifyQueue` (built from `max_concurrency`) | `.identify_queue(..)` |
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

Every port has a bounded in-memory default in `noctis::ports::memory`. An App that swaps nothing runs on these.

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
| `MemoryIdentifyQueue` | `IdentifyQueue` | one slot per bucket (`max_concurrency`) | — |

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

`MemoryIdentifyQueue` grants identifies by Discord's buckets in one process:

```rust
use std::num::NonZeroU16;
use std::sync::Arc;
use noctis::ports::IdentifyQueue;
use noctis::ports::memory::MemoryIdentifyQueue;

// max_concurrency comes from GET /gateway/bot (session_start_limit.max_concurrency)
let max_concurrency = NonZeroU16::new(16).unwrap();
let queue = MemoryIdentifyQueue::new(max_concurrency);
queue.max_concurrency();                        // 16; Default = 1 bucket
MemoryIdentifyQueue::IDENTIFY_INTERVAL;         // 5 s
let queue: Arc<dyn IdentifyQueue> = Arc::new(queue);
let bucket = shard_id % u32::from(max_concurrency.get());
let lease = queue.acquire(bucket).await?;       // waits for its bucket's turn
```

- Each bucket grants at most one `Lease` per `IDENTIFY_INTERVAL` (5 s, counted from the
  previous grant); different buckets grant in parallel; waiters in one bucket are
  granted in arrival order (FIFO). The first acquire in a bucket never waits.
- A bucket at or above `max_concurrency` is `PortError::InvalidValue` (key = the bucket
  in decimal); it is never folded with `%`, so compute `shard_id % max_concurrency`
  yourself.
- A dropped `acquire` future takes nothing (cancel-safe). Waiting needs a tokio runtime
  with the time driver; a paused tokio clock (`#[tokio::test(start_paused = true)]`)
  drives it.
- **In-process only:** it does not cover several processes. A Cluster implementation
  must serialize per bucket across processes (§7).

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
  implementation of `IdentifyQueue` must serialize per bucket across processes: at most
  one Lease per bucket per 5 s across the whole Cluster, different buckets in parallel.
  See the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md) (written with Story 14.6).
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

- **Keys are opaque and stored verbatim.** Core's `Namespaced` wrapper already
  prefixed them with `<app>:<port>:` (or `_shared:<port>:`); never add your own
  prefix.
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
    /// Called once by `App::build` with the App's resolved ports; the default ignores them.
    fn bind(&self, ports: RendererPorts) {}
}
```

`RendererPorts` is `#[non_exhaustive]` with `pub asset_store: Arc<dyn AssetStore>` (the
App's namespaced `AssetStore`, where the takumi Renderer caches fetched remote images)
and `RendererPorts::new(asset_store)`; its `Debug` hides the ports.

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
| `RenderError` | `#[non_exhaustive]`: `UnknownTemplate { id, version }`, `Unsupported { id, why }`, `QueueFull`, `Failed(source)`, …, `RemoteImage { id, image, reason }`, `UndeclaredImage { id, image }` (IMG-03); `RenderError::failed(err)` |

The render enums are closed (exhaustive `match` is fine); the render structs have
public fields, except `Size`, which validates.

## 3. Using ports from handlers

You never call port traits directly; contexts give typed, namespaced handles.
`ctx.kv() -> Kv` exists on `CommandCtx` (context menus too), `ComponentCtx`,
`ModalSubmitCtx`, `ExpiredCtx`, `EventCtx`, `AutocompleteCtx` and the Middleware
contexts `BeforeCtx` and `AfterCtx`. `Kv` (`noctis::Kv`) is a cheap clone over the
App's namespaced `CacheStore`: every key lands under `<app_id>:cache:<key>`, and each
method makes exactly one store call. All Modules of an App share that one keyspace, so
prefix each key with the Module name (`profile:{user}`, `xp:{guild}:{user}`) to keep two
Modules using the same natural key apart.

```rust
let kv = ctx.kv();                                               // CacheStore, namespaced
let cfg: Option<GuildConfig> = kv.get(&config_key(guild)).await?;   // T: DeserializeOwned
kv.put(&config_key(guild), &cfg, None).await?;                   // T: Serialize + ?Sized
let first = kv.put_if_absent(&cooldown, &(), Some(Duration::from_secs(60))).await?;
let n: u64 = kv.incr(&xp_key(guild, user), 1, None).await?;      // creates `by` with the TTL
let n: u64 = kv.counter(&xp_key(guild, user)).await?;            // 0 when missing
kv.delete(&key).await?;                                          // missing is not an error
let v2 = ctx.kv().schema(2);                                     // schema byte for typed ops
```

| Method | Returns | Notes |
| --- | --- | --- |
| `get::<T>(key)` | `Result<Option<T>, PortError>` | missing key or another schema byte = `Ok(None)`; empty, malformed or trailing bytes = `Err(PortError::Value(..))` |
| `put(key, &value, ttl)` | `Result<(), PortError>` | overwrites and resets the TTL; a value that does not serialize is `PortError::Value(ValueError::Serialize)` and makes no store call |
| `put_if_absent(key, &value, ttl)` | `Result<bool, PortError>` | `true` only for the one caller that inserted |
| `incr(key, by, ttl)` | `Result<u64, PortError>` | atomic, saturates at `u64::MAX`; the TTL applies only on create |
| `counter(key)` | `Result<u64, PortError>` | 0 when missing; a value that is not exactly 8 bytes is `PortError::InvalidValue` |
| `delete(key)` | `Result<(), PortError>` | |
| `schema(n)` | `Kv` | the schema byte (default 0); bump it when a stored type changes incompatibly, and old entries read as misses |

Values are postcard with a leading schema byte (`encode_value`/`decode_value`);
counters are 8-byte big-endian `u64`s. Keep counters and typed values under different
keys: `counter` errors only when the stored value is not exactly 8 bytes, so a typed
value that happens to encode to 8 bytes reads as a meaningless `u64`. The
futures are `Send` for any `T` (the value is encoded before the store call). A
detached test context (no App) answers every op with `PortError::Unavailable`.
`PortError` converts into `HandlerError`, so `?` works in handlers.

[planned]:

```rust
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

Wire it in `configure(builder: &mut AppBuilder)`: `builder.cache_store(RedisCache::new(pool))` (the last `cache_store` call wins). Handlers don't change.

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

## 6. Namespacing and sharing (FR-71)

Every App has an `AppId` (`[a-z0-9-]{1,32}`): `.app_id("my-bot")` or
`NOCTIS_APP_ID`; by default the bot user id read from the token, in lowercase base36
(equal to the application id for most bots; call `.app_id(..)` explicitly when it
differs; when the token does not decode it is `default`, and `build()` warns). Core
wraps each of the seven byte stores in `noctis::ports::Namespaced`, which prefixes
every key with `<app_id>:<port>:`; the logical key is appended verbatim. The port
segments are:

| Port | Prefix |
| --- | --- |
| `RefStore` | `<app>:ref:` |
| `SessionStore` | `<app>:session:` |
| `ModuleStateStore` | `<app>:module_state:` |
| `RateLimitStore` | `<app>:rate_limit:` |
| `CacheStore` | `<app>:cache:` |
| `RenderCache` | `<app>:render_cache:` |
| `AssetStore` | `<app>:asset:` |

`IdentifyQueue`, `LocaleSource`, `GuildAccessList` and `DiscordCache` are never
namespaced. So two Apps handed one Adapter instance (every byte store is implemented
for `Arc<P>`, so `.cache_store(Arc::clone(&store))` on both builders works) never see
each other's keys.

Opt one port into cross-App sharing by passing `Shared::new(adapter)`
(`noctis::ports::Shared`) to its normal setter: its prefix becomes `_shared:<port>:`
for every App that does the same (typical: `CacheStore` for data both white-label Apps
read). `_shared` can never equal an App id. `ModuleStateStore::version()` is forwarded
un-namespaced, so it is store-wide: another App's writes to a shared instance bump it
and trigger a harmless snapshot refresh. Only the seven byte-store setters accept
`Shared` (they take the sealed `IntoPort`); `.identify_queue(Shared::new(..))` does not
compile.

The types (rarely needed in bot code):

| Type | API |
| --- | --- |
| `noctis::AppId` | `AppId::new(&str) -> Result<AppId, AppIdError>` (checks empty, then more than 32 chars, then the first char outside `a-z`, `0-9`, `-`; never trims), `AppId::from_application_id(Id<ApplicationMarker>)` (the base36 default, fed the token's bot user id), `as_str()`, `Display`, `FromStr`; `Clone`, `Eq`, `Hash` |
| `noctis::AppIdError` | `#[non_exhaustive]`: `Empty`, `TooLong { len }`, `InvalidChar { index, ch }` (char index); each message names `[a-z0-9-]{1,32}` |
| `noctis::ports::Namespaced<P>` | `Namespaced::new(&app_id, Arc<dyn Port>)`, `Namespaced::shared(Arc<dyn Port>)`, `prefix() -> &str`; implements its port, forwarding each op once with `prefix + key` |
| `noctis::ports::NamespacedPort` | sealed; implemented for the seven `dyn Port` types; `NamespacedPort::SEGMENT` is the port segment (`"cache"`, …) |
| `noctis::ports::Shared<P>` | `Shared::new(port)`, `Shared::into_inner(self) -> P`; implements no port |
| `noctis::ports::IntoPort<dyn Port>` | sealed; what the seven byte-store setters take: any implementation of the port, or `Shared<impl Port>` |

```rust
let store = Arc::new(MyRedisCache::new(pool));
builder.app_id("bot-a").cache_store(Shared::new(Arc::clone(&store)));   // _shared:cache:
builder.ref_store(Arc::clone(&refs));                                  // bot-a:ref:
```

## 7. Clusters

In a Cluster every process must share: `RefStore` (a Ref minted by process A is clicked
on process B), `RateLimitStore`, `ModuleStateStore`, `IdentifyQueue`, and usually
`CacheStore`/`RenderCache`/`AssetStore`. `SessionStore` must persist across restarts to
resume: the gateway reads each shard's entry at start, writes it after Discord
acknowledges the shutdown's 4000 close and deletes it when Discord rejects it or on a
fresh close. A crash removes nothing: an entry kept after a resumed start, or an older
one left by an unacknowledged close, stays, and the next start resumes from its older
sequence (Discord replays events already handled, or rejects it and the shard
identifies). The stored bytes are opaque: return them verbatim (no TTL is set). After
the shard total changes, old `<app>:session:shard:<id>:<old total>` entries are never
read again; the operator may delete them. Official Redis/SQL adapters are planned for v1.x; until then write them as
above and run the conformance suites.

`MemoryIdentifyQueue` does not cover several processes. A Cluster `IdentifyQueue` must
serialize per bucket across processes: at most one Lease per bucket per 5 s across the
whole Cluster, different buckets in parallel. See the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md) (written with Story 14.6).
