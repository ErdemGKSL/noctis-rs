//! The Discord HTTP edge (AD-24): a loopback HTTP/1.1 forwarder in front of
//! one tuned upstream (HTTP/2 with PING keep-alive, TCP tuning, timed dial
//! phases, cached async DNS), a keeper that warms and redials the upstream
//! connection, and the direct interaction-callback path.
//!
//! The App's one twilight client is built with
//! `proxy("127.0.0.1:<port>", true)`, so every request it sends passes
//! through here; twilight's rate limiter is unchanged and keeps learning
//! buckets from the forwarded response headers. The edge never adds
//! `authorization` and never logs a path, query, header value or body.

pub(crate) mod callback;
pub(crate) mod config;
mod connect;
mod dns;
mod metrics;
mod pool;
pub(crate) mod rate_limit;
mod sniff;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod testing;

use std::convert::Infallible;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::{Body, Frame, Incoming, SizeHint};
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::server::conn::http1 as server_http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;
use tokio_util::sync::{CancellationToken, DropGuard};
use tracing::Instrument;
use tracing::field::Empty;

pub(crate) use self::config::HttpConfig;
pub(crate) use self::connect::{DialError, Upstream};
use self::connect::{Dialer, Sender};
use self::dns::{DnsCache, Resolve, SystemResolver};
use self::metrics::{EdgeMetrics, Phase, status_class};
pub(crate) use self::pool::{KeeperGuard, KeeperHandle};
use self::pool::{Lease, Pool, start_keeper};

/// The edge's own error body (502); `App::gateway_info` recognizes its
/// prefix.
pub(crate) const EDGE_ERROR_BODY: &str =
    r#"{"message":"noctis edge: upstream unavailable","code":0}"#;
/// The edge's own error body when a forwarded request exceeds the request
/// timeout (504).
pub(crate) const EDGE_TIMEOUT_BODY: &str =
    r#"{"message":"noctis edge: upstream timed out","code":0}"#;
/// The prefix of every body the edge itself answers with.
pub(crate) const EDGE_BODY_PREFIX: &str = r#"{"message":"noctis edge:"#;
/// The most of an upstream 429 body the edge reads.
const MAX_429_BODY: usize = 64 * 1024;

/// Whether a response is the edge's own failure (502 upstream unavailable,
/// 504 upstream timed out) rather than an answer from upstream.
pub(crate) fn is_edge_failure(status: u16, body: &[u8]) -> bool {
    matches!(status, 502 | 504) && body.starts_with(EDGE_BODY_PREFIX.as_bytes())
}
/// The 403 body for a target the edge does not forward.
const FORBIDDEN_BODY: &str = r#"{"message":"noctis edge: forbidden path","code":0}"#;
/// The 413 body (Discord's own shape and code).
const TOO_LARGE_BODY: &str = r#"{"message":"Request entity too large","code":40005}"#;
/// The largest request body the edge buffers.
const MAX_REQUEST_BODY: usize = 512 * 1024 * 1024;
/// The only path prefix the edge forwards.
const API_PREFIX: &str = "/api/v10/";

/// The edge's response body: streamed upstream bytes or its own JSON.
type EdgeBody = BoxBody<Bytes, hyper::Error>;

/// How an [`Edge`] treats requests.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeOptions {
    pub(crate) config: HttpConfig,
    /// The bound on every forwarded request and callback attempt.
    pub(crate) request_timeout: Duration,
    /// Whether the upstream is a shared rate-limit proxy.
    pub(crate) proxied: bool,
}

/// The App's edge: owned by `AppShared`; dropping it stops the accept
/// task, every connection task and the keeper.
pub(crate) struct Edge {
    pool: Arc<Pool>,
    addr: SocketAddr,
    _stop: DropGuard,
}

impl Edge {
    /// Binds the loopback listener (`build()` stays sync).
    pub(crate) fn bind() -> io::Result<(std::net::TcpListener, SocketAddr)> {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        Ok((listener, addr))
    }

    /// Starts serving `listener` on the current runtime, forwarding to
    /// `upstream`; every forwarded request and callback attempt is bounded
    /// by `request_timeout`.
    pub(crate) fn start(
        (listener, addr): (std::net::TcpListener, SocketAddr),
        upstream: Upstream,
        config: HttpConfig,
        request_timeout: Duration,
        app: &Arc<str>,
    ) -> Self {
        let proxied = matches!(upstream, Upstream::Http { .. });
        Self::start_with(
            listener,
            addr,
            upstream,
            EdgeOptions {
                config,
                request_timeout,
                proxied,
            },
            Arc::clone(app),
            Arc::new(SystemResolver::default()),
        )
    }

    /// [`start`](Self::start) with a custom resolver and an explicit
    /// `proxied` (tests: a TLS upstream cannot be faked).
    pub(crate) fn start_with(
        listener: std::net::TcpListener,
        addr: SocketAddr,
        upstream: Upstream,
        options: EdgeOptions,
        app: Arc<str>,
        resolver: Arc<dyn Resolve>,
    ) -> Self {
        let EdgeOptions {
            config,
            request_timeout,
            proxied,
        } = options;
        let cancel = CancellationToken::new();
        let dialer = Dialer {
            upstream,
            dns: DnsCache::new(resolver, config.dns_ttl_floor, config.dns_ttl_cap),
            config,
            metrics: EdgeMetrics::new(app),
        };
        let mut pool = Pool::new(dialer, cancel.clone());
        pool.request_timeout = request_timeout;
        pool.proxied = proxied;
        let pool = Arc::new(pool);
        tokio::spawn(accept(listener, Arc::clone(&pool)));
        Self {
            pool,
            addr,
            _stop: cancel.drop_guard(),
        }
    }

    /// The loopback address twilight sends to.
    pub(crate) fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Starts the keeper; see [`pool::start_keeper`].
    pub(crate) fn start_keeper(
        &self,
    ) -> (
        KeeperGuard,
        tokio::sync::oneshot::Receiver<Result<(), DialError>>,
    ) {
        start_keeper(&self.pool)
    }

    /// The keeper's redial handle.
    pub(crate) fn keeper(&self) -> KeeperHandle {
        KeeperHandle::new(Arc::clone(&self.pool))
    }

    /// Sends an interaction callback straight upstream (see
    /// [`callback`]).
    pub(crate) async fn interaction_callback(
        &self,
        request: callback::CallbackRequest,
    ) -> Result<callback::CallbackAck, callback::CallbackError> {
        let hedge_after = self.pool.dialer.config.hedge_after;
        callback::interaction_callback(&self.pool, hedge_after, request).await
    }
}

impl std::fmt::Debug for Edge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Edge")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

/// Converts the listener (inside `catch_unwind`: tokio panics without an
/// IO driver) and accepts connections until the edge is dropped.
async fn accept(listener: std::net::TcpListener, pool: Arc<Pool>) {
    let converted = std::panic::catch_unwind(AssertUnwindSafe(|| TcpListener::from_std(listener)));
    let Ok(Ok(listener)) = converted else {
        tracing::error!(
            target: "noctis::http",
            "the Discord HTTP edge could not start: build the tokio runtime with enable_all()"
        );
        return;
    };
    let cancel = pool.cancel.clone();
    loop {
        let accepted = tokio::select! {
            biased;
            () = cancel.cancelled() => return,
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((stream, _)) => {
                tokio::spawn(serve(stream, Arc::clone(&pool)));
            }
            Err(error) => {
                tracing::debug!(target: "noctis::http", %error, "edge accept failed");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

/// Serves one loopback connection (HTTP/1.1 with keep-alive).
async fn serve(stream: TcpStream, pool: Arc<Pool>) {
    let cancel = pool.cancel.clone();
    let service = service_fn(move |request| {
        let pool = Arc::clone(&pool);
        async move { Ok::<_, Infallible>(forward(&pool, request).await) }
    });
    let connection = server_http1::Builder::new().serve_connection(TokioIo::new(stream), service);
    tokio::select! {
        biased;
        () = cancel.cancelled() => {}
        _ = connection => {}
    }
}

/// An edge-generated JSON response.
fn edge_response(status: StatusCode, body: &'static str) -> Response<EdgeBody> {
    let mut headers = HeaderMap::with_capacity(1);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    full_response(status, headers, Bytes::from_static(body.as_bytes()))
}

/// A buffered response; `content-length` comes from `body`.
fn full_response(status: StatusCode, mut headers: HeaderMap, body: Bytes) -> Response<EdgeBody> {
    headers.remove(header::CONTENT_LENGTH);
    let mut response = Response::new(Full::new(body).map_err(|never| match never {}).boxed());
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

/// Whether the edge forwards `method` on `uri`: an origin-form target under
/// `/api/v10/` without `.`/`..` segments, and not `CONNECT`.
pub(crate) fn forwardable(method: &Method, uri: &Uri) -> bool {
    *method != Method::CONNECT
        && uri.scheme().is_none()
        && uri.authority().is_none()
        && uri.path().starts_with(API_PREFIX)
        && !uri.path().split('/').any(is_dot_segment)
}

/// `.`, `..` or their percent-encoded forms (`%2e`, `%2E`).
fn is_dot_segment(segment: &str) -> bool {
    let decoded = segment.to_ascii_lowercase().replace("%2e", ".");
    decoded == "." || decoded == ".."
}

/// The fixed hop-by-hop headers.
const HOP_BY_HOP: [&str; 7] = [
    "connection",
    "keep-alive",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// `headers` without the hop-by-hop ones (and the names `connection`
/// lists), in order, duplicates kept.
pub(crate) fn end_to_end(headers: &HeaderMap) -> HeaderMap {
    let listed: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    let mut out = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name_str = name.as_str();
        if HOP_BY_HOP.contains(&name_str) || listed.iter().any(|l| l == name_str) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

/// One request as the upstream gets it (rebuilt for each attempt).
#[derive(Clone)]
pub(crate) struct Outgoing {
    pub(crate) method: Method,
    /// The raw path and query.
    pub(crate) target: String,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
}

impl Outgoing {
    /// The request for `sender`: an absolute URI over HTTP/2 (`:authority`,
    /// no `host`), an origin-form one with `host` over HTTP/1.1.
    fn to_request(&self, pool: &Pool, sender: &Sender) -> Option<Request<Full<Bytes>>> {
        let upstream = &pool.dialer.upstream;
        let authority = upstream.authority();
        let mut headers = self.headers.clone();
        let uri = if sender.is_h2() {
            headers.remove(header::HOST);
            format!("{}://{authority}{}", upstream.scheme(), self.target)
        } else {
            let host = HeaderValue::from_str(&authority).ok()?;
            headers.insert(header::HOST, host);
            self.target.clone()
        };
        let mut request = Request::new(Full::new(self.body.clone()));
        *request.method_mut() = self.method.clone();
        *request.uri_mut() = uri.parse().ok()?;
        *request.headers_mut() = headers;
        Some(request)
    }
}

/// Why an attempt failed.
pub(crate) enum SendFailure {
    /// Nothing reached the upstream (dial error, unsent request, refused
    /// stream, stream above GOAWAY): one retry is safe.
    RetrySafe,
    /// The request may have been processed.
    Unsafe,
}

/// Sends `outgoing` on `lease`; returns the response headers.
pub(crate) async fn send(
    pool: &Arc<Pool>,
    lease: Lease,
    outgoing: &Outgoing,
) -> Result<(Response<Incoming>, Sender), SendFailure> {
    let Some(request) = outgoing.to_request(pool, &lease.sender) else {
        return Err(SendFailure::Unsafe);
    };
    let mut sender = lease.sender;
    let sent = match &mut sender {
        Sender::H1(s) => s.try_send_request(request).await,
        Sender::H2(s) => s.try_send_request(request).await,
    };
    match sent {
        Ok(response) => Ok((response, sender)),
        Err(error) => {
            let unsent = error.message().is_some();
            let error = error.into_error();
            tracing::debug!(target: "noctis::http", %error, "upstream request failed");
            if unsent || refused(&error) {
                Err(SendFailure::RetrySafe)
            } else {
                Err(SendFailure::Unsafe)
            }
        }
    }
}

/// An `h2` stream error that proves the request was not processed.
fn refused(error: &hyper::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if let Some(h2) = cause.downcast_ref::<h2::Error>() {
            return h2.reason() == Some(h2::Reason::REFUSED_STREAM)
                || (h2.is_go_away() && h2.is_remote());
        }
        source = cause.source();
    }
    false
}

/// Forwards one loopback request (see the module docs).
async fn forward(pool: &Arc<Pool>, request: Request<Incoming>) -> Response<EdgeBody> {
    if !forwardable(request.method(), request.uri()) {
        return edge_response(StatusCode::FORBIDDEN, FORBIDDEN_BODY);
    }
    let (parts, body) = request.into_parts();
    let declared = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    if declared.is_some_and(|len| len > MAX_REQUEST_BODY as u64) {
        return edge_response(StatusCode::PAYLOAD_TOO_LARGE, TOO_LARGE_BODY);
    }
    let body = match Limited::new(body, MAX_REQUEST_BODY).collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(error) if error.downcast_ref::<LengthLimitError>().is_some() => {
            return edge_response(StatusCode::PAYLOAD_TOO_LARGE, TOO_LARGE_BODY);
        }
        Err(_) => return edge_response(StatusCode::BAD_GATEWAY, EDGE_ERROR_BODY),
    };
    let outgoing = Outgoing {
        method: parts.method,
        target: parts
            .uri
            .path_and_query()
            .map_or_else(|| "/".to_owned(), |pq| pq.as_str().to_owned()),
        headers: end_to_end(&parts.headers),
        body,
    };
    let metrics = pool.metrics();
    let span = tracing::info_span!(
        "noctis.http",
        app = metrics.app(),
        kind = "forward",
        method = outgoing.method.as_str(),
        attempt = Empty,
        reused = Empty,
        status_class = Empty,
    );
    let bounded = tokio::time::timeout(pool.request_timeout, upstream(pool, &outgoing, &span))
        .instrument(span.clone())
        .await;
    bounded.unwrap_or_else(|_| {
        tracing::debug!(target: "noctis::http", "upstream request timed out");
        span.record("status_class", "error");
        metrics.request(None);
        edge_response(StatusCode::GATEWAY_TIMEOUT, EDGE_TIMEOUT_BODY)
    })
}

/// Sends a forwarded request with the retry rule; 502 when it fails.
async fn upstream(
    pool: &Arc<Pool>,
    outgoing: &Outgoing,
    span: &tracing::Span,
) -> Response<EdgeBody> {
    let metrics = pool.metrics();
    for attempt in 1..=2_u8 {
        span.record("attempt", attempt);
        let lease = if attempt == 1 {
            pool.checkout().await
        } else {
            pool.retry().await
        };
        let lease = match lease {
            Ok(lease) => lease,
            Err(error) => {
                tracing::debug!(target: "noctis::http", %error, "upstream dial failed");
                continue;
            }
        };
        span.record("reused", lease.reused);
        let started = Instant::now();
        match send(pool, lease, outgoing).await {
            Ok((response, sender)) => {
                metrics.phase(Phase::Ttfb, started.elapsed());
                let status = response.status().as_u16();
                span.record("status_class", status_class(Some(status)));
                metrics.request(Some(status));
                pool.release(sender);
                if status == StatusCode::TOO_MANY_REQUESTS.as_u16() {
                    return rate_limited(pool, response, metrics, started).await;
                }
                metrics.upstream_status(status, None);
                return relay(response, metrics.clone(), started);
            }
            Err(SendFailure::RetrySafe) => {}
            Err(SendFailure::Unsafe) => break,
        }
    }
    span.record("status_class", "error");
    metrics.request(None);
    edge_response(StatusCode::BAD_GATEWAY, EDGE_ERROR_BODY)
}

/// An upstream 429 (AD-24): relayed unchanged when twilight's limiter can
/// wait it out ([`rate_limit::relay_429`], RL-01), else answered as an edge
/// rate-limit response (503, [`rate_limit::edge_body`], Discord's headers
/// kept so the limiter still learns the bucket, RL-04).
async fn rate_limited(
    pool: &Pool,
    response: Response<Incoming>,
    metrics: &EdgeMetrics,
    started: Instant,
) -> Response<EdgeBody> {
    let (parts, body) = response.into_parts();
    let mut headers = end_to_end(&parts.headers);
    let body = callback::read_capped(body, MAX_429_BODY)
        .await
        .unwrap_or_default();
    metrics.phase(Phase::Total, started.elapsed());
    let limited = rate_limit::classify_429(&headers, &decoded_429(&headers, &body));
    metrics.upstream_status(StatusCode::TOO_MANY_REQUESTS.as_u16(), Some(limited.scope));
    let scope = limited.scope.as_str();
    if rate_limit::relay_429(&headers, pool.proxied) {
        tracing::debug!(target: "noctis::http", scope, "upstream 429 relayed to the rate limiter");
        return full_response(parts.status, headers, body);
    }
    tracing::debug!(target: "noctis::http", scope, "upstream 429 surfaced as an edge rate-limit response");
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    // twilight's `Duration::from_secs_f32` panics on a negative, NaN or
    // huge `x-ratelimit-reset-after`; such a header never reaches it.
    if !rate_limit::valid_reset_after(&headers) {
        headers.remove(rate_limit::RESET_AFTER);
    }
    // A body-level encoding would not match the edge's own body.
    headers.remove(header::CONTENT_ENCODING);
    full_response(
        StatusCode::SERVICE_UNAVAILABLE,
        headers,
        rate_limit::edge_body(&limited),
    )
}

/// A 429 body as JSON bytes: Brotli-decoded (capped at [`MAX_429_BODY`])
/// when upstream sent `content-encoding: br`, else as read.
fn decoded_429(headers: &HeaderMap, body: &Bytes) -> Bytes {
    use std::io::Read as _;
    let brotli = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("br"));
    if !brotli {
        return body.clone();
    }
    let mut out = Vec::new();
    let limit = u64::try_from(MAX_429_BODY).unwrap_or(u64::MAX);
    match brotli_decompressor::Decompressor::new(&body[..], 4096)
        .take(limit)
        .read_to_end(&mut out)
    {
        Ok(_) => Bytes::from(out),
        Err(_) => Bytes::new(),
    }
}

/// The upstream response for twilight: hop-by-hop headers dropped, the body
/// streamed (the `total` phase is recorded when it ends).
fn relay(
    response: Response<Incoming>,
    metrics: EdgeMetrics,
    started: Instant,
) -> Response<EdgeBody> {
    let (mut parts, body) = response.into_parts();
    parts.headers = end_to_end(&parts.headers);
    let body = Timed {
        inner: body,
        done: Some((metrics, started)),
    };
    Response::from_parts(parts, body.boxed())
}

/// A body that records `total` when it ends, fails or is dropped.
struct Timed {
    inner: Incoming,
    done: Option<(EdgeMetrics, Instant)>,
}

impl Timed {
    fn finish(&mut self) {
        if let Some((metrics, started)) = self.done.take() {
            metrics.phase(Phase::Total, started.elapsed());
        }
    }
}

/// A body dropped early (twilight's timeout, a cancelled request) still
/// records its `total`.
impl Drop for Timed {
    fn drop(&mut self) {
        self.finish();
    }
}

impl Body for Timed {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, hyper::Error>>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_frame(cx);
        match &polled {
            Poll::Ready(None | Some(Err(_))) => this.finish(),
            Poll::Ready(Some(Ok(_))) if this.inner.is_end_stream() => this.finish(),
            _ => {}
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests;
