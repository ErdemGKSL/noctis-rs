//! Bounded in-memory defaults for every port.
//!
//! An App that swaps nothing runs on these. Each byte store is bounded by a
//! byte budget and a maximum TTL ([`MemoryLimits`]); [`MemoryDiscordCache`] is
//! bounded by the resources it caches and its per-channel message cap.
//!
//! | Store | Port | `max_bytes` | `max_ttl` |
//! | --- | --- | --- | --- |
//! | [`MemoryRefStore`] | [`RefStore`](crate::RefStore) | 32 MiB | 7 days |
//! | [`MemorySessionStore`] | [`SessionStore`](crate::SessionStore) | 1 MiB | none |
//! | [`MemoryModuleStateStore`] | [`ModuleStateStore`](crate::ModuleStateStore) | 16 MiB | none |
//! | [`MemoryRateLimitStore`] | [`RateLimitStore`](crate::RateLimitStore) | 16 MiB | 1 day |
//! | [`MemoryCacheStore`] | [`CacheStore`](crate::CacheStore) | 32 MiB | none |
//! | [`MemoryRenderCache`] | [`RenderCache`](crate::RenderCache) | 64 MiB | 1 day |
//! | [`MemoryAssetStore`] | [`AssetStore`](crate::AssetStore) | 32 MiB | 1 day |
//!
//! An entry weighs `key.len() + value.len() + ENTRY_OVERHEAD` bytes (UTF-8
//! length, [`MemoryLimits::ENTRY_OVERHEAD`] = 64). When the budget is full the
//! least recently used entry is evicted. A single write heavier than the
//! whole budget is rejected with [`PortError::InvalidValue`](crate::PortError)
//! and changes nothing. A write's TTL is clamped to `max_ttl`; with no TTL and
//! a `max_ttl`, the entry expires after `max_ttl`.
//!
//! The defaults are caches: capacity may evict any entry, swap a persistent
//! adapter for durable data.
//!
//! [`NoLocaleOverlay`] (no runtime overlay) and [`AllowAllGuilds`] (serve
//! every guild) are the trivial defaults of `LocaleSource` and
//! `GuildAccessList`.
//!
//! [`MemoryIdentifyQueue`] grants identifies by Discord's buckets in one
//! process: one Lease per bucket per 5 s, buckets in parallel, FIFO within a
//! bucket. Its memory is one slot per bucket (`max_concurrency`). It does not
//! cover several processes; a Cluster needs a shared `IdentifyQueue`.

mod byte_store;
mod discord;
mod identify;
mod stores;
mod trivial;

use std::time::Duration;

pub use discord::{DiscordCacheResources, MemoryDiscordCache};
pub use identify::MemoryIdentifyQueue;
pub use stores::{
    MemoryAssetStore, MemoryCacheStore, MemoryModuleStateStore, MemoryRateLimitStore,
    MemoryRefStore, MemoryRenderCache, MemorySessionStore,
};
pub use trivial::{AllowAllGuilds, NoLocaleOverlay};

/// The bounds of an in-memory byte store: a byte budget and a maximum TTL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MemoryLimits {
    max_bytes: u64,
    max_ttl: Option<Duration>,
}

impl MemoryLimits {
    /// Bytes added to every entry's weight for bookkeeping.
    pub const ENTRY_OVERHEAD: u64 = 64;

    /// Limits of `max_bytes` in total, each entry living at most `max_ttl`
    /// (`None` = no cap on the TTL).
    ///
    /// # Panics
    ///
    /// If `max_bytes` is 0 or `max_ttl` is `Some(Duration::ZERO)`; in a
    /// `const` this is a compile error. Use [`MemoryLimits::try_new`] for
    /// runtime input.
    pub const fn new(max_bytes: u64, max_ttl: Option<Duration>) -> Self {
        match Self::try_new(max_bytes, max_ttl) {
            Some(limits) => limits,
            None => panic!("MemoryLimits: max_bytes and max_ttl must be non-zero"),
        }
    }

    /// Like [`MemoryLimits::new`], but `None` instead of panicking.
    pub const fn try_new(max_bytes: u64, max_ttl: Option<Duration>) -> Option<Self> {
        if max_bytes == 0 || matches!(max_ttl, Some(ttl) if ttl.is_zero()) {
            return None;
        }
        Some(Self { max_bytes, max_ttl })
    }

    /// The byte budget.
    pub const fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// The maximum TTL of an entry; `None` = uncapped.
    pub const fn max_ttl(&self) -> Option<Duration> {
        self.max_ttl
    }
}

/// What an in-memory byte store holds right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MemoryUsage {
    /// The number of entries.
    pub entries: u64,
    /// Their total weight in bytes (see [`MemoryLimits::ENTRY_OVERHEAD`]).
    pub bytes: u64,
}
