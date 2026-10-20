//! [`PortQueue`]: twilight's identify `Queue` over the `IdentifyQueue` port
//! (GW-01).

use std::fmt;
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::Duration;

use noctis_ports::IdentifyQueue;
use tokio::sync::oneshot;
use twilight_gateway::queue::Queue;

/// The first wait after a failed `acquire`.
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
/// The longest wait between two failed `acquire`s.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Asks the [`IdentifyQueue`] port for a lease in bucket
/// `shard_id % max_concurrency` before each identify.
#[derive(Clone)]
pub(crate) struct PortQueue {
    port: Arc<dyn IdentifyQueue>,
    max_concurrency: NonZeroU16,
}

impl PortQueue {
    /// A queue over `port` with `max_concurrency` buckets.
    pub(crate) fn new(port: Arc<dyn IdentifyQueue>, max_concurrency: NonZeroU16) -> Self {
        Self {
            port,
            max_concurrency,
        }
    }
}

impl fmt::Debug for PortQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortQueue")
            .field("max_concurrency", &self.max_concurrency)
            .finish_non_exhaustive()
    }
}

impl Queue for PortQueue {
    fn enqueue(&self, id: u32) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        let bucket = id % u32::from(self.max_concurrency.get());
        tokio::spawn(acquire(Arc::clone(&self.port), bucket, tx));
        rx
    }
}

/// What one `acquire` attempt ended with.
enum Attempt {
    Granted,
    Failed,
    Cancelled,
}

/// Acquires a lease in `bucket` and signals `tx`, retrying failures with a
/// backoff, until granted or until the receiver is dropped.
///
/// `tx` is kept until then: twilight re-enqueues a cancelled receiver at once,
/// so dropping it early would busy-loop.
async fn acquire(port: Arc<dyn IdentifyQueue>, bucket: u32, mut tx: oneshot::Sender<()>) {
    let mut backoff = INITIAL_BACKOFF;
    loop {
        let attempt = tokio::select! {
            result = port.acquire(bucket) => match result {
                // The lease is dropped here: it grants exactly this identify.
                Ok(_lease) => Attempt::Granted,
                Err(error) => {
                    tracing::warn!(
                        target: "noctis::gateway",
                        bucket,
                        %error,
                        retry_in = ?backoff,
                        "identify queue failed; retrying"
                    );
                    Attempt::Failed
                }
            },
            () = tx.closed() => Attempt::Cancelled,
        };
        match attempt {
            Attempt::Granted => {
                let _ = tx.send(());
                return;
            }
            Attempt::Cancelled => return,
            Attempt::Failed => {
                tokio::select! {
                    () = tokio::time::sleep(backoff) => {}
                    () = tx.closed() => return,
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use noctis_ports::memory::MemoryIdentifyQueue;
    use noctis_ports::{BoxFuture, Lease, PortError};
    use tokio::time::Instant;

    use super::*;

    /// Records each bucket asked for; fails the first `failures` acquires and
    /// never grants when `hang` is set.
    #[derive(Default)]
    struct Scripted {
        buckets: Mutex<Vec<(u32, Instant)>>,
        calls: AtomicUsize,
        failures: usize,
        hang: bool,
    }

    impl IdentifyQueue for Scripted {
        fn acquire(&self, bucket: u32) -> BoxFuture<'_, Result<Lease, PortError>> {
            self.buckets.lock().unwrap().push((bucket, Instant::now()));
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if self.hang {
                    std::future::pending::<()>().await;
                }
                if call < self.failures {
                    Err(PortError::Unavailable {
                        why: "scripted".into(),
                    })
                } else {
                    Ok(Lease::new(bucket))
                }
            })
        }
    }

    fn nz(n: u16) -> NonZeroU16 {
        NonZeroU16::new(n).unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn gw_01_bucket_is_shard_mod_max_concurrency() {
        let port = Arc::new(Scripted::default());
        let queue = PortQueue::new(port.clone(), nz(16));
        for id in [17, 16, u32::MAX] {
            queue.enqueue(id).await.unwrap();
        }
        let queue = PortQueue::new(port.clone(), nz(1));
        queue.enqueue(5).await.unwrap();
        let buckets: Vec<u32> = port.buckets.lock().unwrap().iter().map(|b| b.0).collect();
        assert_eq!(buckets, [1, 0, 15, 0]);
    }

    #[tokio::test(start_paused = true)]
    async fn gw_01_memory_queue_through_port() {
        let port = Arc::new(MemoryIdentifyQueue::new(nz(2)));
        let queue = PortQueue::new(port, nz(2));
        let start = Instant::now();
        queue.enqueue(0).await.unwrap();
        queue.enqueue(1).await.unwrap();
        assert_eq!(start.elapsed(), Duration::ZERO);
        queue.enqueue(2).await.unwrap();
        assert!(start.elapsed() >= Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn errors_back_off_then_grant() {
        let port = Arc::new(Scripted {
            failures: 2,
            ..Scripted::default()
        });
        let start = Instant::now();
        let rx = PortQueue::new(port.clone(), nz(1)).enqueue(0);
        assert_eq!(rx.await, Ok(()));
        let calls = port.buckets.lock().unwrap().clone();
        assert_eq!(calls.len(), 3);
        assert!(calls[1].1 - start >= Duration::from_secs(1));
        assert!(calls[2].1 - start >= Duration::from_secs(3));
        tokio::time::sleep(Duration::from_mins(2)).await;
        assert_eq!(port.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_caps_at_thirty_seconds() {
        let port = Arc::new(Scripted {
            failures: 8,
            ..Scripted::default()
        });
        let rx = PortQueue::new(port.clone(), nz(1)).enqueue(0);
        assert_eq!(rx.await, Ok(()));
        let calls = port.buckets.lock().unwrap().clone();
        let gaps: Vec<u64> = calls
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).as_secs())
            .collect();
        assert_eq!(gaps, [1, 2, 4, 8, 16, 30, 30, 30]);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_while_pending_ends_the_task() {
        let port = Arc::new(Scripted {
            hang: true,
            ..Scripted::default()
        });
        let rx = PortQueue::new(port.clone(), nz(1)).enqueue(0);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        drop(rx);
        tokio::time::sleep(Duration::from_mins(2)).await;
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        // The task dropped its acquire future and the port's Arc clone.
        assert_eq!(Arc::strong_count(&port), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_during_backoff_ends_the_task() {
        let port = Arc::new(Scripted {
            failures: usize::MAX,
            ..Scripted::default()
        });
        let rx = PortQueue::new(port.clone(), nz(1)).enqueue(0);
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        drop(rx);
        tokio::time::sleep(Duration::from_mins(2)).await;
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        assert_eq!(Arc::strong_count(&port), 1);
    }
}
