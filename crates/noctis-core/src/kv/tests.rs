use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use noctis_ports::memory::MemoryCacheStore;
use noctis_ports::{BoxFuture, ValueError};
use serde::{Deserialize, Serializer};

use super::*;
use crate::namespace::{AppId, Namespaced};

/// A `MemoryCacheStore` that counts calls and records their keys.
#[derive(Default)]
struct Counting {
    inner: MemoryCacheStore,
    calls: AtomicUsize,
    keys: Mutex<Vec<String>>,
}

impl Counting {
    fn seen(&self, key: &str) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.keys.lock().unwrap().push(key.to_owned());
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl CacheStore for Counting {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.seen(key);
        self.inner.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.seen(key);
        self.inner.put(key, value, ttl)
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<bool, PortError>> {
        self.seen(key);
        self.inner.put_if_absent(key, value, ttl)
    }

    fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
        self.seen(key);
        self.inner.delete(key)
    }

    fn incr_with_ttl<'a>(
        &'a self,
        key: &'a str,
        by: u64,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<u64, PortError>> {
        self.seen(key);
        self.inner.incr_with_ttl(key, by, ttl)
    }
}

/// A `Kv` of App `app` over a fresh counting store.
fn kv() -> (Kv, Arc<Counting>) {
    let store = Arc::new(Counting::default());
    let inner: Arc<dyn CacheStore> = Arc::clone(&store) as Arc<dyn CacheStore>;
    let ns: Arc<dyn CacheStore> = Arc::new(Namespaced::new(&AppId::new("app").unwrap(), inner));
    (Kv::new(ns), store)
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct V {
    n: u32,
}

#[tokio::test]
async fn get_and_put() {
    let (kv, store) = kv();
    kv.put("k", &V { n: 1 }, None).await.unwrap();
    assert_eq!(store.calls(), 1);
    assert_eq!(kv.get::<V>("k").await.unwrap(), Some(V { n: 1 }));
    assert_eq!(store.calls(), 2);
    let raw = store.inner.get("app:cache:k").await.unwrap().unwrap();
    let mut want = vec![0];
    want.extend(postcard_bytes(&V { n: 1 }));
    assert_eq!(raw.as_ref(), want);
    assert_eq!(*store.keys.lock().unwrap(), ["app:cache:k", "app:cache:k"]);
}

fn postcard_bytes(v: &V) -> Vec<u8> {
    encode_value(0, v).unwrap()[1..].to_vec()
}

#[tokio::test]
async fn schema() {
    let (kv, _) = kv();
    kv.clone().schema(1).put("k", &5_u8, None).await.unwrap();
    assert_eq!(kv.clone().schema(2).get::<u8>("k").await.unwrap(), None);
    assert_eq!(kv.clone().schema(1).get::<u8>("k").await.unwrap(), Some(5));
    let high = kv.schema(255);
    high.put("h", "s", None).await.unwrap();
    assert_eq!(high.get::<String>("h").await.unwrap().as_deref(), Some("s"));
    assert_eq!(format!("{high:?}"), "Kv { schema: 255, attached: true }");
}

#[tokio::test]
async fn missing_and_bad_values() {
    let (kv, store) = kv();
    assert_eq!(kv.get::<u64>("none").await.unwrap(), None);
    for (raw, want) in [
        (&[][..], ValueError::Empty),
        (&[0, 0xff][..], ValueError::Malformed),
        (&[0, 1, 2][..], ValueError::Trailing { extra: 1 }),
    ] {
        store
            .inner
            .put("app:cache:bad", Bytes::copy_from_slice(raw), None)
            .await
            .unwrap();
        let err = kv.get::<u64>("bad").await.unwrap_err();
        assert!(
            matches!(err, PortError::Value(got) if got == want),
            "{raw:?}"
        );
    }
}

#[tokio::test]
async fn put_if_absent() {
    let (kv, store) = kv();
    assert!(kv.put_if_absent("k", &1_u8, None).await.unwrap());
    assert!(!kv.put_if_absent("k", &2_u8, None).await.unwrap());
    assert_eq!(store.calls(), 2);
    assert_eq!(kv.get::<u8>("k").await.unwrap(), Some(1));

    let tasks: Vec<_> = (0..32_u8)
        .map(|i| {
            let kv = kv.clone();
            tokio::spawn(async move { kv.put_if_absent("race", &i, None).await.unwrap() })
        })
        .collect();
    let mut winners = 0;
    for task in tasks {
        winners += usize::from(task.await.unwrap());
    }
    assert_eq!(winners, 1);
}

#[tokio::test]
async fn incr_and_counter() {
    let (kv, store) = kv();
    assert_eq!(kv.counter("k").await.unwrap(), 0);
    for want in 1..=3 {
        assert_eq!(kv.incr("k", 1, None).await.unwrap(), want);
    }
    assert_eq!(kv.counter("k").await.unwrap(), 3);
    assert_eq!(
        kv.incr("z", 0, Some(Duration::from_secs(5))).await.unwrap(),
        0
    );
    assert_eq!(store.calls(), 6);

    kv.put("typed", &V { n: 1 }, None).await.unwrap();
    let err = kv.counter("typed").await.unwrap_err();
    assert!(matches!(err, PortError::InvalidValue { ref key, .. } if key == "typed"));
}

#[tokio::test]
async fn incr_saturates() {
    let (kv, store) = kv();
    store
        .inner
        .put(
            "app:cache:k",
            Bytes::copy_from_slice(&(u64::MAX - 1).to_be_bytes()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(kv.incr("k", 5, None).await.unwrap(), u64::MAX);
}

/// The memory store uses `std::time::Instant`, so tokio's paused clock does
/// not move it: this test waits for real.
#[tokio::test]
async fn ttl_expires() {
    let (kv, _) = kv();
    kv.put("k", &1_u8, Some(Duration::from_millis(1)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(kv.get::<u8>("k").await.unwrap(), None);
}

#[tokio::test]
async fn delete() {
    let (kv, store) = kv();
    kv.put("k", &1_u8, None).await.unwrap();
    kv.delete("k").await.unwrap();
    assert_eq!(kv.get::<u8>("k").await.unwrap(), None);
    kv.delete("missing").await.unwrap();
    assert_eq!(store.calls(), 4);
}

struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("no"))
    }
}

#[tokio::test]
async fn serialize_error_makes_no_call() {
    let (kv, store) = kv();
    let err = kv.put("k", &Unserializable, None).await.unwrap_err();
    assert!(matches!(err, PortError::Value(ValueError::Serialize)));
    let err = kv
        .put_if_absent("k", &Unserializable, None)
        .await
        .unwrap_err();
    assert!(matches!(err, PortError::Value(ValueError::Serialize)));
    assert_eq!(store.calls(), 0);
}

#[tokio::test]
async fn detached_is_unavailable() {
    let kv = crate::ComponentCtx::detached().kv();
    assert_eq!(format!("{kv:?}"), "Kv { schema: 0, attached: false }");
    let err = kv.get::<u8>("k").await.unwrap_err();
    assert!(matches!(err, PortError::Unavailable { .. }));
    assert!(matches!(
        kv.put("k", &1_u8, None).await,
        Err(PortError::Unavailable { .. })
    ));
    assert!(matches!(
        kv.counter("k").await,
        Err(PortError::Unavailable { .. })
    ));
}

/// `Send` but not `Sync`.
#[derive(Serialize)]
struct NotSync(std::cell::Cell<u8>);

fn assert_send<F: Future + Send>(_: F) {}

#[test]
fn put_future_is_send_without_sync() {
    let (kv, _) = kv();
    let value = NotSync(std::cell::Cell::new(1));
    assert_send(kv.put("k", &value, None));
    assert_send(kv.put_if_absent("k", &value, None));
    assert_send(kv.get::<u8>("k"));
    assert_send(kv.incr("k", 1, None));
    assert_send(kv.counter("k"));
    assert_send(kv.delete("k"));
}
