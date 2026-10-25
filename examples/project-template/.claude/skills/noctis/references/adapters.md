# Adapters (ports)

Covers: every port, its in-memory default, how to swap one, the typed `ctx.kv()`
key-value API, the Discord resource cache (`ctx.cache()`), namespacing and sharing
across Apps, writing your own Adapter, conformance tests, the render contract.
FR-38..40, FR-71; AD-1, AD-14. Rule: anything that stores, caches, queues or resumes
data that must outlive one call goes through a port, never a `static` map.

## 1. The ports (`noctis::ports`)

| Port | Stores | Ops it uses | Default | Swap with |
| --- | --- | --- | --- | --- |
| `RefStore` | Payloads behind Refs (by_ref/spill Routes) | `put_if_absent`, `get`, `take`, `delete`, `touch` | bounded memory, TTL | `.ref_store(..)` |
| `SessionStore` | shard `(session_id, seq, resume_url)` for resume, under `shard:<id>:<total>`; only the gateway calls it (read at shard start, written after an acknowledged resumable close, deleted on rejection or a fresh close); values are opaque bytes, return them verbatim | `get`, `put`, `delete` | memory (resumes only when the same instance reaches the next App) | `.session_store(..)` |
| `IdentifyQueue` | identify coordination per bucket (Cluster-wide) | `acquire(bucket) -> Lease` | `MemoryIdentifyQueue` (built from `max_concurrency`) | `.identify_queue(..)` |
| `ModuleStateStore` | Module enabled/disabled, global and per guild | `get`, `put`, `version() -> u64` (polled, default 5 s) | memory | `.module_state_store(..)` |
| `RateLimitStore` | your declared rate-limit counters | `incr_with_ttl`, `get`, `put` | memory | `.rate_limit_store(..)` |
| `CacheStore` | general key-value data with TTL for bots and Modules (`ctx.kv()`) | `get`, `put`, `put_if_absent`, `delete`, `incr_with_ttl` | bounded memory | `.cache_store(..)` |
| `DiscordCache` | Discord resources: guilds, channels, roles, members, users, messages (`ctx.cache()`, §1.2) | `update(&Event)`, typed owned getters | `MemoryDiscordCache` of the `.discord_cache(..)` resources (default `GUILD \| CHANNEL \| ROLE`) | `.discord_cache_backend(..)` (never namespaced) |
| `RenderCache` | encoded images by render key | `get`, `put` | byte-bounded memory | `.render_cache(..)` |
| `AssetStore` | fetched remote images (avatars, icons, emoji) | `get`, `put` | byte-bounded memory | `.asset_store(..)` |
| `LocaleSource` | runtime overlays of content strings, loaded once at startup (one `load` per locale with a file, content keys only; a failure or a rejected pair stops startup with `Error::LocaleOverlay`; see i18n §8) | `load(locale)` | none | `.locale_source(..)` |
| `GuildAccessList` | per-App guild allow/deny lists (white-label), asked at pipeline stage 3 (`app-and-sharding.md` §10) | `allows(guild)` | allow all (`GuildAccess` is the ready allow/deny list) | `.guild_access(..)` (never namespaced) |
| `Renderer` (driven port, not a store; `noctis::image::unstable`, outside semver, §2b) | turns a `RenderJob` into `RenderOutput` | `render(job)`, `register`, `bind` | the takumi Renderer from `.theme(..)`/`.image(..)` | `.renderer(..)` (never together with `.theme`/`.image`) |

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
| `GuildAccess` | `GuildAccessList` | `GuildAccess::allow(guilds)` serves only those (empty: none), `GuildAccess::deny(guilds)` all but those (empty: all); one `RwLock<HashSet>`; `insert`/`remove` (`-> bool`), `contains`, `is_allow_list`; share an `Arc` to update it at run time | — |
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

Wire a default or your own Adapter into the App with the `AppBuilder` setter in the
table above (`.cache_store(..)`, `.ref_store(..)`, …), in `configure`.

### 1.2 The Discord resource cache (`ctx.cache()`)

The Discord resource cache is its own port, `DiscordCache`, separate from
`CacheStore`. Every context has `ctx.cache()` (`CommandCtx`, `ComponentCtx`,
`ModalSubmitCtx`, `AutocompleteCtx`, `EventCtx`, `BeforeCtx`, `AfterCtx`), which returns
a `noctis::DiscordCacheReader` (cheap to clone). Its async getters return **owned**
twilight-model values, never a guard or reference, so nothing can be held across an
`.await`:

| Getter | Returns | Resource |
| --- | --- | --- |
| `guild(guild)` | `Result<Option<Guild>, CacheError>` (its channel/role/member lists are empty: use the list getters) | `GUILD` |
| `channel(channel)` | `Option<Channel>` (channels and threads) | `CHANNEL` |
| `role(role)` | `Option<Role>` | `ROLE` |
| `member(guild, user)` | `Option<Member>` | `MEMBER` |
| `user(user)` | `Option<User>` | `USER` |
| `message(message)` | `Option<Message>` | `MESSAGE` |
| `channel_ids(guild)` / `role_ids(guild)` / `member_ids(guild)` | `Option<Vec<Id<_>>>` (`None` = guild not cached) | `CHANNEL` / `ROLE` / `MEMBER` |
| `channels(guild)` / `roles(guild)` / `members(guild)` | `Option<Vec<T>>` in id order (ids, then one read per id; an id no longer cached is skipped, the first error is returned) | `CHANNEL` / `ROLE` / `MEMBER` |

Also `resources()` and `is_enabled()`. Errors:

- `Ok(None)`: not cached (yet). Fall back (`map_or_else`, `unwrap_or_default`), never
  `unwrap`.
- `Err(CacheError::Disabled(CacheDisabled { resource }))`: that resource is not
  configured (or the cache is `none()`); decided before any backend call. A
  configuration mistake, not a runtime condition.
- `Err(CacheError::Port(PortError))`: the backend failed.
- `?` turns a `CacheError` into a `HandlerError`.

```rust
use noctis::{DiscordCacheConfig, DiscordCacheResources};

async fn guild_line(ctx: &CommandCtx, guild: Id<GuildMarker>) -> HandlerResult<String> {
    let name = ctx.cache().guild(guild).await?.map_or_else(|| guild.to_string(), |g| g.name);
    let roles = ctx.cache().roles(guild).await?.unwrap_or_default();
    Ok(format!("{name}: {} roles", roles.len()))
}

builder.discord_cache(
    DiscordCacheConfig::default().with_resources(DiscordCacheResources::USER),
);
```

**Configuration** (`noctis::DiscordCacheConfig`, `noctis::DiscordCacheResources`; no
env var): `.discord_cache(config)` on the builder (last call wins; `App::discord_cache()`
reads it back).

- `DiscordCacheConfig::default()` caches `DEFAULT_RESOURCES = GUILD | CHANNEL | ROLE`:
  what the always-requested `GUILDS` intent delivers, no members, no messages.
- `DiscordCacheConfig::new(resources)`, `.with_resources(r)` (adds),
  `.without_resources(r)` (removes), `.with_message_cap(n)` (per channel, default
  `MemoryDiscordCache::DEFAULT_MESSAGE_CAP` = 100; 0 removes `MESSAGE`); getters
  `resources()`, `message_cap()`, `is_enabled()`.
- `DiscordCacheConfig::none()` turns the cache off: nothing is fed or read and every
  getter is `CacheError::Disabled`.
- `DiscordCacheResources` prints as `GUILD | ROLE` (`none` when empty).

Each resource adds the gateway event types that keep it current to the App's gateway
filter (they are never dropped by the event lane) and its intents (app-and-sharding.md
§ intents): `GUILD`, `CHANNEL`, `ROLE`, `USER` add `GUILDS`; `MESSAGE` adds
`GUILD_MESSAGES | DIRECT_MESSAGES` (message **content** still needs the privileged
`MESSAGE_CONTENT`); `MEMBER` needs the privileged `GUILD_MEMBERS`, which you must request
with `.intents(Intents::GUILD_MEMBERS)` and enable in the Developer Portal, else
`build()` fails with `BuildViolation::MissingCacheIntent` (`[GW-05]`). `USER` and
`MEMBER` are also filled from interactions (the invoker in a DM, resolved users and
members of options); users are never evicted, so `USER` grows with the users you see.

**Feeding:** core applies each cached event type to the backend in the drain loop
**before** any handler of that event runs (a `RoleCreate` handler already sees the role
in `ctx.cache()`), and each interaction (when `MEMBER`, `USER` or `ROLE` is cached)
inside its own task before the pipeline. An update is bounded by 1 s; a failed or
timed-out update never stops the event or interaction: it counts in
`noctis_discord_cache_update_errors_total{app}` and logs one `noctis::cache` warning per
outage (one info when updates recover). The drain waits for each update, so use a fast
backend.

**Backend:** `.discord_cache_backend(impl DiscordCache)` replaces the storage; the
configuration, not the backend, decides what is fed and read. The default is a
`MemoryDiscordCache` of the configured resources and cap (§1.1). `DiscordCache` is
implemented for `Arc<P>`, so one instance serves several Apps of a Cluster
(`.discord_cache_backend(Arc::clone(&cache))` on each builder); it is never namespaced,
and each process feeds only its own shards' guilds. An out-of-process backend
serializes the twilight-model values itself. With `none()` and a custom backend,
`build()` warns once that the backend is never used. A custom backend must cache every
configured resource (e.g. `MemoryDiscordCache::with_message_cap(config.resources(),
config.message_cap())`): getters report `Disabled` only for resources missing from the
configuration, so a resource the backend skips silently reads `Ok(None)`. `update` must
be cancel-safe: core cancels it after 1 s (`CACHE_UPDATE_TIMEOUT`) and does not retry,
so a multi-key store must not be left half-written.

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
    // an empty Vec means no overlay. Called once per locale with a file at startup;
    // only content keys with the compiled placeholders are accepted (i18n §8).
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
  See the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md).
- `GuildAccessList::allows` is **synchronous**: it runs on pipeline stage 3, which does
  no port I/O. Never block in it; a remote-backed implementation keeps a local snapshot
  and refreshes it in the background. It is asked only for interactions, events and
  `Dispatcher::run_command` calls that carry a guild (DMs are always served); `false`
  answers with the `noctis.module.disabled` notice (interactions), an empty autocomplete
  list, no event handler or `DispatchError::GuildNotServed`; a panic counts as `false`.
  Publish ignores it. It is implemented for `Arc<P>`, so you can keep a handle:

  ```rust
  use std::sync::Arc;
  use noctis::ports::memory::GuildAccess;

  let access = Arc::new(GuildAccess::allow([customer_guild]));
  builder.guild_access(Arc::clone(&access));
  // later, while running:
  access.insert(new_customer_guild);   // served from the next interaction on
  access.remove(customer_guild);       // true if it was listed
  ```
- `DiscordCache` is implemented for `Arc<P>` (one backend, several Apps). It uses
  only `twilight_model` types (`gateway::event::Event`,
  `guild::{Guild, Member, Role}`, `channel::{Channel, Message}`, `user::User`). Getters
  return owned values; `None` means not cached. `channel_ids`, `role_ids` and
  `member_ids` return ids only (`None` = guild not cached), so a large guild is never
  cloned per call.
- `Id`, the `*Marker` types, `Event` and the cached resource types come from the
  `twilight-model` crate, which `noctis` re-exports: `Id` and its markers as
  `noctis::id` (`noctis::id::Id`, `noctis::id::marker::GuildMarker`; `Id` is also in
  `noctis::prelude`), the rest under `noctis::twilight::model` (e.g.
  `noctis::twilight::model::gateway::event::Event`). No `twilight-model` dependency
  is needed.

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

The seam between core and the image renderer (FR-37). You normally write Templates with
the image API (`images.md`) and let `.theme(..)`/`.image(..)` install the takumi
Renderer; implement `Renderer` only to replace it (a remote render service, or a test
double that answers fixed bytes).

**Unstable paths.** The renderer-facing items live **only** in
`noctis::image::unstable` (feature `image`): `Renderer`, `RendererPorts`, `RenderJob`,
`RenderOutput`, `TemplateDescriptor`, `ErasedView` and `decode_template`. **Semver does
not cover them**: they may change in any release, so pin the Noctis version when you
ship a custom Renderer. A replacement Renderer needs only them plus the stable
`Template`, `Size`, `Format`, `AnimationSpec`, `AnimatedFormat`, `Frame`,
`OutputFormat`, `EncodeSettings`, `RenderError`, `RenderCap` (in `noctis::image` and
`noctis::ports`) and `noctis::ports::{BoxFuture, RenderCache, AssetStore}`.

**Installing it.** `AppBuilder::renderer<R: Renderer>(r)` takes any Renderer by value,
also `Arc<R>` or `Arc<dyn Renderer>` (`Renderer` is implemented for `Arc<R>`, forwarding
every method). An App has **one** Renderer: a second `.renderer(..)`, or `.renderer(..)`
together with the image plugin (`.theme(..)`/`.image(..)`), fails `build()` with
`BuildViolation::DuplicateRenderer { first, second }` (both type names; an explicit
`.renderer(..)` is always `first` and kept, and the image plugin's Renderer is always
reported as `second` because plugins install in `build`, whatever the call order): "a Renderer is set twice: `{first}`, then `{second}`: an App has one Renderer;
keep `.renderer(..)` or the image plugin (`.theme(..)`/`.image(..)`), not both". Modules,
Templates, views and handlers do not change: `ctx.render(card)` renders through whichever
Renderer the App has. `register` gets every registered Template at `build()` (an error is
`BuildViolation::TemplateRejected`), and `bind` gets the App's namespaced ports once.

**What core checks.** `ctx.render` sets `job.upload_budget` to what this interaction's
upload accepts (`attachment_size_limit`, else `Limits::attachment_bytes`, 20 MiB), then
checks **every** Renderer's output before it builds the attachment (ATT-01):
- empty `bytes`: `RenderError::Failed` (displays "render failed", source "the Renderer
  returned no bytes");
- more than `job.upload_budget` bytes: `RenderError::OverBudget { budget, smallest }`
  (`smallest` = the output's length); exactly the budget is accepted.

Both are reported to the `Render` Error Sink (no `noctis_render_rejected_total` reason)
and no file is sent. A Renderer's own errors pass through unchanged (`QueueFull` is not
reported, the rest are). An accepted output is named by its digest:
`<first 16 hex of digest>.<format.extension()>`, so equal digests are one file name.

**The render key.** `RenderJob::key(&self, renderer: &[u8]) -> [u8; 32]` is a
deterministic blake3 key over every job field (Template id, version, data, size, format
and quality, animation, upload budget), the Noctis version and `renderer`, your
Renderer's own identity: pass its name, version and asset versions
(`b"my-renderer=2;fonts=3"`), and bump them when its output changes. Return the key as
`RenderOutput::digest`, and use it as your `RenderCache` key if you cache (the App's
cache arrives in `bind`). The takumi Renderer keeps its own key (`images.md` §9).

```rust
use noctis::bytes::Bytes;
use noctis::image::unstable::{RenderJob, RenderOutput, Renderer, TemplateDescriptor};
use noctis::image::{EncodeSettings, OutputFormat, RenderError};
use noctis::ports::BoxFuture;

/// Answers every render with fixed bytes: a handler-test double, and the skeleton of a
/// remote Renderer (send `job` to the service in `render`, return its bytes).
pub struct FixedRenderer {
    bytes: Bytes,
    format: OutputFormat,
}

impl FixedRenderer {
    pub const ID: &'static [u8] = b"noctis-template.fixed=1";

    pub fn new(bytes: Bytes, format: OutputFormat) -> Self {
        Self { bytes, format }
    }
}

impl Renderer for FixedRenderer {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let output = RenderOutput {
            bytes: self.bytes.clone(),
            digest: job.key(Self::ID),
            format: self.format,
            settings_used: EncodeSettings { size: job.size, quality: None, fps: None, frames: 1 },
        };
        Box::pin(async move { Ok(output) })
    }

    fn register(&self, _templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

// Instead of `.theme(..)`/`.image(..)`; no font needed.
let app = App::builder()
    .module(modules::profile::module())
    .renderer(FixedRenderer::new(Bytes::from_static(b"\x89PNG"), OutputFormat::Png))
    .build()?;
```

The template ships this as `adapters::FixedRenderer` (`src/adapters/fixed_renderer.rs`).

The contract's traits:

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
and `pub render_cache: Option<Arc<dyn RenderCache>>` (the App's namespaced
`RenderCache`, where the takumi Renderer stores rendered images under
`render:v1:<64 hex>`, i.e. `<app>:render_cache:render:v1:…` in the backing store;
`images.md` §9). It writes entries with no TTL (`None`, "no expiry"), so a swapped
`RenderCache` Adapter must bound their lifetime itself (its own default expiry or LRU
eviction, e.g. Redis `maxmemory-policy allkeys-lru`). Build it with `RendererPorts::new(asset_store)` (no render cache) and
`.with_render_cache(cache)`; `App::build` binds both. Its `Debug` hides the ports. A
replacement `Renderer` may cache its own way or ignore the render cache.

| Type | Shape |
| --- | --- |
| `ErasedView` | `ErasedView::new(view)`, `downcast_ref::<T>() -> Option<&T>`; `Clone` shares the value; `Debug` hides it |
| `TemplateDescriptor` | `{ id: &'static str, version: u32, view: ErasedView }`, `TemplateDescriptor::of::<T>()` |
| `RenderJob` | `{ template_id: &'static str, template_version: u32, data: Bytes, size: Size, format: Format, animation: Option<AnimationSpec>, upload_budget: u64 }`; `RenderJob::of(&template) -> Result<RenderJob, ValueError>` (budget `RenderJob::DEFAULT_UPLOAD_BUDGET`, 20 MiB); `data` is plain postcard, no schema byte; `job.key(renderer: &[u8]) -> [u8; 32]`, the render key above |
| `decode_template` | `decode_template::<T>(data: &[u8]) -> Result<T, ValueError>` (`Malformed`, `Trailing`) |
| `Size` | `Size::new(width, height)` (const; a zero side panics, a compile error in a `const`), `Size::try_new(..) -> Option<Size>`, `width()`, `height()` |
| `Format` | `Png`, `Jpeg { quality: u8 }`, `Webp { quality: u8 }`, `WebpLossless` |
| `AnimationSpec` | `{ duration: Duration, fps: u16, format: AnimatedFormat }`, const `AnimationSpec::new(duration, fps, format)` |
| `AnimatedFormat` | `Webp { quality: u8 }`, `Gif`, `Apng` |
| `Frame` | `{ index: u32, count: u32 }`; `Frame::STILL` = `{ index: 0, count: 1 }` |
| `OutputFormat` | `Png`, `Jpeg`, `Webp`, `Gif`, `Apng`; `extension()` → `"png"`, `"jpg"`, `"webp"`, `"gif"`, `"png"` (Discord shows APNG as `.png`); `From<Format>`, `From<AnimatedFormat>` |
| `RenderOutput` | `{ bytes: Bytes, digest: [u8; 32], format: OutputFormat, settings_used: EncodeSettings }`; `digest` names the attachment (a custom Renderer uses `RenderJob::key`; the takumi Renderer uses its render cache key, except a degraded (uncached) render, which is named by its bytes) |
| `EncodeSettings` | `{ size: Size, quality: Option<u8>, fps: Option<u16>, frames: u32 }` |
| `RenderError` | `#[non_exhaustive]`: `UnknownTemplate { id, version }`, `Unsupported { id, why }`, `QueueFull`, `Failed(source)`, `OverBudget { budget, smallest }`, `NoRenderer`, …, `RemoteImage { id, image, reason }`, `UndeclaredImage { id, image }` (IMG-03); `RenderError::failed(err)` |

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
| `incr(key, by: u64, ttl: Option<Duration>)` | `Result<u64, PortError>` | atomic, saturates at `u64::MAX`; the TTL applies only on create |
| `counter(key)` | `Result<u64, PortError>` | 0 when missing; a value that is not exactly 8 bytes is `PortError::InvalidValue` |
| `delete(key)` | `Result<(), PortError>` | |
| `schema(n)` | `Kv` | the schema byte (default 0); bump it when a stored type changes incompatibly, and old entries read as misses |

`Kv` cannot list or scan keys, and it has no sorted sets: to rank or list entries
(a leaderboard, every member with points), keep an index value of your own (a
`Vec<u64>` of ids under one key, capped) next to the per-entry counters, or keep the data
in your own database behind a custom Adapter. The index update is a read-then-write;
counters stay atomic (`references/patterns.md` §11).

Values are postcard with a leading schema byte (`encode_value`/`decode_value`);
counters are 8-byte big-endian `u64`s. Keep counters and typed values under different
keys: `counter` errors only when the stored value is not exactly 8 bytes, so a typed
value that happens to encode to 8 bytes reads as a meaningless `u64`. The
futures are `Send` for any `T` (the value is encoded before the store call). A
detached test context (no App) answers every op with `PortError::Unavailable`.
`PortError` converts into `HandlerError`, so `?` works in handlers.

Discord ids do not round-trip through `Kv`: postcard is not self-describing, and
twilight's `Id` deserializes with `deserialize_any`, so a stored struct with an
`Id<_>` field reads back as `PortError::Value(ValueError::Malformed)`. Store
`id.get()` (a `u64`) and rebuild with `Id::new_checked(n)`, treating `None` (a `0`)
as no stored value; never `Id::new` or `unwrap` on stored data:

```rust
#[derive(Serialize, Deserialize)]
struct StoredWebhook { id: u64, token: String, channel: u64 }   // not Id<WebhookMarker>

ctx.kv().put(&key, &StoredWebhook { id: hook.id().get(), token, channel: ch.get() }, None).await?;
if let Some(stored) = ctx.kv().get::<StoredWebhook>(&key).await?
    && let Some(id) = Id::<WebhookMarker>::new_checked(stored.id)
{ /* use id */ }
```

Migrating data stored with an `Id<_>` field: the old value can never be read back, so
treat `Err(PortError::Value(ValueError::Malformed))` on that key as "no value" and
overwrite it with the `u64` shape on the next write, or read and write that type through
`ctx.kv().schema(n)` with a new `n`, under which the old entries read as misses (Noctis
0.1 has no guard that rejects `Id<_>` at `put`):

```rust
let stored = match ctx.kv().get::<StoredWebhook>(&key).await {
    Err(PortError::Value(ValueError::Malformed)) => None,   // the old Id<_> shape
    other => other?,
};
```

Discord resources come from `ctx.cache()` (§1.2):

```rust
let guild = ctx.cache().guild(guild_id).await?;                  // Option<Guild>, owned value
```

`ctx.modules()` writes the `ModuleStateStore` and reads a cached
snapshot (`modules-middleware-ratelimits.md` §3):

```rust
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

## 5. Conformance tests (FR-39)

```rust
use noctis::testkit::conformance;

#[tokio::test]
async fn bounded_cache_store_conforms() {
    conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}
```

One suite per port in `noctis::testkit::conformance` (feature `testkit`):

```rust
pub async fn ref_store<P: RefStore, F: Fn() -> P>(factory: F);
pub async fn session_store<P: SessionStore, F: Fn() -> P>(factory: F);
pub async fn module_state_store<P: ModuleStateStore, F: Fn() -> P>(factory: F);
pub async fn rate_limit_store<P: RateLimitStore, F: Fn() -> P>(factory: F);
pub async fn cache_store<P: CacheStore, F: Fn() -> P>(factory: F);
pub async fn render_cache<P: RenderCache, F: Fn() -> P>(factory: F);
pub async fn asset_store<P: AssetStore, F: Fn() -> P>(factory: F);
pub async fn identify_queue<P: IdentifyQueue, F: Fn() -> P>(factory: F);
pub async fn discord_cache<C: DiscordCache, F: Fn(DiscordCacheResources) -> C>(factory: F);
```

- **Factory.** Called once per case; return a fresh, empty store (Redis: a fresh key
  prefix or DB; SQL: a fresh table or schema). Cases use fixed keys and at most 64
  distinct keys per instance, so a bounded Adapter of that size never evicts mid-run.
  `identify_queue` needs at least 2 buckets and a queue that granted no Lease in the
  last 5 s. `discord_cache` passes the resources to cache, as core passes the
  configured set to a custom backend.
- **Result.** `()` when every case passes; otherwise a panic on the first failing case:
  `` conformance::<suite>: case `<case>` failed: <detail> ``. An unexpected
  `Err(PortError)` fails the case with the error's text; a panic in a racing task is
  re-raised with the same prefix.
- **Clock.** Real time: `#[tokio::test]` (either flavor), never `start_paused`. All TTL
  cases share one 2.5 s wait for 1 s TTLs, which tolerates backends that round TTLs up
  to whole seconds; a 1 s entry must stay readable for its full second, so a backend
  that truncates or rounds expiry down may fail the live checks intermittently
  (whenever the write lands just before a second boundary). ~3 s per byte-store
  suite, ~10 s for `identify_queue`.

Byte-store cases, in order (each runs when the port has the ops it needs): `missing_key`
(`None`, `delete` twice is `Ok`, `touch` gives `false` and creates nothing),
`roundtrip_values` (empty, 1 byte, all 256 byte values and 64 KiB returned exactly;
empty is `Some`), `key_shapes` (Unicode, `:` and `%`, a 512-byte key, case- and
prefix-distinct keys stored verbatim), `overwrite`, `put_if_absent_live`, `take_once`,
`delete_live`, `touch_live`, `incr_basic` (8-byte big-endian `u64`, `by = 0` creates
0), `incr_saturates` (at `u64::MAX`), `incr_non_counter` (`PortError::InvalidValue`,
value unchanged), `version_monotonic` (0 on a fresh store, greater after every `put`,
also under 8 concurrent `put`s), `namespacing` (the store wrapped in
`Namespaced::new` for two App ids and `Namespaced::shared`: Apps never see each
other's keys or counters; keys reach the store as `<app>:<port>:<key>` and
`_shared:<port>:<key>`), `race_put_if_absent` (32 callers: exactly one `true`, its value
stored), `race_take` (exactly one `Some`), `race_incr` (32 x 16 increments: the counter
is 512 and the returned values are exactly 1..=512), then `ttl_live_before_expiry`,
`ttl_get_expires`, `ttl_put_resets`, `ttl_put_if_absent_expired`, `ttl_take_expired`,
`ttl_touch` (extends and shortens; `false` once expired) and `ttl_incr_create_only` (TTL
set on create only; an expired counter restarts).

`identify_queue` cases: `lease_bucket` (the Lease names its bucket; an idle bucket
grants within 1 s), `bucket_spacing` (three Leases of one bucket come 5 s apart),
`buckets_parallel` (another bucket grants at once meanwhile).

`discord_cache` cases (gateway fixtures for guild 1, channels 10 and 11, roles 1 and 20,
member/user 30, message 40): `empty`, `guild_create_all`, `unknown_ids`, `owned_reads`
(changing a returned value changes nothing cached), `updates_in_order` (channel
update, role, member, channel, message and guild deletes), `disabled_guild`,
`disabled_channel`, `disabled_role`, `disabled_member`, `disabled_user` (with `MEMBER`
off too) and `disabled_message` (that getter `None`, every other `Some`), and
`channel_only` (`DiscordCacheResources::CHANNEL`).

The suites are proven both ways in noctis itself: every in-memory default passes, and
deliberately broken stores (a TTL-ignoring cache, a non-atomic `take`, a read-then-write
`incr_with_ttl`, a read-then-write `put_if_absent`, an `incr_with_ttl` that resets the
TTL on every call, a Discord cache that ignores its configured resources) fail on the
matching case.

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
read again; the operator may delete them. Noctis 0.1 ships no Redis or SQL Adapters; write them as above and run the conformance
suites.

`MemoryIdentifyQueue` does not cover several processes. A Cluster `IdentifyQueue` must
serialize per bucket across processes: at most one Lease per bucket per 5 s across the
whole Cluster, different buckets in parallel. See the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md).
