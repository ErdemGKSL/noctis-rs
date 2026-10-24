//! Test upstreams for the edge unit tests: a recording HTTP/1.1 or h2c
//! server and raw-request helpers.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "test helpers")]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::HeaderMap;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A request the test server received.
#[derive(Debug, Clone)]
pub(crate) struct Seen {
    pub(crate) conn: usize,
    pub(crate) method: Method,
    pub(crate) path: String,
    pub(crate) query: Option<String>,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
    pub(crate) h2: bool,
}

/// How the test server answers.
pub(crate) type Answer = Arc<dyn Fn(&Seen) -> (StatusCode, Bytes) + Send + Sync>;

/// A full answer: status, headers, body and a delay before the headers.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
    pub(crate) delay: Option<std::time::Duration>,
}

impl Reply {
    pub(crate) fn new(status: StatusCode, headers: HeaderMap, body: impl Into<Bytes>) -> Self {
        Self {
            status,
            headers,
            body: body.into(),
            delay: None,
        }
    }
}

/// How the test server answers, with headers and a delay.
pub(crate) type Replier = Arc<dyn Fn(&Seen) -> Reply + Send + Sync>;

/// An [`Answer`] as a [`Replier`].
fn replier(answer: Answer) -> Replier {
    Arc::new(move |seen| {
        let (status, body) = answer(seen);
        Reply::new(status, HeaderMap::new(), body)
    })
}

/// A recording upstream on 127.0.0.1.
#[derive(Clone)]
pub(crate) struct TestServer {
    pub(crate) addr: SocketAddr,
    pub(crate) log: Arc<Mutex<Vec<Seen>>>,
    pub(crate) conns: Arc<AtomicUsize>,
}

impl TestServer {
    /// Serves h2c (`h2`) or HTTP/1.1, answering 200 `ok`.
    pub(crate) async fn start(h2: bool) -> Self {
        Self::start_with(
            h2,
            Arc::new(|_| (StatusCode::OK, Bytes::from_static(b"ok"))),
        )
        .await
    }

    pub(crate) async fn start_with(h2: bool, answer: Answer) -> Self {
        Self::start_replying(h2, replier(answer)).await
    }

    /// Serves h2c (`h2`) or HTTP/1.1, answering with `reply`.
    pub(crate) async fn start_replying(h2: bool, reply: Replier) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = Self {
            addr: listener.local_addr().unwrap(),
            log: Arc::default(),
            conns: Arc::default(),
        };
        let accept = server.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let conn = accept.conns.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve_reply(
                    stream,
                    conn,
                    h2,
                    Arc::clone(&accept.log),
                    Arc::clone(&reply),
                ));
            }
        });
        server
    }

    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }

    pub(crate) fn authority(&self) -> String {
        format!("127.0.0.1:{}", self.addr.port())
    }
}

/// Serves one connection of a [`TestServer`] (also over TLS).
pub(crate) async fn serve<T>(
    io: T,
    conn: usize,
    h2: bool,
    log: Arc<Mutex<Vec<Seen>>>,
    answer: Answer,
) where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    serve_reply(io, conn, h2, log, replier(answer)).await;
}

/// [`serve`] with a [`Replier`].
pub(crate) async fn serve_reply<T>(
    io: T,
    conn: usize,
    h2: bool,
    log: Arc<Mutex<Vec<Seen>>>,
    reply: Replier,
) where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
        let log = Arc::clone(&log);
        let reply = Arc::clone(&reply);
        async move {
            let (parts, body) = request.into_parts();
            let body = body
                .collect()
                .await
                .map(http_body_util::Collected::to_bytes)
                .unwrap_or_default();
            let seen = Seen {
                conn,
                method: parts.method,
                path: parts.uri.path().to_owned(),
                query: parts.uri.query().map(str::to_owned),
                headers: parts.headers,
                body,
                h2,
            };
            let reply = reply(&seen);
            log.lock().unwrap().push(seen);
            if let Some(delay) = reply.delay {
                tokio::time::sleep(delay).await;
            }
            let mut response = Response::new(Full::new(reply.body));
            *response.status_mut() = reply.status;
            *response.headers_mut() = reply.headers;
            Ok::<_, Infallible>(response)
        }
    });
    let io = TokioIo::new(io);
    if h2 {
        let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
            .serve_connection(io, service)
            .await;
    } else {
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(io, service)
            .await;
    }
}

/// Sends raw bytes to `addr` and returns the status and body of the
/// response (the request should carry `connection: close`).
pub(crate) async fn raw(addr: SocketAddr, request: &[u8]) -> (u16, Vec<u8>) {
    let (status, _, body) = raw_full(addr, request).await;
    (status, body)
}

/// [`raw`] with the response headers (lowercase names, in order).
pub(crate) async fn raw_full(
    addr: SocketAddr,
    request: &[u8],
) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response).await;
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let head = std::str::from_utf8(&response[..split]).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    (status, headers, response[split + 4..].to_vec())
}

/// A request with `connection: close`.
pub(crate) fn get(target: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n\r\n").into_bytes()
}
