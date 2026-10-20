//! The sharding plan: shard total and identify buckets from
//! `GET /gateway/bot` (GW-01, GW-02, GW-03).

use std::num::{NonZeroU16, NonZeroU32};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShardPlan {
    /// The shard total.
    pub(crate) total: NonZeroU32,
    /// The identify buckets.
    pub(crate) max_concurrency: NonZeroU16,
    /// A fixed total that is not a multiple of `max_concurrency > 1`.
    pub(crate) misaligned: bool,
}

/// Works out the plan from `info` and an optional fixed total.
///
/// # Errors
///
/// - [`StartError::InvalidGatewayInfo`] when `max_concurrency` is 0, or the
///   total would come from a recommendation of 0;
/// - [`StartError::ShardTotalTooSmall`] when `fixed` is below
///   ⌈recommended × 2 / 5⌉ (GW-03);
/// - [`StartError::SessionStartLimit`] when `remaining` is below the total
///   (GW-02).
pub(crate) fn plan(info: GatewayInfo, fixed: Option<NonZeroU32>) -> Result<ShardPlan, StartError> {
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
    if info.remaining < total.get() {
        return Err(StartError::SessionStartLimit {
            required: total.get(),
            remaining: info.remaining,
            reset_after: Duration::from_millis(info.reset_after_ms),
        });
    }
    let misaligned = fixed.is_some()
        && max_concurrency.get() > 1
        && total.get() % u32::from(max_concurrency.get()) != 0;
    Ok(ShardPlan {
        total,
        max_concurrency,
        misaligned,
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

    #[test]
    fn recommended_total() {
        for (r, mc) in [(1, 1), (20, 16), (17, 16)] {
            let plan = plan(info(r, mc, 1000), None).unwrap();
            assert_eq!(plan.total.get(), r);
            assert_eq!(plan.max_concurrency.get(), mc);
            assert!(!plan.misaligned, "recommended never warns");
        }
    }

    #[test]
    fn bad_gateway_info() {
        assert_eq!(
            plan(info(5, 0, 1000), None),
            Err(StartError::InvalidGatewayInfo)
        );
        assert_eq!(
            plan(info(5, 0, 0), Some(nz(3))),
            Err(StartError::InvalidGatewayInfo)
        );
        assert_eq!(
            plan(info(0, 1, 1000), None),
            Err(StartError::InvalidGatewayInfo)
        );
    }

    #[test]
    fn gw_03_zero_recommended_with_fixed_total() {
        assert_eq!(plan(info(0, 1, 1000), Some(nz(3))).unwrap().total.get(), 3);
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
            let got = plan(info(r, 1, 1000), Some(nz(f))).map(drop);
            assert_eq!(got, expected, "recommended {r}, fixed {f}");
        }
    }

    #[test]
    fn gw_03_no_overflow() {
        // ⌈(2^32 - 1) × 2 / 5⌉ = 8_589_934_590 / 5 = 1_717_986_918 exactly.
        let max = info(u32::MAX, 1, u32::MAX);
        assert_eq!(
            plan(max, Some(nz(1_717_986_917))),
            Err(StartError::ShardTotalTooSmall {
                total: 1_717_986_917,
                minimum: 1_717_986_918,
            })
        );
        assert_eq!(
            plan(max, Some(nz(1_717_986_918))).unwrap().total.get(),
            1_717_986_918
        );
    }

    #[test]
    fn gw_02_session_start_limit() {
        assert_eq!(
            plan(info(20, 1, 19), None),
            Err(StartError::SessionStartLimit {
                required: 20,
                remaining: 19,
                reset_after: Duration::from_hours(4),
            })
        );
        assert!(plan(info(20, 1, 20), None).is_ok());
        assert!(matches!(
            plan(info(1, 1, 0), None),
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
        let Err(StartError::SessionStartLimit { reset_after, .. }) = plan(huge, None) else {
            panic!("expected SessionStartLimit");
        };
        assert_eq!(reset_after, Duration::from_millis(u64::MAX));
    }

    #[test]
    fn gw_03_checked_before_gw_02() {
        assert_eq!(
            plan(info(10, 1, 0), Some(nz(1))),
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
            let plan = plan(info(1, mc, 1000), Some(nz(total))).unwrap();
            assert_eq!(plan.misaligned, expected, "{total}/{mc}");
        }
    }
}
