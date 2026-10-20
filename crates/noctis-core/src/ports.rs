//! The App's port slots and their bounded in-memory defaults.

use std::collections::BTreeSet;
use std::sync::Arc;

use noctis_ports::memory::{
    AllowAllGuilds, DiscordCacheResources, MemoryAssetStore, MemoryCacheStore, MemoryDiscordCache,
    MemoryIdentifyQueue, MemoryModuleStateStore, MemoryRateLimitStore, MemoryRefStore,
    MemoryRenderCache, MemorySessionStore, NoLocaleOverlay,
};
use noctis_ports::{
    AssetStore, CacheStore, DiscordCache, GuildAccessList, IdentifyQueue, LocaleSource,
    ModuleStateStore, RateLimitStore, RefStore, RenderCache, SessionStore,
};

/// Declares the port slots once: the slot enum, the builder's overrides and
/// the resolved `Ports`.
macro_rules! ports {
    ($($field:ident: $port:ident = $default:expr;)*) => {
        /// One port slot of an App.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) enum PortSlot {
            $($port,)*
        }

        impl PortSlot {
            /// Every slot, in declaration order.
            #[cfg(test)]
            pub(crate) const ALL: &[PortSlot] = &[$(PortSlot::$port,)*];
        }

        /// The ports an `AppBuilder` was given; `None` takes the default.
        #[derive(Default)]
        pub(crate) struct PortOverrides {
            $(pub(crate) $field: Option<Arc<dyn $port>>,)*
        }

        impl PortOverrides {
            /// The slots that were set.
            pub(crate) fn set_slots(&self) -> Vec<PortSlot> {
                let mut slots = Vec::new();
                $(if self.$field.is_some() {
                    slots.push(PortSlot::$port);
                })*
                slots
            }
        }

        /// An App's resolved ports: one `Arc<dyn Port>` per slot.
        #[allow(dead_code, reason = "read by the gateway and pipeline stories (5.4-5.6)")]
        pub(crate) struct Ports {
            $(pub(crate) $field: Arc<dyn $port>,)*
            defaults: BTreeSet<PortSlot>,
        }

        impl Ports {
            /// Takes every set port and fills the rest with memory defaults.
            pub(crate) fn resolve(overrides: PortOverrides) -> Self {
                let mut defaults = BTreeSet::new();
                $(
                    let $field: Arc<dyn $port> = overrides.$field.unwrap_or_else(|| {
                        defaults.insert(PortSlot::$port);
                        Arc::new($default)
                    });
                )*
                Self { $($field,)* defaults }
            }
        }
    };
}

ports! {
    ref_store: RefStore = MemoryRefStore::new();
    session_store: SessionStore = MemorySessionStore::new();
    identify_queue: IdentifyQueue = MemoryIdentifyQueue::default();
    module_state_store: ModuleStateStore = MemoryModuleStateStore::new();
    rate_limit_store: RateLimitStore = MemoryRateLimitStore::new();
    cache_store: CacheStore = MemoryCacheStore::new();
    render_cache: RenderCache = MemoryRenderCache::new();
    asset_store: AssetStore = MemoryAssetStore::new();
    locale_source: LocaleSource = NoLocaleOverlay;
    guild_access: GuildAccessList = AllowAllGuilds;
    discord_cache_backend: DiscordCache = MemoryDiscordCache::new(DiscordCacheResources::empty());
}

impl Ports {
    /// The slots that took their memory default.
    pub(crate) fn default_slots(&self) -> &BTreeSet<PortSlot> {
        &self.defaults
    }

    /// Whether `slot` took its memory default.
    #[cfg(test)]
    pub(crate) fn is_default(&self, slot: PortSlot) -> bool {
        self.defaults.contains(&slot)
    }
}
