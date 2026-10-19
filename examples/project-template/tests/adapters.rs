// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Our custom Adapter must pass the same conformance suite as Noctis's memory default
//! (FR-39): TTL, overwrite, delete, missing key, and atomicity under concurrent callers.

use noctis::testkit::conformance;
use noctis_bot_template::adapters::BoundedCacheStore;

#[tokio::test]
async fn bounded_cache_store_conforms() {
    conformance::cache_store(|| BoundedCacheStore::new(10_000)).await;
}
