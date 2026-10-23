//! Port traits (including `DiscordCache`), the `Template`/`RenderJob`/`Renderer`
//! seam and the typed-value helpers.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! # Ports
//!
//! Every storage-like concern is a small, dyn-compatible port trait:
//! `Send + Sync + 'static`, async methods returning [`BoxFuture`], held as
//! `Arc<dyn Port>`. A new concern adds a new port; it never extends an
//! unrelated one.
//!
//! | Port | Ops |
//! | --- | --- |
//! | [`RefStore`] | `put_if_absent`, `get`, `take`, `delete`, `touch` |
//! | [`SessionStore`] | `get`, `put`, `delete` |
//! | [`ModuleStateStore`] | `get`, `put`, `version` |
//! | [`RateLimitStore`] | `incr_with_ttl`, `get`, `put` |
//! | [`CacheStore`] | `get`, `put`, `put_if_absent`, `delete`, `incr_with_ttl` |
//! | [`RenderCache`], [`AssetStore`] | `get`, `put` |
//! | [`IdentifyQueue`] | `acquire(bucket) -> Lease` |
//! | [`DiscordCache`] | `update(&Event)`, owned getters, per-guild id lists |
//! | [`LocaleSource`] | `load(locale)` |
//! | [`GuildAccessList`] | `allows(guild)` (sync) |
//! | [`Renderer`] (driven port) | `render(job)`, `register(templates)` |
//!
//! # The op vocabulary
//!
//! Keys are `&str`, values are [`Bytes`](bytes::Bytes), and a TTL of `None`
//! means no expiry.
//!
//! | Op | Returns | Contract |
//! | --- | --- | --- |
//! | `get` | `Option<Bytes>` | An expired or missing entry is `None`. |
//! | `put(value, ttl)` | `()` | Overwrites the value and resets the TTL. |
//! | `put_if_absent(value, ttl)` | `bool` | `true` only for the one caller that inserted; an expired entry counts as absent. |
//! | `take` | `Option<Bytes>` | Atomic get + delete; exactly one concurrent caller gets `Some`. |
//! | `delete` | `()` | Removes the entry; a missing key is not an error. |
//! | `touch(ttl)` | `bool` | `true` if a live entry existed, whose expiry becomes now + ttl; otherwise `false` and nothing is created. |
//! | `incr_with_ttl(by, ttl)` | `u64` | Atomic, no lost increments, returns the new value; TTL set only on create (`by = 0` creates 0); 8-byte big-endian `u64`; saturates at `u64::MAX`; a non-counter value is [`PortError::InvalidValue`]. |
//!
//! No port method takes a bot credential, a payload key or any other secret
//! (NFR-6): adapters only ever see opaque keys and values.
//!
//! Adapters store keys verbatim and never prefix them: core wraps every byte
//! store in `noctis::ports::Namespaced`, which adds the `<app>:<port>:`
//! namespace, or `_shared:<port>:` for a port passed as
//! `noctis::ports::Shared::new(adapter)`.
//!
//! Every byte-store port is also implemented for `Arc<P>`, forwarding each
//! op, so one instance can be handed to several Apps with `Arc::clone`.
//!
//! # Typed values
//!
//! [`encode_value`] and [`decode_value`] store a value as postcard with a
//! leading schema byte; a schema mismatch reads as a miss.
//!
//! # Defaults
//!
//! [`memory`] holds a bounded in-memory default for every port: seven byte
//! stores with documented byte budgets and maximum TTLs,
//! [`MemoryDiscordCache`](memory::MemoryDiscordCache) over
//! `twilight-cache-inmemory`,
//! [`MemoryIdentifyQueue`](memory::MemoryIdentifyQueue) for identify buckets
//! in one process, and the trivial
//! [`NoLocaleOverlay`](memory::NoLocaleOverlay) and
//! [`AllowAllGuilds`](memory::AllowAllGuilds).

mod access;
mod discord_cache;
mod error;
mod identify;
mod locale;
pub mod memory;
mod render;
mod store;
mod value;

use std::future::Future;
use std::pin::Pin;

pub use access::GuildAccessList;
pub use discord_cache::DiscordCache;
pub use error::PortError;
pub use identify::{IdentifyQueue, Lease};
pub use locale::LocaleSource;
pub use render::{
    AnimatedFormat, AnimationSpec, EncodeSettings, ErasedView, Format, Frame, OutputFormat,
    RenderCap, RenderError, RenderJob, RenderOutput, Renderer, Size, Template, TemplateDescriptor,
    decode_template,
};
pub use store::{
    AssetStore, CacheStore, ModuleStateStore, RateLimitStore, RefStore, RenderCache, SessionStore,
};
pub use value::{ValueError, decode_value, encode_value};

/// A boxed, `Send` future; the same type as `futures_util::future::BoxFuture`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
