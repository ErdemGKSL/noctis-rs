//! The `IdentifyQueue` suite.

use std::time::{Duration, Instant};

use noctis_ports::{IdentifyQueue, Lease};

use super::Case;

const SUITE: &str = "identify_queue";
/// How fast an idle bucket must grant.
const IDLE_GRANT: Duration = Duration::from_secs(1);
/// The shortest accepted gap between two Leases of one bucket (5 s, less
/// clock slack).
const MIN_SPACING: Duration = Duration::from_millis(4_900);
/// The longest accepted gap between two Leases of one bucket.
const MAX_SPACING: Duration = Duration::from_secs(8);
/// How long any `acquire` may take before the case gives up.
const GIVE_UP: Duration = Duration::from_secs(10);

/// Runs the `IdentifyQueue` suite: a Lease names its bucket, one bucket
/// grants at most once per 5 s, and different buckets grant in parallel.
///
/// The factory must give a queue of at least 2 buckets that granted no
/// Lease in the last 5 s. The suite takes about 5 s. See the
/// [module docs](super) for the clock and runtime requirements.
///
/// # Panics
///
/// On the first failing case, with
/// `` conformance::identify_queue: case `<case>` failed: <detail> ``.
pub async fn identify_queue<P: IdentifyQueue, F: Fn() -> P>(factory: F) {
    lease_bucket(&factory()).await;
    spacing_and_parallel(&factory()).await;
}

/// Acquires a Lease for `bucket`, failing the case on `Err` or after
/// [`GIVE_UP`]; returns it with the instant it was granted.
async fn acquire(case: Case, queue: &impl IdentifyQueue, bucket: u32) -> (Lease, Instant) {
    match tokio::time::timeout(GIVE_UP, queue.acquire(bucket)).await {
        Ok(result) => (
            case.ok(format_args!("acquire({bucket})"), result),
            Instant::now(),
        ),
        Err(_) => case.fail(format_args!(
            "acquire({bucket}) was not granted within {GIVE_UP:?}"
        )),
    }
}

async fn lease_bucket(queue: &impl IdentifyQueue) {
    let case = Case::new(SUITE, "lease_bucket");
    let start = Instant::now();
    let (first, granted) = acquire(case, queue, 0).await;
    case.check(first.bucket() == 0, || {
        format!("acquire(0) returned a Lease for bucket {}", first.bucket())
    });
    let waited = granted - start;
    case.check(waited <= IDLE_GRANT, || {
        format!(
            "the first acquire(0) took {waited:?}; an idle bucket must grant within {IDLE_GRANT:?}"
        )
    });
    let (second, _) = acquire(case, queue, 1).await;
    case.check(second.bucket() == 1, || {
        format!("acquire(1) returned a Lease for bucket {}", second.bucket())
    });
}

async fn spacing_and_parallel(queue: &impl IdentifyQueue) {
    let spacing = Case::new(SUITE, "bucket_spacing");
    let parallel = Case::new(SUITE, "buckets_parallel");
    let (_first, t1) = acquire(spacing, queue, 0).await;
    let started = Instant::now();
    let ((again, t2), (other, t_other)) =
        tokio::join!(acquire(spacing, queue, 0), acquire(parallel, queue, 1));
    let gap = t2 - t1;
    spacing.check(again.bucket() == 0, || {
        format!("acquire(0) returned a Lease for bucket {}", again.bucket())
    });
    spacing.check((MIN_SPACING..=MAX_SPACING).contains(&gap), || {
        format!(
            "the second Lease of bucket 0 came {gap:?} after the first; expected between {MIN_SPACING:?} and {MAX_SPACING:?}"
        )
    });
    let waited = t_other - started;
    parallel.check(other.bucket() == 1, || {
        format!("acquire(1) returned a Lease for bucket {}", other.bucket())
    });
    parallel.check(waited <= IDLE_GRANT && t_other < t2, || {
        format!(
            "acquire(1) took {waited:?} while bucket 0 was waiting; another bucket must grant within {IDLE_GRANT:?}"
        )
    });
}
