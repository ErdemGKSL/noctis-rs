//! Story 5.9 Matrix 20: the redial back-off.

use super::*;

/// Matrix 20: 0, 0.5, 1, 2, 5, 5 s, each within the jitter band, and a
/// reset after a success (a new `Backoff`).
#[test]
fn backoff_steps_and_jitter() {
    let bands = [
        (0, 0),
        (400, 600),
        (800, 1200),
        (1600, 2400),
        (4000, 6000),
        (4000, 6000),
    ];
    for jitter in [0.8, 1.0, 1.2] {
        let mut backoff = Backoff::default();
        for (low, high) in bands {
            let delay = backoff.next_delay(jitter);
            assert!(
                (Duration::from_millis(low)..=Duration::from_millis(high)).contains(&delay),
                "{delay:?} not in {low}..={high} ms"
            );
        }
        let mut reset = Backoff::default();
        assert_eq!(reset.next_delay(jitter), Duration::ZERO);
    }
}

#[test]
fn jitter_band() {
    for _ in 0..1000 {
        let j = jitter();
        assert!((0.8..=1.2).contains(&j), "{j}");
    }
}

/// An HTTP/1.1 sender over an in-memory pipe with its connection future,
/// left undriven (the far end is kept open by the returned stream).
async fn h1_sender() -> (
    http1::SendRequest<Full<Bytes>>,
    http1::Connection<hyper_util::rt::TokioIo<tokio::io::DuplexStream>, Full<Bytes>>,
    tokio::io::DuplexStream,
) {
    let (near, far) = tokio::io::duplex(1024);
    let (sender, conn) = http1::handshake(hyper_util::rt::TokioIo::new(near))
        .await
        .unwrap();
    (sender, conn, far)
}

fn pool() -> Pool {
    let config = crate::edge::HttpConfig::default();
    let dialer = Dialer {
        upstream: crate::edge::Upstream::http("127.0.0.1:1", false),
        dns: crate::edge::dns::DnsCache::new(
            Arc::new(crate::edge::dns::SystemResolver::default()),
            config.dns_ttl_floor,
            config.dns_ttl_cap,
        ),
        config,
        metrics: EdgeMetrics::new(Arc::from("default")),
    };
    Pool::new(dialer, CancellationToken::new())
}

/// `ready` returns the ready sender, drops the closed one and keeps the
/// open-but-not-ready ones pooled in their original order.
#[tokio::test]
async fn ready_keeps_pending_senders() {
    let (mut ready, conn, _ready_far) = h1_sender().await;
    tokio::spawn(conn);
    ready.ready().await.unwrap();
    let (closed, conn, _closed_far) = h1_sender().await;
    drop(conn);
    assert!(closed.is_closed());
    let (first, first_conn, _first_far) = h1_sender().await;
    let (second, _second_conn, _second_far) = h1_sender().await;
    assert!(!first.is_ready() && !first.is_closed());
    let pool = pool();
    pool.lock().idle = vec![ready, first, closed, second];

    let Some(Sender::H1(found)) = pool.ready() else {
        panic!("expected the ready HTTP/1.1 sender");
    };
    assert!(found.is_ready());
    assert_eq!(pool.idle_len(), 2);
    // Closing `first` marks the bottom of the stack: the order held.
    drop(first_conn);
    let state = pool.lock();
    assert!(state.idle[0].is_closed());
    assert!(!state.idle[1].is_closed());
}
