//! The upstream connections: one shared HTTP/2 primary (or a LIFO stack of
//! idle HTTP/1.1 connections) and the keeper that warms and redials it.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use futures_util::FutureExt;
use http_body_util::Full;
use hyper::client::conn::{http1, http2};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::Instrument;

use super::connect::{DialError, Dialer, Sender};
use super::metrics::{EdgeMetrics, Reason};

/// The most idle HTTP/1.1 connections kept.
const MAX_IDLE: usize = 32;
/// How long a used HTTP/1.1 connection may take to become ready again
/// before it is dropped instead of pooled.
const RELEASE_WAIT: Duration = Duration::from_secs(30);
/// The back-off steps after consecutive failed dials, in milliseconds.
const BACKOFF_MS: [u64; 5] = [0, 500, 1000, 2000, 5000];

/// What happens to a freshly dialed connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Use {
    /// A request needs it now; an HTTP/2 one becomes the primary.
    Demand,
    /// A never-pooled connection (callback hedge).
    Fresh,
    /// The keeper's connection: it becomes the primary (HTTP/2) or the top
    /// of the idle stack (HTTP/1.1), and the keeper watches it.
    Keep,
}

/// A connection handed to one request.
pub(crate) struct Lease {
    pub(crate) sender: Sender,
    pub(crate) reused: bool,
}

#[derive(Default)]
struct PoolState {
    /// The shared HTTP/2 connection and its id.
    primary: Option<(u64, http2::SendRequest<Full<Bytes>>)>,
    /// The connection the keeper redials when it closes.
    watched: Option<u64>,
    /// Idle HTTP/1.1 connections, most recently used last.
    idle: Vec<http1::SendRequest<Full<Bytes>>>,
}

/// The upstream connection pool shared by the forwarder, the callback path
/// and the keeper.
pub(crate) struct Pool {
    pub(crate) dialer: Dialer,
    /// Fired when the edge is dropped: every task the pool spawned ends.
    pub(crate) cancel: CancellationToken,
    next_id: AtomicU64,
    state: Mutex<PoolState>,
    /// Wakes the keeper for a redial.
    redial: Notify,
    /// How many redials were requested from outside (shard network loss,
    /// a won hedge).
    redial_requests: AtomicUsize,
}

impl Pool {
    pub(crate) fn new(dialer: Dialer, cancel: CancellationToken) -> Self {
        Self {
            dialer,
            cancel,
            next_id: AtomicU64::new(0),
            state: Mutex::new(PoolState::default()),
            redial: Notify::new(),
            redial_requests: AtomicUsize::new(0),
        }
    }

    pub(crate) fn metrics(&self) -> &EdgeMetrics {
        &self.dialer.metrics
    }

    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A ready pooled connection, or a new one (reason `demand`).
    pub(crate) async fn checkout(self: &Arc<Self>) -> Result<Lease, DialError> {
        if let Some(sender) = self.ready() {
            self.metrics().reused(true);
            return Ok(Lease {
                sender,
                reused: true,
            });
        }
        let sender = self.open(Reason::Demand, Use::Demand).await?;
        self.fresh_lease(sender)
    }

    /// A new connection that is never pooled.
    pub(crate) async fn fresh(self: &Arc<Self>, reason: Reason) -> Result<Lease, DialError> {
        let sender = self.open(reason, Use::Fresh).await?;
        self.fresh_lease(sender)
    }

    /// A new connection for a retry: an HTTP/2 one becomes the primary.
    pub(crate) async fn retry(self: &Arc<Self>) -> Result<Lease, DialError> {
        let sender = self.open(Reason::Demand, Use::Demand).await?;
        self.fresh_lease(sender)
    }

    fn fresh_lease(&self, sender: Option<Sender>) -> Result<Lease, DialError> {
        self.metrics().reused(false);
        sender
            .map(|sender| Lease {
                sender,
                reused: false,
            })
            .ok_or(DialError {
                phase: super::connect::DialPhase::Handshake,
                kind: std::io::ErrorKind::Other,
            })
    }

    /// The primary, else the most recent idle HTTP/1.1 connection that is
    /// ready (closed ones are discarded). An open one that is not ready yet,
    /// such as the keeper's warm-up before its driver first ran, stays
    /// pooled: dropping its only sender would close it unused.
    fn ready(&self) -> Option<Sender> {
        let mut state = self.lock();
        if let Some((_, sender)) = &state.primary {
            if !sender.is_closed() {
                return Some(Sender::H2(sender.clone()));
            }
            state.primary = None;
        }
        let mut pending = Vec::new();
        let mut found = None;
        while let Some(sender) = state.idle.pop() {
            if sender.is_closed() {
                continue;
            }
            if sender.is_ready() {
                found = Some(Sender::H1(sender));
                break;
            }
            pending.push(sender);
        }
        state.idle.extend(pending.into_iter().rev());
        found
    }

    /// Dials one connection, counts it under `reason`, starts its task and
    /// places it as `use_` says. Returns the sender for `Demand`/`Fresh`.
    pub(crate) async fn open(
        self: &Arc<Self>,
        reason: Reason,
        use_: Use,
    ) -> Result<Option<Sender>, DialError> {
        let dialed = self.dialer.dial().await?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.metrics().connection(reason);
        let sender = dialed.sender;
        let mut state = self.lock();
        let placed = match (use_, sender) {
            (Use::Demand, Sender::H2(sender)) => {
                // The demand primary replaces the keeper's: watch it instead,
                // so its close (not the dropped old one's) triggers a redial.
                state.primary = Some((id, sender.clone()));
                state.watched = Some(id);
                Some(Sender::H2(sender))
            }
            (Use::Fresh | Use::Demand, sender) => Some(sender),
            (Use::Keep, Sender::H2(sender)) => {
                state.primary = Some((id, sender));
                state.watched = Some(id);
                None
            }
            (Use::Keep, Sender::H1(sender)) => {
                state.idle.push(sender);
                if state.idle.len() > MAX_IDLE {
                    state.idle.remove(0);
                }
                state.watched = Some(id);
                None
            }
        };
        drop(state);
        // Spawned only after the connection is placed and watched, so a
        // driver that ends at once still finds `watched == Some(id)`.
        self.spawn_driver(id, dialed.driver);
        Ok(placed)
    }

    /// Drives a connection until it closes or the edge is dropped.
    fn spawn_driver(self: &Arc<Self>, id: u64, driver: super::connect::Driver) {
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            let cancel = pool.cancel.clone();
            let result = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                result = driver.run() => result,
            };
            let ping_timeout = result.as_ref().err().is_some_and(is_ping_timeout);
            if let Err(error) = &result {
                tracing::debug!(target: "noctis::http", %error, "upstream connection ended");
            }
            pool.closed(id, ping_timeout);
        });
    }

    /// A connection task ended: forget it and wake the keeper when it was
    /// the watched one.
    fn closed(&self, id: u64, ping_timeout: bool) {
        if ping_timeout {
            self.metrics().ping_timeout();
        }
        let mut state = self.lock();
        if state.primary.as_ref().is_some_and(|(p, _)| *p == id) {
            state.primary = None;
        }
        if state.watched == Some(id) {
            state.watched = None;
            drop(state);
            self.redial.notify_one();
        }
    }

    /// Returns a used HTTP/1.1 connection to the idle stack once its
    /// response is finished; HTTP/2 senders are just dropped.
    pub(crate) fn release(self: &Arc<Self>, sender: Sender) {
        let Sender::H1(mut sender) = sender else {
            return;
        };
        // Already ready: pool it now, so a checkout right after (before a
        // spawned task would run, under load) reuses it instead of dialing.
        if sender.is_ready() {
            let mut state = self.lock();
            if state.idle.len() < MAX_IDLE {
                state.idle.push(sender);
            }
            return;
        }
        let pool = Arc::clone(self);
        tokio::spawn(async move {
            let cancel = pool.cancel.clone();
            let ready = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                ready = tokio::time::timeout(RELEASE_WAIT, sender.ready()) => ready,
            };
            if matches!(ready, Ok(Ok(()))) {
                let mut state = pool.lock();
                if state.idle.len() < MAX_IDLE {
                    state.idle.push(sender);
                }
            }
        });
    }

    /// How many idle HTTP/1.1 connections are pooled.
    #[cfg(test)]
    pub(crate) fn idle_len(&self) -> usize {
        self.lock().idle.len()
    }

    /// Asks the keeper to replace the watched connection.
    pub(crate) fn request_redial(&self) {
        self.redial_requests.fetch_add(1, Ordering::Relaxed);
        self.redial.notify_one();
    }
}

/// Whether a connection ended because a PING got no ACK in time.
fn is_ping_timeout(error: &hyper::Error) -> bool {
    if error.is_timeout() {
        return true;
    }
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if cause.to_string() == "keep-alive timed out" {
            return true;
        }
        source = cause.source();
    }
    false
}

/// The keeper's handle: asks for redials (shard network loss) and counts
/// the requests.
#[derive(Clone)]
pub(crate) struct KeeperHandle {
    pool: Arc<Pool>,
}

impl KeeperHandle {
    pub(crate) fn new(pool: Arc<Pool>) -> Self {
        Self { pool }
    }

    /// Asks the keeper to redial.
    pub(crate) fn request_redial(&self) {
        self.pool.request_redial();
    }

    /// How many redials were requested so far.
    #[cfg(test)]
    pub(crate) fn redial_requests(&self) -> usize {
        self.pool.redial_requests.load(Ordering::Relaxed)
    }
}

/// Stops the keeper when dropped.
pub(crate) struct KeeperGuard {
    _guard: DropGuard,
}

/// Starts the keeper: it warms one connection (reason `warmup`, no
/// request), then redials (reason `redial`) whenever the watched
/// connection closes or a redial is requested, with jittered back-off
/// between failed dials. The receiver gets the first warm-up's result.
pub(crate) fn start_keeper(
    pool: &Arc<Pool>,
) -> (KeeperGuard, oneshot::Receiver<Result<(), DialError>>) {
    let token = pool.cancel.child_token();
    let guard = KeeperGuard {
        _guard: token.clone().drop_guard(),
    };
    let (tx, rx) = oneshot::channel();
    tokio::spawn(keep(Arc::clone(pool), token, tx));
    (guard, rx)
}

async fn keep(
    pool: Arc<Pool>,
    cancel: CancellationToken,
    first: oneshot::Sender<Result<(), DialError>>,
) {
    let mut first = Some(first);
    let mut reason = Reason::Warmup;
    loop {
        let mut backoff = Backoff::default();
        loop {
            let span = tracing::info_span!(
                "noctis.http",
                app = pool.metrics().app(),
                kind = "warmup",
                method = tracing::field::Empty,
                attempt = tracing::field::Empty,
                reused = false,
                status_class = tracing::field::Empty,
            );
            let result = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                result = pool.open(reason, Use::Keep).instrument(span) => result,
            };
            let failed = result.err();
            if let Some(tx) = first.take() {
                let _ = tx.send(failed.map_or(Ok(()), Err));
            }
            let Some(error) = failed else {
                // A redial requested while this dial ran is already served by
                // the new connection: drop its stored permit, unless that
                // connection closed meanwhile.
                let _ = pool.redial.notified().now_or_never();
                if pool.lock().watched.is_none() {
                    reason = Reason::Redial;
                    continue;
                }
                break;
            };
            tracing::debug!(target: "noctis::http", %error, "upstream dial failed");
            let delay = backoff.next_delay(jitter());
            tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                () = tokio::time::sleep(delay) => {}
            }
            reason = Reason::Redial;
        }
        reason = Reason::Redial;
        tokio::select! {
            biased;
            () = cancel.cancelled() => return,
            () = pool.redial.notified() => {}
        }
    }
}

/// The redial back-off: 0, 0.5, 1, 2, 5, 5 … s after consecutive failed
/// dials, each times a jitter factor.
#[derive(Debug, Default)]
pub(crate) struct Backoff {
    failures: usize,
}

impl Backoff {
    /// The delay after one more failed dial; `jitter` is in [0.8, 1.2].
    pub(crate) fn next_delay(&mut self, jitter: f64) -> Duration {
        let step = BACKOFF_MS[self.failures.min(BACKOFF_MS.len() - 1)];
        self.failures = self.failures.saturating_add(1);
        Duration::from_millis(step).mul_f64(jitter)
    }
}

/// A uniform factor in [0.8, 1.2] (1.0 when no randomness is available).
pub(crate) fn jitter() -> f64 {
    getrandom::u32().map_or(1.0, |n| 0.8 + 0.4 * f64::from(n) / f64::from(u32::MAX))
}

#[cfg(test)]
mod tests;
