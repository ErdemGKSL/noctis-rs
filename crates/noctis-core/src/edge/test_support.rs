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
                tokio::spawn(serve(
                    stream,
                    conn,
                    h2,
                    Arc::clone(&accept.log),
                    Arc::clone(&answer),
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
    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
        let log = Arc::clone(&log);
        let answer = Arc::clone(&answer);
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
            let (status, body) = answer(&seen);
            log.lock().unwrap().push(seen);
            let mut response = Response::new(Full::new(body));
            *response.status_mut() = status;
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
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response).await;
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head");
    let head = std::str::from_utf8(&response[..split]).unwrap();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, response[split + 4..].to_vec())
}

/// A request with `connection: close`.
pub(crate) fn get(target: &str) -> Vec<u8> {
    format!("GET {target} HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n\r\n").into_bytes()
}
