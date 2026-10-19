//! [`MemoryIdentifyQueue`]: the in-process default of `IdentifyQueue`.

use std::fmt;
use std::num::NonZeroU16;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::{BoxFuture, IdentifyQueue, Lease, PortError};

/// The default [`IdentifyQueue`]: Discord's identify buckets in one process.
///
/// A bucket is `shard_id % max_concurrency`; the caller computes it. Each
/// bucket grants at most one [`Lease`] per [`IDENTIFY_INTERVAL`](Self::IDENTIFY_INTERVAL)
/// (5 s), and different buckets grant in parallel. Waiters in one bucket are
/// granted in arrival order (FIFO). A bucket at or above `max_concurrency` is
/// a [`PortError::InvalidValue`] and is never folded with `%`.
///
/// An `acquire` future dropped before it is granted takes nothing, so it is
/// cancel-safe. The clock is `tokio::time`, so a paused tokio clock drives it.
///
/// This queue does not cover several processes. A Cluster implementation must
/// serialize per bucket across processes: at most one Lease per bucket per
/// 5 s across the whole Cluster, different buckets in parallel. See the
/// [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md)
/// (written with Story 14.6).
///
/// # Panics
///
/// Waiting needs a tokio runtime with the time driver enabled; `acquire`
/// panics when it must wait outside one. The first acquire in a bucket never
/// waits.
pub struct MemoryIdentifyQueue {
    /// Per bucket, the earliest instant it may grant next (`None` = now).
    buckets: Box<[Mutex<Option<Instant>>]>,
    max_concurrency: NonZeroU16,
}

impl MemoryIdentifyQueue {
    /// The time between two identifies in one bucket.
    pub const IDENTIFY_INTERVAL: Duration = Duration::from_secs(5);

    /// A queue of `max_concurrency` buckets (Discord's
    /// `session_start_limit.max_concurrency`).
    pub fn new(max_concurrency: NonZeroU16) -> Self {
        let buckets = (0..max_concurrency.get())
            .map(|_| Mutex::new(None))
            .collect();
        Self {
            buckets,
            max_concurrency,
        }
    }

    /// The number of buckets.
    pub const fn max_concurrency(&self) -> NonZeroU16 {
        self.max_concurrency
    }
}

impl Default for MemoryIdentifyQueue {
    /// One bucket: `max_concurrency` = 1.
    fn default() -> Self {
        Self::new(NonZeroU16::MIN)
    }
}

impl fmt::Debug for MemoryIdentifyQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryIdentifyQueue")
            .field("max_concurrency", &self.max_concurrency())
            .field("interval", &Self::IDENTIFY_INTERVAL)
            .finish()
    }
}

impl IdentifyQueue for MemoryIdentifyQueue {
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>> {
        Box::pin(async move {
            let slot = usize::try_from(bucket)
                .ok()
                .and_then(|i| self.buckets.get(i))
                .ok_or_else(|| {
                    PortError::invalid_value(
                        &bucket.to_string(),
                        "bucket must be below max_concurrency",
                    )
                })?;
            // The guard is held across the sleep: tokio's Mutex is FIFO, so
            // waiters in this bucket are granted in arrival order.
            let mut next = slot.lock().await;
            if let Some(at) = *next {
                tokio::time::sleep_until(at).await;
            }
            *next = Some(Instant::now() + Self::IDENTIFY_INTERVAL);
            drop(next);
            Ok(Lease::new(bucket))
        })
    }
}
