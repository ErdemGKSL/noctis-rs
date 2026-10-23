//! The process-scoped render pool (AD-24): a dedicated rayon pool behind a
//! bounded admission counter.
//!
//! Renders never run on tokio workers, on rayon's global pool or on any pool
//! the author can reach. Every App on one [`Runtime`](noctis_core::Runtime)
//! shares one pool; [`try_submit`](RenderPool::try_submit) admits a job only
//! while fewer than `queue_capacity` jobs are queued or running, and a full
//! queue fails at once with [`RenderError::QueueFull`].

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use noctis_ports::RenderError;
use tokio::sync::oneshot;

/// The frozen configuration of the [`RenderPool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PoolConfig {
    /// The number of pool threads.
    pub(crate) threads: usize,
    /// The most jobs admitted at once, queued plus running.
    pub(crate) queue_capacity: usize,
}

/// The render pool: a rayon pool of `noctis-render-<i>` threads and the
/// count of admitted jobs.
pub(crate) struct RenderPool {
    /// The pool, or rayon's build error (reported at install, since
    /// [`Resource::init`](noctis_core::Resource::init) is infallible).
    pool: Result<rayon::ThreadPool, String>,
    /// Admitted jobs: queued plus running.
    admitted: Arc<AtomicUsize>,
    capacity: usize,
}

impl noctis_core::Resource for RenderPool {
    type Config = PoolConfig;

    fn init(config: &PoolConfig) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(config.threads)
            .thread_name(|i| format!("noctis-render-{i}"))
            .build()
            .map_err(|err| err.to_string());
        Self {
            pool,
            admitted: Arc::new(AtomicUsize::new(0)),
            capacity: config.queue_capacity,
        }
    }
}

impl std::fmt::Debug for RenderPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPool")
            .field(
                "threads",
                &self
                    .pool
                    .as_ref()
                    .map(rayon::ThreadPool::current_num_threads),
            )
            .field("admitted", &self.admitted.load(Ordering::Relaxed))
            .field("capacity", &self.capacity)
            .finish()
    }
}

/// Releases one admission slot when dropped, panics included.
struct Release(Arc<AtomicUsize>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RenderPool {
    /// rayon's error when the pool could not be built.
    pub(crate) fn build_error(&self) -> Option<&str> {
        self.pool.as_ref().err().map(String::as_str)
    }

    /// Admits `job` and spawns it on the pool; the receiver gets its result.
    ///
    /// `job` runs under `catch_unwind`: a panic becomes
    /// [`RenderError::Panicked`]. The admission slot is released before the
    /// result is sent, in every case, so a caller that saw the result can
    /// submit again at once.
    ///
    /// # Errors
    ///
    /// [`RenderError::QueueFull`] when `queue_capacity` jobs are already
    /// admitted; [`RenderError::Failed`] when the pool could not be built.
    pub(crate) fn try_submit<T, F>(
        &self,
        job: F,
    ) -> Result<oneshot::Receiver<Result<T, RenderError>>, RenderError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, RenderError> + Send + 'static,
    {
        let pool = self.pool.as_ref().map_err(|err| {
            RenderError::failed(format!("the render pool could not be started: {err}"))
        })?;
        self.admitted
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |admitted| {
                (admitted < self.capacity).then_some(admitted + 1)
            })
            .map_err(|_| RenderError::QueueFull)?;
        let release = Release(Arc::clone(&self.admitted));
        let (tx, rx) = oneshot::channel();
        pool.spawn(move || {
            let result = {
                let _release = release;
                match catch_unwind(AssertUnwindSafe(job)) {
                    Ok(result) => result,
                    Err(panic) => Err(RenderError::Panicked {
                        message: panic_message(&*panic).to_owned(),
                    }),
                }
            };
            // The requester may be gone (its future was dropped); nothing
            // is left to notify.
            let _ = tx.send(result);
        });
        Ok(rx)
    }
}

/// A panic payload's message: `&str` or `String`, else a placeholder.
fn panic_message(panic: &(dyn Any + Send)) -> &str {
    if let Some(message) = panic.downcast_ref::<&str>() {
        message
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message
    } else {
        "<non-string panic>"
    }
}

#[cfg(test)]
mod tests;
