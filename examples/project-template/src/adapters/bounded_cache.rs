// NOTE: Compiles against the real `noctis::ports` API (checked by noctis-rs `crates/noctis/tests/template_adapter.rs`) and passes `noctis::testkit::conformance::cache_store` (run in noctis-rs `crates/noctis-testkit/tests/conformance.rs`).

//! A custom `CacheStore` Adapter: a bounded in-process map with TTL.
//!
//! It exists to show the shape every Adapter has (AD-14):
//! - `Send + Sync + 'static`, dyn-compatible, methods return `BoxFuture<Result<_, PortError>>`;
//! - keys are opaque and stored verbatim: core already prefixed them with `<app>:cache:`;
//! - values are opaque `Bytes`; counters are 8-byte big-endian `u64`;
//! - `put_if_absent` and `incr_with_ttl` are atomic;
//! - bounded: it never grows past `capacity` entries (UD-8).
//!
//! A Redis or SQL Adapter has the same shape. Prove it with
//! `noctis::testkit::conformance::cache_store` (see `tests/adapters.rs`).

use std::collections::HashMap;
use std::future::ready;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use noctis::bytes::Bytes;
use noctis::ports::{BoxFuture, CacheStore, PortError};

/// Bounded TTL map. When full it drops expired entries first, then the entry closest
/// to expiry (entries without TTL go last).
pub struct BoundedCacheStore {
    capacity: usize,
    entries: Mutex<HashMap<String, Entry>>,
}

struct Entry {
    value: Bytes,
    expires_at: Option<Instant>,
}

impl Entry {
    fn is_live(&self, now: Instant) -> bool {
        self.expires_at.is_none_or(|at| at > now)
    }
}

impl BoundedCacheStore {
    /// `capacity` is the maximum number of entries; it must be at least 1.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        // A panic while holding the lock cannot leave an entry half-written: every
        // mutation below is a single insert or remove. Keep serving.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn insert(map: &mut HashMap<String, Entry>, capacity: usize, key: &str, entry: Entry) {
        if !map.contains_key(key) && map.len() >= capacity {
            let now = Instant::now();
            map.retain(|_, e| e.is_live(now));
            if map.len() >= capacity {
                let victim = map
                    .iter()
                    .min_by_key(|(_, e)| e.expires_at.map_or((1, now), |at| (0, at)))
                    .map(|(k, _)| k.clone());
                if let Some(victim) = victim {
                    map.remove(&victim);
                }
            }
        }
        map.insert(key.to_owned(), entry);
    }
}

fn expiry(ttl: Option<Duration>) -> Option<Instant> {
    ttl.map(|ttl| Instant::now() + ttl)
}

impl CacheStore for BoundedCacheStore {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        let now = Instant::now();
        let value = self
            .lock()
            .get(key)
            .filter(|e| e.is_live(now))
            .map(|e| e.value.clone());
        Box::pin(ready(Ok(value)))
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        let entry = Entry {
            value,
            expires_at: expiry(ttl),
        };
        Self::insert(&mut self.lock(), self.capacity, key, entry);
        Box::pin(ready(Ok(())))
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        let now = Instant::now();
        let mut map = self.lock();
        let inserted = if map.get(key).is_some_and(|e| e.is_live(now)) {
            false
        } else {
            let entry = Entry {
                value,
                expires_at: expiry(ttl),
            };
            Self::insert(&mut map, self.capacity, key, entry);
            true
        };
        Box::pin(ready(Ok(inserted)))
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.lock().remove(key);
        Box::pin(ready(Ok(())))
    }

    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        let now = Instant::now();
        let mut map = self.lock();
        // Existing counter: add, keep its TTL (TTL is set on create only).
        let result = if let Some(entry) = map.get_mut(key).filter(|e| e.is_live(now)) {
            match <[u8; 8]>::try_from(entry.value.as_ref()) {
                Ok(raw) => {
                    let next = u64::from_be_bytes(raw).saturating_add(by);
                    entry.value = Bytes::copy_from_slice(&next.to_be_bytes());
                    Ok(next)
                }
                Err(_) => Err(PortError::invalid_value(key, "not an 8-byte counter")),
            }
        } else {
            let entry = Entry {
                value: Bytes::copy_from_slice(&by.to_be_bytes()),
                expires_at: expiry(ttl),
            };
            Self::insert(&mut map, self.capacity, key, entry);
            Ok(by)
        };
        Box::pin(ready(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evicts_instead_of_growing() {
        let store = BoundedCacheStore::new(2);
        for key in ["a", "b", "c"] {
            store
                .put(key, Bytes::from_static(b"x"), None)
                .await
                .unwrap();
        }
        assert_eq!(store.lock().len(), 2);
    }
}
