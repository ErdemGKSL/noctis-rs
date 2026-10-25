//! Nearest-rank percentiles in integer math and the table's number
//! formatting.

/// The per-mille ranks the table reports: p50, p99 and p99.9.
pub(crate) const RANKS: [u64; 3] = [500, 990, 999];

/// The smallest sample count each rank of [`RANKS`] needs: p99 needs 100
/// samples and p99.9 needs 1000, so a rank never names the maximum of a
/// handful of samples.
const MIN_SAMPLES: [usize; 3] = [1, 100, 1000];

/// The nearest-rank percentile `per_mille` of `sorted` (ascending): the
/// sample at index `(q·n + 999) / 1000 − 1`; `None` when there are fewer
/// samples than the rank needs.
pub(crate) fn percentile(sorted: &[u64], per_mille: u64) -> Option<u64> {
    let n = sorted.len();
    let needed = RANKS
        .iter()
        .zip(MIN_SAMPLES)
        .find_map(|(rank, min)| (*rank == per_mille).then_some(min))
        .unwrap_or(1);
    if n == 0 || n < needed {
        return None;
    }
    let n64 = u64::try_from(n).ok()?;
    let index = (per_mille.checked_mul(n64)?.checked_add(999)? / 1000).checked_sub(1)?;
    sorted.get(usize::try_from(index).ok()?).copied()
}

/// p50, p99 and p99.9 of `samples`, which this sorts.
pub(crate) fn percentiles(samples: &mut [u64]) -> [Option<u64>; 3] {
    samples.sort_unstable();
    RANKS.map(|rank| percentile(samples, rank))
}

/// A self-check case: its name, its samples and the expected ranks.
type Case<'a> = (&'a str, &'a [u64], [Option<u64>; 3]);

/// Checks [`percentile`] against the Matrix's exact and edge cases before
/// anything is measured.
///
/// # Errors
///
/// The first case that does not hold.
pub(crate) fn self_check() -> Result<(), String> {
    let exact: Vec<u64> = (1..=1000).collect();
    let cases: [Case<'_>; 4] = [
        ("n = 1000", &exact, [Some(500), Some(990), Some(999)]),
        ("n = 0", &[], [None, None, None]),
        ("n = 1", &[7], [Some(7), None, None]),
        ("n = 99", &exact[..99], [Some(50), None, None]),
    ];
    for (name, samples, expected) in cases {
        let got = RANKS.map(|rank| percentile(samples, rank));
        if got != expected {
            return Err(format!(
                "percentile self-check {name}: expected {expected:?}, got {got:?}"
            ));
        }
    }
    Ok(())
}

/// Nanoseconds as microseconds with one decimal, or `n/a`.
pub(crate) fn micros(ns: Option<u64>) -> String {
    ns.map_or_else(
        || "n/a".to_owned(),
        |ns| format!("{}.{}", ns / 1000, ns % 1000 / 100),
    )
}

/// `count` items over `ns` nanoseconds as a whole rate per second.
pub(crate) fn per_second(count: u64, ns: u64) -> u64 {
    if ns == 0 {
        return 0;
    }
    let rate = u128::from(count) * 1_000_000_000 / u128::from(ns);
    u64::try_from(rate).unwrap_or(u64::MAX)
}
