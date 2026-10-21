//! The in-memory byte stores honour the op contracts, their limits and their
//! documented defaults (Story 3.2, byte-store matrix).

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::pin;
use std::sync::{Arc, Barrier};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use noctis_ports::memory::{
    AllowAllGuilds, MemoryAssetStore, MemoryCacheStore, MemoryLimits, MemoryModuleStateStore,
    MemoryRateLimitStore, MemoryRefStore, MemoryRenderCache, MemorySessionStore, MemoryUsage,
    NoLocaleOverlay,
};
use noctis_ports::{
    AssetStore, CacheStore, GuildAccessList, LocaleSource, ModuleStateStore, PortError,
    RateLimitStore, RefStore, RenderCache, SessionStore,
};
use twilight_model::id::Id;

const MIB: u64 = 1024 * 1024;
const DAY: Duration = Duration::from_hours(24);
const TTL: Duration = Duration::from_millis(100);
const PAST_TTL: Duration = Duration::from_millis(250);
const THREADS: usize = 100;

/// Polls a future that is ready on its first poll.
fn now<F: Future>(future: F) -> F::Output {
    let mut cx = Context::from_waker(Waker::noop());
    match pin!(future).poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("future was not ready"),
    }
}

fn b(text: &'static str) -> Bytes {
    Bytes::from_static(text.as_bytes())
}

fn store(max_bytes: u64, max_ttl: Option<Duration>) -> MemoryCacheStore {
    MemoryCacheStore::with_limits(MemoryLimits::new(max_bytes, max_ttl))
}

fn ref_store(max_ttl: Option<Duration>) -> MemoryRefStore {
    MemoryRefStore::with_limits(MemoryLimits::new(MIB, max_ttl))
}

fn assert_invalid(err: PortError, expected_key: &str) {
    match err {
        PortError::InvalidValue { key, .. } => assert_eq!(key, expected_key),
        other => panic!("expected InvalidValue, got {other:?}"),
    }
}

fn sleep(duration: Duration) {
    thread::sleep(duration);
}

// --- MemoryLimits ---

#[test]
fn limits_valid_edges() {
    let small = MemoryLimits::new(1, None);
    assert_eq!((small.max_bytes(), small.max_ttl()), (1, None));
    let large = MemoryLimits::new(u64::MAX, Some(Duration::from_nanos(1)));
    assert_eq!(large.max_bytes(), u64::MAX);
    assert_eq!(large.max_ttl(), Some(Duration::from_nanos(1)));
    assert_eq!(MemoryLimits::try_new(1, None), Some(small));
    assert_eq!(MemoryLimits::ENTRY_OVERHEAD, 64);
}

#[test]
fn limits_zero_is_rejected() {
    assert_eq!(MemoryLimits::try_new(0, None), None);
    assert_eq!(MemoryLimits::try_new(1, Some(Duration::ZERO)), None);
}

#[test]
#[should_panic(expected = "non-zero")]
fn limits_zero_bytes_panics() {
    let _ = MemoryLimits::new(0, None);
}

#[test]
#[should_panic(expected = "non-zero")]
fn limits_zero_ttl_panics() {
    let _ = MemoryLimits::new(1, Some(Duration::ZERO));
}

#[test]
fn every_default_matches_the_table() {
    macro_rules! check {
        ($($store:ident => $bytes:expr, $ttl:expr;)*) => {$(
            assert_eq!($store::DEFAULT_LIMITS, MemoryLimits::new($bytes, $ttl), stringify!($store));
            assert_eq!($store::new().limits(), $store::DEFAULT_LIMITS);
            assert_eq!($store::default().limits(), $store::DEFAULT_LIMITS);
            assert_eq!($store::new().usage(), MemoryUsage::default());
        )*};
    }
    check! {
        MemoryRefStore => 32 * MIB, Some(7 * DAY);
        MemorySessionStore => MIB, None;
        MemoryModuleStateStore => 16 * MIB, None;
        MemoryRateLimitStore => 16 * MIB, Some(DAY);
        MemoryCacheStore => 32 * MIB, None;
        MemoryRenderCache => 64 * MIB, Some(DAY);
        MemoryAssetStore => 32 * MIB, Some(DAY);
    }
}

#[test]
fn debug_prints_limits_not_data() {
    let store = MemoryCacheStore::new();
    now(store.put("secret-key", b("secret-value"), None)).unwrap();
    let debug = format!("{store:?}");
    assert!(debug.contains("MemoryCacheStore"), "{debug}");
    assert!(debug.contains("limits"), "{debug}");
    assert!(!debug.contains("secret"), "{debug}");
}

// --- Basic ops ---

#[test]
fn missing_key_is_none() {
    let store = MemoryCacheStore::new();
    assert_eq!(now(store.get("nope")).unwrap(), None);
    assert_eq!(
        store.usage(),
        MemoryUsage {
            entries: 0,
            bytes: 0
        }
    );
}

#[test]
fn empty_key_and_value_round_trip() {
    let store = MemoryCacheStore::new();
    now(store.put("", Bytes::new(), None)).unwrap();
    assert_eq!(now(store.get("")).unwrap(), Some(Bytes::new()));
}

#[test]
fn unicode_key_round_trips_and_weighs_utf8_bytes() {
    let store = MemoryCacheStore::new();
    let key = "ğüş😀\u{0}";
    now(store.put(key, b("value"), None)).unwrap();
    assert_eq!(now(store.get(key)).unwrap(), Some(b("value")));
    let expected = (key.len() + "value".len()) as u64 + MemoryLimits::ENTRY_OVERHEAD;
    assert_eq!(
        store.usage(),
        MemoryUsage {
            entries: 1,
            bytes: expected
        }
    );
}

#[test]
fn overwrite_resets_the_ttl() {
    let store = MemoryCacheStore::new();
    now(store.put("k", b("a"), Some(TTL))).unwrap();
    now(store.put("k", b("b"), None)).unwrap();
    sleep(PAST_TTL);
    assert_eq!(now(store.get("k")).unwrap(), Some(b("b")));
}

// --- Budget ---

#[test]
fn budget_exact_is_accepted() {
    let store = store(100, None);
    now(store.put("k", Bytes::from(vec![7; 35]), None)).unwrap();
    assert_eq!(now(store.get("k")).unwrap(), Some(Bytes::from(vec![7; 35])));
    assert_eq!(store.usage().bytes, 100);
}

#[test]
fn budget_plus_one_is_rejected_and_changes_nothing() {
    let store = store(100, None);
    now(store.put("k", b("prior"), None)).unwrap();
    let big = Bytes::from(vec![7; 36]);
    assert_invalid(now(store.put("k", big.clone(), None)).unwrap_err(), "k");
    assert_invalid(
        now(store.put_if_absent("k", big.clone(), None)).unwrap_err(),
        "k",
    );
    assert_invalid(
        now(store.put_if_absent("fresh", big, None)).unwrap_err(),
        "fresh",
    );
    assert_eq!(now(store.get("k")).unwrap(), Some(b("prior")));
    assert_eq!(now(store.get("fresh")).unwrap(), None);

    let key = "k".repeat(29);
    assert_invalid(now(store.incr_with_ttl(&key, 1, None)).unwrap_err(), &key);
    assert_eq!(now(store.get(&key)).unwrap(), None);
}

#[test]
fn usage_stays_within_the_budget() {
    let max_bytes = 10 * (64 + 4 + 8);
    let store = store(max_bytes, None);
    for i in 0..1000 {
        let key = format!("k{i:03}");
        now(store.put(&key, Bytes::from_static(b"12345678"), None)).unwrap();
    }
    let usage = store.usage();
    assert!(usage.bytes <= max_bytes, "{usage:?}");
    assert!(usage.entries <= 10, "{usage:?}");
    assert!(usage.entries > 0, "{usage:?}");
}

#[test]
fn full_budget_admits_the_newest_write() {
    let store = MemoryRefStore::with_limits(MemoryLimits::new(10 * (64 + 4 + 8), None));
    for i in 0..1000 {
        let key = format!("k{i:03}");
        now(store.put_if_absent(&key, Bytes::from_static(b"12345678"), None)).unwrap();
    }
    store.usage();
    assert!(now(store.get("k999")).unwrap().is_some());
    assert_eq!(now(store.get("k000")).unwrap(), None);
}

// --- TTL ---

#[test]
fn expired_entry_is_a_miss() {
    let store = ref_store(None);
    now(store.put_if_absent("k", b("v"), Some(TTL))).unwrap();
    sleep(PAST_TTL);
    assert_eq!(now(store.get("k")).unwrap(), None);
    assert_eq!(now(store.take("k")).unwrap(), None);
    assert!(!now(store.touch("k", Duration::from_secs(1))).unwrap());
}

#[test]
fn zero_ttl_is_dead_immediately() {
    let store = MemoryCacheStore::new();
    now(store.put("k", b("v"), Some(Duration::ZERO))).unwrap();
    assert_eq!(now(store.get("k")).unwrap(), None);
}

#[test]
fn huge_ttl_does_not_panic() {
    let store = MemoryCacheStore::new();
    now(store.put("k", b("v"), Some(Duration::MAX))).unwrap();
    assert_eq!(now(store.get("k")).unwrap(), Some(b("v")));
}

#[test]
fn max_ttl_clamps_every_write() {
    let store = store(MIB, Some(TTL));
    now(store.put("k", b("v"), None)).unwrap();
    now(store.put("k2", b("v"), Some(Duration::from_hours(1)))).unwrap();
    sleep(PAST_TTL);
    assert_eq!(now(store.get("k")).unwrap(), None);
    assert_eq!(now(store.get("k2")).unwrap(), None);
}

// --- put_if_absent ---

#[test]
fn put_if_absent_keeps_a_live_value_and_replaces_an_expired_one() {
    let store = MemoryCacheStore::new();
    assert!(now(store.put_if_absent("k", b("first"), None)).unwrap());
    assert!(!now(store.put_if_absent("k", b("second"), None)).unwrap());
    assert_eq!(now(store.get("k")).unwrap(), Some(b("first")));

    now(store.put("e", b("old"), Some(TTL))).unwrap();
    sleep(PAST_TTL);
    assert!(now(store.put_if_absent("e", b("new"), None)).unwrap());
    assert_eq!(now(store.get("e")).unwrap(), Some(b("new")));
}

#[test]
fn put_if_absent_race_has_one_winner() {
    let store = MemoryRefStore::new();
    let barrier = Barrier::new(THREADS);
    let winners: Vec<usize> = thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let (store, barrier) = (&store, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    let value = Bytes::from(i.to_string());
                    now(store.put_if_absent("k", value, None))
                        .unwrap()
                        .then_some(i)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect()
    });
    assert_eq!(winners.len(), 1, "{winners:?}");
    assert_eq!(
        now(store.get("k")).unwrap(),
        Some(Bytes::from(winners[0].to_string()))
    );
}

// --- take / delete / touch ---

#[test]
fn take_race_has_one_winner() {
    let store = MemoryRefStore::new();
    now(store.put_if_absent("k", b("v"), None)).unwrap();
    let barrier = Barrier::new(THREADS);
    let taken = thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let (store, barrier) = (&store, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    now(store.take("k")).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(taken, vec![b("v")]);
    assert_eq!(now(store.get("k")).unwrap(), None);
}

#[test]
fn delete_missing_and_live() {
    let store = MemoryRefStore::new();
    now(store.delete("missing")).unwrap();
    now(store.put_if_absent("k", b("v"), None)).unwrap();
    now(store.delete("k")).unwrap();
    assert_eq!(now(store.get("k")).unwrap(), None);
}

#[test]
fn expired_entries_leave_usage() {
    let store = ref_store(None);
    now(store.put_if_absent("k", b("v"), Some(TTL))).unwrap();
    // moka's timer wheel evicts at ~1.07 s granularity.
    sleep(Duration::from_millis(1500));
    assert_eq!(store.usage(), MemoryUsage::default());
}

#[test]
fn touch_extends_a_live_entry() {
    let store = ref_store(None);
    now(store.put_if_absent("k", b("v"), Some(Duration::from_millis(400)))).unwrap();
    sleep(Duration::from_millis(80));
    assert!(now(store.touch("k", Duration::from_secs(1))).unwrap());
    sleep(Duration::from_millis(150));
    assert_eq!(now(store.get("k")).unwrap(), Some(b("v")));
}

#[test]
fn touch_never_creates() {
    let store = MemoryRefStore::new();
    assert!(!now(store.touch("missing", Duration::from_secs(1))).unwrap());
    assert_eq!(now(store.get("missing")).unwrap(), None);
}

#[test]
fn touch_is_clamped_to_max_ttl() {
    let store = ref_store(Some(TTL));
    now(store.put_if_absent("k", b("v"), None)).unwrap();
    assert!(now(store.touch("k", Duration::from_hours(1))).unwrap());
    sleep(PAST_TTL);
    assert_eq!(now(store.get("k")).unwrap(), None);
}

// --- incr_with_ttl ---

#[test]
fn incr_creates_a_big_endian_counter() {
    let store = MemoryRateLimitStore::new();
    assert_eq!(now(store.incr_with_ttl("k", 0, None)).unwrap(), 0);
    assert_eq!(
        now(store.get("k")).unwrap(),
        Some(Bytes::copy_from_slice(&0u64.to_be_bytes()))
    );
}

#[test]
fn incr_race_loses_nothing() {
    let store = MemoryRateLimitStore::new();
    let barrier = Barrier::new(THREADS);
    let returned: BTreeSet<u64> = thread::scope(|scope| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let (store, barrier) = (&store, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    now(store.incr_with_ttl("k", 1, None)).unwrap()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(returned, (1..=100).collect::<BTreeSet<u64>>());
    assert_eq!(now(store.incr_with_ttl("k", 0, None)).unwrap(), 100);
}

#[test]
fn incr_sets_the_ttl_on_create_only() {
    let store = MemoryRateLimitStore::new();
    let first = now(store.incr_with_ttl("k", 1, Some(Duration::from_millis(400))));
    assert_eq!(first.unwrap(), 1);
    sleep(Duration::from_millis(80));
    let second = now(store.incr_with_ttl("k", 1, Some(Duration::from_secs(10))));
    assert_eq!(second.unwrap(), 2);
    sleep(Duration::from_millis(500));
    assert_eq!(now(store.get("k")).unwrap(), None);
}

#[test]
fn incr_saturates() {
    let store = MemoryRateLimitStore::new();
    let raw = Bytes::copy_from_slice(&(u64::MAX - 1).to_be_bytes());
    now(store.put("k", raw, None)).unwrap();
    assert_eq!(now(store.incr_with_ttl("k", 5, None)).unwrap(), u64::MAX);
    assert_eq!(now(store.incr_with_ttl("k", 5, None)).unwrap(), u64::MAX);
}

#[test]
fn incr_on_a_non_counter_is_invalid() {
    let store = MemoryCacheStore::new();
    for value in [
        Bytes::from(vec![1; 7]),
        Bytes::from(vec![1; 9]),
        Bytes::new(),
    ] {
        now(store.put("k", value.clone(), None)).unwrap();
        assert_invalid(now(store.incr_with_ttl("k", 1, None)).unwrap_err(), "k");
        assert_eq!(now(store.get("k")).unwrap(), Some(value));
    }
}

#[test]
fn incr_after_expiry_recreates() {
    let store = MemoryRateLimitStore::new();
    now(store.incr_with_ttl("k", 41, Some(TTL))).unwrap();
    sleep(PAST_TTL);
    assert_eq!(now(store.incr_with_ttl("k", 3, None)).unwrap(), 3);
}

// --- ModuleStateStore::version ---

#[test]
fn version_grows_on_every_successful_put() {
    let store = MemoryModuleStateStore::with_limits(MemoryLimits::new(1024, None));
    assert_eq!(now(store.version()).unwrap(), 0);
    now(store.put("m", b("on"), None)).unwrap();
    assert_eq!(now(store.version()).unwrap(), 1);
    let oversized = Bytes::from(vec![0; 2048]);
    assert!(now(store.put("m", oversized, None)).is_err());
    assert_eq!(now(store.version()).unwrap(), 1);

    let barrier = Barrier::new(THREADS);
    thread::scope(|scope| {
        for i in 0..THREADS {
            let (store, barrier) = (&store, &barrier);
            scope.spawn(move || {
                barrier.wait();
                now(store.put(&format!("m{}", i % 5), b("on"), None)).unwrap();
            });
        }
    });
    assert_eq!(now(store.version()).unwrap(), 101);
}

// --- Swap via dyn ---

/// Callers that know only the port trait.
#[cfg(test)]
mod dyn_swap {
    use super::*;

    pub(super) fn use_ref(store: &Arc<dyn RefStore>) {
        assert!(now(store.put_if_absent("k", b("v"), None)).unwrap());
        assert!(now(store.touch("k", Duration::from_secs(1))).unwrap());
        assert_eq!(now(store.take("k")).unwrap(), Some(b("v")));
        now(store.delete("k")).unwrap();
        assert_eq!(now(store.get("k")).unwrap(), None);
    }

    pub(super) fn use_session(store: &Arc<dyn SessionStore>) {
        now(store.put("k", b("v"), None)).unwrap();
        assert_eq!(now(store.get("k")).unwrap(), Some(b("v")));
        now(store.delete("k")).unwrap();
        assert_eq!(now(store.get("k")).unwrap(), None);
    }

    pub(super) fn use_module_state(store: &Arc<dyn ModuleStateStore>) {
        now(store.put("k", b("v"), None)).unwrap();
        assert_eq!(now(store.get("k")).unwrap(), Some(b("v")));
        assert_eq!(now(store.version()).unwrap(), 1);
    }

    pub(super) fn use_rate_limit(store: &Arc<dyn RateLimitStore>) {
        assert_eq!(now(store.incr_with_ttl("k", 2, None)).unwrap(), 2);
        now(store.put("k", Bytes::copy_from_slice(&9u64.to_be_bytes()), None)).unwrap();
        assert_eq!(now(store.incr_with_ttl("k", 1, None)).unwrap(), 10);
        assert!(now(store.get("k")).unwrap().is_some());
    }

    pub(super) fn use_cache(store: &Arc<dyn CacheStore>) {
        now(store.put("k", b("v"), None)).unwrap();
        assert!(!now(store.put_if_absent("k", b("w"), None)).unwrap());
        assert_eq!(now(store.get("k")).unwrap(), Some(b("v")));
        now(store.delete("k")).unwrap();
        assert_eq!(now(store.incr_with_ttl("k", 3, None)).unwrap(), 3);
    }

    pub(super) fn use_render(store: &Arc<dyn RenderCache>) {
        now(store.put("k", b("png"), None)).unwrap();
        assert_eq!(now(store.get("k")).unwrap(), Some(b("png")));
    }

    pub(super) fn use_asset(store: &Arc<dyn AssetStore>) {
        now(store.put("k", b("avatar"), None)).unwrap();
        assert_eq!(now(store.get("k")).unwrap(), Some(b("avatar")));
    }
}

#[test]
fn every_store_swaps_in_as_dyn_port() {
    dyn_swap::use_ref(&(Arc::new(MemoryRefStore::new()) as Arc<dyn RefStore>));
    dyn_swap::use_session(&(Arc::new(MemorySessionStore::new()) as Arc<dyn SessionStore>));
    dyn_swap::use_module_state(
        &(Arc::new(MemoryModuleStateStore::new()) as Arc<dyn ModuleStateStore>),
    );
    dyn_swap::use_rate_limit(&(Arc::new(MemoryRateLimitStore::new()) as Arc<dyn RateLimitStore>));
    dyn_swap::use_cache(&(Arc::new(MemoryCacheStore::new()) as Arc<dyn CacheStore>));
    dyn_swap::use_render(&(Arc::new(MemoryRenderCache::new()) as Arc<dyn RenderCache>));
    dyn_swap::use_asset(&(Arc::new(MemoryAssetStore::new()) as Arc<dyn AssetStore>));
}

// --- Trivial defaults ---

#[test]
fn trivial_defaults() {
    let locales: Arc<dyn LocaleSource> = Arc::new(NoLocaleOverlay);
    assert_eq!(now(locales.load("en-US")).unwrap(), Vec::new());
    assert_eq!(now(locales.load("")).unwrap(), Vec::new());
    let access: Arc<dyn GuildAccessList> = Arc::new(AllowAllGuilds);
    assert!(access.allows(Id::new(1)));
    assert!(AllowAllGuilds.allows(Id::new(u64::MAX)));
    let defaults: (NoLocaleOverlay, AllowAllGuilds) = Default::default();
    assert_eq!(defaults, (NoLocaleOverlay, AllowAllGuilds));
}

/// Every byte store is implemented for `Arc<P>`: one instance behind
/// `dyn CacheStore`, twice wrapped, still sees the same entries.
#[test]
fn arc_of_a_store_is_a_store() {
    let inner = Arc::new(MemoryCacheStore::new());
    let erased: Arc<dyn CacheStore> = Arc::new(Arc::clone(&inner));
    now(erased.put("k", b("v"), None)).unwrap();
    assert_eq!(now(inner.get("k")).unwrap(), Some(b("v")));
    let dyn_arc: Arc<dyn CacheStore> = Arc::new(MemoryCacheStore::new());
    let twice: Arc<dyn CacheStore> = Arc::new(Arc::clone(&dyn_arc));
    assert_eq!(now(twice.incr_with_ttl("n", 2, None)).unwrap(), 2);
    assert_eq!(now(dyn_arc.incr_with_ttl("n", 1, None)).unwrap(), 3);
}
