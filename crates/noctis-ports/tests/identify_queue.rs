//! `MemoryIdentifyQueue`: Discord's identify buckets on a paused tokio clock
//! (FR-2, GW-01, Story 3.3).

use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::Duration;

use noctis_ports::memory::MemoryIdentifyQueue;
use noctis_ports::{IdentifyQueue, PortError};
use tokio::time::{Instant, sleep, timeout};

fn queue(max_concurrency: u16) -> MemoryIdentifyQueue {
    let Some(max_concurrency) = NonZeroU16::new(max_concurrency) else {
        panic!("max_concurrency must be non-zero in tests");
    };
    MemoryIdentifyQueue::new(max_concurrency)
}

const fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// Acquires an in-range `bucket` and returns its Lease's bucket.
async fn lease_bucket(q: &dyn IdentifyQueue, bucket: u32) -> u32 {
    match q.acquire(bucket).await {
        Ok(lease) => lease.bucket(),
        Err(err) => panic!("bucket {bucket} must be in range: {err:?}"),
    }
}

/// Acquires `bucket` and returns when it was granted, relative to `start`.
async fn granted_at(q: &dyn IdentifyQueue, bucket: u32, start: Instant) -> Duration {
    assert_eq!(lease_bucket(q, bucket).await, bucket);
    start.elapsed()
}

#[tokio::test(start_paused = true)]
async fn buckets_of_16_grant_in_parallel_then_after_5s() {
    let q: Arc<dyn IdentifyQueue> = Arc::new(queue(16));
    let start = Instant::now();
    let tasks: Vec<_> = (0..32u32)
        .map(|shard| {
            let q = Arc::clone(&q);
            tokio::spawn(async move {
                let lease = q.acquire(shard % 16).await.expect("bucket in range");
                assert_eq!(lease.bucket(), shard % 16);
                (shard, start.elapsed())
            })
        })
        .collect();
    for task in tasks {
        let (shard, at) = task.await.expect("task completes");
        let expected = if shard < 16 { secs(0) } else { secs(5) };
        assert_eq!(at, expected, "shard {shard}");
    }
}

#[tokio::test(start_paused = true)]
async fn serial_bucket_is_fifo_5s_apart() {
    let q: Arc<dyn IdentifyQueue> = Arc::new(queue(1));
    let start = Instant::now();
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let q = Arc::clone(&q);
        tasks.push(tokio::spawn(async move {
            granted_at(q.as_ref(), 0, start).await
        }));
        tokio::task::yield_now().await;
    }
    for (i, task) in (0u64..).zip(tasks) {
        assert_eq!(task.await.expect("task completes"), secs(5 * i), "task {i}");
    }
}

#[tokio::test(start_paused = true)]
async fn sequential_acquires_in_one_task() {
    let q = queue(1);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    assert_eq!(granted_at(&q, 0, start).await, secs(5));
    assert_eq!(granted_at(&q, 0, start).await, secs(10));
}

#[tokio::test(start_paused = true)]
async fn idle_bucket_does_not_wait() {
    let q = queue(1);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    sleep(secs(10)).await;
    assert_eq!(granted_at(&q, 0, start).await, secs(10));
}

#[tokio::test(start_paused = true)]
async fn exact_boundary_does_not_wait() {
    let q = queue(1);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    sleep(secs(5)).await;
    assert_eq!(granted_at(&q, 0, start).await, secs(5));
}

#[tokio::test(start_paused = true)]
async fn partial_wait() {
    let q = queue(1);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    sleep(secs(3)).await;
    assert_eq!(granted_at(&q, 0, start).await, secs(5));
}

#[tokio::test(start_paused = true)]
async fn buckets_are_independent() {
    let q = queue(2);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    assert_eq!(granted_at(&q, 1, start).await, secs(0));
    assert_eq!(granted_at(&q, 1, start).await, secs(5));
}

#[tokio::test(start_paused = true)]
async fn interval_counts_from_the_grant() {
    let q = queue(1);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    sleep(secs(7)).await;
    assert_eq!(granted_at(&q, 0, start).await, secs(7));
    assert_eq!(granted_at(&q, 0, start).await, secs(12));
}

#[tokio::test(start_paused = true)]
async fn cancelled_waiter_takes_nothing() {
    let q = queue(1);
    let start = Instant::now();
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
    assert!(timeout(secs(1), q.acquire(0)).await.is_err());
    assert_eq!(granted_at(&q, 0, start).await, secs(5));
}

#[tokio::test(start_paused = true)]
async fn highest_bucket_is_granted() {
    let start = Instant::now();
    assert_eq!(granted_at(&queue(16), 15, start).await, secs(0));
    assert_eq!(granted_at(&queue(u16::MAX), 65_534, start).await, secs(0));
}

fn assert_invalid_bucket(result: Result<noctis_ports::Lease, PortError>, expected: &str) {
    match result {
        Err(PortError::InvalidValue { key, .. }) => assert_eq!(key, expected),
        other => panic!("expected InvalidValue for bucket {expected}, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn bucket_at_or_above_max_is_invalid() {
    assert_invalid_bucket(queue(16).acquire(16).await, "16");
    assert_invalid_bucket(queue(1).acquire(1).await, "1");
    assert_invalid_bucket(queue(u16::MAX).acquire(65_535).await, "65535");
    assert_invalid_bucket(queue(16).acquire(u32::MAX).await, "4294967295");
}

#[tokio::test(start_paused = true)]
async fn error_leaves_state_unchanged() {
    let q = queue(1);
    let start = Instant::now();
    assert_invalid_bucket(q.acquire(7).await, "7");
    assert_eq!(granted_at(&q, 0, start).await, secs(0));
}

#[test]
fn getters_and_default() {
    assert_eq!(queue(16).max_concurrency().get(), 16);
    assert_eq!(MemoryIdentifyQueue::default().max_concurrency().get(), 1);
    assert_eq!(MemoryIdentifyQueue::IDENTIFY_INTERVAL, secs(5));
}

#[test]
fn debug_shows_max_concurrency() {
    let debug = format!("{:?}", queue(16));
    assert!(debug.contains("max_concurrency"), "{debug}");
    assert!(debug.contains("16"), "{debug}");
}

#[test]
fn is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<MemoryIdentifyQueue>();
}

async fn identify_through(q: Arc<dyn IdentifyQueue>, bucket: u32) -> u32 {
    lease_bucket(q.as_ref(), bucket).await
}

#[tokio::test(start_paused = true)]
async fn swaps_in_as_dyn_port() {
    let q: Arc<dyn IdentifyQueue> = Arc::new(queue(4));
    let start = Instant::now();
    assert_eq!(identify_through(Arc::clone(&q), 3).await, 3);
    assert_eq!(identify_through(q, 3).await, 3);
    assert_eq!(start.elapsed(), secs(5));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distinct_buckets_on_many_threads() {
    let q: Arc<dyn IdentifyQueue> = Arc::new(queue(64));
    let tasks: Vec<_> = (0..64u32)
        .map(|bucket| {
            let q = Arc::clone(&q);
            tokio::spawn(async move { q.acquire(bucket).await })
        })
        .collect();
    let all = async {
        for (bucket, task) in (0u32..).zip(tasks) {
            let lease = task
                .await
                .expect("task completes")
                .expect("bucket in range");
            assert_eq!(lease.bucket(), bucket);
        }
    };
    timeout(Duration::from_secs(1), all)
        .await
        .expect("64 distinct buckets grant within 1 s");
}
