//! A Cluster in one process (Story 12.1, G9 and G10): two Apps with Shard
//! Ranges `0..2` and `2..4` of 4 (`max_concurrency` 2), each against its own
//! `FakeDiscordHttp` and `FakeGateway`. Sharing one `IdentifyQueue` (an
//! `Arc`) serializes each identify bucket across both Apps; without it, both
//! Apps identify bucket 0 at once.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::collections::BTreeMap;
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::{Duration, Instant};

use noctis_core::{App, Runtime, Token};
use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_testkit::fake::{
    FakeDiscordHttp, FakeGateway, GatewayBot, RecordedFrame, SessionStartLimit,
};

/// The bound on every single wait.
const WAIT: Duration = Duration::from_secs(15);

/// One Cluster process: its two fakes.
struct Process {
    http: FakeDiscordHttp,
    gw: FakeGateway,
}

impl Process {
    async fn start() -> Self {
        let bot = GatewayBot {
            shards: 4,
            session_start_limit: SessionStartLimit {
                max_concurrency: 2,
                ..SessionStartLimit::default()
            },
            ..GatewayBot::default()
        };
        Self {
            http: FakeDiscordHttp::start_with(bot).await.unwrap(),
            gw: FakeGateway::start().await.unwrap(),
        }
    }

    /// An App running `shards` of 4 against this process's fakes, sharing
    /// `queue` when given.
    fn app(&self, shards: std::ops::Range<u32>, queue: Option<&Arc<MemoryIdentifyQueue>>) -> App {
        let mut builder = App::builder();
        builder
            .runtime(&Runtime::for_test())
            .token(Token::new("fake-token").unwrap())
            .http_proxy(self.http.url())
            .gateway_url(self.gw.url())
            .shard_total(4)
            .shard_range(shards);
        if let Some(queue) = queue {
            builder.identify_queue(Arc::clone(queue));
        }
        builder.build().unwrap()
    }

    /// Every IDENTIFY received so far.
    fn identifies(&self) -> Vec<RecordedFrame> {
        self.gw
            .frames()
            .into_iter()
            .filter(|f| f.op() == Some(2))
            .collect()
    }
}

/// `d.shard` of an IDENTIFY frame.
fn shard_of(frame: &RecordedFrame) -> (u64, u64) {
    let shard = &frame.json().unwrap()["d"]["shard"];
    (shard[0].as_u64().unwrap(), shard[1].as_u64().unwrap())
}

/// Polls until `process` received `n` IDENTIFYs; returns shard id → time.
async fn wait_identifies(process: &Process, n: usize) -> BTreeMap<u64, Instant> {
    let deadline = Instant::now() + WAIT;
    loop {
        let frames = process.identifies();
        if frames.len() >= n {
            let mut at = BTreeMap::new();
            for frame in &frames {
                let (shard, total) = shard_of(frame);
                assert_eq!(total, 4);
                assert!(
                    at.insert(shard, frame.received_at).is_none(),
                    "shard {shard} twice"
                );
            }
            return at;
        }
        assert!(
            Instant::now() < deadline,
            "only {} of {n} identifies",
            frames.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Starts both Apps, waits for their identifies, and returns shard id →
/// IDENTIFY time across both, after checking each gateway saw only its own
/// shards.
async fn run_cluster(queue: Option<&Arc<MemoryIdentifyQueue>>) -> BTreeMap<u64, Instant> {
    let (a, b) = (Process::start().await, Process::start().await);
    let run_a = tokio::spawn(a.app(0..2, queue).run_with_args(std::iter::empty::<&str>()));
    let run_b = tokio::spawn(b.app(2..4, queue).run_with_args(std::iter::empty::<&str>()));
    let at_a = wait_identifies(&a, 2).await;
    let at_b = wait_identifies(&b, 2).await;
    assert_eq!(at_a.keys().copied().collect::<Vec<_>>(), [0, 1]);
    assert_eq!(at_b.keys().copied().collect::<Vec<_>>(), [2, 3]);
    assert!(!run_a.is_finished());
    assert!(!run_b.is_finished());
    run_a.abort();
    run_b.abort();
    at_a.into_iter().chain(at_b).collect()
}

/// The time between two instants, whichever is first.
fn gap(a: Instant, b: Instant) -> Duration {
    a.max(b) - a.min(b)
}

/// G9: a shared queue grants each bucket (shards 0/2 and 1/3) once per 5 s
/// across both Apps; both buckets start at once.
#[tokio::test(flavor = "multi_thread")]
async fn gw_01_shared_queue_serializes_buckets_across_apps() {
    let queue = Arc::new(MemoryIdentifyQueue::new(NonZeroU16::new(2).unwrap()));
    let at = run_cluster(Some(&queue)).await;
    let first = *at.values().min().unwrap();
    for (bucket, (x, y)) in [(0, (0, 2)), (1, (1, 3))] {
        assert!(
            gap(at[&x], at[&y]) >= Duration::from_millis(4500),
            "bucket {bucket}: {:?}",
            gap(at[&x], at[&y])
        );
        assert!(
            at[&x].min(at[&y]) - first <= Duration::from_secs(2),
            "bucket {bucket} started late"
        );
    }
}

/// G10: without a shared queue, each App has its own buckets, so shards 0
/// and 2 identify together (the control for G9).
#[tokio::test(flavor = "multi_thread")]
async fn without_a_shared_queue_buckets_overlap() {
    let at = run_cluster(None).await;
    assert!(
        gap(at[&0], at[&2]) <= Duration::from_secs(2),
        "{:?}",
        gap(at[&0], at[&2])
    );
}
