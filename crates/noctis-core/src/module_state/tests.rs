//! Story 13.3 unit tests: the snapshot semantics (S1-S19), propagation
//! through the poller (P1-P4) and the Display strings.

#![allow(clippy::unwrap_used, reason = "tests panic with the failing case")]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bytes::Bytes;
use noctis_ports::BoxFuture;
use noctis_ports::memory::MemoryModuleStateStore;

use super::*;
use crate::test_support::Capture;

/// A store counting its calls, with switchable failures.
#[derive(Default)]
struct Counting {
    inner: MemoryModuleStateStore,
    gets: AtomicUsize,
    puts: AtomicUsize,
    fail_version: AtomicBool,
    fail_get: AtomicBool,
    fail_put: AtomicBool,
}

fn broken() -> PortError {
    PortError::unavailable("broken")
}

impl ModuleStateStore for Counting {
    fn get<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<Option<Bytes>, PortError>> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        if self.fail_get.load(Ordering::SeqCst) {
            return Box::pin(async { Err(broken()) });
        }
        self.inner.get(key)
    }

    fn put<'a>(
        &'a self,
        key: &'a str,
        value: Bytes,
        ttl: Option<Duration>,
    ) -> BoxFuture<'a, Result<(), PortError>> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        if self.fail_put.load(Ordering::SeqCst) {
            return Box::pin(async { Err(broken()) });
        }
        self.inner.put(key, value, ttl)
    }

    fn version(&self) -> BoxFuture<'_, Result<u64, PortError>> {
        if self.fail_version.load(Ordering::SeqCst) {
            return Box::pin(async { Err(broken()) });
        }
        self.inner.version()
    }
}

/// The index of `p`, `p/c`, `q`.
fn index() -> Arc<ModuleIndex> {
    let mut index = ModuleIndex::default();
    index.push("p", &[]);
    index.push("p/c", &["p"]);
    index.push("q", &[]);
    Arc::new(index)
}

/// A handle over `store` and its counting store.
fn modules_over(store: &Arc<Counting>) -> Modules {
    let store: Arc<dyn ModuleStateStore> = Arc::clone(store) as _;
    Modules {
        state: Some(Arc::new(State::new(store, index()))),
    }
}

fn fresh() -> (Arc<Counting>, Modules) {
    let store = Arc::new(Counting::default());
    let modules = modules_over(&store);
    (store, modules)
}

const G1: ModuleScope = ModuleScope::Guild(Id::new(1));
const G2: ModuleScope = ModuleScope::Guild(Id::new(2));

/// The enabled state of `p`, `p/c`, `q` in `scope`.
fn states(modules: &Modules, scope: ModuleScope) -> [bool; 3] {
    ["p", "p/c", "q"].map(|id| modules.is_enabled(id, scope))
}

async fn version(store: &Counting) -> u64 {
    store.inner.version().await.unwrap()
}

async fn record(store: &Counting, id: &str) -> Record {
    let raw = store.inner.get(id).await.unwrap().unwrap();
    decode_value(0, &raw).unwrap().unwrap()
}

#[tokio::test]
async fn s1_fresh_store_is_all_enabled() {
    let (_, modules) = fresh();
    modules.refresh().await.unwrap();
    for scope in [ModuleScope::Global, G1] {
        assert_eq!(states(&modules, scope), [true; 3]);
    }
}

#[tokio::test]
async fn s2_guild_override_disables_the_subtree_there() {
    let (_, modules) = fresh();
    modules.set_enabled("p", G1, false).await.unwrap();
    assert_eq!(states(&modules, G1), [false, false, true]);
    assert_eq!(states(&modules, G2), [true; 3]);
    assert_eq!(states(&modules, ModuleScope::Global), [true; 3]);
}

#[tokio::test]
async fn s3_s4_s5_global_off_override_on_and_ancestor_and() {
    let (_, modules) = fresh();
    modules
        .set_enabled("p", ModuleScope::Global, false)
        .await
        .unwrap();
    // S3.
    for scope in [ModuleScope::Global, G2] {
        assert_eq!(states(&modules, scope), [false, false, true]);
    }
    // S5: a child override cannot lift a disabled ancestor.
    modules.set_enabled("p/c", G1, true).await.unwrap();
    assert!(!modules.is_enabled("p/c", G1));
    // S4.
    modules.set_enabled("p", G1, true).await.unwrap();
    assert_eq!(states(&modules, G1), [true; 3]);
    assert_eq!(states(&modules, G2), [false, false, true]);
}

#[tokio::test]
async fn s6_reset_removes_the_override() {
    let (store, modules) = fresh();
    modules.set_enabled("p", G1, false).await.unwrap();
    modules.reset("p", G1).await.unwrap();
    for scope in [ModuleScope::Global, G1, G2] {
        assert_eq!(states(&modules, scope), [true; 3]);
    }
    assert_eq!(record(&store, "p").await, Record::default());
}

#[tokio::test]
async fn s7_global_reset_keeps_overrides() {
    let (store, modules) = fresh();
    modules
        .set_enabled("p", ModuleScope::Global, false)
        .await
        .unwrap();
    modules.set_enabled("p", G1, true).await.unwrap();
    modules.reset("p", ModuleScope::Global).await.unwrap();
    for scope in [ModuleScope::Global, G1, G2] {
        assert_eq!(states(&modules, scope), [true; 3]);
    }
    let stored = record(&store, "p").await;
    assert!(stored.global);
    assert_eq!(stored.guilds, [(1, true)]);
}

#[tokio::test]
async fn s8_s9_s10_unchanged_records_are_not_written() {
    let (store, modules) = fresh();
    modules.reset("p", G2).await.unwrap();
    modules
        .set_enabled("p", ModuleScope::Global, true)
        .await
        .unwrap();
    assert_eq!(version(&store).await, 0);
    assert_eq!(store.puts.load(Ordering::SeqCst), 0);
    modules.set_enabled("p", G1, false).await.unwrap();
    modules.set_enabled("p", G1, false).await.unwrap();
    assert_eq!(version(&store).await, 1);
    assert_eq!(store.puts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn s11_unknown_ids_fail_writes_and_read_disabled() {
    let (store, modules) = fresh();
    for id in ["", "P", "prőfile", "nope"] {
        let error = modules.set_enabled(id, G1, false).await.unwrap_err();
        assert!(
            matches!(&error, ModuleStateError::UnknownModule { id: got } if got == id),
            "{error:?}"
        );
        assert!(matches!(
            modules.reset(id, ModuleScope::Global).await,
            Err(ModuleStateError::UnknownModule { .. })
        ));
        assert!(!modules.is_enabled(id, G1));
        assert!(!modules.is_enabled(id, ModuleScope::Global));
    }
    assert_eq!(store.gets.load(Ordering::SeqCst), 0);
    assert_eq!(store.puts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn s12_boot_flag_pruned_module_is_unknown() {
    let mut builder = crate::App::builder();
    builder
        .runtime(&crate::Runtime::for_test())
        .boot_flags(crate::BootFlags::only(["eco"]).unwrap())
        .module(crate::Module::new("eco").flags(["eco"]))
        .module(crate::Module::new("fun").flags(["fun"]));
    let app = builder.build().unwrap();
    let modules = app.modules();
    assert_eq!(modules.ids().collect::<Vec<_>>(), ["eco"]);
    assert!(!modules.is_enabled("fun", None));
    assert!(matches!(
        modules.set_enabled("fun", None, false).await,
        Err(ModuleStateError::UnknownModule { .. })
    ));
}

#[tokio::test]
async fn s13_scope_conversions() {
    let (_, modules) = fresh();
    modules.set_enabled("p", Id::new(1), false).await.unwrap();
    assert!(modules.is_enabled("p", None::<Id<GuildMarker>>));
    assert!(!modules.is_enabled("p", Some(Id::new(1))));
    assert!(!modules.is_enabled("p", Id::new(1)));
    assert!(!modules.is_enabled("p", G1));
}

#[tokio::test]
async fn s14_ten_thousand_overrides_round_trip() {
    let (store, modules) = fresh();
    // 9 999 overrides stored at once, then one written in the middle: the
    // record stays sorted.
    let guilds = (1..=10_000_u64)
        .filter(|&guild| guild != 5_000)
        .map(|guild| (guild, guild % 2 == 0))
        .collect();
    let bulk = Record {
        global: true,
        guilds,
    };
    store
        .inner
        .put("p", encode_value(0, &bulk).unwrap(), None)
        .await
        .unwrap();
    modules
        .set_enabled("p", ModuleScope::Guild(Id::new(5_000)), true)
        .await
        .unwrap();
    let stored = record(&store, "p").await;
    assert_eq!(stored.guilds.len(), 10_000);
    assert!(stored.guilds.is_sorted_by_key(|&(guild, _)| guild));
    let other = modules_over(&store);
    other.refresh().await.unwrap();
    for guild in [1_u64, 2, 4_999, 5_000, 9_999, 10_000] {
        let scope = ModuleScope::Guild(Id::new(guild));
        assert_eq!(other.is_enabled("p", scope), guild % 2 == 0, "{guild}");
        assert_eq!(other.is_enabled("p/c", scope), guild % 2 == 0, "{guild}");
    }
    assert!(other.is_enabled("p", ModuleScope::Guild(Id::new(10_001))));
}

#[tokio::test]
async fn s15_other_schema_reads_as_default_without_warning() {
    let (store, modules) = fresh();
    store
        .inner
        .put("p", Bytes::from_static(&[0xff, 0x00, 0x00]), None)
        .await
        .unwrap();
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    modules.refresh().await.unwrap();
    assert!(modules.is_enabled("p", G1));
    assert!(
        capture.lines_with(&["WARN"]).is_empty(),
        "{}",
        capture.text()
    );
}

#[tokio::test]
async fn s16_malformed_reads_as_default_with_one_warning() {
    let (store, modules) = fresh();
    store
        .inner
        .put("p", Bytes::from_static(&[0x00, 0xff]), None)
        .await
        .unwrap();
    let capture = Capture::default();
    {
        let _guard = tracing::subscriber::set_default(capture.subscriber());
        modules.refresh().await.unwrap();
    }
    assert!(modules.is_enabled("p", G1));
    let warns = capture.lines_with(&["WARN", "noctis::modules", "module=\"p\""]);
    assert_eq!(warns.len(), 1, "{}", capture.text());
    modules.set_enabled("p", G1, false).await.unwrap();
    assert_eq!(
        record(&store, "p").await,
        Record {
            global: true,
            guilds: vec![(1, false)]
        }
    );
}

#[tokio::test]
async fn s17_keys_of_unmounted_ids_are_never_read() {
    let (store, modules) = fresh();
    let off = encode_value(
        0,
        &Record {
            global: false,
            guilds: Vec::new(),
        },
    )
    .unwrap();
    store.inner.put("gone", off, None).await.unwrap();
    modules.refresh().await.unwrap();
    // One get per kept Module, none for `gone`.
    assert_eq!(store.gets.load(Ordering::SeqCst), 3);
    assert_eq!(states(&modules, ModuleScope::Global), [true; 3]);
}

#[tokio::test]
async fn s18_detached_handle() {
    let modules = Modules { state: None };
    assert!(modules.is_enabled("anything", G1));
    assert!(modules.is_enabled("", None));
    assert_eq!(modules.ids().count(), 0);
    for result in [
        modules.set_enabled("p", G1, false).await,
        modules.reset("p", G1).await,
        modules.refresh().await,
    ] {
        assert!(matches!(
            result,
            Err(ModuleStateError::Port(PortError::Unavailable { .. }))
        ));
    }
    assert_eq!(format!("{modules:?}"), "Modules { modules: 0, version: 0 }");
}

#[tokio::test]
async fn s19_failed_put_keeps_the_snapshot() {
    let (store, modules) = fresh();
    store.fail_put.store(true, Ordering::SeqCst);
    assert!(matches!(
        modules.set_enabled("p", G1, false).await,
        Err(ModuleStateError::Port(_))
    ));
    assert!(modules.is_enabled("p", G1));
}

#[tokio::test]
async fn refresh_reads_version_then_every_record() {
    let (store, a) = fresh();
    let b = modules_over(&store);
    a.set_enabled("q", G2, false).await.unwrap();
    assert!(b.is_enabled("q", G2));
    b.refresh().await.unwrap();
    assert!(!b.is_enabled("q", G2));
    assert_eq!(format!("{b:?}"), "Modules { modules: 3, version: 1 }");
    assert_eq!(b.ids().collect::<Vec<_>>(), ["p", "p/c", "q"]);
}

/// Spawns `modules`' poller at `interval`; dropping the guard stops it.
fn spawn_poll(modules: &Modules, interval: Duration) -> crate::worker::AbortOnDrop {
    let state = Arc::clone(modules.state.as_ref().unwrap());
    crate::worker::AbortOnDrop(tokio::spawn(poll(state, interval)))
}

/// Lets the spawned poller run what is ready at the current paused time.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn p1_another_app_sees_a_toggle_after_one_poll() {
    let (store, a) = fresh();
    let b = modules_over(&store);
    let _poller = spawn_poll(&b, Duration::from_secs(5));
    a.set_enabled("p", G1, false).await.unwrap();
    assert!(!a.is_enabled("p", G1));
    tokio::time::advance(Duration::from_millis(4_900)).await;
    settle().await;
    assert!(b.is_enabled("p", G1));
    tokio::time::advance(Duration::from_millis(100)).await;
    settle().await;
    assert!(!b.is_enabled("p", G1));
}

#[tokio::test(start_paused = true)]
async fn p2_a_failing_streak_warns_once_and_recovers_once() {
    let (store, a) = fresh();
    let b = modules_over(&store);
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let _poller = spawn_poll(&b, Duration::from_secs(5));
    a.set_enabled("p", G1, false).await.unwrap();
    store.fail_version.store(true, Ordering::SeqCst);
    for _ in 0..3 {
        tokio::time::advance(Duration::from_secs(5)).await;
        settle().await;
    }
    assert!(b.is_enabled("p", G1), "the last snapshot is kept");
    store.fail_version.store(false, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(5)).await;
    settle().await;
    assert!(!b.is_enabled("p", G1));
    let warns = capture.lines_with(&["WARN", "noctis::modules", "keeping the last snapshot"]);
    assert_eq!(warns.len(), 1, "{}", capture.text());
    let infos = capture.lines_with(&["INFO", "noctis::modules", "readable again"]);
    assert_eq!(infos.len(), 1, "{}", capture.text());
}

#[tokio::test(start_paused = true)]
async fn p3_an_unchanged_version_reads_no_record() {
    let (store, modules) = fresh();
    modules.refresh().await.unwrap();
    let before = store.gets.load(Ordering::SeqCst);
    let _poller = spawn_poll(&modules, Duration::from_secs(5));
    for _ in 0..3 {
        tokio::time::advance(Duration::from_secs(5)).await;
        settle().await;
    }
    assert_eq!(store.gets.load(Ordering::SeqCst), before);
}

#[tokio::test(start_paused = true)]
async fn p4_a_failed_load_keeps_all_enabled_until_a_tick_loads() {
    let (store, a) = fresh();
    a.set_enabled("p", ModuleScope::Global, false)
        .await
        .unwrap();
    let b = modules_over(&store);
    store.fail_get.store(true, Ordering::SeqCst);
    assert!(matches!(b.refresh().await, Err(ModuleStateError::Port(_))));
    assert_eq!(states(&b, ModuleScope::Global), [true; 3]);
    store.fail_get.store(false, Ordering::SeqCst);
    let _poller = spawn_poll(&b, Duration::from_secs(5));
    tokio::time::advance(Duration::from_secs(5)).await;
    settle().await;
    assert_eq!(states(&b, ModuleScope::Global), [false, false, true]);
}

#[test]
fn display_strings() {
    assert_eq!(
        ModuleStateError::UnknownModule { id: "nope".into() }.to_string(),
        "unknown Module `nope`: pass the id of a mounted Module (as written in `Module::new(..)`) \
         that the Boot Flags select"
    );
    assert_eq!(
        ModuleStateError::Port(broken()).to_string(),
        "the ModuleStateStore failed: port unavailable: broken"
    );
    assert_eq!(ModuleScope::Global.to_string(), "global");
    assert_eq!(ModuleScope::Guild(Id::new(42)).to_string(), "guild 42");
}

#[test]
fn index_paths_are_root_first() {
    let index = index();
    assert_eq!(index.ids(), ["p", "p/c", "q"]);
    assert_eq!(index.path("p/c"), Some(&[0, 1][..]));
    assert_eq!(index.path("q"), Some(&[2][..]));
    assert_eq!(index.path("x"), None);
}
