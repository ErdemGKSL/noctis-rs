//! The direct interaction-callback path: `POST
//! /interactions/{id}/{token}/callback` straight to the upstream pool,
//! outside twilight and its rate limiter, hedged once on a fresh connection
//! and bounded by the ack deadline (INT-01).

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::future::OptionFuture;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{Method, StatusCode};
use tokio::time::{Instant, sleep, sleep_until, timeout_at};
use tracing::Instrument;
use tracing::field::Empty;
use twilight_model::http::interaction::InteractionResponse;
use twilight_model::id::Id;
use twilight_model::id::marker::InteractionMarker;

use super::metrics::{HedgeOutcome, Phase, Reason, status_class};
use super::pool::Pool;
use super::{Outgoing, SendFailure, send};
use crate::AckDeadlineMissed;

/// Discord allows 3 s; the edge keeps 100 ms of margin.
pub(crate) const ACK_DEADLINE: Duration = Duration::from_millis(2900);
/// The shortest hedge delay.
const MIN_HEDGE: Duration = Duration::from_millis(300);
/// Time kept free for the hedge itself before the deadline.
const HEDGE_RESERVE: Duration = Duration::from_millis(1200);
/// The most of a 2xx body kept.
const MAX_SUCCESS_BODY: usize = 1024 * 1024;
/// The most of an error body read (for its JSON `code`).
const MAX_ERROR_BODY: usize = 64 * 1024;
/// Discord's "Interaction has already been acknowledged".
const ALREADY_ACKNOWLEDGED: u64 = 40060;
/// The User-Agent of callback requests.
const USER_AGENT: &str = concat!(
    "DiscordBot (https://github.com/ErdemGKSL/noctis-rs, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

/// A callback body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackBody {
    /// `application/json`.
    Json(Bytes),
    /// Any other content type (e.g. multipart with attachments).
    Raw {
        /// The `content-type`.
        content_type: HeaderValue,
        /// The body bytes.
        body: Bytes,
    },
}

impl CallbackBody {
    /// The JSON of `response`.
    ///
    /// # Errors
    ///
    /// When `serde_json` fails (it does not for twilight's models).
    pub fn json(response: &InteractionResponse) -> Result<Self, serde_json::Error> {
        Ok(Self::Json(Bytes::from(serde_json::to_vec(response)?)))
    }
}

/// One interaction callback.
#[derive(Clone)]
pub struct CallbackRequest {
    /// The interaction id.
    pub interaction_id: Id<InteractionMarker>,
    /// The interaction token (redacted in `Debug`).
    pub token: String,
    /// The body.
    pub body: CallbackBody,
    /// Adds `?with_response=true`.
    pub with_response: bool,
    /// When the gateway received the interaction.
    pub received_at: Instant,
}

impl fmt::Debug for CallbackRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackRequest")
            .field("interaction_id", &self.interaction_id)
            .field("token", &"<redacted>")
            .field("body", &self.body)
            .field("with_response", &self.with_response)
            .field("received_at", &self.received_at)
            .finish()
    }
}

/// A successful callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackAck {
    /// The HTTP status (2xx, or 400 when already acknowledged).
    pub status: u16,
    /// The 2xx body (capped at 1 MiB); empty when already acknowledged.
    pub body: Bytes,
    /// Discord answered 40060: another attempt already acknowledged it.
    pub already_acknowledged: bool,
}

/// Why a callback failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CallbackError {
    /// The token is not 1..=512 bytes of `[A-Za-z0-9._-]`; nothing was sent.
    #[error("the interaction token is not 1-512 characters of [A-Za-z0-9._-]")]
    InvalidToken,
    /// The ack deadline passed.
    #[error(transparent)]
    DeadlineMissed(#[from] AckDeadlineMissed),
    /// Discord answered a 4xx (or both attempts a 5xx).
    #[error("Discord rejected the interaction callback with HTTP {status}")]
    Rejected {
        /// The HTTP status.
        status: u16,
        /// Discord's JSON error code, when the body has one.
        code: Option<u32>,
    },
    /// No attempt got a response.
    #[error("the interaction callback could not reach Discord")]
    Transport,
}

/// The hedge delay for `remaining` time before the deadline: `max(300 ms,
/// min(hedge_after, remaining - 1200 ms))`, or `None` when that does not
/// end before the deadline.
pub(crate) fn hedge_delay(remaining: Duration, hedge_after: Duration) -> Option<Duration> {
    let delay = hedge_after
        .min(remaining.saturating_sub(HEDGE_RESERVE))
        .max(MIN_HEDGE);
    (delay < remaining).then_some(delay)
}

/// Whether `token` is 1..=512 bytes of `[A-Za-z0-9._-]`.
pub(crate) fn valid_token(token: &str) -> bool {
    (1..=512).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// What one attempt ended with.
enum Outcome {
    Success(CallbackAck),
    RateLimited(Option<Duration>),
    /// A final 4xx.
    Rejected {
        status: u16,
        code: Option<u32>,
    },
    /// A 5xx or a transport error: the other attempt may still go out.
    Retryable(CallbackError),
}

/// Which connection an attempt uses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conn {
    Pooled,
    Fresh,
}

/// Sends the callback (see the module docs).
pub(crate) async fn interaction_callback(
    pool: &Arc<Pool>,
    hedge_after: Duration,
    request: CallbackRequest,
) -> Result<CallbackAck, CallbackError> {
    if !valid_token(&request.token) {
        return Err(CallbackError::InvalidToken);
    }
    let received_at = request.received_at;
    let missed = move || {
        CallbackError::DeadlineMissed(AckDeadlineMissed {
            elapsed: Instant::now().saturating_duration_since(received_at),
        })
    };
    let now = Instant::now();
    let deadline = received_at + ACK_DEADLINE;
    if now >= deadline {
        return Err(missed());
    }
    let hedge_at = hedge_delay(deadline - now, hedge_after).map(|delay| now + delay);
    let outgoing = outgoing(&request);
    match timeout_at(deadline, race(pool, &outgoing, hedge_at, deadline)).await {
        Ok(Ok(ack)) => Ok(ack),
        Ok(Err(Some(error))) => Err(error),
        Ok(Err(None)) | Err(_) => Err(missed()),
    }
}

/// The upstream request of `request`.
fn outgoing(request: &CallbackRequest) -> Outgoing {
    let mut target = format!(
        "/api/v10/interactions/{}/{}/callback",
        request.interaction_id, request.token
    );
    if request.with_response {
        target.push_str("?with_response=true");
    }
    let (content_type, body) = match &request.body {
        CallbackBody::Json(body) => (HeaderValue::from_static("application/json"), body.clone()),
        CallbackBody::Raw { content_type, body } => (content_type.clone(), body.clone()),
    };
    let mut headers = HeaderMap::with_capacity(3);
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len()));
    headers.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    Outgoing {
        method: Method::POST,
        target,
        headers,
        body,
    }
}

type Attempt<'a> = Pin<Box<dyn Future<Output = Outcome> + Send + 'a>>;

/// Runs at most two attempts; `Err(None)` means the deadline rule decided
/// (`AckDeadlineMissed`).
async fn race(
    pool: &Arc<Pool>,
    outgoing: &Outgoing,
    hedge_at: Option<Instant>,
    deadline: Instant,
) -> Result<CallbackAck, Option<CallbackError>> {
    let metrics = pool.metrics();
    let mut slots: [Option<Attempt<'_>>; 2] =
        [Some(attempt(pool, outgoing, Conn::Pooled, 1, None)), None];
    let mut hedge_at = hedge_at;
    // The slot of the timer hedge, once it was sent.
    let mut hedge_slot: Option<usize> = None;
    let mut used = 1_u8;
    let mut last: Option<CallbackError> = None;
    // A 429 that did not fit decides the result, whatever ends after it.
    let mut unfit_429 = false;
    loop {
        if slots.iter().all(Option::is_none) {
            return Err(if unfit_429 { None } else { last });
        }
        let can_hedge = used < 2 && hedge_at.is_some();
        let [first, second] = &mut slots;
        let (index, outcome) = tokio::select! {
            biased;
            Some(outcome) = OptionFuture::from(first.as_mut()), if first.is_some() => (0, outcome),
            Some(outcome) = OptionFuture::from(second.as_mut()), if second.is_some() => (1, outcome),
            () = sleep_until(hedge_at.unwrap_or(deadline)), if can_hedge => {
                hedge_at = None;
                used = 2;
                let free = usize::from(slots[0].is_some());
                slots[free] = Some(attempt(pool, outgoing, Conn::Fresh, 2, None));
                hedge_slot = Some(free);
                continue;
            }
        };
        slots[index] = None;
        match outcome {
            Outcome::Success(ack) => {
                if ack.already_acknowledged {
                    metrics.hedge(HedgeOutcome::AlreadyAcknowledged);
                    tracing::debug!(target: "noctis::http", "interaction callback already acknowledged (40060)");
                } else if let Some(hedge) = hedge_slot {
                    if hedge == index {
                        metrics.hedge(HedgeOutcome::Won);
                        // Attempt 1 got no headers: its connection is suspect.
                        if slots[1 - index].is_some() {
                            pool.request_redial();
                        }
                    } else {
                        metrics.hedge(HedgeOutcome::Lost);
                    }
                }
                return Ok(ack);
            }
            Outcome::Rejected { status, code } => {
                return Err(Some(CallbackError::Rejected { status, code }));
            }
            Outcome::RateLimited(retry_after) => {
                let fits = retry_after.filter(|wait| {
                    Instant::now()
                        .checked_add(*wait)
                        .is_some_and(|at| at < deadline)
                });
                match fits {
                    Some(wait) if used < 2 => {
                        used = 2;
                        hedge_at = None;
                        slots[index] = Some(attempt(pool, outgoing, Conn::Pooled, 2, Some(wait)));
                    }
                    _ => unfit_429 = true,
                }
                if slots.iter().all(Option::is_none) {
                    return Err(None);
                }
            }
            Outcome::Retryable(error) => {
                last = Some(error);
                if used < 2 {
                    used = 2;
                    hedge_at = None;
                    slots[index] = Some(attempt(pool, outgoing, Conn::Fresh, 2, None));
                }
            }
        }
    }
}

/// One attempt, optionally after `wait` (a 429's `retry_after`).
fn attempt<'a>(
    pool: &'a Arc<Pool>,
    outgoing: &'a Outgoing,
    conn: Conn,
    number: u8,
    wait: Option<Duration>,
) -> Attempt<'a> {
    let span = tracing::info_span!(
        "noctis.http",
        app = pool.metrics().app(),
        kind = "callback",
        method = "POST",
        attempt = number,
        reused = Empty,
        status_class = Empty,
    );
    Box::pin(
        async move {
            if let Some(wait) = wait {
                sleep(wait).await;
            }
            let metrics = pool.metrics();
            let lease = match conn {
                Conn::Pooled => pool.checkout().await,
                Conn::Fresh => pool.fresh(Reason::Hedge).await,
            };
            let lease = match lease {
                Ok(lease) => lease,
                Err(error) => {
                    tracing::debug!(target: "noctis::http", %error, "callback dial failed");
                    return transport(metrics);
                }
            };
            tracing::Span::current().record("reused", lease.reused);
            let started = Instant::now();
            let (response, sender) = match send(pool, lease, outgoing).await {
                Ok(sent) => sent,
                Err(SendFailure::RetrySafe | SendFailure::Unsafe) => return transport(metrics),
            };
            metrics.phase(Phase::Ttfb, started.elapsed());
            let status = response.status();
            metrics.request(Some(status.as_u16()));
            tracing::Span::current().record("status_class", status_class(Some(status.as_u16())));
            let retry_header = retry_after_header(response.headers());
            let cap = if status.is_success() {
                MAX_SUCCESS_BODY
            } else {
                MAX_ERROR_BODY
            };
            let body = read_capped(response.into_body(), cap).await;
            metrics.phase(Phase::Total, started.elapsed());
            if conn == Conn::Pooled {
                pool.release(sender);
            }
            let Ok(body) = body else {
                return Outcome::Retryable(CallbackError::Transport);
            };
            classify(status, &body, retry_header)
        }
        .instrument(span),
    )
}

fn transport(metrics: &super::metrics::EdgeMetrics) -> Outcome {
    metrics.request(None);
    tracing::Span::current().record("status_class", "error");
    Outcome::Retryable(CallbackError::Transport)
}

/// Turns a status and body into an [`Outcome`].
fn classify(status: StatusCode, body: &Bytes, retry_header: Option<Duration>) -> Outcome {
    let json: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    let code = json
        .as_ref()
        .and_then(|j| j.get("code"))
        .and_then(serde_json::Value::as_u64);
    let status_u16 = status.as_u16();
    if status.is_success() {
        return Outcome::Success(CallbackAck {
            status: status_u16,
            body: body.clone(),
            already_acknowledged: false,
        });
    }
    if status == StatusCode::BAD_REQUEST && code == Some(ALREADY_ACKNOWLEDGED) {
        return Outcome::Success(CallbackAck {
            status: status_u16,
            body: Bytes::new(),
            already_acknowledged: true,
        });
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        let from_body = json
            .as_ref()
            .and_then(|j| j.get("retry_after"))
            .and_then(serde_json::Value::as_f64)
            .and_then(|secs| Duration::try_from_secs_f64(secs).ok());
        return Outcome::RateLimited(from_body.or(retry_header));
    }
    let code = code.and_then(|c| u32::try_from(c).ok());
    if status.is_server_error() {
        return Outcome::Retryable(CallbackError::Rejected {
            status: status_u16,
            code,
        });
    }
    Outcome::Rejected {
        status: status_u16,
        code,
    }
}

/// The `retry-after` header in seconds.
fn retry_after_header(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(header::RETRY_AFTER)?.to_str().ok()?;
    let secs: f64 = value.trim().parse().ok()?;
    Duration::try_from_secs_f64(secs).ok()
}

/// Reads at most about `cap` bytes of `body` (stops after the frame that
/// reaches it) and keeps the first `cap`.
async fn read_capped(mut body: Incoming, cap: usize) -> Result<Bytes, hyper::Error> {
    let mut out = BytesMut::new();
    while out.len() < cap {
        let Some(frame) = body.frame().await else {
            break;
        };
        if let Ok(data) = frame?.into_data() {
            let room = cap - out.len();
            out.extend_from_slice(&data[..data.len().min(room)]);
        }
    }
    Ok(out.freeze())
}

#[cfg(test)]
mod tests;
