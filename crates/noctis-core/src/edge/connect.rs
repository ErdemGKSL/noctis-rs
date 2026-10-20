//! The upstream and its dial phases: DNS, TCP (with socket tuning), TLS
//! and the HTTP/1.1 or HTTP/2 handshake, each timed and bounded.

use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::client::conn::{http1, http2};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::config::HttpConfig;
use super::dns::DnsCache;
use super::metrics::{EdgeMetrics, Phase};
use super::sniff::{PingTracker, Sniff};

/// How many resolved addresses one dial tries.
const MAX_ADDRESSES: usize = 3;
/// The TLS handshake bound.
const TLS_TIMEOUT: Duration = Duration::from_secs(3);
/// The HTTP handshake bound.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
/// TCP keepalive: idle time before the first probe.
const KEEPALIVE_TIME: Duration = Duration::from_secs(30);
/// TCP keepalive: time between probes.
#[cfg(target_os = "linux")]
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);
/// TCP keepalive: unanswered probes before the kernel drops the connection.
#[cfg(target_os = "linux")]
const KEEPALIVE_PROBES: u32 = 3;
/// How long written data may stay unacknowledged (Linux `TCP_USER_TIMEOUT`).
#[cfg(target_os = "linux")]
const USER_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the edge sends requests.
#[derive(Clone)]
pub(crate) enum Upstream {
    /// HTTPS; ALPN (`h2`, `http/1.1`) picks the protocol.
    Tls {
        host: String,
        port: u16,
        server_name: ServerName<'static>,
        config: Arc<rustls::ClientConfig>,
    },
    /// Plain HTTP: HTTP/2 prior knowledge with `h2c`, else HTTP/1.1.
    Http { host: String, port: u16, h2c: bool },
}

impl Upstream {
    /// The `http_proxy` upstream when one is set, else Discord.
    pub(crate) fn for_proxy(proxy: Option<&str>, h2c: bool) -> Self {
        proxy.map_or_else(Self::discord, |proxy| Self::http(proxy, h2c))
    }

    /// `https://discord.com:443` with the platform verifier.
    pub(crate) fn discord() -> Self {
        use rustls_platform_verifier::ConfigVerifierExt;

        let config = rustls::ClientConfig::with_platform_verifier().unwrap_or_else(|error| {
            tracing::error!(
                target: "noctis::http",
                %error,
                "the platform TLS verifier is unavailable; Discord HTTPS connections will fail"
            );
            rustls::ClientConfig::builder()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth()
        });
        let server_name = ServerName::try_from("discord.com").unwrap_or(ServerName::IpAddress(
            rustls::pki_types::IpAddr::from(std::net::Ipv4Addr::UNSPECIFIED),
        ));
        Self::tls("discord.com", 443, server_name, config)
    }

    /// An HTTPS upstream; the ALPN list is set to `h2`, `http/1.1`.
    pub(crate) fn tls(
        host: &str,
        port: u16,
        server_name: ServerName<'static>,
        mut config: rustls::ClientConfig,
    ) -> Self {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Self::Tls {
            host: host.to_owned(),
            port,
            server_name,
            config: Arc::new(config),
        }
    }

    /// The `http_proxy` upstream: a normalized `host[:port]` authority.
    pub(crate) fn http(authority: &str, h2c: bool) -> Self {
        let (host, port) = split_authority(authority);
        Self::Http {
            host: host.to_owned(),
            port: port.unwrap_or(80),
            h2c,
        }
    }

    fn host(&self) -> &str {
        match self {
            Self::Tls { host, .. } | Self::Http { host, .. } => host,
        }
    }

    fn port(&self) -> u16 {
        match self {
            Self::Tls { port, .. } | Self::Http { port, .. } => *port,
        }
    }

    /// `http` or `https`.
    pub(crate) fn scheme(&self) -> &'static str {
        match self {
            Self::Tls { .. } => "https",
            Self::Http { .. } => "http",
        }
    }

    /// The `host` header / `:authority` value (the port only when it is
    /// not the scheme's default).
    pub(crate) fn authority(&self) -> String {
        let host = self.host();
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        match (self, self.port()) {
            (Self::Tls { .. }, 443) | (Self::Http { .. }, 80) => host,
            (_, port) => format!("{host}:{port}"),
        }
    }
}

/// Splits a normalized `host[:port]` (IPv6 hosts in brackets).
fn split_authority(authority: &str) -> (&str, Option<u16>) {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']').unwrap_or((rest, ""));
        return (host, after.strip_prefix(':').and_then(|p| p.parse().ok()));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()),
        None => (authority, None),
    }
}

/// Which dial phase failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialPhase {
    Dns,
    Connect,
    Tls,
    Handshake,
}

/// A failed dial; never retried after a write, so always retry-safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DialError {
    pub(crate) phase: DialPhase,
    pub(crate) kind: io::ErrorKind,
}

impl fmt::Display for DialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} phase failed ({:?})", self.phase, self.kind)
    }
}

impl DialError {
    fn new(phase: DialPhase, kind: io::ErrorKind) -> Self {
        Self { phase, kind }
    }
}

/// A connection's request half.
pub(crate) enum Sender {
    H1(http1::SendRequest<Full<Bytes>>),
    H2(http2::SendRequest<Full<Bytes>>),
}

impl Sender {
    pub(crate) fn is_h2(&self) -> bool {
        matches!(self, Self::H2(_))
    }
}

/// The connection half of a handshake, driven by its own task.
pub(crate) enum Driver {
    H1(Box<http1::Connection<TokioIo<Io>, Full<Bytes>>>),
    H2(http2::Connection<TokioIo<Sniff<Io>>, Full<Bytes>, TokioExecutor>),
}

impl Driver {
    /// Runs the connection until it closes.
    pub(crate) async fn run(self) -> Result<(), hyper::Error> {
        match self {
            Self::H1(conn) => (*conn).await,
            Self::H2(conn) => conn.await,
        }
    }
}

/// A finished dial.
pub(crate) struct Dialed {
    pub(crate) sender: Sender,
    pub(crate) driver: Driver,
}

/// The connection IO: plain TCP or TLS over TCP.
pub(crate) enum Io {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for Io {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => Pin::new(io).poll_read(cx, buf),
            Self::Tls(io) => Pin::new(io.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Io {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(io) => Pin::new(io).poll_write(cx, buf),
            Self::Tls(io) => Pin::new(io.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => Pin::new(io).poll_flush(cx),
            Self::Tls(io) => Pin::new(io.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(io) => Pin::new(io).poll_shutdown(cx),
            Self::Tls(io) => Pin::new(io.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Opens upstream connections.
pub(crate) struct Dialer {
    pub(crate) upstream: Upstream,
    pub(crate) dns: DnsCache,
    pub(crate) config: HttpConfig,
    pub(crate) metrics: EdgeMetrics,
}

impl Dialer {
    /// Runs every dial phase and the handshake.
    pub(crate) async fn dial(&self) -> Result<Dialed, DialError> {
        let started = Instant::now();
        let addrs = self
            .dns
            .lookup(self.upstream.host())
            .await
            .map_err(|e| DialError::new(DialPhase::Dns, e.kind()))?;
        self.metrics.phase(Phase::Dns, started.elapsed());
        let started = Instant::now();
        let tcp = self.connect(&addrs).await?;
        self.metrics.phase(Phase::Connect, started.elapsed());
        tune(&tcp);
        let (io, h2) = match &self.upstream {
            Upstream::Tls {
                server_name,
                config,
                ..
            } => {
                let started = Instant::now();
                let connector = TlsConnector::from(Arc::clone(config));
                let tls = timeout(TLS_TIMEOUT, connector.connect(server_name.clone(), tcp))
                    .await
                    .map_err(|_| DialError::new(DialPhase::Tls, io::ErrorKind::TimedOut))?
                    .map_err(|e| DialError::new(DialPhase::Tls, e.kind()))?;
                self.metrics.phase(Phase::Tls, started.elapsed());
                let h2 = tls.get_ref().1.alpn_protocol() == Some(b"h2");
                (Io::Tls(Box::new(tls)), h2)
            }
            Upstream::Http { h2c, .. } => (Io::Plain(tcp), *h2c),
        };
        timeout(HANDSHAKE_TIMEOUT, self.handshake(io, h2))
            .await
            .map_err(|_| DialError::new(DialPhase::Handshake, io::ErrorKind::TimedOut))?
    }

    /// Tries up to [`MAX_ADDRESSES`] addresses in order, each under
    /// `connect_timeout`.
    async fn connect(&self, addrs: &[IpAddr]) -> Result<TcpStream, DialError> {
        let port = self.upstream.port();
        let mut last = io::ErrorKind::NotFound;
        for ip in addrs.iter().take(MAX_ADDRESSES) {
            let addr = SocketAddr::new(*ip, port);
            match timeout(self.config.connect_timeout, TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => return Ok(stream),
                Ok(Err(error)) => last = error.kind(),
                Err(_) => last = io::ErrorKind::TimedOut,
            }
        }
        Err(DialError::new(DialPhase::Connect, last))
    }

    async fn handshake(&self, io: Io, h2: bool) -> Result<Dialed, DialError> {
        let failed = |error: hyper::Error| {
            tracing::debug!(target: "noctis::http", %error, "upstream handshake failed");
            DialError::new(DialPhase::Handshake, io::ErrorKind::Other)
        };
        if h2 {
            let metrics = self.metrics.clone();
            let tracker = Arc::new(PingTracker::new(move |rtt| metrics.ping_rtt(rtt)));
            let (sender, conn) = http2::Builder::new(TokioExecutor::new())
                .timer(TokioTimer::new())
                .keep_alive_interval(self.config.ping_interval)
                .keep_alive_timeout(self.config.ping_timeout)
                .keep_alive_while_idle(true)
                .handshake(TokioIo::new(Sniff::new(io, tracker)))
                .await
                .map_err(failed)?;
            Ok(Dialed {
                sender: Sender::H2(sender),
                driver: Driver::H2(conn),
            })
        } else {
            let (sender, conn) = http1::Builder::new()
                .handshake(TokioIo::new(io))
                .await
                .map_err(failed)?;
            Ok(Dialed {
                sender: Sender::H1(sender),
                driver: Driver::H1(Box::new(conn)),
            })
        }
    }
}

/// `TCP_NODELAY`, keepalive 30 s / 10 s / 3 probes and, on Linux,
/// `TCP_USER_TIMEOUT` 10 s; a failure is logged at `debug` and ignored.
fn tune(stream: &TcpStream) {
    let socket = socket2::SockRef::from(stream);
    if let Err(error) = socket.set_tcp_nodelay(true) {
        tracing::debug!(target: "noctis::http", %error, "TCP_NODELAY failed");
    }
    let keepalive = socket2::TcpKeepalive::new().with_time(KEEPALIVE_TIME);
    #[cfg(target_os = "linux")]
    let keepalive = keepalive
        .with_interval(KEEPALIVE_INTERVAL)
        .with_retries(KEEPALIVE_PROBES);
    if let Err(error) = socket.set_tcp_keepalive(&keepalive) {
        tracing::debug!(target: "noctis::http", %error, "TCP keepalive failed");
    }
    #[cfg(target_os = "linux")]
    if let Err(error) = socket.set_tcp_user_timeout(Some(USER_TIMEOUT)) {
        tracing::debug!(target: "noctis::http", %error, "TCP_USER_TIMEOUT failed");
    }
}

#[cfg(test)]
mod tests;
