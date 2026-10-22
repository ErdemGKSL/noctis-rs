//! The HTTP/1.1 and h2c server behind
//! [`FakeDiscordHttp`](super::FakeDiscordHttp), with a per-connection IO
//! wrapper that logs HTTP/2 PINGs and can black-hole the connection.

use std::convert::Infallible;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode, header};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{AbortHandle, JoinSet};
use tokio::time::Instant;

use super::{
    FakeResponse, HttpProtocol, MAX_BODY_BYTES, RecordedHttpConnection, RecordedRequest, Shared,
};

/// The HTTP/2 client connection preface.
const PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// One accepted connection: its record and its controls.
pub(super) struct Connection {
    pub(super) record: RecordedHttpConnection,
    black_holed: Arc<AtomicBool>,
    abort: Option<AbortHandle>,
}

impl Connection {
    /// Swallows the connection's bytes from now on.
    pub(super) fn black_hole(&mut self) {
        self.black_holed.store(true, Ordering::SeqCst);
        self.record.black_holed = true;
    }

    /// Closes the connection (aborts its task, dropping the socket).
    pub(super) fn close(&self) {
        if let Some(abort) = &self.abort {
            abort.abort();
        }
    }
}

/// The pause after a failed `accept` (e.g. out of file descriptors).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(10);

/// Accepts connections until the task is aborted; dropping it drops the
/// `JoinSet`, which aborts every connection.
pub(super) async fn accept_loop(listener: TcpListener, shared: Shared) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let black_holed = Arc::new(AtomicBool::new(false));
                let id = {
                    let mut state = shared.lock();
                    let id = state.connections.len();
                    state.connections.push(Connection {
                        record: RecordedHttpConnection {
                            id,
                            protocol: HttpProtocol::Http1,
                            opened_at: Instant::now(),
                            closed_at: None,
                            requests: 0,
                            pings: Vec::new(),
                            black_holed: false,
                        },
                        black_holed: Arc::clone(&black_holed),
                        abort: None,
                    });
                    id
                };
                let io = FakeIo {
                    inner: stream,
                    shared: shared.clone(),
                    id,
                    black_holed,
                    scan: Scan::Detect(0),
                };
                let abort = connections.spawn(serve(io, id, shared.clone()));
                if let Some(conn) = shared.lock().connections.get_mut(id) {
                    conn.abort = Some(abort);
                }
            }
            Err(_) => tokio::time::sleep(ACCEPT_BACKOFF).await,
        }
        // Reap finished connections so the set does not grow forever.
        while connections.try_join_next().is_some() {}
    }
}

/// Marks its connection closed when the connection task ends or is
/// aborted.
struct Closed {
    shared: Shared,
    id: usize,
}

impl Drop for Closed {
    fn drop(&mut self) {
        if let Some(conn) = self.shared.lock().connections.get_mut(self.id) {
            conn.record.closed_at.get_or_insert_with(Instant::now);
        }
    }
}

/// Serves one connection (HTTP/1.1, or h2c when the preface arrives).
async fn serve(io: FakeIo, id: usize, shared: Shared) {
    let _closed = Closed {
        shared: shared.clone(),
        id,
    };
    let service = service_fn(move |request| {
        let shared = shared.clone();
        async move { Ok::<_, Infallible>(handle(&shared, id, request).await) }
    });
    // A connection error (client gone, malformed request) only ends this
    // connection.
    let _ = auto::Builder::new(TokioExecutor::new())
        .serve_connection(TokioIo::new(io), service)
        .await;
}

/// What the IO wrapper knows about the client's bytes.
enum Scan {
    /// Comparing the first bytes with the HTTP/2 preface (bytes matched).
    Detect(usize),
    /// HTTP/1.1: nothing to scan.
    Http1,
    /// HTTP/2 frames: the frame header so far, then payload bytes left.
    Header([u8; 9], usize),
    Payload(usize),
}

/// The connection IO: logs HTTP/2 PINGs from the client and black-holes
/// the connection on command.
struct FakeIo {
    inner: TcpStream,
    shared: Shared,
    id: usize,
    black_holed: Arc<AtomicBool>,
    scan: Scan,
}

impl FakeIo {
    /// Scans bytes the client sent.
    fn scan(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            match &mut self.scan {
                Scan::Http1 => return,
                Scan::Detect(matched) => {
                    let n = (PREFACE.len() - *matched).min(bytes.len());
                    if bytes[..n] != PREFACE[*matched..*matched + n] {
                        self.scan = Scan::Http1;
                        return;
                    }
                    *matched += n;
                    bytes = &bytes[n..];
                    if *matched == PREFACE.len() {
                        if let Some(conn) = self.shared.lock().connections.get_mut(self.id) {
                            conn.record.protocol = HttpProtocol::Http2;
                        }
                        self.scan = Scan::Header([0; 9], 0);
                    }
                }
                Scan::Header(header, filled) => {
                    let n = (9 - *filled).min(bytes.len());
                    header[*filled..*filled + n].copy_from_slice(&bytes[..n]);
                    *filled += n;
                    bytes = &bytes[n..];
                    if *filled == 9 {
                        let len = usize::from(header[0]) << 16
                            | usize::from(header[1]) << 8
                            | usize::from(header[2]);
                        // A PING without the ACK flag.
                        if header[3] == 0x6
                            && header[4] & 0x1 == 0
                            && let Some(conn) = self.shared.lock().connections.get_mut(self.id)
                        {
                            conn.record.pings.push(Instant::now());
                        }
                        self.scan = if len == 0 {
                            Scan::Header([0; 9], 0)
                        } else {
                            Scan::Payload(len)
                        };
                    }
                }
                Scan::Payload(left) => {
                    let n = (*left).min(bytes.len());
                    *left -= n;
                    bytes = &bytes[n..];
                    if *left == 0 {
                        self.scan = Scan::Header([0; 9], 0);
                    }
                }
            }
        }
    }
}

impl AsyncRead for FakeIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.black_holed.load(Ordering::SeqCst) {
            // Read and drop everything; never hand bytes to the server.
            let mut scratch = [0; 4096];
            loop {
                let mut sink = ReadBuf::new(&mut scratch);
                match Pin::new(&mut this.inner).poll_read(cx, &mut sink) {
                    // EOF: the client closed; let the server see the end.
                    Poll::Ready(Ok(())) if sink.filled().is_empty() => {
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Ready(Ok(())) => {}
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                    Poll::Pending => return Poll::Pending,
                }
            }
        }
        let before = buf.filled().len();
        let polled = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = polled {
            let fresh = buf.filled()[before..].to_vec();
            this.scan(&fresh);
        }
        polled
    }
}

impl AsyncWrite for FakeIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.black_holed.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(buf.len()));
        }
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.black_holed.load(Ordering::SeqCst) {
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Why a request body was not read.
enum BodyRejection {
    TooLarge,
    Broken,
}

/// Reads the body, logs the request, then answers it from the routes.
async fn handle(
    shared: &Shared,
    connection: usize,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let (parts, body) = request.into_parts();
    let declared_too_large = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|len| len > MAX_BODY_BYTES as u64);
    let body = if declared_too_large {
        // Never polled, so a client sending `expect: 100-continue` never
        // uploads it.
        Err(BodyRejection::TooLarge)
    } else {
        match Limited::new(body, MAX_BODY_BYTES).collect().await {
            Ok(collected) => Ok(collected.to_bytes()),
            Err(err) if err.downcast_ref::<LengthLimitError>().is_some() => {
                Err(BodyRejection::TooLarge)
            }
            Err(_) => Err(BodyRejection::Broken),
        }
    };
    let recorded = RecordedRequest {
        method: parts.method,
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().map(str::to_owned),
        headers: parts.headers,
        body: body.as_ref().map_or_else(|_| Bytes::new(), Bytes::clone),
        connection,
    };
    let stalled = log(shared, recorded.clone());
    if stalled {
        // Never answered; the connection's end drops this future.
        std::future::pending::<()>().await;
    }
    let response = match body {
        Err(BodyRejection::TooLarge) => FakeResponse::discord_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Request entity too large",
            40005,
        ),
        Err(BodyRejection::Broken) => {
            FakeResponse::discord_error(StatusCode::BAD_REQUEST, "400: Bad Request", 0)
        }
        Ok(_) => match lookup(shared, &recorded) {
            Lookup::Route(route) => route.respond(&recorded),
            Lookup::Fallback(fallback) => fallback(&recorded).unwrap_or_else(not_found),
            Lookup::Response(response) => response,
        },
    };
    if let Some(delay) = response.delay {
        tokio::time::sleep(delay).await;
    }
    to_hyper(response)
}

/// What a lookup found; a route is called after the lock is released.
enum Lookup {
    Route(super::Route),
    Fallback(super::Fallback),
    Response(FakeResponse),
}

/// Finds the answer for `request` without holding the lock afterwards.
fn lookup(shared: &Shared, request: &RecordedRequest) -> Lookup {
    let state = shared.lock();
    let Some(route_path) = request.route_path() else {
        return Lookup::Response(not_found());
    };
    if let Some(route) = state
        .routes
        .get(&(request.method.clone(), route_path.to_owned()))
    {
        return Lookup::Route(route.clone());
    }
    if request.method == hyper::Method::GET && route_path == "/gateway/bot" {
        return Lookup::Response(FakeResponse::json(
            StatusCode::OK,
            &state.gateway_bot.to_json(),
        ));
    }
    match &state.fallback {
        Some(fallback) => Lookup::Fallback(Arc::clone(fallback)),
        None => Lookup::Response(not_found()),
    }
}

/// Discord's `404` body.
fn not_found() -> FakeResponse {
    FakeResponse::discord_error(StatusCode::NOT_FOUND, "404: Not Found", 0)
}

/// Appends `request` to the log and counts it on its connection; returns
/// whether it must stall.
fn log(shared: &Shared, request: RecordedRequest) -> bool {
    let mut state = shared.lock();
    if let Some(conn) = state.connections.get_mut(request.connection) {
        conn.record.requests += 1;
    }
    let stalled = request.route_path().is_some_and(|path| {
        state
            .stalls
            .get_mut(&(request.method.clone(), path.to_owned()))
            .is_some_and(|left| {
                let stall = *left > 0;
                *left = left.saturating_sub(1);
                stall
            })
    });
    state.requests.push(request);
    stalled
}

/// Turns a [`FakeResponse`] into a hyper response.
fn to_hyper(response: FakeResponse) -> Response<Full<Bytes>> {
    let mut out = Response::new(Full::new(response.body));
    *out.status_mut() = response.status;
    *out.headers_mut() = response.headers;
    out
}
