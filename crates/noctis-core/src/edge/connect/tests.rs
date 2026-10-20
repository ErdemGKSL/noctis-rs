//! Story 5.9: the dial's address fallback and per-address bound, and the
//! upstream authority helpers.

use std::net::Ipv4Addr;

use futures_util::FutureExt;
use futures_util::future::BoxFuture;

use super::*;
use crate::edge::dns::{Resolve, Resolved};
use crate::edge::test_support::TestServer;

/// A resolver that always answers `addrs`.
struct Fixed(Vec<IpAddr>);

impl Resolve for Fixed {
    fn resolve(&self, _host: &str) -> BoxFuture<'_, io::Result<Resolved>> {
        let addrs = self.0.clone();
        async move {
            Ok(Resolved {
                addrs,
                ttl: Duration::from_mins(1),
            })
        }
        .boxed()
    }
}

fn dialer(addrs: Vec<IpAddr>, port: u16, connect_timeout: Duration) -> Dialer {
    let config = HttpConfig {
        connect_timeout,
        ..HttpConfig::default()
    };
    Dialer {
        upstream: Upstream::Http {
            host: "upstream.test".to_owned(),
            port,
            h2c: false,
        },
        dns: DnsCache::new(
            Arc::new(Fixed(addrs)),
            config.dns_ttl_floor,
            config.dns_ttl_cap,
        ),
        config,
        metrics: EdgeMetrics::new(Arc::from("default")),
    }
}

fn ip(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, last))
}

/// A refused first address falls through to the live second one.
#[tokio::test]
async fn refused_address_falls_back() {
    let server = TestServer::start(false).await;
    let dialer = dialer(
        vec![ip(3), ip(1)],
        server.addr.port(),
        Duration::from_secs(5),
    );
    let conn = dialer.dial().await.expect("the second address answers");
    assert!(!conn.sender.is_h2());
    tokio::spawn(conn.driver.run());
    timeout(Duration::from_secs(5), async {
        while server.conns.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the live listener accepted");
}

/// A first address that never answers is abandoned after `connect_timeout`.
#[tokio::test]
async fn silent_address_is_abandoned_after_connect_timeout() {
    let server = TestServer::start(false).await;
    let port = server.addr.port();
    // A listener whose accept queue is full: the kernel drops further SYNs,
    // so a connect to it hangs.
    let silent = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).unwrap();
    silent.bind(&SocketAddr::new(ip(2), port).into()).unwrap();
    silent.listen(0).unwrap();
    let mut held = Vec::new();
    loop {
        match timeout(
            Duration::from_millis(200),
            TcpStream::connect(SocketAddr::new(ip(2), port)),
        )
        .await
        {
            Ok(stream) => held.push(stream.unwrap()),
            Err(_) => break,
        }
        assert!(held.len() < 16, "the accept queue never filled");
    }
    let connect_timeout = Duration::from_millis(300);
    let dialer = dialer(vec![ip(2), ip(1)], port, connect_timeout);
    let started = Instant::now();
    let conn = dialer.dial().await.expect("the second address answers");
    let elapsed = started.elapsed();
    assert!(elapsed >= connect_timeout, "{elapsed:?}");
    assert!(elapsed < connect_timeout * 4, "{elapsed:?}");
    assert!(!conn.sender.is_h2());
}

#[test]
fn split_authority_rows() {
    for (authority, expected) in [
        ("proxy", ("proxy", None)),
        ("proxy:3000", ("proxy", Some(3000))),
        ("proxy:", ("proxy", None)),
        ("proxy:http", ("proxy", None)),
        ("proxy:70000", ("proxy", None)),
        ("[::1]", ("::1", None)),
        ("[::1]:8080", ("::1", Some(8080))),
        ("[::1]:x", ("::1", None)),
        ("[::1", ("::1", None)),
        ("127.0.0.1:1", ("127.0.0.1", Some(1))),
    ] {
        assert_eq!(split_authority(authority), expected, "{authority}");
    }
}

#[test]
fn authority_rows() {
    crate::startup::ensure_crypto_provider().unwrap();
    let tls = |host: &str, port| {
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        Upstream::tls(
            host,
            port,
            ServerName::try_from("localhost").unwrap(),
            config,
        )
    };
    let rows = [
        (Upstream::http("proxy", false), "proxy"),
        (Upstream::http("proxy:80", false), "proxy"),
        (Upstream::http("proxy:443", false), "proxy:443"),
        (Upstream::http("[::1]", true), "[::1]"),
        (Upstream::http("[::1]:80", true), "[::1]"),
        (Upstream::http("[::1]:3000", false), "[::1]:3000"),
        (tls("discord.com", 443), "discord.com"),
        (tls("discord.com", 80), "discord.com:80"),
        (tls("::1", 443), "[::1]"),
        (tls("::1", 8443), "[::1]:8443"),
    ];
    for (upstream, expected) in rows {
        assert_eq!(upstream.authority(), expected);
    }
}
