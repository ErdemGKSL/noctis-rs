//! The typed Discord rate-limit error (FR-52, AD-24, RL-01..04).
//!
//! The Discord HTTP edge answers an upstream 429 that twilight's own rate
//! limiter cannot wait out (every 429 behind a shared rate-limit proxy, and
//! a `global`, `shared` or header-less one in direct mode) with an edge
//! rate-limit response. Noctis surfaces it as [`RateLimited`] and never
//! retries it in a loop.

use std::fmt;
use std::time::Duration;

use twilight_http::error::ErrorType;

/// The `X-RateLimit-Scope` of a Discord 429.
///
/// ```
/// use noctis_core::RateLimitScope;
///
/// assert_eq!(RateLimitScope::Shared.as_str(), "shared");
/// assert_eq!(RateLimitScope::Unknown.to_string(), "unknown");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitScope {
    /// The per-route limit of this bot (`user`).
    User,
    /// The bot-wide global limit (`global`, or a body with `"global": true`).
    Global,
    /// A per-resource limit shared by every bot (`shared`); Discord does not
    /// count it against the invalid-request limit.
    Shared,
    /// No or an unknown scope (a Cloudflare 429, say).
    Unknown,
}

impl RateLimitScope {
    /// The lowercase name (`user`, `global`, `shared`, `unknown`), also the
    /// `scope` label of `noctis_http_rate_limited_total`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Global => "global",
            Self::Shared => "shared",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for RateLimitScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Discord rate limited a request and Noctis did not retry it.
///
/// Every Noctis surface returns it typed (`ResponderError::RateLimited`,
/// `WebhookError::RateLimited`, `RawHttpError::RateLimited`); a raw
/// twilight call on `ctx.http()` gets it back with
/// [`from_http_error`](Self::from_http_error). Wait at least
/// [`retry_after`](Self::retry_after) before sending the request again,
/// and never retry it in a loop.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_core::{RateLimitScope, RateLimited};
///
/// let mut limited = RateLimited::new(RateLimitScope::Shared);
/// limited.retry_after = Some(Duration::from_millis(1500));
/// assert_eq!(
///     limited.to_string(),
///     "Discord rate limited the request (shared scope), retry after 1.500 s"
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct RateLimited {
    /// The 429's scope.
    pub scope: RateLimitScope,
    /// How long Discord asked to wait (the body's `retry_after`, else the
    /// `Retry-After` header), when it said.
    pub retry_after: Option<Duration>,
    /// Discord's JSON error `code` (such as 20028), when the body had one.
    pub discord_code: Option<u32>,
}

impl RateLimited {
    /// A rate limit of `scope` without a wait or a code.
    #[must_use]
    pub fn new(scope: RateLimitScope) -> Self {
        Self {
            scope,
            retry_after: None,
            discord_code: None,
        }
    }

    /// The rate limit behind a twilight error from the App's client (such
    /// as a `ctx.http()` call): `Some` only for the edge's rate-limit
    /// response, `None` for every other error.
    ///
    /// ```no_run
    /// use noctis_core::RateLimited;
    ///
    /// # async fn f(http: &twilight_http::Client) {
    /// # let channel = twilight_model::id::Id::new(1);
    /// if let Err(error) = http.channel(channel).await {
    ///     if let Some(limited) = RateLimited::from_http_error(&error) {
    ///         tracing::warn!(%limited, "rate limited; not retrying");
    ///     }
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn from_http_error(error: &twilight_http::Error) -> Option<Self> {
        match error.kind() {
            ErrorType::Response { body, status, .. } => {
                crate::edge::rate_limit::parse_edge_body(status.get(), body)
            }
            _ => None,
        }
    }
}

impl fmt::Display for RateLimited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Discord rate limited the request ({} scope)", self.scope)?;
        if let Some(wait) = self.retry_after {
            write!(f, ", retry after {:.3} s", wait.as_secs_f64())?;
        }
        Ok(())
    }
}
