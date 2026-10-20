//! [`HttpConfig`]: the tuning of the Discord HTTP edge (AD-24).

use std::ops::RangeInclusive;
use std::time::Duration;

/// The allowed `ping_interval` range.
const PING_INTERVAL: RangeInclusive<Duration> = Duration::from_millis(50)..=Duration::from_mins(5);
/// The allowed `ping_timeout` range.
const PING_TIMEOUT: RangeInclusive<Duration> = Duration::from_millis(50)..=Duration::from_mins(1);
/// The allowed `hedge_after` range.
const HEDGE_AFTER: RangeInclusive<Duration> =
    Duration::from_millis(300)..=Duration::from_millis(2800);
/// The allowed `connect_timeout` range.
const CONNECT_TIMEOUT: RangeInclusive<Duration> =
    Duration::from_millis(100)..=Duration::from_secs(30);
/// The allowed `dns_ttl_floor` and `dns_ttl_cap` range.
const DNS_TTL: RangeInclusive<Duration> = Duration::from_secs(1)..=Duration::from_hours(24);

/// How the App's Discord HTTP edge keeps its upstream connection warm, how
/// fast it gives up on a dead one and when an interaction callback is
/// hedged. The defaults are tuned for Discord; most bots never change them.
///
/// Every request of the App's HTTP client passes a loopback edge inside the
/// App, which keeps one HTTP/2 connection to Discord warm with PING frames
/// (never with API requests) and redials it when it dies.
///
/// `HttpConfig` is `#[non_exhaustive]`: start from [`HttpConfig::default`]
/// and change fields.
///
/// | Field | Default | Range |
/// | --- | --- | --- |
/// | `ping_interval` | 20 s | 50 ms..=300 s |
/// | `ping_timeout` | 5 s | 50 ms..=60 s |
/// | `hedge_after` | 1000 ms | 300 ms..=2800 ms |
/// | `connect_timeout` | 1500 ms | 100 ms..=30 s |
/// | `dns_ttl_floor` | 30 s | 1 s..=86 400 s |
/// | `dns_ttl_cap` | 300 s | 1 s..=86 400 s |
/// | `upstream_h2c` | `false` | — |
///
/// ```
/// use std::time::Duration;
///
/// use noctis_core::{App, HttpConfig};
///
/// let mut http = HttpConfig::default();
/// http.ping_interval = Duration::from_secs(15);
/// let mut builder = App::builder();
/// builder.http(http);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpConfig {
    /// How often an idle HTTP/2 connection sends a PING frame.
    pub ping_interval: Duration,
    /// How long a PING may wait for its ACK before the connection counts as
    /// dead and is redialed.
    pub ping_timeout: Duration,
    /// How long an interaction callback waits for response headers before
    /// a second, hedged attempt goes out on a fresh connection (shortened
    /// so the hedge still fits the 3 s ack deadline).
    pub hedge_after: Duration,
    /// The TCP connect timeout per upstream address.
    pub connect_timeout: Duration,
    /// The shortest time a DNS answer is cached.
    pub dns_ttl_floor: Duration,
    /// The longest time a DNS answer is cached.
    pub dns_ttl_cap: Duration,
    /// Speak HTTP/2 without TLS (prior knowledge) to a plain `http://`
    /// upstream set with `AppBuilder::http_proxy`; HTTP/1.1 otherwise. TLS
    /// upstreams negotiate the protocol with ALPN and ignore it.
    pub upstream_h2c: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_secs(20),
            ping_timeout: Duration::from_secs(5),
            hedge_after: Duration::from_secs(1),
            connect_timeout: Duration::from_millis(1500),
            dns_ttl_floor: Duration::from_secs(30),
            dns_ttl_cap: Duration::from_mins(5),
            upstream_h2c: false,
        }
    }
}

/// One range check: the value, its range and its error.
type Check = (
    Duration,
    &'static RangeInclusive<Duration>,
    fn(Duration) -> HttpConfigError,
);

impl HttpConfig {
    /// Every out-of-range field, in field order, then the TTL order (only
    /// when both TTL bounds are in range).
    pub(crate) fn violations(&self) -> Vec<HttpConfigError> {
        let checks: [Check; 6] = [
            (
                self.ping_interval,
                &PING_INTERVAL,
                HttpConfigError::PingInterval,
            ),
            (
                self.ping_timeout,
                &PING_TIMEOUT,
                HttpConfigError::PingTimeout,
            ),
            (self.hedge_after, &HEDGE_AFTER, HttpConfigError::HedgeAfter),
            (
                self.connect_timeout,
                &CONNECT_TIMEOUT,
                HttpConfigError::ConnectTimeout,
            ),
            (self.dns_ttl_floor, &DNS_TTL, HttpConfigError::DnsTtlFloor),
            (self.dns_ttl_cap, &DNS_TTL, HttpConfigError::DnsTtlCap),
        ];
        let mut out: Vec<_> = checks
            .into_iter()
            .filter(|(actual, range, _)| !range.contains(actual))
            .map(|(actual, _, error)| error(actual))
            .collect();
        if DNS_TTL.contains(&self.dns_ttl_floor)
            && DNS_TTL.contains(&self.dns_ttl_cap)
            && self.dns_ttl_floor > self.dns_ttl_cap
        {
            out.push(HttpConfigError::DnsTtlOrder {
                floor: self.dns_ttl_floor,
                cap: self.dns_ttl_cap,
            });
        }
        out
    }
}

/// Why an [`HttpConfig`] was rejected by `AppBuilder::build`.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_core::HttpConfigError;
///
/// let error = HttpConfigError::HedgeAfter(Duration::from_millis(100));
/// assert_eq!(
///     error.to_string(),
///     "HttpConfig.hedge_after 100ms is out of range: use 300 ms..=2800 ms"
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HttpConfigError {
    /// `ping_interval` is outside 50 ms..=300 s.
    #[error("HttpConfig.ping_interval {0:?} is out of range: use 50 ms..=300 s")]
    PingInterval(Duration),
    /// `ping_timeout` is outside 50 ms..=60 s.
    #[error("HttpConfig.ping_timeout {0:?} is out of range: use 50 ms..=60 s")]
    PingTimeout(Duration),
    /// `hedge_after` is outside 300 ms..=2800 ms.
    #[error("HttpConfig.hedge_after {0:?} is out of range: use 300 ms..=2800 ms")]
    HedgeAfter(Duration),
    /// `connect_timeout` is outside 100 ms..=30 s.
    #[error("HttpConfig.connect_timeout {0:?} is out of range: use 100 ms..=30 s")]
    ConnectTimeout(Duration),
    /// `dns_ttl_floor` is outside 1 s..=86 400 s.
    #[error("HttpConfig.dns_ttl_floor {0:?} is out of range: use 1 s..=86400 s")]
    DnsTtlFloor(Duration),
    /// `dns_ttl_cap` is outside 1 s..=86 400 s.
    #[error("HttpConfig.dns_ttl_cap {0:?} is out of range: use 1 s..=86400 s")]
    DnsTtlCap(Duration),
    /// `dns_ttl_floor` is above `dns_ttl_cap`.
    #[error("HttpConfig.dns_ttl_floor {floor:?} is above dns_ttl_cap {cap:?}")]
    DnsTtlOrder {
        /// The rejected floor.
        floor: Duration,
        /// The rejected cap.
        cap: Duration,
    },
}

#[cfg(test)]
mod tests;
