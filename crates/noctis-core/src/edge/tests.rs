//! Story 5.9 Matrix 6-9, 11, 13, 16, 47 and 48: the forwarder against local
//! upstreams.

use std::time::Duration;

use hyper::StatusCode;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::test_support::{TestServer, get, raw};
use super::*;

fn start(upstream: Upstream) -> Edge {
    start_config(upstream, HttpConfig::default())
}

fn start_config(upstream: Upstream, config: HttpConfig) -> Edge {
    Edge::start(
        Edge::bind().unwrap(),
        upstream,
        config,
        &Arc::from("default"),
    )
}

fn h2c(server: &TestServer) -> Upstream {
    Upstream::http(&server.authority(), true)
}

/// Every histogram sample count and counter value, by name and labels.
/// A metric name, its labels and its count.
type Sample = (String, Vec<(String, String)>, usize);

fn samples(snapshotter: &Snapshotter) -> Vec<Sample> {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .map(|(key, _, _, value)| {
            let labels = key
                .key()
                .labels()
                .map(|l| (l.key().to_owned(), l.value().to_owned()))
                .collect();
            let n = match value {
                DebugValue::Histogram(v) => v.len(),
                DebugValue::Counter(c) => usize::try_from(c).unwrap(),
                DebugValue::Gauge(_) => 1,
            };
            (key.key().name().to_owned(), labels, n)
        })
        .collect()
}

fn count(samples: &[Sample], name: &str, label: Option<(&str, &str)>) -> usize {
    samples
        .iter()
        .filter(|(n, labels, _)| {
            n == name && label.is_none_or(|(k, v)| labels.iter().any(|(lk, lv)| lk == k && lv == v))
        })
        .map(|(.., c)| *c)
        .sum()
}

/// Matrix 6: the listener is loopback with a real port.
#[test]
fn bind_is_loopback() {
    let (_listener, addr) = Edge::bind().unwrap();
    assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
    assert_ne!(addr.port(), 0);
}

/// Matrix 8: only origin-form `/api/v10/` targets without dot segments are
/// forwarded.
#[tokio::test]
async fn path_filter() {
    let server = TestServer::start(true).await;
    let edge = start(h2c(&server));
    let (status, _) = raw(edge.local_addr(), &get("/api/v10/gateway/bot")).await;
    assert_eq!(status, 200);
    for target in [
        "/api/v9/x",
        "/api/v10",
        "/",
        "/api/v100/x",
        "/API/V10/x",
        "/api/v10/../x",
        "/api/v10/./x",
        "/api/v10/%2e%2e/x",
        "/api/v10/%2E/x",
        "http://evil/api/v10/x",
    ] {
        let (status, body) = raw(edge.local_addr(), &get(target)).await;
        assert_eq!(status, 403, "{target}");
        assert_eq!(body, FORBIDDEN_BODY.as_bytes(), "{target}");
    }
    let (status, _) = raw(
        edge.local_addr(),
        b"CONNECT discord.com:443 HTTP/1.1\r\nhost: discord.com:443\r\nconnection: close\r\n\r\n",
    )
    .await;
    assert_eq!(status, 403);
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/api/v10/gateway/bot");
    assert_eq!(seen[0].method, Method::GET);
}

/// Matrix 9: path and query bytes arrive unchanged.
#[tokio::test]
async fn query_raw() {
    let server = TestServer::start(false).await;
    let edge = start(Upstream::http(&server.authority(), false));
    raw(
        edge.local_addr(),
        &get("/api/v10/channels/1/messages?limit=5&after=%31"),
    )
    .await;
    let seen = server.seen();
    assert_eq!(seen[0].path, "/api/v10/channels/1/messages");
    assert_eq!(seen[0].query.as_deref(), Some("limit=5&after=%31"));
    assert_eq!(seen[0].headers["host"], server.authority().as_str());
}

/// Matrix 11: hop-by-hop headers (and the names `connection` lists) are
/// dropped; the others are kept in order.
#[tokio::test]
async fn hop_by_hop() {
    for h2 in [false, true] {
        let server = TestServer::start(h2).await;
        let edge = start(Upstream::http(&server.authority(), h2));
        raw(
            edge.local_addr(),
            b"GET /api/v10/x HTTP/1.1\r\nhost: 127.0.0.1\r\nx-a: 1\r\nconnection: x-foo, keep-alive\r\nconnection: close\r\n\
              x-foo: 1\r\nte: trailers\r\nupgrade: h2c\r\nx-b: 2\r\nx-a: 3\r\n\r\n",
        )
        .await;
        let seen = &server.seen()[0];
        for gone in ["connection", "x-foo", "te", "upgrade", "keep-alive"] {
            assert!(seen.headers.get(gone).is_none(), "{gone} (h2 {h2})");
        }
        let x_a: Vec<_> = seen.headers.get_all("x-a").iter().collect();
        assert_eq!(x_a, ["1", "3"]);
        assert_eq!(seen.headers["x-b"], "2");
    }
}

/// Matrix 13: bodies up to 512 MiB are forwarded; a larger declared one is
/// 413 and never forwarded.
#[tokio::test]
async fn body_sizes() {
    let server = TestServer::start(true).await;
    let edge = start(h2c(&server));
    for len in [0, 64 * 1024 * 1024] {
        let mut request = format!(
            "POST /api/v10/x HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: {len}\r\nconnection: close\r\n\r\n"
        )
        .into_bytes();
        request.extend(std::iter::repeat_n(b'z', len));
        let (status, _) = raw(edge.local_addr(), &request).await;
        assert_eq!(status, 200, "{len}");
    }
    let seen = server.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].body.len(), 64 * 1024 * 1024);
    let head = format!(
        "POST /api/v10/x HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        512 * 1024 * 1024 + 1
    );
    let (status, body) = raw(edge.local_addr(), head.as_bytes()).await;
    assert_eq!(status, 413);
    assert_eq!(body, TOO_LARGE_BODY.as_bytes());
    assert_eq!(server.seen().len(), 2);
}

/// What the first connection of a [`scripted`] upstream does after the
/// request's HEADERS (or HEADERS + DATA).
#[derive(Clone, Copy, Debug)]
enum Script {
    RefusedStream,
    GoAway,
    InternalError,
    CloseAfterData,
}

fn h2_frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len()).unwrap();
    let mut out = len.to_be_bytes()[1..].to_vec();
    out.extend([kind, flags]);
    out.extend(stream.to_be_bytes());
    out.extend(payload);
    out
}

/// A raw HTTP/2 server playing `script` on its first connection.
async fn script_conn(mut stream: TcpStream, script: Script) {
    let mut preface = [0; 24];
    if stream.read_exact(&mut preface).await.is_err() {
        return;
    }
    let _ = stream.write_all(&h2_frame(0x4, 0, 0, &[])).await;
    loop {
        let mut header = [0; 9];
        if stream.read_exact(&mut header).await.is_err() {
            return;
        }
        let len =
            usize::from(header[0]) << 16 | usize::from(header[1]) << 8 | usize::from(header[2]);
        let mut payload = vec![0; len];
        if stream.read_exact(&mut payload).await.is_err() {
            return;
        }
        let (kind, flags) = (header[3], header[4]);
        let id = u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff;
        match (kind, script) {
            (0x4, _) if flags & 1 == 0 => {
                let _ = stream.write_all(&h2_frame(0x4, 1, 0, &[])).await;
            }
            (0x1, Script::RefusedStream) => {
                let _ = stream
                    .write_all(&h2_frame(0x3, 0, id, &7_u32.to_be_bytes()))
                    .await;
            }
            (0x1, Script::InternalError) => {
                let _ = stream
                    .write_all(&h2_frame(0x3, 0, id, &2_u32.to_be_bytes()))
                    .await;
            }
            (0x1, Script::GoAway) => {
                let mut goaway = 0_u32.to_be_bytes().to_vec();
                goaway.extend(0_u32.to_be_bytes());
                let _ = stream.write_all(&h2_frame(0x7, 0, 0, &goaway)).await;
            }
            (0x0, Script::CloseAfterData) if flags & 1 != 0 => return,
            _ => {}
        }
    }
}

/// An upstream whose first connection plays `script` and whose later
/// connections are a normal h2c server.
async fn scripted(script: Script) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = TestServer {
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
            let conn = accept
                .conns
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if conn == 0 {
                tokio::spawn(script_conn(stream, script));
            } else {
                let answer: test_support::Answer =
                    Arc::new(|_| (StatusCode::OK, Bytes::from_static(b"ok")));
                tokio::spawn(test_support::serve(
                    stream,
                    conn,
                    true,
                    Arc::clone(&accept.log),
                    answer,
                ));
            }
        }
    });
    server
}

/// Matrix 16 (a)-(d): only refused streams and streams above GOAWAY are
/// retried, once, on a new connection.
#[tokio::test]
async fn retry_safe_classes() {
    for (script, retried) in [
        (Script::RefusedStream, true),
        (Script::GoAway, true),
        (Script::InternalError, false),
        (Script::CloseAfterData, false),
    ] {
        let server = scripted(script).await;
        let edge = start(h2c(&server));
        let request = b"POST /api/v10/x HTTP/1.1\r\nhost: 127.0.0.1\r\ncontent-length: 2\r\nconnection: close\r\n\r\nhi";
        let (status, body) =
            tokio::time::timeout(Duration::from_secs(15), raw(edge.local_addr(), request))
                .await
                .unwrap();
        if retried {
            assert_eq!(status, 200, "{script:?}");
            let seen = server.seen();
            assert_eq!(seen.len(), 1, "{script:?}");
            assert_eq!(seen[0].conn, 1, "{script:?}");
            assert_eq!(seen[0].body, "hi");
        } else {
            assert_eq!(status, 502, "{script:?}");
            assert_eq!(body, EDGE_ERROR_BODY.as_bytes());
            assert!(server.seen().is_empty(), "{script:?}");
            assert_eq!(
                server.conns.load(std::sync::atomic::Ordering::SeqCst),
                1,
                "{script:?}"
            );
        }
    }
}

/// Matrix 16 (e): a refused dial is retried once (two dials), then 502.
#[tokio::test]
async fn refused_dial_twice_then_502() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = ::metrics::set_default_local_recorder(&recorder);
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let edge = start(Upstream::http(&format!("127.0.0.1:{port}"), true));
    let (status, body) = raw(edge.local_addr(), &get("/api/v10/x")).await;
    assert_eq!(status, 502);
    assert_eq!(body, EDGE_ERROR_BODY.as_bytes());
    let samples = samples(&snapshotter);
    // Every dial records a `dns` sample before it connects.
    assert_eq!(
        count(
            &samples,
            "noctis_http_phase_seconds",
            Some(("phase", "dns"))
        ),
        2
    );
    assert_eq!(
        count(
            &samples,
            "noctis_http_phase_seconds",
            Some(("phase", "connect"))
        ),
        0
    );
    assert_eq!(
        count(
            &samples,
            "noctis_http_requests_total",
            Some(("status_class", "error"))
        ),
        1
    );
}

/// Matrix 48: phase samples and reuse.
#[tokio::test]
async fn phases_and_reuse() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = ::metrics::set_default_local_recorder(&recorder);
    let server = TestServer::start(true).await;
    let edge = start(h2c(&server));
    raw(edge.local_addr(), &get("/api/v10/a")).await;
    let first = samples(&snapshotter);
    let phase = |s: &[_], p| count(s, "noctis_http_phase_seconds", Some(("phase", p)));
    for p in ["dns", "connect", "ttfb", "total"] {
        assert_eq!(phase(&first, p), 1, "{p}");
    }
    assert_eq!(phase(&first, "tls"), 0);
    assert_eq!(
        count(
            &first,
            "noctis_http_conn_reused_total",
            Some(("reused", "false"))
        ),
        1
    );
    assert_eq!(
        count(
            &first,
            "noctis_http_connections_total",
            Some(("reason", "demand"))
        ),
        1
    );
    raw(edge.local_addr(), &get("/api/v10/b")).await;
    // The debugging recorder drains histograms on snapshot.
    let second = samples(&snapshotter);
    assert_eq!(phase(&second, "dns"), 0);
    assert_eq!(phase(&second, "ttfb"), 1);
    assert_eq!(phase(&second, "total"), 1);
    assert_eq!(
        count(
            &second,
            "noctis_http_conn_reused_total",
            Some(("reused", "true"))
        ),
        1
    );
    assert!(server.seen().iter().all(|s| s.conn == 0));
}

/// Matrix 47: a TLS upstream negotiates h2 or HTTP/1.1 with ALPN.
#[tokio::test]
async fn tls_upstream() {
    use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};

    crate::startup::ensure_crypto_provider().unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["localhost".to_owned()])
        .unwrap()
        .signed_by(&leaf_key, &ca)
        .unwrap();
    for (alpn, h2) in [(b"h2".to_vec(), true), (b"http/1.1".to_vec(), false)] {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = ::metrics::set_default_local_recorder(&recorder);
        let mut server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
            )
            .unwrap();
        server_config.alpn_protocols = vec![alpn];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::default();
        let server_log = Arc::clone(&log);
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let tls = acceptor.accept(stream).await.unwrap();
            let answer: test_support::Answer =
                Arc::new(|_| (StatusCode::OK, Bytes::from_static(b"ok")));
            test_support::serve(tls, 0, h2, server_log, answer).await;
        });
        let mut roots = rustls::RootCertStore::empty();
        roots.add(ca.der().clone()).unwrap();
        let client = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let upstream = Upstream::tls(
            "127.0.0.1",
            port,
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            client,
        );
        let edge = start(upstream);
        let (status, body) = raw(edge.local_addr(), &get("/api/v10/tls")).await;
        assert_eq!((status, body.as_slice()), (200, &b"ok"[..]), "h2 {h2}");
        let seen: Vec<test_support::Seen> = log.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].h2, h2);
        if !h2 {
            assert_eq!(
                seen[0].headers["host"],
                format!("127.0.0.1:{port}").as_str()
            );
        }
        let samples = samples(&snapshotter);
        assert_eq!(
            count(
                &samples,
                "noctis_http_phase_seconds",
                Some(("phase", "tls"))
            ),
            1
        );
    }
}

/// The upstream authority omits default ports and brackets IPv6.
#[test]
fn upstream_authority() {
    assert_eq!(Upstream::http("proxy", false).authority(), "proxy");
    assert_eq!(Upstream::http("proxy:80", false).authority(), "proxy");
    assert_eq!(
        Upstream::http("proxy:3000", false).authority(),
        "proxy:3000"
    );
    assert_eq!(
        Upstream::http("[::1]:3000", false).authority(),
        "[::1]:3000"
    );
    assert_eq!(Upstream::http("[::1]", false).scheme(), "http");
}

/// End-to-end headers: `end_to_end` keeps order and duplicates.
#[test]
fn end_to_end_keeps_order() {
    let mut headers = HeaderMap::new();
    headers.append("x-a", HeaderValue::from_static("1"));
    headers.append("connection", HeaderValue::from_static("X-Foo"));
    headers.append("x-foo", HeaderValue::from_static("2"));
    headers.append("x-a", HeaderValue::from_static("3"));
    headers.append("transfer-encoding", HeaderValue::from_static("chunked"));
    let out = end_to_end(&headers);
    let names: Vec<_> = out
        .iter()
        .map(|(n, v)| (n.as_str(), v.to_str().unwrap()))
        .collect();
    assert_eq!(names, [("x-a", "1"), ("x-a", "3")]);
}

/// `Outgoing::to_request`: an absolute URI and no `host` over h2, the
/// origin form with the upstream `host` over HTTP/1.1.
#[tokio::test]
async fn to_request_forms() {
    for h2 in [true, false] {
        let server = TestServer::start(h2).await;
        let config = HttpConfig::default();
        let dialer = Dialer {
            upstream: Upstream::http(&server.authority(), h2),
            dns: DnsCache::new(
                Arc::new(SystemResolver::default()),
                config.dns_ttl_floor,
                config.dns_ttl_cap,
            ),
            config,
            metrics: EdgeMetrics::new(Arc::from("default")),
        };
        let pool = Pool::new(dialer, CancellationToken::new());
        let conn = pool.dialer.dial().await.unwrap();
        let mut headers = HeaderMap::new();
        headers.append(header::HOST, HeaderValue::from_static("127.0.0.1:1"));
        headers.append("x-a", HeaderValue::from_static("1"));
        let outgoing = Outgoing {
            method: Method::POST,
            target: "/api/v10/a?b=%31".to_owned(),
            headers,
            body: Bytes::from_static(b"body"),
        };
        let request = outgoing.to_request(&pool, &conn.sender).unwrap();
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.headers()["x-a"], "1");
        if h2 {
            assert_eq!(
                request.uri().to_string(),
                format!("http://{}/api/v10/a?b=%31", server.authority())
            );
            assert!(!request.headers().contains_key(header::HOST));
        } else {
            assert_eq!(request.uri().to_string(), "/api/v10/a?b=%31");
            assert_eq!(request.headers()[header::HOST], server.authority().as_str());
        }
    }
}

/// Matrix 48 over HTTP/1.1: a released idle connection serves the next
/// request.
#[tokio::test]
async fn h1_idle_reuse() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = ::metrics::set_default_local_recorder(&recorder);
    let server = TestServer::start(false).await;
    let edge = start(Upstream::http(&server.authority(), false));
    raw(edge.local_addr(), &get("/api/v10/a")).await;
    let first = samples(&snapshotter);
    let demand = |s: &[_]| {
        count(
            s,
            "noctis_http_connections_total",
            Some(("reason", "demand")),
        )
    };
    let reused = |s: &[_]| count(s, "noctis_http_conn_reused_total", Some(("reused", "true")));
    assert_eq!(demand(&first), 1);
    assert_eq!(reused(&first), 0);
    // The release task pools the connection once its response finished.
    tokio::time::timeout(Duration::from_secs(5), async {
        while edge.pool.idle_len() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    raw(edge.local_addr(), &get("/api/v10/b")).await;
    // The debugging recorder resets counters on snapshot: no new dial.
    let second = samples(&snapshotter);
    assert_eq!(demand(&second), 0);
    assert_eq!(reused(&second), 1);
    let seen = server.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].conn, seen[1].conn);
}
