//! The one moka-backed byte store behind every memory store.

use std::time::{Duration, Instant};

use bytes::Bytes;
use moka::Expiry;
use moka::ops::compute::{CompResult, Op};
use moka::policy::EvictionPolicy;
use moka::sync::Cache;

use super::{MemoryLimits, MemoryUsage};
use crate::PortError;

const TOO_LARGE: &str = "entry larger than the store's byte budget";

/// A stored value with its absolute deadline (`None` = no expiry).
#[derive(Clone)]
struct Slot {
    value: Bytes,
    deadline: Option<Instant>,
}

impl Slot {
    fn is_live(&self, now: Instant) -> bool {
        self.deadline.is_none_or(|deadline| deadline > now)
    }
}

/// Mirrors each slot's deadline into moka, so expired entries are reclaimed.
struct DeadlineExpiry;

impl DeadlineExpiry {
    fn remaining(slot: &Slot, at: Instant) -> Option<Duration> {
        slot.deadline
            .map(|deadline| deadline.saturating_duration_since(at))
    }
}

impl Expiry<String, Slot> for DeadlineExpiry {
    fn expire_after_create(&self, _key: &String, value: &Slot, at: Instant) -> Option<Duration> {
        Self::remaining(value, at)
    }

    fn expire_after_update(
        &self,
        _key: &String,
        value: &Slot,
        at: Instant,
        _current: Option<Duration>,
    ) -> Option<Duration> {
        Self::remaining(value, at)
    }
}

/// `now + min(ttl, max_ttl)`, ignoring a missing side; `None` when both are
/// missing or the sum overflows.
fn deadline(now: Instant, ttl: Option<Duration>, max_ttl: Option<Duration>) -> Option<Instant> {
    let ttl = match (ttl, max_ttl) {
        (Some(ttl), Some(max)) => ttl.min(max),
        (Some(ttl), None) | (None, Some(ttl)) => ttl,
        (None, None) => return None,
    };
    now.checked_add(ttl)
}

/// The weight of an entry in bytes.
fn weight(key: &str, value_len: usize) -> u64 {
    // usize -> u64 never truncates on supported targets.
    (key.len() as u64)
        .saturating_add(value_len as u64)
        .saturating_add(MemoryLimits::ENTRY_OVERHEAD)
}

/// A byte-budgeted, TTL-capped map; every mutation is atomic per key.
pub(super) struct ByteStore {
    cache: Cache<String, Slot>,
    limits: MemoryLimits,
}

impl ByteStore {
    pub(super) fn new(limits: MemoryLimits) -> Self {
        let cache = Cache::builder()
            .max_capacity(limits.max_bytes())
            .weigher(|key: &String, slot: &Slot| {
                u32::try_from(weight(key, slot.value.len())).unwrap_or(u32::MAX)
            })
            // TinyLFU may refuse a fresh entry; LRU always admits it.
            .eviction_policy(EvictionPolicy::lru())
            .expire_after(DeadlineExpiry)
            .build();
        Self { cache, limits }
    }

    pub(super) const fn limits(&self) -> MemoryLimits {
        self.limits
    }

    pub(super) fn usage(&self) -> MemoryUsage {
        self.cache.run_pending_tasks();
        MemoryUsage {
            entries: self.cache.entry_count(),
            bytes: self.cache.weighted_size(),
        }
    }

    fn check_fits(&self, key: &str, value_len: usize) -> Result<(), PortError> {
        if weight(key, value_len) > self.limits.max_bytes() {
            return Err(PortError::invalid_value(key, TOO_LARGE));
        }
        Ok(())
    }

    fn slot(&self, now: Instant, value: Bytes, ttl: Option<Duration>) -> Slot {
        Slot {
            value,
            deadline: deadline(now, ttl, self.limits.max_ttl()),
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<Bytes> {
        let now = Instant::now();
        self.cache
            .get(key)
            .filter(|slot| slot.is_live(now))
            .map(|slot| slot.value)
    }

    pub(super) fn put(
        &self,
        key: &str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> Result<(), PortError> {
        self.check_fits(key, value.len())?;
        self.cache.entry_by_ref(key).and_compute_with(|_| {
            let slot = self.slot(Instant::now(), value, ttl);
            Op::Put(slot)
        });
        Ok(())
    }

    pub(super) fn put_if_absent(
        &self,
        key: &str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> Result<bool, PortError> {
        self.check_fits(key, value.len())?;
        let result = self.cache.entry_by_ref(key).and_compute_with(|current| {
            let now = Instant::now();
            if current.is_some_and(|entry| entry.value().is_live(now)) {
                Op::Nop
            } else {
                Op::Put(self.slot(now, value, ttl))
            }
        });
        Ok(matches!(
            result,
            CompResult::Inserted(_) | CompResult::ReplacedWith(_)
        ))
    }

    pub(super) fn take(&self, key: &str) -> Option<Bytes> {
        let mut taken = None;
        self.cache.entry_by_ref(key).and_compute_with(|current| {
            let now = Instant::now();
            match current.filter(|entry| entry.value().is_live(now)) {
                Some(entry) => {
                    taken = Some(entry.into_value().value);
                    Op::Remove
                }
                None => Op::Nop,
            }
        });
        taken
    }

    pub(super) fn delete(&self, key: &str) {
        self.cache
            .entry_by_ref(key)
            .and_compute_with(|_| Op::Remove);
    }

    pub(super) fn touch(&self, key: &str, ttl: Duration) -> bool {
        let result = self.cache.entry_by_ref(key).and_compute_with(|current| {
            let now = Instant::now();
            match current.filter(|entry| entry.value().is_live(now)) {
                Some(entry) => Op::Put(self.slot(now, entry.into_value().value, Some(ttl))),
                None => Op::Nop,
            }
        });
        matches!(result, CompResult::ReplacedWith(_))
    }

    pub(super) fn incr_with_ttl(
        &self,
        key: &str,
        by: u64,
        ttl: Option<Duration>,
    ) -> Result<u64, PortError> {
        self.check_fits(key, size_of::<u64>())?;
        let mut outcome = Ok(by);
        self.cache.entry_by_ref(key).and_compute_with(|current| {
            let now = Instant::now();
            let live = current
                .map(moka::Entry::into_value)
                .filter(|slot| slot.is_live(now));
            let Some(slot) = live else {
                return Op::Put(self.slot(now, Bytes::copy_from_slice(&by.to_be_bytes()), ttl));
            };
            if let Ok(raw) = <[u8; 8]>::try_from(slot.value.as_ref()) {
                let next = u64::from_be_bytes(raw).saturating_add(by);
                outcome = Ok(next);
                // An existing counter keeps its deadline.
                Op::Put(Slot {
                    value: Bytes::copy_from_slice(&next.to_be_bytes()),
                    deadline: slot.deadline,
                })
            } else {
                outcome = Err(PortError::invalid_value(key, "not an 8-byte counter"));
                Op::Nop
            }
        });
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn no_ttl_and_no_cap_is_no_deadline() {
        assert_eq!(deadline(Instant::now(), None, None), None);
    }

    #[test]
    fn ttl_is_clamped_to_the_cap() {
        let now = Instant::now();
        assert_eq!(
            deadline(now, Some(60 * SECOND), Some(SECOND)),
            Some(now + SECOND)
        );
        assert_eq!(
            deadline(now, Some(SECOND), Some(60 * SECOND)),
            Some(now + SECOND)
        );
        assert_eq!(deadline(now, None, Some(SECOND)), Some(now + SECOND));
        assert_eq!(deadline(now, Some(SECOND), None), Some(now + SECOND));
    }

    #[test]
    fn overflow_is_no_deadline() {
        assert_eq!(deadline(Instant::now(), Some(Duration::MAX), None), None);
    }

    #[test]
    fn zero_ttl_is_already_dead() {
        let now = Instant::now();
        let slot = Slot {
            value: Bytes::new(),
            deadline: deadline(now, Some(Duration::ZERO), None),
        };
        assert_eq!(slot.deadline, Some(now));
        assert!(!slot.is_live(now));
    }
}
