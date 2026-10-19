//! The byte-store ports: each one a subset of the shared op vocabulary.
//!
//! Keys are opaque and stored verbatim; values are opaque [`Bytes`]; a `ttl`
//! of `None` means no expiry. An expired entry behaves exactly like a missing
//! one.

use std::time::Duration;

use bytes::Bytes;

use crate::{BoxFuture, PortError};

/// Declares one op of the shared vocabulary, with its contract as rustdoc.
macro_rules! op {
    (get) => {
        /// Reads the value under `key`.
        ///
        /// An expired or missing entry is `None`.
        fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    };
    (put) => {
        /// Stores `value` under `key`, overwriting any value and resetting the
        /// TTL (`None` = no expiry).
        fn put<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<(), PortError>>;
    };
    (put_if_absent) => {
        /// Stores `value` under `key` only if no live entry exists.
        ///
        /// Returns `true` only for the one caller that inserted, even under
        /// concurrency. An expired entry counts as absent.
        fn put_if_absent<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<bool, PortError>>;
    };
    (take) => {
        /// Atomically reads and deletes the value under `key`.
        ///
        /// Exactly one concurrent caller gets `Some`; an expired or missing
        /// entry is `None`.
        fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>>;
    };
    (delete) => {
        /// Removes the entry under `key`. A missing key is not an error.
        fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>>;
    };
    (touch) => {
        /// Sets the expiry of a live entry to now + `ttl`.
        ///
        /// Returns `true` if a live entry existed. A missing or expired entry
        /// gives `false`, and nothing is created.
        fn touch<'a>(
            &'a self,
            key: &'a str,
            ttl: Duration,
        ) -> BoxFuture<'a, Result<bool, PortError>>;
    };
    (incr_with_ttl) => {
        /// Atomically adds `by` to the counter under `key` and returns the new
        /// value; no increment is ever lost.
        ///
        /// The counter is stored as an 8-byte big-endian `u64`, and the add
        /// saturates at `u64::MAX`. A missing or expired entry is created with
        /// the value `by` (`by = 0` creates 0), and `ttl` is applied only then;
        /// an existing counter keeps its expiry. A value that is not an 8-byte
        /// counter gives [`PortError::InvalidValue`].
        fn incr_with_ttl<'a>(
            &'a self,
            key: &'a str,
            by: u64,
            ttl: Option<Duration>,
        ) -> BoxFuture<'a, Result<u64, PortError>>;
    };
}

/// Payloads behind Refs (by-ref and spill Routes).
///
/// Ops: `put_if_absent`, `get`, `take`, `delete`, `touch`.
pub trait RefStore: Send + Sync + 'static {
    op!(put_if_absent);
    op!(get);
    op!(take);
    op!(delete);
    op!(touch);
}

/// Shard session data for gateway resume.
///
/// Ops: `get`, `put`, `delete`.
pub trait SessionStore: Send + Sync + 'static {
    op!(get);
    op!(put);
    op!(delete);
}

/// Module enabled/disabled state, global and per guild.
///
/// Ops: `get`, `put`, `version`.
pub trait ModuleStateStore: Send + Sync + 'static {
    op!(get);
    op!(put);

    /// The store's version: 0 on an empty store, monotonic, and greater after
    /// every `put`. Core polls it to refresh its snapshot.
    fn version(&self) -> BoxFuture<'_, Result<u64, PortError>>;
}

/// Declared rate-limit counters.
///
/// Ops: `incr_with_ttl`, `get`, `put`.
pub trait RateLimitStore: Send + Sync + 'static {
    op!(incr_with_ttl);
    op!(get);
    op!(put);
}

/// General key-value data with TTL for bots and Modules. It is a cache: it may
/// evict.
///
/// Ops: `get`, `put`, `put_if_absent`, `delete`, `incr_with_ttl`.
pub trait CacheStore: Send + Sync + 'static {
    op!(get);
    op!(put);
    op!(put_if_absent);
    op!(delete);
    op!(incr_with_ttl);
}

/// Encoded images by render key.
///
/// Ops: `get`, `put`.
pub trait RenderCache: Send + Sync + 'static {
    op!(get);
    op!(put);
}

/// Fetched remote images (avatars, icons, emoji).
///
/// Ops: `get`, `put`.
pub trait AssetStore: Send + Sync + 'static {
    op!(get);
    op!(put);
}
