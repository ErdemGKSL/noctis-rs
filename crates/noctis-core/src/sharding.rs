//! The sharding plan: shard total, this process's shards and identify
//! buckets from `GET /gateway/bot` (GW-01, GW-02, GW-03).

use std::num::{NonZeroU16, NonZeroU32};
use std::ops::Range;
use std::time::Duration;

use twilight_model::gateway::connection_info::BotConnectionInfo;

use crate::StartError;

/// The parts of `GET /gateway/bot` the plan reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GatewayInfo {
    /// Discord's recommended shard count.
    pub(crate) recommended: u32,
    /// `session_start_limit.max_concurrency`.
    pub(crate) max_concurrency: u16,
    /// `session_start_limit.remaining`.
    pub(crate) remaining: u32,
    /// `session_start_limit.reset_after`, in milliseconds.
    pub(crate) reset_after_ms: u64,
}

impl From<&BotConnectionInfo> for GatewayInfo {
    fn from(info: &BotConnectionInfo) -> Self {
        Self {
            recommended: info.shards,
            max_concurrency: info.session_start_limit.max_concurrency,
            remaining: info.session_start_limit.remaining,
            reset_after_ms: info.session_start_limit.reset_after,
        }
    }
}

/// How many shards start and in how many identify buckets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShardPlan {
    /// The shard total.
    pub(crate) total: NonZeroU32,
    /// The shard ids this process starts: the Shard Range, or `0..total`.
    pub(crate) shards: Range<u32>,
    /// The identify buckets.
    pub(crate) max_concurrency: NonZeroU16,
    /// A fixed total that is not a multiple of `max_concurrency > 1`.
    pub(crate) total_misaligned: bool,
    /// A Shard Range whose start, or whose end below the total, is not a
    /// multiple of `max_concurrency > 1`: its buckets are split across
    /// processes.
    pub(crate) range_misaligned: bool,
}

/// Works out the plan from `info`, an optional fixed total and an optional
/// Shard Range (`AppBuilder::build` has checked that a range is non-empty,
/// comes with a fixed total and lies within it).
///
/// # Errors
///
/// - [`StartError::InvalidGatewayInfo`] when `max_concurrency` is 0, or the
///   total would come from a recommendation of 0;
/// - [`StartError::ShardTotalTooSmall`] when `fixed` is below
///   ⌈recommended × 2 / 5⌉ (GW-03);
/// - [`StartError::SessionStartLimit`] when `remaining` is below the number
///   of shards this process identifies (GW-02).
pub(crate) fn plan(
    info: GatewayInfo,
    fixed: Option<NonZeroU32>,
    range: Option<Range<u32>>,
) -> Result<ShardPlan, StartError> {
    let max_concurrency =
        NonZeroU16::new(info.max_concurrency).ok_or(StartError::InvalidGatewayInfo)?;
    let total = match fixed {
        Some(total) => total,
        None => NonZeroU32::new(info.recommended).ok_or(StartError::InvalidGatewayInfo)?,
    };
    if fixed.is_some() {
        // Discord recommends about one shard per 1000 guilds; a shard may hold
        // at most 2500. The quotient is at most u32::MAX * 2 / 5 + 1.
        let minimum = (u64::from(info.recommended) * 2).div_ceil(5);
        if u64::from(total.get()) < minimum {
            return Err(StartError::ShardTotalTooSmall {
                total: total.get(),
                minimum: u32::try_from(minimum).unwrap_or(u32::MAX),
            });
        }
    }
    let shards = range.unwrap_or(0..total.get());
    let required = shards.end.saturating_sub(shards.start);
    if info.remaining < required {
        return Err(StartError::SessionStartLimit {
            required,
            remaining: info.remaining,
            reset_after: Duration::from_millis(info.reset_after_ms),
        });
    }
    let mc = u32::from(max_concurrency.get());
    let total_misaligned = fixed.is_some() && mc > 1 && !total.get().is_multiple_of(mc);
    let range_misaligned = mc > 1
        && (!shards.start.is_multiple_of(mc)
            || (!shards.end.is_multiple_of(mc) && shards.end != total.get()));
    Ok(ShardPlan {
        total,
        shards,
        max_concurrency,
        total_misaligned,
        range_misaligned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(recommended: u32, max_concurrency: u16, remaining: u32) -> GatewayInfo {
        GatewayInfo {
            recommended,
            max_concurrency,
            remaining,
            reset_after_ms: 14_400_000,
        }
    }

    fn nz(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).unwrap()
    }

    /// The plan without a Shard Range (P10: everything as before).
    fn plan_of(info: GatewayInfo, fixed: Option<NonZeroU32>) -> Result<ShardPlan, StartError> {
        let planned = plan(info, fixed, None);
        if let Ok(planned) = &planned {
            assert_eq!(planned.shards, 0..planned.total.get());
            assert!(!planned.range_misaligned);
        }
        planned
    }

    /// The plan for a fixed `total` and Shard Range `range`.
    fn ranged(info: GatewayInfo, total: u32, range: Range<u32>) -> Result<ShardPlan, StartError> {
        plan(info, Some(nz(total)), Some(range))
    }

    #[test]
    fn recommended_total() {
        for (r, mc) in [(1, 1), (20, 16), (17, 16)] {
            let plan = plan_of(info(r, mc, 1000), None).unwrap();
            assert_eq!(plan.total.get(), r);
            assert_eq!(plan.max_concurrency.get(), mc);
            assert!(!plan.total_misaligned, "recommended never warns");
        }
    }

    #[test]
    fn bad_gateway_info() {
        assert_eq!(
            plan_of(info(5, 0, 1000), None),
            Err(StartError::InvalidGatewayInfo)
        );
        assert_eq!(
            plan_of(info(5, 0, 0), Some(nz(3))),
            Err(StartError::InvalidGatewayInfo)
        );
        assert_eq!(
            plan_of(info(0, 1, 1000), None),
            Err(StartError::InvalidGatewayInfo)
        );
    }

    #[test]
    fn gw_03_zero_recommended_with_fixed_total() {
        assert_eq!(
            plan_of(info(0, 1, 1000), Some(nz(3))).unwrap().total.get(),
            3
        );
    }

    #[test]
    fn gw_03_minimum_boundaries() {
        let too_small = |total, minimum| Err(StartError::ShardTotalTooSmall { total, minimum });
        let cases: [(u32, u32, Result<(), StartError>); 8] = [
            (1, 1, Ok(())),
            (2, 1, Ok(())),
            (3, 1, too_small(1, 2)),
            (3, 2, Ok(())),
            (5, 2, Ok(())),
            (10, 3, too_small(3, 4)),
            (10, 4, Ok(())),
            (2, 100, Ok(())),
        ];
        for (r, f, expected) in cases {
            let got = plan_of(info(r, 1, 1000), Some(nz(f))).map(drop);
            assert_eq!(got, expected, "recommended {r}, fixed {f}");
        }
    }

    #[test]
    fn gw_03_no_overflow() {
        // ⌈(2^32 - 1) × 2 / 5⌉ = 8_589_934_590 / 5 = 1_717_986_918 exactly.
        let max = info(u32::MAX, 1, u32::MAX);
        assert_eq!(
            plan_of(max, Some(nz(1_717_986_917))),
            Err(StartError::ShardTotalTooSmall {
                total: 1_717_986_917,
                minimum: 1_717_986_918,
            })
        );
        assert_eq!(
            plan_of(max, Some(nz(1_717_986_918))).unwrap().total.get(),
            1_717_986_918
        );
    }

    #[test]
    fn gw_02_session_start_limit() {
        assert_eq!(
            plan_of(info(20, 1, 19), None),
            Err(StartError::SessionStartLimit {
                required: 20,
                remaining: 19,
                reset_after: Duration::from_hours(4),
            })
        );
        assert!(plan_of(info(20, 1, 20), None).is_ok());
        assert!(matches!(
            plan_of(info(1, 1, 0), None),
            Err(StartError::SessionStartLimit {
                required: 1,
                remaining: 0,
                ..
            })
        ));
        let huge = GatewayInfo {
            reset_after_ms: u64::MAX,
            ..info(1, 1, 0)
        };
        let Err(StartError::SessionStartLimit { reset_after, .. }) = plan_of(huge, None) else {
            panic!("expected SessionStartLimit");
        };
        assert_eq!(reset_after, Duration::from_millis(u64::MAX));
    }

    #[test]
    fn gw_03_checked_before_gw_02() {
        assert_eq!(
            plan_of(info(10, 1, 0), Some(nz(1))),
            Err(StartError::ShardTotalTooSmall {
                total: 1,
                minimum: 4
            })
        );
    }

    #[test]
    fn misaligned() {
        for (total, mc, expected) in [
            (3, 2, true),
            (4, 2, false),
            (3, 1, false),
            (16, 16, false),
            (1, 16, true),
            (17, 16, true),
            (32, 16, false),
        ] {
            let plan = plan_of(info(1, mc, 1000), Some(nz(total))).unwrap();
            assert_eq!(plan.total_misaligned, expected, "{total}/{mc}");
        }
    }

    #[test]
    fn p1_aligned_range_starts_only_its_shards() {
        let plan = ranged(info(1, 16, 1000), 128, 32..64).unwrap();
        assert_eq!(plan.total.get(), 128);
        assert_eq!(plan.shards, 32..64);
        assert!(!plan.range_misaligned);
        assert!(!plan.total_misaligned);
    }

    #[test]
    fn p2_to_p6_range_alignment() {
        for (total, range, mc, expected) in [
            (4, 1..3, 2, true),   // P2 misaligned start
            (8, 0..3, 2, true),   // P3 misaligned end
            (3, 2..3, 2, false),  // P4 last range ends at the unaligned total
            (4, 1..3, 1, false),  // P5 mc = 1
            (7, 3..5, 1, false),  // P5
            (32, 0..4, 16, true), // P6 mc above the range length
            (32, 16..32, 16, false),
            (128, 32..64, 16, false),
            (3, 1..3, 2, true), // start misaligned, end == total
        ] {
            let plan = ranged(info(1, mc, 1000), total, range.clone()).unwrap();
            assert_eq!(plan.shards, range);
            assert_eq!(
                plan.range_misaligned, expected,
                "{range:?} of {total}, mc {mc}"
            );
        }
        // P4: the total warning still fires on its own.
        assert!(ranged(info(1, 2, 1000), 3, 2..3).unwrap().total_misaligned);
    }

    #[test]
    fn p7_p8_gw_02_counts_the_range() {
        assert_eq!(
            ranged(info(1, 1, 2), 4, 2..4).unwrap().shards,
            2..4,
            "P7: remaining covers the range"
        );
        assert_eq!(
            ranged(info(1, 1, 1), 4, 2..4),
            Err(StartError::SessionStartLimit {
                required: 2,
                remaining: 1,
                reset_after: Duration::from_hours(4),
            })
        );
        assert!(ranged(info(1, 1, 1), 4, 3..4).is_ok());
    }

    #[test]
    fn p9_gw_03_checks_the_total_before_gw_02() {
        assert_eq!(
            ranged(info(10, 1, 0), 3, 0..1),
            Err(StartError::ShardTotalTooSmall {
                total: 3,
                minimum: 4
            })
        );
        assert_eq!(
            ranged(info(10, 0, 1000), 3, 0..1),
            Err(StartError::InvalidGatewayInfo)
        );
    }

    #[test]
    fn whole_range_is_the_same_as_no_range() {
        let whole = ranged(info(1, 2, 1000), 4, 0..4).unwrap();
        assert_eq!(whole, plan(info(1, 2, 1000), Some(nz(4)), None).unwrap());
    }
}
