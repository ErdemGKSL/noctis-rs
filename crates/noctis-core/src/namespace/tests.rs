use std::sync::Mutex;

use noctis_ports::memory::{
    MemoryCacheStore, MemoryModuleStateStore, MemoryRateLimitStore, MemoryRefStore,
    MemorySessionStore,
};

use super::*;

fn id(raw: &str) -> AppId {
    AppId::new(raw).unwrap()
}

// ---- AppId (rows 1-5) ----

#[test]
fn app_id_bounds() {
    for raw in ["a", "0", "-", "my-bot-1"] {
        assert_eq!(AppId::new(raw).unwrap().as_str(), raw);
    }
    let max = "a".repeat(32);
    assert_eq!(AppId::new(&max).unwrap().as_str(), max);
    assert_eq!(
        AppId::new(&"a".repeat(33)),
        Err(AppIdError::TooLong { len: 33 })
    );
    assert_eq!(AppId::new(""), Err(AppIdError::Empty));
}

#[test]
fn app_id_invalid_chars() {
    for (raw, index, ch) in [
        ("My-bot", 0, 'M'),
        ("a_b", 1, '_'),
        ("a:b", 1, ':'),
        ("a b", 1, ' '),
        (" a", 0, ' '),
        ("a.", 1, '.'),
        ("bötü", 1, 'ö'),
    ] {
        assert_eq!(
            AppId::new(raw),
            Err(AppIdError::InvalidChar { index, ch }),
            "{raw:?}"
        );
    }
    assert_eq!(
        AppId::new(&"ö".repeat(33)),
        Err(AppIdError::TooLong { len: 33 })
    );
}

#[test]
fn app_id_traits_and_messages() {
    let parsed: AppId = "bot-a".parse().unwrap();
    assert_eq!(parsed, id("bot-a"));
    assert_eq!(parsed.to_string(), "bot-a");
    assert_eq!(format!("{parsed:?}"), r#"AppId("bot-a")"#);
    assert_eq!(fallback_app_id().as_str(), "default");
    for err in [
        AppIdError::Empty,
        AppIdError::TooLong { len: 40 },
        AppIdError::InvalidChar { index: 1, ch: '_' },
    ] {
        assert!(err.to_string().contains("[a-z0-9-]{1,32}"), "{err}");
    }
    assert_eq!(
        AppIdError::InvalidChar { index: 1, ch: '_' }.to_string(),
        "App id has `_` at index 1: use only a-z, 0-9 and - ([a-z0-9-]{1,32})"
    );
}

// ---- base36 (row 6) ----

#[test]
fn base36() {
    for (raw, want) in [
        (1, "1"),
        (35, "z"),
        (36, "10"),
        (1_234_567_890_123_456_789, "9do1sj396nf9"),
        (u64::MAX, "3w5e11264sgsf"),
    ] {
        let got = AppId::from_application_id(Id::new(raw));
        assert_eq!(got.as_str(), want, "{raw}");
        assert_eq!(AppId::new(got.as_str()).unwrap(), got);
    }
}

// ---- escape_segment (row 17) ----

#[test]
fn escape() {
    assert_eq!(escape_segment("a:b"), "a%3Ab");
    assert_eq!(escape_segment("%"), "%25");
    assert_eq!(escape_segment("%3A"), "%253A");
    assert!(matches!(escape_segment(""), Cow::Borrowed("")));
    assert!(matches!(escape_segment("abc"), Cow::Borrowed("abc")));
}

// ---- a recording inner store ----

/// Records every call as `(op, key)`; answers fixed values, or `fail`.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<(&'static str, String)>>,
    fail: Option<fn() -> PortError>,
}

impl Recorder {
    fn failing(fail: fn() -> PortError) -> Self {
        Self {
            fail: Some(fail),
            ..Self::default()
        }
    }

    fn record<T: Send + 'static>(
        &self,
        op: &'static str,
        key: &str,
        value: T,
    ) -> BoxFuture<'static, Result<T, PortError>> {
        self.calls.lock().unwrap().push((op, key.to_owned()));
        let result = match self.fail {
            Some(fail) => Err(fail()),
            None => Ok(value),
        };
        Box::pin(async move { result })
    }

    fn calls(&self) -> Vec<(&'static str, String)> {
        self.calls.lock().unwrap().clone()
    }
}

macro_rules! recorder_op {
    (get) => {
        fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            self.record("get", key, Some(Bytes::from_static(b"v")))
        }
    };
    (put) => {
        fn put<'a>(
            &'a self,
            key: &'a str,
            value: Bytes,
            _: Option<Duration>,
        ) -> BoxFuture<'a, Result<(), PortError>> {
            assert_eq!(value.as_ref(), b"x");
            self.record("put", key, ())
        }
    };
    (put_if_absent) => {
        fn put_if_absent<'a>(
            &'a self,
            key: &'a str,
            _: Bytes,
            _: Option<Duration>,
        ) -> BoxFuture<'a, Result<bool, PortError>> {
            self.record("put_if_absent", key, true)
        }
    };
    (take) => {
        fn take<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
            self.record("take", key, Some(Bytes::from_static(b"t")))
        }
    };
    (delete) => {
        fn delete<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<(), PortError>> {
            self.record("delete", key, ())
        }
    };
    (touch) => {
        fn touch<'a>(
            &'a self,
            key: &'a str,
            _: Duration,
        ) -> BoxFuture<'a, Result<bool, PortError>> {
            self.record("touch", key, true)
        }
    };
    (incr_with_ttl) => {
        fn incr_with_ttl<'a>(
            &'a self,
            key: &'a str,
            by: u64,
            _: Option<Duration>,
        ) -> BoxFuture<'a, Result<u64, PortError>> {
            self.record("incr_with_ttl", key, by + 6)
        }
    };
    (version) => {
        fn version(&self) -> BoxFuture<'_, Result<u64, PortError>> {
            self.record("version", "", 42)
        }
    };
}

macro_rules! recorder_ports {
    ($($port:ident: $($op:ident),+;)*) => {$(
        impl $port for Recorder {
            $(recorder_op!($op);)+
        }
    )*};
}

recorder_ports! {
    RefStore: put_if_absent, get, take, delete, touch;
    SessionStore: get, put, delete;
    ModuleStateStore: get, put, version;
    RateLimitStore: incr_with_ttl, get, put;
    CacheStore: get, put, put_if_absent, delete, incr_with_ttl;
    RenderCache: get, put;
    AssetStore: get, put;
}

const SECOND: Duration = Duration::from_secs(1);

fn x() -> Bytes {
    Bytes::from_static(b"x")
}

// ---- prefixes (rows 13-16) ----

#[tokio::test]
async fn prefix_and_key_edges() {
    let rec = Arc::new(Recorder::default());
    let ns = Namespaced::<dyn CacheStore>::new(&id("bot-a"), rec.clone());
    assert_eq!(ns.prefix(), "bot-a:cache:");
    assert_eq!(
        format!("{ns:?}"),
        r#"Namespaced { prefix: "bot-a:cache:", .. }"#
    );
    let long = "k".repeat(10_000);
    for key in ["profile:1:2", "", "ü:%", &long] {
        CacheStore::get(&ns, key).await.unwrap();
    }
    let keys: Vec<String> = rec.calls().into_iter().map(|(_, key)| key).collect();
    assert_eq!(
        keys,
        [
            "bot-a:cache:profile:1:2".to_owned(),
            "bot-a:cache:".to_owned(),
            "bot-a:cache:ü:%".to_owned(),
            format!("bot-a:cache:{long}"),
        ]
    );
}

#[test]
fn segments_of_every_slot() {
    let rec = Arc::new(Recorder::default());
    let a = id("a");
    assert_eq!(
        Namespaced::<dyn RefStore>::new(&a, rec.clone()).prefix(),
        "a:ref:"
    );
    assert_eq!(
        Namespaced::<dyn SessionStore>::new(&a, rec.clone()).prefix(),
        "a:session:"
    );
    assert_eq!(
        Namespaced::<dyn ModuleStateStore>::new(&a, rec.clone()).prefix(),
        "a:module_state:"
    );
    assert_eq!(
        Namespaced::<dyn RateLimitStore>::new(&a, rec.clone()).prefix(),
        "a:rate_limit:"
    );
    assert_eq!(
        Namespaced::<dyn CacheStore>::new(&a, rec.clone()).prefix(),
        "a:cache:"
    );
    assert_eq!(
        Namespaced::<dyn RenderCache>::new(&a, rec.clone()).prefix(),
        "a:render_cache:"
    );
    assert_eq!(
        Namespaced::<dyn AssetStore>::new(&a, rec.clone()).prefix(),
        "a:asset:"
    );
    assert_eq!(
        Namespaced::<dyn CacheStore>::shared(rec).prefix(),
        "_shared:cache:"
    );
    assert!(AppId::new(SHARED_SEGMENT).is_err());
}

#[test]
fn into_port_marks_shared() {
    let (_, shared) = IntoPort::<dyn CacheStore>::into_port(MemoryCacheStore::new());
    assert!(!shared);
    let (_, shared) = IntoPort::<dyn CacheStore>::into_port(Shared::new(MemoryCacheStore::new()));
    assert!(shared);
    let wrapped = Shared::new(7_u8);
    assert_eq!(format!("{wrapped:?}"), "Shared(7)");
    assert_eq!(wrapped.clone().into_inner(), 7);
    let shared = namespace::<dyn CacheStore>(&id("a"), (Arc::new(MemoryCacheStore::new()), true));
    assert_eq!(shared.prefix(), "_shared:cache:");
}

// ---- forwarding (rows 21-22) ----

#[tokio::test]
async fn every_op_forwards_once() {
    let rec = Arc::new(Recorder::default());
    let a = id("a");

    let ns = Namespaced::<dyn RefStore>::new(&a, rec.clone());
    assert!(ns.put_if_absent("k", x(), None).await.unwrap());
    assert_eq!(
        RefStore::get(&ns, "k").await.unwrap(),
        Some(Bytes::from_static(b"v"))
    );
    assert_eq!(ns.take("k").await.unwrap(), Some(Bytes::from_static(b"t")));
    RefStore::delete(&ns, "k").await.unwrap();
    assert!(ns.touch("k", SECOND).await.unwrap());

    let ns = Namespaced::<dyn SessionStore>::new(&a, rec.clone());
    SessionStore::get(&ns, "k").await.unwrap();
    SessionStore::put(&ns, "k", x(), None).await.unwrap();
    SessionStore::delete(&ns, "k").await.unwrap();

    let ns = Namespaced::<dyn ModuleStateStore>::new(&a, rec.clone());
    ModuleStateStore::get(&ns, "k").await.unwrap();
    ModuleStateStore::put(&ns, "k", x(), None).await.unwrap();
    assert_eq!(ns.version().await.unwrap(), 42);

    let ns = Namespaced::<dyn RateLimitStore>::new(&a, rec.clone());
    assert_eq!(
        RateLimitStore::incr_with_ttl(&ns, "k", 1, None)
            .await
            .unwrap(),
        7
    );
    RateLimitStore::get(&ns, "k").await.unwrap();
    RateLimitStore::put(&ns, "k", x(), None).await.unwrap();

    let ns = Namespaced::<dyn CacheStore>::new(&a, rec.clone());
    CacheStore::get(&ns, "k").await.unwrap();
    CacheStore::put(&ns, "k", x(), None).await.unwrap();
    assert!(
        CacheStore::put_if_absent(&ns, "k", x(), None)
            .await
            .unwrap()
    );
    CacheStore::delete(&ns, "k").await.unwrap();
    assert_eq!(
        CacheStore::incr_with_ttl(&ns, "k", 2, None).await.unwrap(),
        8
    );

    let ns = Namespaced::<dyn RenderCache>::new(&a, rec.clone());
    RenderCache::get(&ns, "k").await.unwrap();
    RenderCache::put(&ns, "k", x(), None).await.unwrap();

    let ns = Namespaced::<dyn AssetStore>::new(&a, rec.clone());
    AssetStore::get(&ns, "k").await.unwrap();
    AssetStore::put(&ns, "k", x(), None).await.unwrap();

    let calls = rec.calls();
    let expected: Vec<(&str, &str)> = vec![
        ("put_if_absent", "a:ref:k"),
        ("get", "a:ref:k"),
        ("take", "a:ref:k"),
        ("delete", "a:ref:k"),
        ("touch", "a:ref:k"),
        ("get", "a:session:k"),
        ("put", "a:session:k"),
        ("delete", "a:session:k"),
        ("get", "a:module_state:k"),
        ("put", "a:module_state:k"),
        ("version", ""),
        ("incr_with_ttl", "a:rate_limit:k"),
        ("get", "a:rate_limit:k"),
        ("put", "a:rate_limit:k"),
        ("get", "a:cache:k"),
        ("put", "a:cache:k"),
        ("put_if_absent", "a:cache:k"),
        ("delete", "a:cache:k"),
        ("incr_with_ttl", "a:cache:k"),
        ("get", "a:render_cache:k"),
        ("put", "a:render_cache:k"),
        ("get", "a:asset:k"),
        ("put", "a:asset:k"),
    ];
    let calls: Vec<(&str, &str)> = calls.iter().map(|(op, key)| (*op, key.as_str())).collect();
    assert_eq!(calls, expected);
}

#[tokio::test]
async fn errors_pass_through() {
    let backend: fn() -> PortError = || PortError::backend("down");
    let invalid: fn() -> PortError = || PortError::invalid_value("a:cache:k", "bad");
    let unavailable: fn() -> PortError = || PortError::unavailable("closed");
    let a = id("a");
    for fail in [backend, invalid, unavailable] {
        let ns = Namespaced::<dyn CacheStore>::new(&a, Arc::new(Recorder::failing(fail)));
        let err = CacheStore::get(&ns, "k").await.unwrap_err();
        match (fail(), err) {
            (PortError::Backend(_), PortError::Backend(_))
            | (PortError::Unavailable { .. }, PortError::Unavailable { .. }) => {}
            (PortError::InvalidValue { .. }, PortError::InvalidValue { key, .. }) => {
                assert_eq!(key, "a:cache:k");
            }
            (want, got) => panic!("{want:?} became {got:?}"),
        }
    }
}

// ---- isolation (rows 18-20) ----

#[tokio::test]
async fn cache_and_session_isolation() {
    let one = Bytes::from_static(b"1");
    let two = Bytes::from_static(b"2");

    let cache: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new());
    let a = Namespaced::new(&id("a"), Arc::clone(&cache));
    let b = Namespaced::new(&id("b"), Arc::clone(&cache));
    a.put("k", one.clone(), None).await.unwrap();
    b.put("k", two.clone(), None).await.unwrap();
    assert_eq!(a.get("k").await.unwrap(), Some(one.clone()));
    assert_eq!(b.get("k").await.unwrap(), Some(two.clone()));
    a.delete("k").await.unwrap();
    assert_eq!(b.get("k").await.unwrap(), Some(two.clone()));
    assert_eq!(a.incr_with_ttl("n", 5, None).await.unwrap(), 5);
    assert_eq!(b.incr_with_ttl("n", 1, None).await.unwrap(), 1);
    assert!(a.put_if_absent("p", one.clone(), None).await.unwrap());
    assert!(b.put_if_absent("p", two.clone(), None).await.unwrap());

    let session: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::new());
    let a = Namespaced::new(&id("a"), Arc::clone(&session));
    let b = Namespaced::new(&id("b"), Arc::clone(&session));
    a.put("k", one.clone(), None).await.unwrap();
    b.put("k", two.clone(), None).await.unwrap();
    assert_eq!(a.get("k").await.unwrap(), Some(one));
    a.delete("k").await.unwrap();
    assert_eq!(a.get("k").await.unwrap(), None);
    assert_eq!(b.get("k").await.unwrap(), Some(two));
}

#[tokio::test]
async fn ref_isolation() {
    let one = Bytes::from_static(b"1");
    let two = Bytes::from_static(b"2");
    let store: Arc<dyn RefStore> = Arc::new(MemoryRefStore::new());
    let a = Namespaced::new(&id("a"), Arc::clone(&store));
    let b = Namespaced::new(&id("b"), Arc::clone(&store));
    assert!(a.put_if_absent("k", one.clone(), None).await.unwrap());
    assert!(b.put_if_absent("k", two.clone(), None).await.unwrap());
    assert_eq!(a.get("k").await.unwrap(), Some(one.clone()));
    assert_eq!(a.take("k").await.unwrap(), Some(one));
    assert_eq!(b.get("k").await.unwrap(), Some(two.clone()));
    // A's entry is gone: touching it creates nothing, B's stays.
    assert!(!a.touch("k", SECOND).await.unwrap());
    assert_eq!(a.get("k").await.unwrap(), None);
    assert!(b.touch("k", SECOND).await.unwrap());
    b.delete("k").await.unwrap();
    assert!(!a.touch("only-b", SECOND).await.unwrap());
    assert!(b.put_if_absent("only-b", two, None).await.unwrap());
    assert!(!a.touch("only-b", SECOND).await.unwrap());
    assert_eq!(a.get("only-b").await.unwrap(), None);
}

#[tokio::test]
async fn rate_limit_and_module_state_isolation() {
    let store: Arc<dyn RateLimitStore> = Arc::new(MemoryRateLimitStore::new());
    let a = Namespaced::new(&id("a"), Arc::clone(&store));
    let b = Namespaced::new(&id("b"), Arc::clone(&store));
    assert_eq!(a.incr_with_ttl("k", 1, None).await.unwrap(), 1);
    assert_eq!(a.incr_with_ttl("k", 1, None).await.unwrap(), 2);
    assert_eq!(b.incr_with_ttl("k", 1, None).await.unwrap(), 1);
    b.put("j", Bytes::from_static(b"2"), None).await.unwrap();
    assert_eq!(a.get("j").await.unwrap(), None);

    let store: Arc<dyn ModuleStateStore> = Arc::new(MemoryModuleStateStore::new());
    let a = Namespaced::new(&id("a"), Arc::clone(&store));
    let b = Namespaced::new(&id("b"), Arc::clone(&store));
    let before = a.version().await.unwrap();
    a.put("k", Bytes::from_static(b"1"), None).await.unwrap();
    b.put("k", Bytes::from_static(b"2"), None).await.unwrap();
    assert_eq!(a.get("k").await.unwrap(), Some(Bytes::from_static(b"1")));
    assert_eq!(b.get("k").await.unwrap(), Some(Bytes::from_static(b"2")));
    // The version is the inner store's, forwarded as is.
    assert_eq!(a.version().await.unwrap(), store.version().await.unwrap());
    assert!(a.version().await.unwrap() > before);
}

#[tokio::test]
async fn shared_and_mixed() {
    let store: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new());
    let shared_a = Namespaced::shared(Arc::clone(&store));
    let shared_b = Namespaced::shared(Arc::clone(&store));
    shared_a
        .put("k", Bytes::from_static(b"1"), None)
        .await
        .unwrap();
    assert_eq!(
        shared_b.get("k").await.unwrap(),
        Some(Bytes::from_static(b"1"))
    );
    assert_eq!(
        store.get("_shared:cache:k").await.unwrap(),
        Some(Bytes::from_static(b"1"))
    );

    let own_b = Namespaced::new(&id("b"), Arc::clone(&store));
    assert_eq!(own_b.get("k").await.unwrap(), None);
    own_b
        .put("k", Bytes::from_static(b"2"), None)
        .await
        .unwrap();
    assert_eq!(
        shared_a.get("k").await.unwrap(),
        Some(Bytes::from_static(b"1"))
    );
}
