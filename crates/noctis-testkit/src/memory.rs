//! [`assert_memory_flat`]: NFR-2's "framework memory stays flat over N
//! interactions", measured with dhat.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{TestApp, TestError};

/// How much live heap may grow over the measured runs (64 KiB): enough for
/// connection-pool and HPACK noise, little enough to catch ≥ 330 bytes kept
/// per run at 200 runs.
pub const MEMORY_FLAT_SLACK_BYTES: u64 = 64 * 1024;

/// Whether an [`assert_memory_flat`] is running in this process.
static BUSY: AtomicBool = AtomicBool::new(false);

/// What [`assert_memory_flat`] measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct MemoryReport {
    /// Warm-up runs (not measured): `max(n / 10, 10)`.
    pub warm_up: usize,
    /// Measured runs: `n`.
    pub runs: usize,
    /// Live heap bytes after the warm-up.
    pub before_bytes: u64,
    /// Live heap bytes after the measured runs.
    pub after_bytes: u64,
}

/// Releases [`BUSY`] when dropped.
struct Busy;

impl Busy {
    fn acquire() -> Option<Self> {
        BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// The live heap bytes dhat counts now.
fn live_bytes() -> u64 {
    u64::try_from(dhat::HeapStats::get().curr_bytes).unwrap_or(u64::MAX)
}

/// Asserts that live heap memory stays flat while `f` runs `n` times
/// (NFR-2): per-run state the App keeps (a cache, a listener, a `Vec` in
/// State) makes it fail.
///
/// It starts a dhat testing profiler, runs `max(n / 10, 10)` warm-up calls,
/// reads the live heap, runs `n` calls and reads it again; growth over
/// [`MEMORY_FLAT_SLACK_BYTES`] is [`TestError::MemoryGrew`]. dhat is
/// process-wide, so put the test in its own test binary (one `#[test]`)
/// with the dhat allocator:
///
/// ```no_run
/// #[global_allocator]
/// static ALLOC: noctis_testkit::dhat::Alloc = noctis_testkit::dhat::Alloc;
///
/// #[test]
/// fn handlers_hold_no_per_run_state() -> Result<(), noctis_testkit::TestError> {
///     let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
///     rt.block_on(async {
///         let mut builder = noctis_core::App::builder();
///         let app = noctis_testkit::TestApp::build(&mut builder).await?;
///         noctis_testkit::assert_memory_flat(&app, 200, |app| async move {
///             app.command("ping").run().await.map(drop)
///         })
///         .await?;
///         Ok(())
///     })
/// }
/// ```
///
/// # Errors
///
/// [`TestError::TooFewRuns`] for `n < 10`, [`TestError::ProfilerBusy`] when
/// another call is running, [`TestError::AllocatorMissing`] without the
/// dhat global allocator, the first error `f` returns (as is), or
/// [`TestError::MemoryGrew`].
pub async fn assert_memory_flat<'a, F, Fut>(
    app: &'a TestApp,
    n: usize,
    mut f: F,
) -> Result<MemoryReport, TestError>
where
    F: FnMut(&'a TestApp) -> Fut,
    Fut: Future<Output = Result<(), TestError>>,
{
    if n < 10 {
        return Err(TestError::TooFewRuns { n });
    }
    let _busy = Busy::acquire().ok_or(TestError::ProfilerBusy)?;
    let profiler = dhat::Profiler::builder().testing().build();
    let probe = std::hint::black_box(Box::new([0_u8; 64]));
    let tracked = dhat::HeapStats::get().total_blocks;
    drop(probe);
    if tracked == 0 {
        return Err(TestError::AllocatorMissing);
    }
    let warm_up = (n / 10).max(10);
    for _ in 0..warm_up {
        f(app).await?;
    }
    let before_bytes = live_bytes();
    for _ in 0..n {
        f(app).await?;
    }
    let after_bytes = live_bytes();
    drop(profiler);
    if after_bytes > before_bytes.saturating_add(MEMORY_FLAT_SLACK_BYTES) {
        return Err(TestError::MemoryGrew {
            before: before_bytes,
            after: after_bytes,
            runs: n,
        });
    }
    Ok(MemoryReport {
        warm_up,
        runs: n,
        before_bytes,
        after_bytes,
    })
}
