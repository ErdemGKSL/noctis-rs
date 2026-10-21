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

use crate::namespace::{AppId, Namespaced, namespace};

/// Declares the port slots once: the slot enum, the builder's overrides and
/// the resolved `Ports`. The byte stores come first; core wraps each of them
/// (defaults included) in a [`Namespaced`] port. The plain slots are never
/// wrapped (AD-14).
macro_rules! ports {
    (
        namespaced { $($ns_field:ident: $ns_port:ident = $ns_default:expr;)* }
        plain { $($field:ident: $port:ident = $default:expr;)* }
    ) => {
        /// One port slot of an App.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) enum PortSlot {
            $($ns_port,)*
            $($port,)*
        }

        impl PortSlot {
            /// Every slot, in declaration order.
            #[cfg(test)]
            pub(crate) const ALL: &[PortSlot] = &[$(PortSlot::$ns_port,)* $(PortSlot::$port,)*];
        }

        /// The ports an `AppBuilder` was given; `None` takes the default. A
        /// byte store is kept with whether it was passed as `Shared`.
        #[derive(Default)]
        pub(crate) struct PortOverrides {
            $(pub(crate) $ns_field: Option<(Arc<dyn $ns_port>, bool)>,)*
            $(pub(crate) $field: Option<Arc<dyn $port>>,)*
        }

        impl PortOverrides {
            /// The slots that were set.
            pub(crate) fn set_slots(&self) -> Vec<PortSlot> {
                let mut slots = Vec::new();
                $(if self.$ns_field.is_some() {
                    slots.push(PortSlot::$ns_port);
                })*
                $(if self.$field.is_some() {
                    slots.push(PortSlot::$port);
                })*
                slots
            }
        }

        /// An App's resolved ports: a `Namespaced` port per byte-store slot,
        /// an `Arc<dyn Port>` per plain slot.
        #[allow(dead_code, reason = "read by the gateway and pipeline stories (5.4-5.6)")]
        pub(crate) struct Ports {
            $(pub(crate) $ns_field: Arc<Namespaced<dyn $ns_port>>,)*
            $(pub(crate) $field: Arc<dyn $port>,)*
            defaults: BTreeSet<PortSlot>,
        }

        impl Ports {
            /// Takes every set port, fills the rest with memory defaults and
            /// namespaces every byte store under `app` (or `_shared`).
            pub(crate) fn resolve(overrides: PortOverrides, app: &AppId) -> Self {
                let mut defaults = BTreeSet::new();
                $(
                    let $ns_field = namespace::<dyn $ns_port>(
                        app,
                        overrides.$ns_field.unwrap_or_else(|| {
                            defaults.insert(PortSlot::$ns_port);
                            (Arc::new($ns_default), false)
                        }),
                    );
                )*
                $(
                    let $field: Arc<dyn $port> = overrides.$field.unwrap_or_else(|| {
                        defaults.insert(PortSlot::$port);
                        Arc::new($default)
                    });
                )*
                Self { $($ns_field,)* $($field,)* defaults }
            }
        }
    };
}

ports! {
    namespaced {
        ref_store: RefStore = MemoryRefStore::new();
        session_store: SessionStore = MemorySessionStore::new();
        module_state_store: ModuleStateStore = MemoryModuleStateStore::new();
        rate_limit_store: RateLimitStore = MemoryRateLimitStore::new();
        cache_store: CacheStore = MemoryCacheStore::new();
        render_cache: RenderCache = MemoryRenderCache::new();
        asset_store: AssetStore = MemoryAssetStore::new();
    }
    plain {
        identify_queue: IdentifyQueue = MemoryIdentifyQueue::default();
        locale_source: LocaleSource = NoLocaleOverlay;
        guild_access: GuildAccessList = AllowAllGuilds;
        discord_cache_backend: DiscordCache = MemoryDiscordCache::new(DiscordCacheResources::empty());
    }
}

impl Ports {
    /// The slots that took their memory default.
    pub(crate) fn default_slots(&self) -> &BTreeSet<PortSlot> {
        &self.defaults
    }

    /// Whether `slot` took its memory default.
    pub(crate) fn is_default(&self, slot: PortSlot) -> bool {
        self.defaults.contains(&slot)
    }
}
