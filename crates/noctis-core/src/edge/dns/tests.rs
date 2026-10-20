//! Story 5.9 Matrix 41-45: the DNS cache (scripted resolver, paused clock).

use std::net::Ipv4Addr;
use std::sync::Mutex;

use tokio::time::{advance, sleep};

use super::*;

const A: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
const B: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

type Behaviour = Box<dyn Fn(Duration) -> (Duration, io::Result<Resolved>) + Send + Sync>;

/// A resolver that answers from `behaviour(time since start)` and records
/// each call's time.
struct Script {
    start: Instant,
    calls: Mutex<Vec<Duration>>,
    behaviour: Behaviour,
}

impl Script {
    fn new(
        behaviour: impl Fn(Duration) -> (Duration, io::Result<Resolved>) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            calls: Mutex::new(Vec::new()),
            behaviour: Box::new(behaviour),
        })
    }

    fn calls(&self) -> Vec<u64> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(Duration::as_secs)
            .collect()
    }
}

impl Resolve for Script {
    fn resolve(&self, _host: &str) -> BoxFuture<'_, io::Result<Resolved>> {
        let at = self.start.elapsed();
        self.calls.lock().unwrap().push(at);
        let (delay, result) = (self.behaviour)(at);
        async move {
            sleep(delay).await;
            result
        }
        .boxed()
    }
}

#[expect(clippy::unnecessary_wraps, reason = "what a resolver returns")]
fn answer(addr: IpAddr, ttl: u64) -> io::Result<Resolved> {
    Ok(Resolved {
        addrs: vec![addr],
        ttl: Duration::from_secs(ttl),
    })
}

fn cache(script: &Arc<Script>) -> DnsCache {
    DnsCache::new(
        Arc::clone(script) as Arc<dyn Resolve>,
        Duration::from_secs(30),
        Duration::from_mins(5),
    )
}

/// Advances the paused clock to `secs` after `start`.
async fn at(start: Instant, secs: u64) {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    let target = start + Duration::from_secs(secs);
    let now = Instant::now();
    if target > now {
        advance(target - now).await;
    }
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

/// Matrix 41: fresh answers, one refresh from 75 % of the TTL.
#[tokio::test(start_paused = true)]
async fn fresh_and_refresh() {
    let script = Script::new(|at| {
        if at < Duration::from_secs(40) {
            (Duration::ZERO, answer(A, 60))
        } else {
            (Duration::from_secs(10), answer(B, 60))
        }
    });
    let start = script.start;
    let cache = cache(&script);
    assert_eq!(cache.lookup("discord.com").await.unwrap(), [A]);
    at(start, 44).await;
    assert_eq!(cache.lookup("discord.com").await.unwrap(), [A]);
    at(start, 46).await;
    assert_eq!(cache.lookup("discord.com").await.unwrap(), [A]);
    at(start, 50).await;
    let before = Instant::now();
    assert_eq!(cache.lookup("discord.com").await.unwrap(), [A]);
    assert_eq!(Instant::now(), before, "the 50 s lookup waited");
    at(start, 61).await;
    assert_eq!(cache.lookup("discord.com").await.unwrap(), [B]);
    assert_eq!(script.calls(), [0, 46]);
}

/// Matrix 42: the TTL is clamped to [floor, cap].
#[tokio::test(start_paused = true)]
async fn ttl_clamp() {
    for (ttl, effective) in [(5, 30), (0, 30), (3600, 300)] {
        let script = Script::new(move |_| (Duration::ZERO, answer(A, ttl)));
        let cache = cache(&script);
        let start = script.start;
        cache.lookup("h").await.unwrap();
        // Before 75 % of the effective TTL: no refresh.
        at(start, effective * 3 / 4 - 1).await;
        cache.lookup("h").await.unwrap();
        assert_eq!(script.calls().len(), 1, "ttl {ttl}");
        // After the effective TTL: a new lookup.
        at(start, effective + 1).await;
        cache.lookup("h").await.unwrap();
        assert_eq!(script.calls().len(), 2, "ttl {ttl}");
    }
}

/// Matrix 43: stale answers on failure, up to 1 h past the TTL.
#[tokio::test(start_paused = true)]
async fn stale_on_failure() {
    let script = Script::new(|at| {
        if at < Duration::from_secs(70) {
            (Duration::ZERO, answer(A, 60))
        } else {
            (Duration::ZERO, Err(io::Error::other("down")))
        }
    });
    let start = script.start;
    let cache = cache(&script);
    cache.lookup("h").await.unwrap();
    at(start, 70).await;
    assert_eq!(cache.lookup("h").await.unwrap(), [A]);
    at(start, 3600 + 59).await;
    assert_eq!(cache.lookup("h").await.unwrap(), [A]);
    at(start, 3600 + 61).await;
    assert!(cache.lookup("h").await.is_err());
}

/// Matrix 44: concurrent lookups of an expired entry share one call.
#[tokio::test(start_paused = true)]
async fn single_flight() {
    let script = Script::new(|_| (Duration::from_secs(1), answer(A, 60)));
    let start = script.start;
    let cache = cache(&script);
    cache.lookup("h").await.unwrap();
    at(start, 120).await;
    let lookups = (0..10).map(|_| cache.lookup("h"));
    let results = futures_util::future::join_all(lookups).await;
    assert!(results.iter().all(|r| r.as_ref().unwrap() == &[A]));
    assert_eq!(script.calls().len(), 2);
}

/// Matrix 45: IP literals skip the resolver; an empty answer is a failure.
#[tokio::test(start_paused = true)]
async fn literal_and_empty() {
    let script = Script::new(|_| {
        (
            Duration::ZERO,
            Ok(Resolved {
                addrs: Vec::new(),
                ttl: Duration::from_mins(1),
            }),
        )
    });
    let cache = cache(&script);
    assert_eq!(
        cache.lookup("127.0.0.1").await.unwrap(),
        [IpAddr::from(Ipv4Addr::LOCALHOST)]
    );
    assert_eq!(
        cache.lookup("[::1]").await.unwrap(),
        [IpAddr::from(std::net::Ipv6Addr::LOCALHOST)]
    );
    assert!(script.calls().is_empty());
    assert!(cache.lookup("h").await.is_err());
    assert_eq!(script.calls().len(), 1);
}

/// An empty answer after a good one serves the cached addresses.
#[tokio::test(start_paused = true)]
async fn empty_answer_is_stale_served() {
    let script = Script::new(|at| {
        if at < Duration::from_secs(1) {
            (Duration::ZERO, answer(A, 30))
        } else {
            (
                Duration::ZERO,
                Ok(Resolved {
                    addrs: Vec::new(),
                    ttl: Duration::from_secs(30),
                }),
            )
        }
    });
    let start = script.start;
    let cache = cache(&script);
    cache.lookup("h").await.unwrap();
    at(start, 31).await;
    assert_eq!(cache.lookup("h").await.unwrap(), [A]);
}
