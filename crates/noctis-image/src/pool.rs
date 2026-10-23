//! The process-scoped render pool (AD-24): a dedicated rayon pool behind a
//! weighted, two-lane admission queue (AD-15).
//!
//! Renders never run on tokio workers, on rayon's global pool or on any pool
//! the author can reach. Every App on one [`Runtime`](noctis_core::Runtime)
//! shares one pool. [`try_submit`](RenderPool::try_submit) weighs each job in
//! admission units ([`units`]): a still weighs 1, an animation
//! `ceil(frames / 8)`, at most the animation share. A quarter of
//! `queue_capacity` ([`reserve`]) is kept for stills, and a job that does not
//! fit fails at once with [`RenderError::QueueFull`].
//!
//! Admitted jobs wait in two lanes; a free pool thread always takes the
//! oldest still before any animation. A job whose requester is gone (its
//! receiver was dropped) is skipped when it reaches a thread.

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use noctis_ports::RenderError;
use tokio::sync::oneshot;

/// The frames one admission unit of an animation covers.
const FRAMES_PER_UNIT: u64 = 8;

/// The frozen configuration of the [`RenderPool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PoolConfig {
    /// The number of pool threads.
    pub(crate) threads: usize,
    /// The most admission units admitted at once, queued plus running.
    pub(crate) queue_capacity: usize,
}

/// The lane a job is queued in, with what its admission weight needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    /// A still: weight 1, dispatched first.
    Still,
    /// An animation of `frames` frames.
    Animated {
        /// The animation's frame count.
        frames: u64,
    },
}

/// The units a quarter of `capacity` (floored) keeps for stills.
fn reserve(capacity: usize) -> usize {
    capacity / 4
}

/// The admission weight of a job in `lane` on a pool of `capacity` units:
/// 1 for a still; `ceil(frames / 8)`, at least 1 and at most the animation
/// share `capacity - reserve(capacity)`, for an animation.
fn units(lane: Lane, capacity: usize) -> usize {
    match lane {
        Lane::Still => 1,
        Lane::Animated { frames } => {
            let share = capacity - reserve(capacity);
            let units = frames.div_ceil(FRAMES_PER_UNIT).max(1);
            usize::try_from(units).map_or(share, |units| units.min(share))
        }
    }
}

/// A queued job, ready to run on a pool thread.
type Task = Box<dyn FnOnce() + Send>;

/// The admitted units and the two lanes of queued jobs.
#[derive(Default)]
struct State {
    /// Admitted units, queued plus running, of both lanes.
    total: usize,
    /// Admitted units of animations.
    animated: usize,
    stills: VecDeque<Task>,
    animations: VecDeque<Task>,
}

/// The admission state every task shares.
struct Queue {
    state: Mutex<State>,
    capacity: usize,
}

impl Queue {
    fn lock(&self) -> MutexGuard<'_, State> {
        // No code panics while holding the lock; a poisoned lock still holds
        // consistent counts.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Pops the oldest still, else the oldest animation, and runs it outside
    /// the lock.
    fn run_next(&self) {
        let task = {
            let mut state = self.lock();
            state
                .stills
                .pop_front()
                .or_else(|| state.animations.pop_front())
        };
        if let Some(task) = task {
            task();
        }
    }
}

/// The render pool: a rayon pool of `noctis-render-<i>` threads and the
/// admission queue.
pub(crate) struct RenderPool {
    /// The pool, or rayon's build error (reported at install, since
    /// [`Resource::init`](noctis_core::Resource::init) is infallible).
    pool: Result<rayon::ThreadPool, String>,
    queue: Arc<Queue>,
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
            queue: Arc::new(Queue {
                state: Mutex::new(State::default()),
                capacity: config.queue_capacity,
            }),
        }
    }
}

impl std::fmt::Debug for RenderPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (total, animated) = {
            let state = self.queue.lock();
            (state.total, state.animated)
        };
        f.debug_struct("RenderPool")
            .field(
                "threads",
                &self
                    .pool
                    .as_ref()
                    .map(rayon::ThreadPool::current_num_threads),
            )
            .field("admitted", &total)
            .field("animated", &animated)
            .field("capacity", &self.queue.capacity)
            .finish()
    }
}

/// Releases a job's admission units when dropped: after it ran (panics
/// included), or when it was skipped or never run.
struct Release {
    queue: Arc<Queue>,
    units: usize,
    animated: bool,
}

impl Drop for Release {
    fn drop(&mut self) {
        let mut state = self.queue.lock();
        state.total -= self.units;
        if self.animated {
            state.animated -= self.units;
        }
    }
}

/// The requester of a running job, which the job asks whether to go on.
pub(crate) struct Requester<'a> {
    gone: &'a dyn Fn() -> bool,
}

impl<'a> Requester<'a> {
    /// A requester that is gone once `gone` returns `true`.
    pub(crate) fn new(gone: &'a dyn Fn() -> bool) -> Self {
        Self { gone }
    }

    /// Whether the requester dropped its receiver: the result would be
    /// thrown away.
    pub(crate) fn gone(&self) -> bool {
        (self.gone)()
    }
}

impl std::fmt::Debug for Requester<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Requester")
            .field("gone", &self.gone())
            .finish()
    }
}

impl RenderPool {
    /// The units admitted now, queued plus running.
    #[cfg(test)]
    pub(crate) fn admitted(&self) -> usize {
        self.queue.lock().total
    }

    /// rayon's error when the pool could not be built.
    pub(crate) fn build_error(&self) -> Option<&str> {
        self.pool.as_ref().err().map(String::as_str)
    }

    /// Admits `job` into `lane` and queues it; the receiver gets its result.
    ///
    /// `job` runs under `catch_unwind`: a panic becomes
    /// [`RenderError::Panicked`]. It gets the [`Requester`], which tells it
    /// when the receiver was dropped. A job whose receiver is dropped before
    /// it reaches a thread is skipped without running. Its units are released
    /// before the result is sent, in every case, so a caller that saw the
    /// result can submit again at once.
    ///
    /// # Errors
    ///
    /// [`RenderError::QueueFull`] when the job's units do not fit (see the
    /// module docs); [`RenderError::Failed`] when the pool could not be
    /// built.
    pub(crate) fn try_submit<T, F>(
        &self,
        lane: Lane,
        job: F,
    ) -> Result<oneshot::Receiver<Result<T, RenderError>>, RenderError>
    where
        T: Send + 'static,
        F: FnOnce(&Requester<'_>) -> Result<T, RenderError> + Send + 'static,
    {
        let pool = self.pool.as_ref().map_err(|err| {
            RenderError::failed(format!("the render pool could not be started: {err}"))
        })?;
        let capacity = self.queue.capacity;
        let weight = units(lane, capacity);
        let animated = matches!(lane, Lane::Animated { .. });
        let (tx, rx) = oneshot::channel();
        {
            let mut state = self.queue.lock();
            let fits = state.total + weight <= capacity
                && (!animated || state.animated + weight <= capacity - reserve(capacity));
            if !fits {
                return Err(RenderError::QueueFull);
            }
            state.total += weight;
            if animated {
                state.animated += weight;
            }
            let release = Release {
                queue: Arc::clone(&self.queue),
                units: weight,
                animated,
            };
            let task: Task = Box::new(move || run(job, tx, release));
            if animated {
                state.animations.push_back(task);
            } else {
                state.stills.push_back(task);
            }
        }
        let queue = Arc::clone(&self.queue);
        pool.spawn(move || queue.run_next());
        Ok(rx)
    }
}

/// Runs `job` and sends its result, unless the receiver is already gone;
/// `release` frees its units before the result is sent.
fn run<T, F>(job: F, tx: oneshot::Sender<Result<T, RenderError>>, release: Release)
where
    F: FnOnce(&Requester<'_>) -> Result<T, RenderError>,
{
    if tx.is_closed() {
        // Nobody waits for the result: skip the job.
        return;
    }
    let result = {
        let _release = release;
        let gone = || tx.is_closed();
        let requester = Requester::new(&gone);
        match catch_unwind(AssertUnwindSafe(|| job(&requester))) {
            Ok(result) => result,
            Err(panic) => Err(RenderError::Panicked {
                message: panic_message(&*panic).to_owned(),
            }),
        }
    };
    // The requester may be gone (its future was dropped); nothing is left
    // to notify.
    let _ = tx.send(result);
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
