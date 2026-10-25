//! The conformance suites (Story 14.1, FR-39): every in-memory default and
//! the template's `BoundedCacheStore` pass, and deliberately broken stores
//! fail on the case they break.

#[path = "../../../examples/project-template/src/adapters/bounded_cache.rs"]
mod bounded_cache;

use std::any::Any;
use std::num::NonZeroU16;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use bounded_cache::BoundedCacheStore;
use bytes::Bytes;
use futures_util::FutureExt;
use noctis_ports::memory::{
    DiscordCacheResources, MemoryAssetStore, MemoryCacheStore, MemoryDiscordCache,
    MemoryIdentifyQueue, MemoryModuleStateStore, MemoryRateLimitStore, MemoryRefStore,
    MemoryRenderCache, MemorySessionStore,
};
use noctis_ports::{BoxFuture, CacheStore, PortError, RateLimitStore, RefStore};
use noctis_testkit::conformance;

#[tokio::test(flavor = "multi_thread")]
async fn memory_ref_store_conforms() {
    conformance::ref_store(MemoryRefStore::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_session_store_conforms() {
    conformance::session_store(MemorySessionStore::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_module_state_store_conforms() {
    conformance::module_state_store(MemoryModuleStateStore::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_rate_limit_store_conforms() {
    conformance::rate_limit_store(MemoryRateLimitStore::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_cache_store_conforms() {
    conformance::cache_store(MemoryCacheStore::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_render_cache_conforms() {
    conformance::render_cache(MemoryRenderCache::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_asset_store_conforms() {
    conformance::asset_store(MemoryAssetStore::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_identify_queue_conforms() {
    conformance::identify_queue(|| {
        MemoryIdentifyQueue::new(NonZeroU16::new(2).unwrap_or(NonZeroU16::MIN))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn memory_discord_cache_conforms() {
    conformance::discord_cache(MemoryDiscordCache::new).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn template_bounded_cache_store_conforms() {
    conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}

/// The text of a panic payload.
fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

/// Runs `suite`, which must panic with the suite's prefix and `case`.
async fn assert_fails(suite: impl Future<Output = ()>, prefix: &str, case: &str) {
    let Err(payload) = AssertUnwindSafe(suite).catch_unwind().await else {
        panic!("the suite passed a broken store; expected it to fail `{case}`");
    };
    let text = panic_text(payload.as_ref());
    assert!(text.starts_with(prefix), "unexpected panic: {text}");
    assert!(text.contains(case), "expected `{case}`, got: {text}");
}

/// A `CacheStore` that ignores every TTL.
struct TtlIgnoringCache(MemoryCacheStore);

impl CacheStore for TtlIgnoringCache {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.put(key, value, None)
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        self.0.put_if_absent(key, value, None)
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.delete(key)
    }

    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        _ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        self.0.incr_with_ttl(key, by, None)
    }
}

/// A `RefStore` whose `take` is a non-atomic get, yield, delete.
struct RacyTakeRefs(MemoryRefStore);

impl RefStore for RacyTakeRefs {
    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        self.0.put_if_absent(key, value, ttl)
    }

    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.get(key)
    }

    fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        Box::pin(async move {
            let value = self.0.get(key).await?;
            tokio::task::yield_now().await;
            self.0.delete(key).await?;
            Ok(value)
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.delete(key)
    }

    fn touch<'a>(&'a self, key: &'a str, ttl: Duration) -> BoxFuture<'a, Result<bool, PortError>> {
        self.0.touch(key, ttl)
    }
}

/// A `RateLimitStore` whose `incr_with_ttl` reads, yields, then writes, so
/// concurrent increments are lost.
struct LostIncrements(MemoryRateLimitStore);

impl RateLimitStore for LostIncrements {
    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        Box::pin(async move {
            let current = self.0.get(key).await?;
            tokio::task::yield_now().await;
            let (next, ttl) = match current {
                None => (by, ttl),
                Some(raw) => {
                    let raw = <[u8; 8]>::try_from(raw.as_ref())
                        .map_err(|_| PortError::invalid_value(key, "not an 8-byte counter"))?;
                    (u64::from_be_bytes(raw).saturating_add(by), None)
                }
            };
            self.0
                .put(key, Bytes::copy_from_slice(&next.to_be_bytes()), ttl)
                .await?;
            Ok(next)
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.put(key, value, ttl)
    }
}

/// A `CacheStore` whose `put_if_absent` is a non-atomic get, yield, put.
struct RacyPutIfAbsent(MemoryCacheStore);

impl CacheStore for RacyPutIfAbsent {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.put(key, value, ttl)
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        Box::pin(async move {
            let current = self.0.get(key).await?;
            tokio::task::yield_now().await;
            if current.is_some() {
                return Ok(false);
            }
            self.0.put(key, value, ttl).await?;
            Ok(true)
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.delete(key)
    }

    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        self.0.incr_with_ttl(key, by, ttl)
    }
}

/// A `RateLimitStore` whose `incr_with_ttl` re-applies a given TTL on every
/// call, not only when it creates the counter.
struct TtlResettingIncr(MemoryRateLimitStore);

impl RateLimitStore for TtlResettingIncr {
    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        Box::pin(async move {
            let next = self.0.incr_with_ttl(key, by, ttl).await?;
            if ttl.is_some() {
                self.0
                    .put(key, Bytes::copy_from_slice(&next.to_be_bytes()), ttl)
                    .await?;
            }
            Ok(next)
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.0.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.0.put(key, value, ttl)
    }
}

#[tokio::test]
async fn ttl_ignoring_cache_store_fails_a_ttl_case() {
    assert_fails(
        conformance::cache_store(|| TtlIgnoringCache(MemoryCacheStore::new())),
        "conformance::cache_store: case ",
        "case `ttl_",
    )
    .await;
}

#[tokio::test]
async fn racy_take_fails_race_take() {
    assert_fails(
        conformance::ref_store(|| RacyTakeRefs(MemoryRefStore::new())),
        "conformance::ref_store: case ",
        "case `race_take`",
    )
    .await;
}

#[tokio::test]
async fn lost_increments_fail_race_incr() {
    assert_fails(
        conformance::rate_limit_store(|| LostIncrements(MemoryRateLimitStore::new())),
        "conformance::rate_limit_store: case ",
        "case `race_incr`",
    )
    .await;
}

#[tokio::test]
async fn racy_put_if_absent_fails_race_put_if_absent() {
    assert_fails(
        conformance::cache_store(|| RacyPutIfAbsent(MemoryCacheStore::new())),
        "conformance::cache_store: case ",
        "case `race_put_if_absent`",
    )
    .await;
}

#[tokio::test]
async fn ttl_resetting_incr_fails_ttl_incr_create_only() {
    assert_fails(
        conformance::rate_limit_store(|| TtlResettingIncr(MemoryRateLimitStore::new())),
        "conformance::rate_limit_store: case ",
        "case `ttl_incr_create_only`",
    )
    .await;
}

#[tokio::test]
async fn resource_ignoring_discord_cache_fails_disabled() {
    assert_fails(
        Box::pin(conformance::discord_cache(|_| {
            MemoryDiscordCache::new(DiscordCacheResources::all())
        })),
        "conformance::discord_cache: case ",
        "case `disabled_",
    )
    .await;
}
