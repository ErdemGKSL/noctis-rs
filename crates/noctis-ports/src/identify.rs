//! [`IdentifyQueue`]: identify coordination per bucket.

use crate::{BoxFuture, PortError};

/// Grants gateway identifies, one bucket at a time.
///
/// A bucket is `shard_id % max_concurrency`. A Cluster implementation must
/// serialize per bucket across processes.
pub trait IdentifyQueue: Send + Sync + 'static {
    /// Waits until `bucket` may identify, and returns the [`Lease`] for it.
    fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>>;
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
