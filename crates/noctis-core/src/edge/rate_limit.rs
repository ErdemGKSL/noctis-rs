//! The edge's 429 rules (AD-24, RL-01, RL-04): how an upstream 429 is
//! classified, when twilight's own limiter may see it, and the edge
//! rate-limit response that surfaces it as [`RateLimited`]. Pure functions,
//! no I/O.
//!
//! twilight-http 0.17.1 re-sends a 429 at once when its limiter is off
//! (proxy mode) and cannot wait out a `global` or scope-less one either, so
//! only a direct-mode `user` 429 with valid bucket headers reaches it; every
//! other 429 becomes a 503 with the body of [`edge_body`], which twilight
//! returns as a plain error.

use std::fmt::Write as _;
use std::time::Duration;

use bytes::Bytes;
use hyper::header::{self, HeaderMap, HeaderValue};
use serde_json::Value;

use crate::{RateLimitScope, RateLimited};

/// The status of an edge rate-limit response.
pub(crate) const RATE_LIMIT_STATUS: u16 = 503;
/// The start of every edge rate-limit body (`message` is always first).
pub(crate) const RATE_LIMIT_BODY_PREFIX: &str = r#"{"message":"noctis edge: rate limited""#;
/// The largest `x-ratelimit-reset-after` twilight is handed (one day).
const MAX_RESET_AFTER: f32 = 86_400.0;

const SCOPE: &str = "x-ratelimit-scope";
const BUCKET: &str = "x-ratelimit-bucket";
const LIMIT: &str = "x-ratelimit-limit";
const REMAINING: &str = "x-ratelimit-remaining";
pub(crate) const RESET_AFTER: &str = "x-ratelimit-reset-after";

/// The `x-ratelimit-scope` of `headers`: exactly `user`, `global` or
/// `shared`, anything else [`RateLimitScope::Unknown`].
pub(crate) fn scope(headers: &HeaderMap) -> RateLimitScope {
    match headers.get(SCOPE).map(HeaderValue::as_bytes) {
        Some(b"user") => RateLimitScope::User,
        Some(b"global") => RateLimitScope::Global,
        Some(b"shared") => RateLimitScope::Shared,
        _ => RateLimitScope::Unknown,
    }
}

/// A non-negative, finite number of seconds as a [`Duration`].
fn seconds(secs: f64) -> Option<Duration> {
    (secs.is_finite() && secs >= 0.0)
        .then(|| Duration::try_from_secs_f64(secs).ok())
        .flatten()
}

/// What an upstream 429 says: its scope (a scope-less one with a body
/// `"global": true` is global), the wait (the body's `retry_after`, else
/// the `retry-after` header) and Discord's `code`.
pub(crate) fn classify_429(headers: &HeaderMap, body: &[u8]) -> RateLimited {
    let json: Option<Value> = serde_json::from_slice(body).ok();
    let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
    let mut scope = scope(headers);
    if scope == RateLimitScope::Unknown && field("global").and_then(Value::as_bool) == Some(true) {
        scope = RateLimitScope::Global;
    }
    let from_body = field("retry_after")
        .and_then(Value::as_f64)
        .and_then(seconds);
    let from_header = || {
        headers
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<f64>().ok())
            .and_then(seconds)
    };
    let discord_code = field("code")
        .and_then(Value::as_u64)
        .and_then(|code| u32::try_from(code).ok());
    RateLimited {
        scope,
        retry_after: from_body.or_else(from_header),
        discord_code,
    }
}

/// Whether `headers` carry a valid `x-ratelimit-reset-after` for twilight
/// (finite, `0..=86400` seconds: `Duration::from_secs_f32` panics on
/// anything else).
pub(crate) fn valid_reset_after(headers: &HeaderMap) -> bool {
    headers
        .get(RESET_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<f32>().ok())
        .is_some_and(|secs| secs.is_finite() && (0.0..=MAX_RESET_AFTER).contains(&secs))
}

/// Whether a 429 goes to twilight unchanged (RL-01): only in direct mode,
/// for scope `user` with a bucket, `u16` limit and remaining, and a valid
/// reset-after. Every other 429 is surfaced (RL-04).
pub(crate) fn relay_429(headers: &HeaderMap, proxied: bool) -> bool {
    let parses_u16 = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.parse::<u16>().is_ok())
    };
    !proxied
        && headers.get(SCOPE).map(HeaderValue::as_bytes) == Some(b"user")
        && headers.contains_key(BUCKET)
        && parses_u16(LIMIT)
        && parses_u16(REMAINING)
        && valid_reset_after(headers)
}

/// The edge rate-limit body of `limited`: fixed field order, `message`
/// first, never a `code` field (so twilight parses it as
/// `ApiError::Ratelimited` when `retry_after` is present).
pub(crate) fn edge_body(limited: &RateLimited) -> Bytes {
    let mut body = format!(
        r#"{RATE_LIMIT_BODY_PREFIX},"global":{},"scope":"{}""#,
        limited.scope == RateLimitScope::Global,
        limited.scope.as_str()
    );
    if let Some(wait) = limited.retry_after {
        let secs = serde_json::to_string(&wait.as_secs_f64()).unwrap_or_else(|_| "0".to_owned());
        body.push_str(r#","retry_after":"#);
        body.push_str(&secs);
    }
    if let Some(code) = limited.discord_code {
        let _ = write!(body, r#","discord_code":{code}"#);
    }
    body.push('}');
    Bytes::from(body)
}

/// The [`RateLimited`] of an edge rate-limit response: `Some` only for
/// status 503 and a body of [`edge_body`].
pub(crate) fn parse_edge_body(status: u16, body: &[u8]) -> Option<RateLimited> {
    if status != RATE_LIMIT_STATUS || !body.starts_with(RATE_LIMIT_BODY_PREFIX.as_bytes()) {
        return None;
    }
    let json: Value = serde_json::from_slice(body).ok()?;
    let scope = match json.get("scope")?.as_str()? {
        "user" => RateLimitScope::User,
        "global" => RateLimitScope::Global,
        "shared" => RateLimitScope::Shared,
        "unknown" => RateLimitScope::Unknown,
        _ => return None,
    };
    let retry_after = match json.get("retry_after") {
        None => None,
        Some(value) => Some(seconds(value.as_f64()?)?),
    };
    let discord_code = match json.get("discord_code") {
        None => None,
        Some(value) => Some(u32::try_from(value.as_u64()?).ok()?),
    };
    Some(RateLimited {
        scope,
        retry_after,
        discord_code,
    })
}

#[cfg(test)]
mod tests;
