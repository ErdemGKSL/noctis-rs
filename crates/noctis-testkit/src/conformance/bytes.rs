//! The seven byte-store suites, over one generic case list.

use std::fmt::Write as _;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use noctis_core::{AppId, Namespaced};
use noctis_ports::{
    AssetStore, BoxFuture, CacheStore, ModuleStateStore, PortError, RateLimitStore, RefStore,
    RenderCache, SessionStore,
};

use super::{Case, race};

/// The TTL of every short-lived entry.
const SHORT_TTL: Duration = Duration::from_secs(1);
/// The one wait after writing short-lived entries: 2.5 s tolerates backends
/// whose TTLs have second resolution.
const EXPIRY_WAIT: Duration = Duration::from_millis(2_500);
/// A TTL that never expires during a run, below every memory default's
/// maximum TTL.
const LONG_TTL: Duration = Duration::from_hours(1);
/// Concurrent callers of each `race_*` case.
const RACE_CALLERS: usize = 32;
/// `incr_with_ttl` calls per caller in `race_incr`.
const INCRS_PER_CALLER: u64 = 16;

/// Which ops of the shared vocabulary a port has, besides `get`.
#[derive(Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // one flag per op
struct Ops {
    put: bool,
    put_if_absent: bool,
    take: bool,
    delete: bool,
    touch: bool,
    incr_with_ttl: bool,
    version: bool,
}

impl Ops {
    const NONE: Self = Self {
        put: false,
        put_if_absent: false,
        take: false,
        delete: false,
        touch: false,
        incr_with_ttl: false,
        version: false,
    };
}

/// The namespace of a [`Namespaced`] wrapper.
#[derive(Clone, Copy)]
enum Namespace<'a> {
    App(&'a AppId),
    Shared,
}

type Get<'a> = BoxFuture<'a, Result<Option<Bytes>, PortError>>;

/// One byte-store port: its `dyn` type, its ops and its suite name. An op
/// the port lacks returns `None`.
trait Kind: 'static {
    type Port: ?Sized + Send + Sync + 'static;
    const SUITE: &'static str;
    /// The port segment of the namespace prefix.
    const SEGMENT: &'static str;
    const OPS: Ops;

    fn namespaced(namespace: Namespace<'_>, inner: Arc<Self::Port>) -> Arc<Self::Port>;

    fn get<'a>(port: &'a Self::Port, key: &'a str) -> Get<'a>;

    fn put<'a>(
        _port: &'a Self::Port,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> Option<BoxFuture<'a, Result<(), PortError>>> {
        None
    }

    fn put_if_absent<'a>(
        _port: &'a Self::Port,
        _key: &'a str,
        _value: Bytes,
        _ttl: Option<Duration>,
    ) -> Option<BoxFuture<'a, Result<bool, PortError>>> {
        None
    }

    fn take<'a>(_port: &'a Self::Port, _key: &'a str) -> Option<Get<'a>> {
        None
    }

    fn delete<'a>(
        _port: &'a Self::Port,
        _key: &'a str,
    ) -> Option<BoxFuture<'a, Result<(), PortError>>> {
        None
    }

    fn touch<'a>(
        _port: &'a Self::Port,
        _key: &'a str,
        _ttl: Duration,
    ) -> Option<BoxFuture<'a, Result<bool, PortError>>> {
        None
    }

    fn incr_with_ttl<'a>(
        _port: &'a Self::Port,
        _key: &'a str,
        _by: u64,
        _ttl: Option<Duration>,
    ) -> Option<BoxFuture<'a, Result<u64, PortError>>> {
        None
    }

    fn version(_port: &Self::Port) -> Option<BoxFuture<'_, Result<u64, PortError>>> {
        None
    }
}

/// Implements one op of [`Kind`] by calling the port.
macro_rules! kind_op {
    (put) => {
        fn put<'a>(
            port: &'a Self::Port,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> Option<BoxFuture<'a, Result<(), PortError>>> {
            Some(port.put(key, value, ttl))
        }
    };
    (put_if_absent) => {
        fn put_if_absent<'a>(
            port: &'a Self::Port,
            key: &'a str,
            value: Bytes,
            ttl: Option<Duration>,
        ) -> Option<BoxFuture<'a, Result<bool, PortError>>> {
            Some(port.put_if_absent(key, value, ttl))
        }
    };
    (take) => {
        fn take<'a>(port: &'a Self::Port, key: &'a str) -> Option<Get<'a>> {
            Some(port.take(key))
        }
    };
    (delete) => {
        fn delete<'a>(
            port: &'a Self::Port,
            key: &'a str,
        ) -> Option<BoxFuture<'a, Result<(), PortError>>> {
            Some(port.delete(key))
        }
    };
    (touch) => {
        fn touch<'a>(
            port: &'a Self::Port,
            key: &'a str,
            ttl: Duration,
        ) -> Option<BoxFuture<'a, Result<bool, PortError>>> {
            Some(port.touch(key, ttl))
        }
    };
    (incr_with_ttl) => {
        fn incr_with_ttl<'a>(
            port: &'a Self::Port,
            key: &'a str,
            by: u64,
            ttl: Option<Duration>,
        ) -> Option<BoxFuture<'a, Result<u64, PortError>>> {
            Some(port.incr_with_ttl(key, by, ttl))
        }
    };
    (version) => {
        fn version(port: &Self::Port) -> Option<BoxFuture<'_, Result<u64, PortError>>> {
            Some(port.version())
        }
    };
}

/// Declares the [`Kind`] of each port and its public suite.
macro_rules! byte_suites {
    ($(
        $(#[$doc:meta])*
        $suite:ident, $kind:ident: $port:ident = $segment:literal: $($op:ident),+;
    )*) => {$(
        struct $kind;

        impl Kind for $kind {
            type Port = dyn $port;
            const SUITE: &'static str = stringify!($suite);
            const SEGMENT: &'static str = $segment;
            const OPS: Ops = Ops { $($op: true,)+ ..Ops::NONE };

            fn namespaced(namespace: Namespace<'_>, inner: Arc<Self::Port>) -> Arc<Self::Port> {
                Arc::new(match namespace {
                    Namespace::App(app) => Namespaced::new(app, inner),
                    Namespace::Shared => Namespaced::shared(inner),
                })
            }

            fn get<'a>(port: &'a Self::Port, key: &'a str) -> Get<'a> {
                port.get(key)
            }

            $(kind_op!($op);)+
        }

        $(#[$doc])*
        ///
        /// See the [module docs](super) for the factory contract, the clock
        /// and runtime requirements and the case list.
        ///
        /// # Panics
        ///
        /// On the first failing case, with
        /// `` conformance::<suite>: case `<case>` failed: <detail> ``.
        pub async fn $suite<P: $port, F: Fn() -> P>(factory: F) {
            run::<$kind>(&|| {
                let port: Arc<dyn $port> = Arc::new(factory());
                port
            })
            .await;
        }
    )*};
}

byte_suites! {
    /// Runs the `RefStore` suite: `put_if_absent`, `get`, `take`, `delete`
    /// and `touch`, with races on `put_if_absent` and `take`.
    ref_store, RefKind: RefStore = "ref": put_if_absent, take, delete, touch;
    /// Runs the `SessionStore` suite: `get`, `put` and `delete`.
    session_store, SessionKind: SessionStore = "session": put, delete;
    /// Runs the `ModuleStateStore` suite: `get`, `put` and `version`.
    module_state_store, ModuleStateKind: ModuleStateStore = "module_state": put, version;
    /// Runs the `RateLimitStore` suite: `incr_with_ttl`, `get` and `put`,
    /// with a race of 512 increments.
    rate_limit_store, RateLimitKind: RateLimitStore = "rate_limit": put, incr_with_ttl;
    /// Runs the `CacheStore` suite: `get`, `put`, `put_if_absent`, `delete`
    /// and `incr_with_ttl`, with races on `put_if_absent` and
    /// `incr_with_ttl`.
    cache_store, CacheKind: CacheStore = "cache": put, put_if_absent, delete, incr_with_ttl;
    /// Runs the `RenderCache` suite: `get` and `put`.
    render_cache, RenderCacheKind: RenderCache = "render_cache": put;
    /// Runs the `AssetStore` suite: `get` and `put`.
    asset_store, AssetKind: AssetStore = "asset": put;
}

/// A store under test in one case: every op unwraps its result or fails
/// the case.
struct Store<K: Kind> {
    port: Arc<K::Port>,
    case: Case,
    kind: PhantomData<K>,
}

impl<K: Kind> Store<K> {
    fn new(port: Arc<K::Port>, name: &'static str) -> Self {
        Self {
            port,
            case: Case::new(K::SUITE, name),
            kind: PhantomData,
        }
    }

    /// The same case on another view of the store (a namespace).
    fn view(&self, port: Arc<K::Port>) -> Self {
        Self {
            port,
            case: self.case,
            kind: PhantomData,
        }
    }

    fn missing(&self, op: &str) -> ! {
        self.case.fail(format_args!(
            "the suite called `{op}`, which the port lacks"
        ))
    }

    async fn get(&self, key: &str) -> Option<Bytes> {
        let result = K::get(&self.port, key).await;
        self.case.ok(format_args!("get({})", show_key(key)), result)
    }

    async fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>) {
        let Some(op) = K::put(&self.port, key, Bytes::copy_from_slice(value), ttl) else {
            self.missing("put");
        };
        let result = op.await;
        self.case.ok(format_args!("put({})", show_key(key)), result);
    }

    async fn put_if_absent(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> bool {
        let Some(op) = K::put_if_absent(&self.port, key, Bytes::copy_from_slice(value), ttl) else {
            self.missing("put_if_absent");
        };
        let result = op.await;
        self.case
            .ok(format_args!("put_if_absent({})", show_key(key)), result)
    }

    async fn take(&self, key: &str) -> Option<Bytes> {
        let Some(op) = K::take(&self.port, key) else {
            self.missing("take");
        };
        let result = op.await;
        self.case
            .ok(format_args!("take({})", show_key(key)), result)
    }

    async fn delete(&self, key: &str) {
        let Some(op) = K::delete(&self.port, key) else {
            self.missing("delete");
        };
        let result = op.await;
        self.case
            .ok(format_args!("delete({})", show_key(key)), result);
    }

    async fn touch(&self, key: &str, ttl: Duration) -> bool {
        let Some(op) = K::touch(&self.port, key, ttl) else {
            self.missing("touch");
        };
        let result = op.await;
        self.case
            .ok(format_args!("touch({})", show_key(key)), result)
    }

    async fn try_incr(&self, key: &str, by: u64, ttl: Option<Duration>) -> Result<u64, PortError> {
        let Some(op) = K::incr_with_ttl(&self.port, key, by, ttl) else {
            self.missing("incr_with_ttl");
        };
        op.await
    }

    async fn incr(&self, key: &str, by: u64, ttl: Option<Duration>) -> u64 {
        let result = self.try_incr(key, by, ttl).await;
        self.case.ok(
            format_args!("incr_with_ttl({}, {by}, {ttl:?})", show_key(key)),
            result,
        )
    }

    async fn version(&self) -> u64 {
        let Some(op) = K::version(&self.port) else {
            self.missing("version");
        };
        let result = op.await;
        self.case.ok("version()", result)
    }

    /// Writes `value` with `put`, or with `put_if_absent` on a port without
    /// `put` (which must then insert).
    async fn seed(&self, key: &str, value: &[u8], ttl: Option<Duration>) {
        if K::OPS.put {
            self.put(key, value, ttl).await;
        } else {
            let inserted = self.put_if_absent(key, value, ttl).await;
            self.case.check(inserted, || {
                format!(
                    "put_if_absent({}) on an absent key returned false",
                    show_key(key)
                )
            });
        }
    }

    /// Fails unless `get(key)` returns `want`.
    async fn expect(&self, key: &str, want: Option<&[u8]>, when: &str) {
        let got = self.get(key).await;
        self.case.check(got.as_deref() == want, || {
            format!(
                "get({}) {when}: expected {}, got {}",
                show_key(key),
                show(want),
                show(got.as_deref())
            )
        });
    }
}

/// A key for a failure message, shortened when long.
fn show_key(key: &str) -> String {
    if key.len() <= 40 {
        format!("{key:?}")
    } else {
        format!("<{}-byte key>", key.len())
    }
}

/// A value for a failure message: its length and first bytes.
fn show(value: Option<&[u8]>) -> String {
    let Some(value) = value else {
        return "None".to_owned();
    };
    let mut out = format!("Some({} bytes", value.len());
    if !value.is_empty() {
        out.push_str(": ");
        for byte in value.iter().take(16) {
            let _ = write!(out, "{byte:02x}");
        }
        if value.len() > 16 {
            out.push_str("..");
        }
    }
    out.push(')');
    out
}

/// The value set `V`: empty, one byte, every byte value, and 64 KiB.
fn values() -> [Vec<u8>; 4] {
    [
        Vec::new(),
        b"v".to_vec(),
        (0..=255).collect(),
        vec![0xAB; 64 * 1024],
    ]
}

async fn run<K: Kind>(factory: &dyn Fn() -> Arc<K::Port>) {
    let ops = K::OPS;
    let store = |name| Store::<K>::new(factory(), name);

    missing_key(store("missing_key")).await;
    roundtrip_values(store("roundtrip_values")).await;
    key_shapes(store("key_shapes")).await;
    if ops.put {
        overwrite(store("overwrite")).await;
    }
    if ops.put_if_absent {
        put_if_absent_live(store("put_if_absent_live")).await;
    }
    if ops.take {
        take_once(store("take_once")).await;
    }
    if ops.delete {
        delete_live(store("delete_live")).await;
    }
    if ops.touch {
        touch_live(store("touch_live")).await;
    }
    if ops.incr_with_ttl {
        incr_basic(store("incr_basic")).await;
        incr_saturates(store("incr_saturates")).await;
        incr_non_counter(store("incr_non_counter")).await;
    }
    if ops.version {
        version_monotonic(store("version_monotonic")).await;
    }
    namespacing(store("namespacing")).await;

    if ops.put_if_absent {
        race_put_if_absent(store("race_put_if_absent")).await;
    }
    if ops.take {
        race_take(store("race_take")).await;
    }
    if ops.incr_with_ttl {
        race_incr(store("race_incr")).await;
    }

    ttl_cases(&store).await;
}

async fn missing_key<K: Kind>(s: Store<K>) {
    s.expect("k", None, "on a fresh store").await;
    if K::OPS.take {
        let got = s.take("k").await;
        s.case.check(got.is_none(), || {
            format!(
                "take(\"k\") on a fresh store: expected None, got {}",
                show(got.as_deref())
            )
        });
    }
    if K::OPS.delete {
        s.delete("k").await;
        s.delete("k").await;
    }
    if K::OPS.touch {
        let touched = s.touch("k", LONG_TTL).await;
        s.case.check(!touched, || {
            "touch(\"k\") on a missing key returned true".to_owned()
        });
        s.expect(
            "k",
            None,
            "after touch on a missing key (touch must not create)",
        )
        .await;
    }
}

async fn roundtrip_values<K: Kind>(s: Store<K>) {
    let values = values();
    for (i, value) in values.iter().enumerate() {
        s.seed(&format!("v{i}"), value, None).await;
    }
    for (i, value) in values.iter().enumerate() {
        s.expect(&format!("v{i}"), Some(value), "after a write")
            .await;
    }
}

async fn key_shapes<K: Kind>(s: Store<K>) {
    let long = "x".repeat(512);
    let keys = ["ключ:🔑:%25", long.as_str(), "K", "k", "a", "ab"];
    for (i, key) in keys.iter().enumerate() {
        s.seed(key, format!("value-{i}").as_bytes(), None).await;
    }
    for (i, key) in keys.iter().enumerate() {
        let want = format!("value-{i}");
        s.expect(
            key,
            Some(want.as_bytes()),
            "(keys are verbatim, case- and prefix-distinct)",
        )
        .await;
    }
}

async fn overwrite<K: Kind>(s: Store<K>) {
    s.put("k", b"a", None).await;
    s.put("k", b"b", None).await;
    s.expect("k", Some(b"b"), "after put \"a\" then put \"b\"")
        .await;
}

async fn put_if_absent_live<K: Kind>(s: Store<K>) {
    s.seed("k", b"a", None).await;
    let inserted = s.put_if_absent("k", b"b", None).await;
    s.case.check(!inserted, || {
        "put_if_absent on a live key returned true".to_owned()
    });
    s.expect("k", Some(b"a"), "after put_if_absent on a live key")
        .await;
    let inserted = s.put_if_absent("absent", b"c", None).await;
    s.case.check(inserted, || {
        "put_if_absent on an absent key returned false".to_owned()
    });
    s.expect("absent", Some(b"c"), "after put_if_absent on an absent key")
        .await;
}

async fn take_once<K: Kind>(s: Store<K>) {
    s.seed("k", b"a", None).await;
    let first = s.take("k").await;
    s.case.check(first.as_deref() == Some(b"a"), || {
        format!(
            "first take: expected Some(\"a\"), got {}",
            show(first.as_deref())
        )
    });
    let second = s.take("k").await;
    s.case.check(second.is_none(), || {
        format!(
            "second take: expected None, got {}",
            show(second.as_deref())
        )
    });
    s.expect("k", None, "after take").await;
}

async fn delete_live<K: Kind>(s: Store<K>) {
    s.seed("k", b"a", None).await;
    s.delete("k").await;
    s.delete("k").await;
    s.expect("k", None, "after delete").await;
}

async fn touch_live<K: Kind>(s: Store<K>) {
    s.seed("k", b"a", None).await;
    let touched = s.touch("k", LONG_TTL).await;
    s.case
        .check(touched, || "touch on a live key returned false".to_owned());
    s.expect("k", Some(b"a"), "after touch").await;
}

async fn incr_basic<K: Kind>(s: Store<K>) {
    let first = s.incr("n", 5, None).await;
    s.case.check(first == 5, || {
        format!("incr(n, 5) on a missing key: expected 5, got {first}")
    });
    let same = s.incr("n", 0, None).await;
    s.case
        .check(same == 5, || format!("incr(n, 0): expected 5, got {same}"));
    let zero = s.incr("z", 0, None).await;
    s.case.check(zero == 0, || {
        format!("incr(z, 0) on a missing key: expected 0, got {zero}")
    });
    s.expect(
        "z",
        Some(&0u64.to_be_bytes()),
        "after incr(z, 0) (8 zero bytes)",
    )
    .await;
    s.expect(
        "n",
        Some(&5u64.to_be_bytes()),
        "after incr (8-byte big-endian u64)",
    )
    .await;
}

async fn incr_saturates<K: Kind>(s: Store<K>) {
    s.put("n", &(u64::MAX - 1).to_be_bytes(), None).await;
    let first = s.incr("n", 5, None).await;
    s.case.check(first == u64::MAX, || {
        format!("incr(u64::MAX - 1, 5): expected u64::MAX, got {first}")
    });
    let second = s.incr("n", 1, None).await;
    s.case.check(second == u64::MAX, || {
        format!("incr(u64::MAX, 1): expected u64::MAX, got {second}")
    });
}

async fn incr_non_counter<K: Kind>(s: Store<K>) {
    let bad: [(&str, &[u8]); 3] = [
        ("empty", b""),
        ("text", b"abc"),
        ("nine", &[1, 2, 3, 4, 5, 6, 7, 8, 9]),
    ];
    for (key, value) in bad {
        s.put(key, value, None).await;
        match s.try_incr(key, 1, None).await {
            Err(PortError::InvalidValue { .. }) => {}
            Err(err) => s.case.fail(format_args!(
                "incr on {}: expected PortError::InvalidValue, got Err: {err}",
                show(Some(value))
            )),
            Ok(n) => s.case.fail(format_args!(
                "incr on {}: expected PortError::InvalidValue, got Ok({n})",
                show(Some(value))
            )),
        }
        s.expect(
            key,
            Some(value),
            "after a failed incr (value must be unchanged)",
        )
        .await;
    }
}

async fn version_monotonic<K: Kind>(s: Store<K>) {
    let start = s.version().await;
    s.case.check(start == 0, || {
        format!("version() on a fresh store: expected 0, got {start}")
    });
    let again = s.version().await;
    s.case.check(again >= start, || {
        format!("version() went back from {start} to {again} without a put")
    });
    let mut before = again;
    for i in 0..3 {
        s.put(&format!("m{i}"), b"on", None).await;
        let after = s.version().await;
        s.case.check(after > before, || {
            format!("version() after put #{i}: expected > {before}, got {after}")
        });
        before = after;
    }

    let shared = Arc::clone(&s.port);
    let reads = race(s.case, &shared, 8, |port, caller| async move {
        let key = format!("r{caller}");
        let Some(put) = K::put(&port, &key, Bytes::from_static(b"on"), None) else {
            return Err(PortError::unavailable("the port has no put"));
        };
        put.await?;
        match K::version(&port) {
            Some(version) => version.await,
            None => Err(PortError::unavailable("the port has no version")),
        }
    })
    .await;
    for (caller, read) in reads.into_iter().enumerate() {
        s.case.check(read > before, || {
            format!(
                "caller {caller} read version() {read} after its concurrent put; expected > {before}"
            )
        });
    }
}

async fn namespacing<K: Kind>(s: Store<K>) {
    let ops = K::OPS;
    let app_a = AppId::new("conformance-a").unwrap_or_else(|err| s.case.fail(err));
    let app_b = AppId::new("conformance-b").unwrap_or_else(|err| s.case.fail(err));
    let a = s.view(K::namespaced(Namespace::App(&app_a), Arc::clone(&s.port)));
    let b = s.view(K::namespaced(Namespace::App(&app_b), Arc::clone(&s.port)));
    let shared = s.view(K::namespaced(Namespace::Shared, Arc::clone(&s.port)));
    let segment = K::SEGMENT;

    a.seed("k", b"from-a", None).await;
    if ops.touch {
        let touched = a.touch("k", LONG_TTL).await;
        a.case.check(touched, || {
            "touch via app A on A's key returned false".to_owned()
        });
    }
    if ops.incr_with_ttl {
        let n = a.incr("n", 3, None).await;
        a.case
            .check(n == 3, || format!("incr via app A: expected 3, got {n}"));
    }

    b.expect("k", None, "via app B for app A's key").await;
    shared.expect("k", None, "via shared for app A's key").await;
    s.expect(
        &format!("conformance-a:{segment}:k"),
        Some(b"from-a"),
        "on the inner store",
    )
    .await;
    s.expect("k", None, "on the inner store for the logical key")
        .await;

    if ops.take {
        let got = b.take("k").await;
        b.case.check(got.is_none(), || {
            format!(
                "take via app B of app A's key: expected None, got {}",
                show(got.as_deref())
            )
        });
    }
    if ops.touch {
        let touched = b.touch("k", LONG_TTL).await;
        b.case.check(!touched, || {
            "touch via app B of app A's key returned true".to_owned()
        });
    }
    if ops.delete {
        b.delete("k").await;
        shared.delete("k").await;
    }
    if ops.put_if_absent {
        let inserted = b.put_if_absent("k", b"from-b", None).await;
        b.case.check(inserted, || {
            "put_if_absent via app B returned false while only app A wrote the key".to_owned()
        });
        b.expect(
            "k",
            Some(b"from-b"),
            "via app B after its own put_if_absent",
        )
        .await;
    }
    if ops.incr_with_ttl {
        let nb = b.incr("n", 1, None).await;
        b.case.check(nb == 1, || {
            format!("incr via app B: expected an independent counter (1), got {nb}")
        });
        let ns = shared.incr("n", 1, None).await;
        shared.case.check(ns == 1, || {
            format!("incr via shared: expected an independent counter (1), got {ns}")
        });
        a.expect(
            "n",
            Some(&3u64.to_be_bytes()),
            "via app A after B and shared incremented",
        )
        .await;
    }
    a.expect(
        "k",
        Some(b"from-a"),
        "via app A after app B and shared wrote",
    )
    .await;

    shared.seed("s", b"from-shared", None).await;
    s.expect(
        &format!("_shared:{segment}:s"),
        Some(b"from-shared"),
        "on the inner store",
    )
    .await;
    a.expect("s", None, "via app A for a shared key").await;
}

async fn race_put_if_absent<K: Kind>(s: Store<K>) {
    let results = race(s.case, &s.port, RACE_CALLERS, |port, caller| async move {
        let value = Bytes::from(format!("caller-{caller}"));
        match K::put_if_absent(&port, "k", value, None) {
            Some(op) => op.await,
            None => Err(PortError::unavailable("the port has no put_if_absent")),
        }
    })
    .await;
    let winners: Vec<usize> = (0..RACE_CALLERS).filter(|&i| results[i]).collect();
    let [winner] = winners[..] else {
        s.case.fail(format_args!(
            "{} of {RACE_CALLERS} concurrent put_if_absent calls returned true; expected exactly 1",
            winners.len()
        ));
    };
    let want = format!("caller-{winner}");
    s.expect(
        "k",
        Some(want.as_bytes()),
        "after the race (the winner's value)",
    )
    .await;
}

async fn race_take<K: Kind>(s: Store<K>) {
    s.seed("k", b"once", None).await;
    let results = race(s.case, &s.port, RACE_CALLERS, |port, _| async move {
        match K::take(&port, "k") {
            Some(op) => op.await,
            None => Err(PortError::unavailable("the port has no take")),
        }
    })
    .await;
    let hits = results.iter().filter(|got| got.is_some()).count();
    s.case.check(hits == 1, || {
        format!("{hits} of {RACE_CALLERS} concurrent take calls returned Some; expected exactly 1")
    });
}

async fn race_incr<K: Kind>(s: Store<K>) {
    let results = race(s.case, &s.port, RACE_CALLERS, |port, _| async move {
        let mut seen = Vec::new();
        for _ in 0..INCRS_PER_CALLER {
            match K::incr_with_ttl(&port, "n", 1, None) {
                Some(op) => seen.push(op.await?),
                None => return Err(PortError::unavailable("the port has no incr_with_ttl")),
            }
        }
        Ok(seen)
    })
    .await;
    let total = RACE_CALLERS as u64 * INCRS_PER_CALLER;
    let final_value = s.get("n").await;
    s.case.check(
        final_value.as_deref() == Some(&total.to_be_bytes()[..]),
        || {
            format!(
                "after {total} concurrent increments: expected the counter {total}, got {}",
                show(final_value.as_deref())
            )
        },
    );
    let mut returned: Vec<u64> = results.into_iter().flatten().collect();
    returned.sort_unstable();
    let distinct = returned.iter().copied().eq(1..=total);
    s.case.check(distinct, || {
        format!("the {total} concurrent increments did not return exactly 1..={total} (an increment was lost or repeated)")
    });
}

/// Writes every short-lived entry, waits once, then checks them all.
async fn ttl_cases<K: Kind>(store: &dyn Fn(&'static str) -> Store<K>) {
    let pending = ttl_write(store).await;
    tokio::time::sleep(EXPIRY_WAIT).await;
    ttl_check(pending).await;
}

/// The stores of the `ttl_*` cases, written and waiting for expiry; `None`
/// for a case the port has no ops for.
struct TtlPending<K: Kind> {
    expires: Store<K>,
    resets: Option<Store<K>>,
    reinsert: Option<Store<K>>,
    take: Option<Store<K>>,
    touch: Option<Store<K>>,
    incr: Option<Store<K>>,
}

async fn ttl_write<K: Kind>(store: &dyn Fn(&'static str) -> Store<K>) -> TtlPending<K> {
    let ops = K::OPS;

    let live = store("ttl_live_before_expiry");
    live.seed("k", b"short", Some(SHORT_TTL)).await;
    live.expect("k", Some(b"short"), "right after a write with a 1 s TTL")
        .await;

    let expires = store("ttl_get_expires");
    expires.seed("k", b"short", Some(SHORT_TTL)).await;

    let resets = if ops.put {
        let s = store("ttl_put_resets");
        s.put("cleared", b"short", Some(SHORT_TTL)).await;
        s.put("cleared", b"forever", None).await;
        s.put("set", b"forever", None).await;
        s.put("set", b"short", Some(SHORT_TTL)).await;
        Some(s)
    } else {
        None
    };

    let reinsert = if ops.put_if_absent {
        let s = store("ttl_put_if_absent_expired");
        let inserted = s.put_if_absent("k", b"old", Some(SHORT_TTL)).await;
        s.case.check(inserted, || {
            "put_if_absent on an absent key returned false".to_owned()
        });
        Some(s)
    } else {
        None
    };

    let take = if ops.take {
        let s = store("ttl_take_expired");
        s.seed("k", b"short", Some(SHORT_TTL)).await;
        Some(s)
    } else {
        None
    };

    let touch = if ops.touch {
        let s = store("ttl_touch");
        s.seed("extended", b"a", Some(SHORT_TTL)).await;
        s.seed("shortened", b"b", Some(LONG_TTL)).await;
        s.seed("expired", b"c", Some(SHORT_TTL)).await;
        let extended = s.touch("extended", LONG_TTL).await;
        let shortened = s.touch("shortened", SHORT_TTL).await;
        s.case.check(extended && shortened, || {
            "touch on a live key returned false".to_owned()
        });
        Some(s)
    } else {
        None
    };

    let incr = if ops.incr_with_ttl {
        let s = store("ttl_incr_create_only");
        let first = s.incr("n", 1, Some(SHORT_TTL)).await;
        let second = s.incr("n", 1, Some(LONG_TTL)).await;
        s.case.check(first == 1 && second == 2, || {
            format!("incr(n, 1, 1 s) then incr(n, 1, 1 h): expected 1, 2; got {first}, {second}")
        });
        Some(s)
    } else {
        None
    };

    TtlPending {
        expires,
        resets,
        reinsert,
        take,
        touch,
        incr,
    }
}

async fn ttl_check<K: Kind>(pending: TtlPending<K>) {
    let TtlPending {
        expires,
        resets,
        reinsert,
        take,
        touch,
        incr,
    } = pending;
    let after = "after its 1 s TTL passed";
    expires.expect("k", None, after).await;
    if let Some(s) = resets {
        s.expect(
            "cleared",
            Some(b"forever"),
            "after put(None) replaced a 1 s TTL",
        )
        .await;
        s.expect(
            "set",
            None,
            "after put(1 s) replaced no expiry and 1 s passed",
        )
        .await;
    }
    if let Some(s) = reinsert {
        let inserted = s.put_if_absent("k", b"new", Some(LONG_TTL)).await;
        s.case.check(inserted, || {
            "put_if_absent on an expired key returned false (expired counts as absent)".to_owned()
        });
        s.expect("k", Some(b"new"), "after put_if_absent on an expired key")
            .await;
    }
    if let Some(s) = take {
        let got = s.take("k").await;
        s.case.check(got.is_none(), || {
            format!(
                "take of an expired key: expected None, got {}",
                show(got.as_deref())
            )
        });
    }
    if let Some(s) = touch {
        s.expect(
            "extended",
            Some(b"a"),
            "after touch extended a 1 s TTL to 1 h",
        )
        .await;
        s.expect("shortened", None, "after touch shortened a 1 h TTL to 1 s")
            .await;
        let touched = s.touch("expired", LONG_TTL).await;
        s.case.check(!touched, || {
            "touch on an expired key returned true".to_owned()
        });
        s.expect("expired", None, "after touch on an expired key")
            .await;
    }
    if let Some(s) = incr {
        let recreated = s.incr("n", 1, None).await;
        s.case.check(recreated == 1, || {
            format!(
                "incr after the 1 s TTL of its creation passed: expected 1 (a fresh counter), got {recreated} (the second incr must not reset the TTL)"
            )
        });
    }
}
