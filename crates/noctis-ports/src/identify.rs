//! [`IdentifyQueue`]: identify coordination per bucket.

use crate::{BoxFuture, PortError};

/// Grants gateway identifies, one bucket at a time.
///
/// A bucket is `shard_id % max_concurrency`. Each bucket grants at most one
/// [`Lease`] per 5 s; different buckets grant in parallel.
///
/// [`memory::MemoryIdentifyQueue`](crate::memory::MemoryIdentifyQueue) is the
/// in-process default; it does not cover several processes.
///
/// A Cluster implementation must serialize per bucket across processes: at
/// most one Lease per bucket per 5 s across the whole Cluster, different
/// buckets in parallel. See the [Cluster deployment guide](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/guides/cluster.md)
/// (written with Story 14.6).
pub trait IdentifyQueue: Send + Sync + 'static {
    /// Waits until `bucket` may identify, and returns the [`Lease`] for it.
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>>;
}

/// Forwards [`acquire`](IdentifyQueue::acquire) to the shared instance, so one
/// queue can serialize identifies for several Apps of one Cluster
/// (`Arc::clone` per App); results and errors pass through unchanged.
///
/// ```
/// use std::{num::NonZeroU16, sync::Arc};
///
/// use noctis_ports::{IdentifyQueue, memory::MemoryIdentifyQueue};
///
/// fn takes_queue(_queue: impl IdentifyQueue) {}
///
/// let shared = Arc::new(MemoryIdentifyQueue::new(NonZeroU16::MIN));
/// takes_queue(Arc::clone(&shared));
/// takes_queue(shared);
/// ```
impl<P: IdentifyQueue + ?Sized> IdentifyQueue for std::sync::Arc<P> {
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>> {
        (**self).acquire(bucket)
    }
}

/// Permission for one identify in its bucket.
///
/// A Cluster implementation of [`IdentifyQueue`] serializes leases per bucket
/// across processes. A Lease is not `Clone`: one Lease, one identify.
#[must_use = "a Lease grants one identify; use it"]
#[derive(Debug, PartialEq, Eq)]
pub struct Lease {
    bucket: u32,
}

impl Lease {
    /// A Lease for `bucket`.
    pub const fn new(bucket: u32) -> Self {
        Self { bucket }
    }

    /// The bucket this Lease grants an identify in.
    pub const fn bucket(&self) -> u32 {
        self.bucket
    }
}
