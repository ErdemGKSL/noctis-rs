//! `HttpConfig::violations`: order and the TTL-order rule.

use std::time::Duration;

use super::*;

#[test]
fn default_is_valid() {
    assert!(HttpConfig::default().violations().is_empty());
}

#[test]
#[expect(
    clippy::field_reassign_with_default,
    reason = "bot code must mutate a default: HttpConfig is non_exhaustive"
)]
fn order_rule_only_with_both_in_range() {
    let mut config = HttpConfig::default();
    config.dns_ttl_floor = Duration::from_mins(1);
    config.dns_ttl_cap = Duration::from_secs(30);
    assert_eq!(
        config.violations(),
        [HttpConfigError::DnsTtlOrder {
            floor: Duration::from_mins(1),
            cap: Duration::from_secs(30)
        }]
    );
    config.dns_ttl_floor = Duration::ZERO;
    config.dns_ttl_cap = Duration::ZERO;
    assert_eq!(
        config.violations(),
        [
            HttpConfigError::DnsTtlFloor(Duration::ZERO),
            HttpConfigError::DnsTtlCap(Duration::ZERO)
        ]
    );
}
