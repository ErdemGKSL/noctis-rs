//! Single-flight on plain futures (FR-36): joining, the leader dropped,
//! every caller dropped, the removal guard and error sharing.

use std::sync::atomic::AtomicUsize;

use bytes::Bytes;
use noctis_ports::{EncodeSettings, OutputFormat, RenderCap, Size};
use tokio::sync::oneshot;

use super::*;

fn key(byte: u8) -> RenderKey {
    RenderKey([byte; 32])
}

fn output() -> RenderOutput {
    RenderOutput {
        bytes: Bytes::from_static(b"png"),
        digest: [1; 32],
        format: OutputFormat::Png,
        settings_used: EncodeSettings {
            size: Size::new(1, 1),
            quality: None,
            fps: None,
            frames: 1,
        },
    }
}

/// A body that waits for `rx`, counting its starts in `started`; it owns
/// `guard` and reports its drop on `dropped`.
fn body(
    guard: FlightGuard,
    rx: oneshot::Receiver<FlightResult>,
    started: &Arc<AtomicUsize>,
    dropped: oneshot::Sender<()>,
) -> FlightBody {
    struct OnDrop(Option<oneshot::Sender<()>>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }
    let started = Arc::clone(started);
    Box::pin(async move {
        let _guard = guard;
        let _on_drop = OnDrop(Some(dropped));
        started.fetch_add(1, Ordering::SeqCst);
        rx.await
            .unwrap_or_else(|_| Err(Arc::new(RenderError::failed("sender gone"))))
    })
}

#[tokio::test]
async fn fr_36_identical_keys_join_one_flight_and_the_entry_goes_on_completion() {
    let flights = Flights::default();
    let started = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = oneshot::channel();
    let (dropped_tx, dropped) = oneshot::channel();
    let a = flights.join(key(1), |guard| body(guard, rx, &started, dropped_tx));
    let b = flights.join(key(1), |_| unreachable!("joined"));
    assert_eq!(flights.len(), 1);
    tx.send(Ok(output())).unwrap();
    let (a, b) = tokio::join!(a, b);
    assert_eq!(a.unwrap(), output());
    assert_eq!(b.unwrap(), output());
    assert_eq!(started.load(Ordering::SeqCst), 1);
    let _ = dropped.await;
    assert_eq!(flights.len(), 0, "the guard removed the entry");
}

#[tokio::test]
async fn fr_36_the_leader_dropped_leaves_the_joiner_its_result() {
    let flights = Flights::default();
    let started = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = oneshot::channel();
    let (dropped_tx, _dropped) = oneshot::channel();
    let a = flights.join(key(2), |guard| body(guard, rx, &started, dropped_tx));
    let b = flights.join(key(2), |_| unreachable!("joined"));
    drop(a);
    assert_eq!(flights.len(), 1);
    tx.send(Ok(output())).unwrap();
    assert_eq!(b.await.unwrap(), output());
    assert_eq!(started.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn fr_36_every_caller_dropped_drops_the_flight() {
    let flights = Flights::default();
    let started = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = oneshot::channel::<FlightResult>();
    let (dropped_tx, dropped) = oneshot::channel();
    let a = flights.join(key(3), |guard| body(guard, rx, &started, dropped_tx));
    let b = flights.join(key(3), |_| unreachable!("joined"));
    drop(a);
    drop(b);
    let _ = dropped.await;
    assert!(tx.is_closed(), "the body was dropped");
    assert_eq!(flights.len(), 0);
    assert_eq!(started.load(Ordering::SeqCst), 0, "never polled");
    // A later request starts a new flight.
    let (tx, rx) = oneshot::channel();
    let (dropped_tx, _dropped) = oneshot::channel();
    let c = flights.join(key(3), |guard| body(guard, rx, &started, dropped_tx));
    tx.send(Ok(output())).unwrap();
    assert_eq!(c.await.unwrap(), output());
}

#[tokio::test]
async fn fr_36_an_old_guard_never_removes_a_newer_flight() {
    let flights = Flights::default();
    let started = Arc::new(AtomicUsize::new(0));
    let (_tx, rx) = oneshot::channel::<FlightResult>();
    let (dropped_tx, _dropped) = oneshot::channel();
    let old = FlightGuard {
        map: Arc::clone(&flights.map),
        key: key(4),
        generation: u64::MAX,
    };
    let live = flights.join(key(4), |guard| body(guard, rx, &started, dropped_tx));
    drop(old);
    assert_eq!(flights.len(), 1);
    drop(live);
    assert_eq!(flights.len(), 0);
}

#[tokio::test]
async fn fr_36_errors_are_shared_and_the_last_waiter_unwraps_its_own() {
    let flights = Flights::default();
    let started = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = oneshot::channel();
    let (dropped_tx, _dropped) = oneshot::channel();
    let a = flights.join(key(5), |guard| body(guard, rx, &started, dropped_tx));
    let b = flights.join(key(5), |_| unreachable!("joined"));
    let io = std::io::Error::other("disk on fire");
    tx.send(Err(Arc::new(RenderError::failed(io)))).unwrap();
    let first = unshare(a.await);
    let last = unshare(b.await);
    let (Err(RenderError::Failed(first)), Err(RenderError::Failed(last))) = (first, last) else {
        panic!("both fail");
    };
    assert_eq!(first.to_string(), last.to_string());
    let sources = [&first, &last].map(|e| e.downcast_ref::<std::io::Error>().is_some());
    assert_eq!(sources, [false, true], "the last waiter keeps the source");
}

#[test]
fn fr_36_share_error_copies_every_variant() {
    let cases = [
        RenderError::UnknownTemplate {
            id: "t".into(),
            version: 2,
        },
        RenderError::Unsupported {
            id: "t".into(),
            why: "why".into(),
        },
        RenderError::OverBudget {
            budget: 1,
            smallest: 2,
        },
        RenderError::OverCap {
            id: "t".into(),
            cap: RenderCap::Fps,
            limit: 50,
            actual: 60,
        },
        RenderError::SizeMismatch {
            id: "t".into(),
            frame: 1,
            expected: Size::new(2, 2),
            declared: (1, 2),
        },
        RenderError::QueueFull,
        RenderError::NoRenderer,
        RenderError::Panicked {
            message: "boom".into(),
        },
        RenderError::NoFont,
        RenderError::UnknownImage {
            id: "t".into(),
            image: "logo".into(),
        },
        RenderError::RemoteImage {
            id: "t".into(),
            image: "a".into(),
            reason: "HTTP 404".into(),
        },
        RenderError::UndeclaredImage {
            id: "t".into(),
            image: "a".into(),
        },
        RenderError::failed("x"),
    ];
    for err in cases {
        let copy = share_error(&err);
        assert_eq!(format!("{copy:?}"), format!("{err:?}"));
        assert_eq!(copy.to_string(), err.to_string());
    }
}
