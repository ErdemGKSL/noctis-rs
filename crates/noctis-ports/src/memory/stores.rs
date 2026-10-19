//! The seven byte stores: thin wrappers over one [`ByteStore`], each
//! implementing only its port.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;

use super::byte_store::ByteStore;
use super::{MemoryLimits, MemoryUsage};
use crate::{
    AssetStore, BoxFuture, CacheStore, ModuleStateStore, PortError, RateLimitStore, RefStore,
    RenderCache, SessionStore,
};

const MIB: u64 = 1024 * 1024;
const DAY: Duration = Duration::from_hours(24);

/// Declares a memory store: the struct, its shared inherent API, `Default`
/// and a `Debug` that prints only the limits.
macro_rules! memory_store {
    (
        $(#[$doc:meta])*
        $name:ident: $port:literal, $bytes:expr, $ttl:expr $(, $field:ident: $field_ty:ty)* $(,)?
    ) => {
        $(#[$doc])*
        pub struct $name {
            inner: ByteStore,
            $($field: $field_ty,)*
        }

        impl $name {
            #[doc = concat!("The documented defaults of the in-memory `", $port, "`.")]
            pub const DEFAULT_LIMITS: MemoryLimits = MemoryLimits::new($bytes, $ttl);

            /// An empty store with [`Self::DEFAULT_LIMITS`].
            pub fn new() -> Self {
                Self::with_limits(Self::DEFAULT_LIMITS)
            }

            /// An empty store with `limits`.
            pub fn with_limits(limits: MemoryLimits) -> Self {
                Self {
                    inner: ByteStore::new(limits),
                    $($field: <$field_ty>::default(),)*
                }
            }

            /// The store's byte budget and maximum TTL.
            pub const fn limits(&self) -> MemoryLimits {
                self.inner.limits()
            }

            /// The current entry count and weight in bytes, after pending
            /// evictions and expirations have run.
            pub fn usage(&self) -> MemoryUsage {
                self.inner.usage()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("limits", &self.limits())
                    .finish_non_exhaustive()
            }
        }
    };
}

/// Implements one op of the shared vocabulary by delegating to `self.inner`.
macro_rules! delegate {
    (get) => {
        fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            Box::pin(async move { Ok(self.inner.get(key)) })
        }
    };
    (put) => {
        fn put<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<(), PortError>> {
            Box::pin(async move { self.inner.put(key, value, ttl) })
        }
    };
    (put_if_absent) => {
        fn put_if_absent<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<bool, PortError>> {
            Box::pin(async move { self.inner.put_if_absent(key, value, ttl) })
        }
    };
    (take) => {
        fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            Box::pin(async move { Ok(self.inner.take(key)) })
        }
    };
    (delete) => {
        fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
            Box::pin(async move {
                self.inner.delete(key);
                Ok(())
            })
        }
    };
    (touch) => {
        fn touch<'a>(
            &'a self,
            key: &'a str,
            ttl: Duration,
        ) -> BoxFuture<'a, Result<bool, PortError>> {
            Box::pin(async move { Ok(self.inner.touch(key, ttl)) })
        }
    };
    (incr_with_ttl) => {
        fn incr_with_ttl<'a>(
            &'a self,
            key: &'a str,
            by: u64,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<u64, PortError>> {
            Box::pin(async move { self.inner.incr_with_ttl(key, by, ttl) })
        }
    };
}

memory_store! {
    /// The in-memory [`RefStore`]: 32 MiB, entries live at most 7 days.
    MemoryRefStore: "RefStore", 32 * MIB, Some(Duration::from_hours(7 * 24))
}

memory_store! {
    /// The in-memory [`SessionStore`]: 1 MiB, no TTL cap. Sessions resume
    /// only within one process life.
    MemorySessionStore: "SessionStore", MIB, None
}

memory_store! {
    /// The in-memory [`ModuleStateStore`]: 16 MiB, no TTL cap. `version()`
    /// starts at 0 and grows by one on every successful `put`.
    MemoryModuleStateStore: "ModuleStateStore", 16 * MIB, None, version: AtomicU64
}

memory_store! {
    /// The in-memory [`RateLimitStore`]: 16 MiB, entries live at most 1 day.
    MemoryRateLimitStore: "RateLimitStore", 16 * MIB, Some(DAY)
}

memory_store! {
    /// The in-memory [`CacheStore`]: 32 MiB, no TTL cap.
    MemoryCacheStore: "CacheStore", 32 * MIB, None
}

memory_store! {
    /// The in-memory [`RenderCache`]: 64 MiB, entries live at most 1 day.
    MemoryRenderCache: "RenderCache", 64 * MIB, Some(DAY)
}

memory_store! {
    /// The in-memory [`AssetStore`]: 32 MiB, entries live at most 1 day.
    MemoryAssetStore: "AssetStore", 32 * MIB, Some(DAY)
}

impl RefStore for MemoryRefStore {
    delegate!(put_if_absent);
    delegate!(get);
    delegate!(take);
    delegate!(delete);
    delegate!(touch);
}

impl SessionStore for MemorySessionStore {
    delegate!(get);
    delegate!(put);
    delegate!(delete);
}

impl ModuleStateStore for MemoryModuleStateStore {
    delegate!(get);

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        Box::pin(async move {
            self.inner.put(key, value, ttl)?;
            self.version.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    }

    fn version(&self) -> BoxFuture<'_, Result<u64, PortError>> {
        Box::pin(async move { Ok(self.version.load(Ordering::Acquire)) })
    }
}

impl RateLimitStore for MemoryRateLimitStore {
    delegate!(incr_with_ttl);
    delegate!(get);
    delegate!(put);
}

impl CacheStore for MemoryCacheStore {
    delegate!(get);
    delegate!(put);
    delegate!(put_if_absent);
    delegate!(delete);
    delegate!(incr_with_ttl);
}

impl RenderCache for MemoryRenderCache {
    delegate!(get);
    delegate!(put);
}

impl AssetStore for MemoryAssetStore {
    delegate!(get);
    delegate!(put);
}
