//! The project template's custom `CacheStore` Adapter compiles against the
//! real `noctis::ports` API and honours the op contracts it uses.

#[path = "../../../examples/project-template/src/adapters/bounded_cache.rs"]
mod bounded_cache;

use std::sync::Arc;

use bounded_cache::BoundedCacheStore;
use noctis::bytes::Bytes;
use noctis::ports::CacheStore;

#[tokio::test]
async fn bounded_cache_store_honours_the_cache_store_contract() {
    let store: Arc<dyn CacheStore> = Arc::new(BoundedCacheStore::new(2));
    let v = Bytes::from_static(b"v");

    store.put("a", v.clone(), None).await.unwrap();
    assert_eq!(store.get("a").await.unwrap(), Some(v.clone()));
    assert!(
        !store
            .put_if_absent("a", Bytes::from_static(b"w"), None)
            .await
            .unwrap()
    );
    assert_eq!(store.get("a").await.unwrap(), Some(v.clone()));
    store.delete("a").await.unwrap();
    assert_eq!(store.get("a").await.unwrap(), None);

    assert_eq!(store.incr_with_ttl("n", 5, None).await.unwrap(), 5);
    assert_eq!(store.incr_with_ttl("n", 5, None).await.unwrap(), 10);
    store.delete("n").await.unwrap();

    for key in ["x", "y", "z"] {
        store.put(key, v.clone(), None).await.unwrap();
    }
    let mut hits = 0;
    for key in ["x", "y", "z"] {
        hits += usize::from(store.get(key).await.unwrap().is_some());
    }
    assert!(hits <= 2, "{hits} of 3 keys survived a capacity of 2");
}
