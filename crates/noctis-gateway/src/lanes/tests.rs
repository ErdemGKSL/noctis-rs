#![allow(clippy::float_cmp, reason = "gauges hold exact small integer depths")]

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use metrics_util::MetricKind;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use serde_json::json;
use tokio::time::Instant;
use twilight_gateway::{Event, ShardId};
use twilight_model::gateway::payload::incoming::{MessageDelete, PresenceUpdate, TypingStart};
use twilight_model::id::Id;

use super::*;

const SENTINEL: &str = "sentinel-payload-7f3a";

fn item(kind: InboundKind) -> Inbound {
    Inbound {
        shard: ShardId::ONE,
        received_at: Instant::now(),
        kind,
    }
}

/// A non-droppable `MESSAGE_DELETE` tagged `n`.
fn m(n: u64) -> Inbound {
    item(InboundKind::Event(Box::new(Event::MessageDelete(
        MessageDelete {
            channel_id: Id::new(1),
            guild_id: None,
            id: Id::new(n),
        },
    ))))
}

/// A `TYPING_START` tagged `n`.
fn t(n: u64) -> Inbound {
    let typing: TypingStart = serde_json::from_value(json!({
        "channel_id": "1",
        "timestamp": 0,
        "user_id": n.to_string(),
    }))
    .unwrap();
    item(InboundKind::Event(Box::new(Event::TypingStart(Box::new(
        typing,
    )))))
}

/// A `PRESENCE_UPDATE` tagged `n`.
fn p(n: u64) -> Inbound {
    let presence = serde_json::from_value(json!({
        "user": { "id": n.to_string() },
        "guild_id": "1",
        "status": "online",
        "activities": [],
        "client_status": {},
    }))
    .unwrap();
    item(InboundKind::Event(Box::new(Event::PresenceUpdate(
        Box::new(PresenceUpdate(presence)),
    ))))
}

/// An interaction tagged `n`, whose token is the payload sentinel.
fn i(n: u64) -> Inbound {
    let d = json!({
        "id": n.to_string(),
        "application_id": "1",
        "type": 1,
        "token": SENTINEL,
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
    });
    let raw = crate::RawJson::new(bytes::Bytes::from(d.to_string()));
    item(InboundKind::Interaction(
        Box::new(serde_json::from_value(d).unwrap()),
        raw,
    ))
}

/// `M1`, `T2`, `P3`, `I4`, or the event type name.
fn tag(inbound: &Inbound) -> String {
    match &inbound.kind {
        InboundKind::Interaction(interaction, _) => format!("I{}", interaction.id),
        InboundKind::Event(event) => match &**event {
            Event::MessageDelete(delete) => format!("M{}", delete.id),
            Event::TypingStart(typing) => format!("T{}", typing.user_id),
            Event::PresenceUpdate(presence) => format!("P{}", presence.0.user.id()),
            other => format!("{:?}", other.kind()),
        },
    }
}

fn config(event_capacity: usize, overflow_capacity: usize) -> LaneConfig {
    LaneConfig {
        app: Arc::from("test"),
        event_capacity: NonZeroUsize::new(event_capacity).unwrap(),
        overflow_capacity,
        droppable: DROPPABLE_EVENTS,
        drop_observer: None,
    }
}

fn push_all(sender: &LaneSender, items: impl IntoIterator<Item = Inbound>) {
    for inbound in items {
        sender.push(inbound);
    }
}

fn drain(receiver: &mut LaneReceiver) -> Vec<String> {
    std::iter::from_fn(|| receiver.try_recv())
        .map(|inbound| tag(&inbound))
        .collect()
}

/// The queued tags of the event lane, by popping a clone-free view.
fn event_tags(receiver: &LaneReceiver) -> Vec<String> {
    receiver.shared.lock().event.iter().map(tag).collect()
}

fn overflow_tags(receiver: &LaneReceiver) -> Vec<String> {
    receiver.shared.lock().overflow.iter().map(tag).collect()
}

fn depths(interaction: usize, event: usize, overflow: usize) -> LaneDepths {
    LaneDepths {
        interaction,
        event,
        overflow,
    }
}

/// One metric in a snapshot: its kind, name and sorted labels.
type MetricId = (MetricKind, String, Vec<(String, String)>);

/// Takes a snapshot (which resets every value it reads).
fn snapshot(snapshotter: &Snapshotter) -> HashMap<MetricId, DebugValue> {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .map(|(key, _, _, value)| {
            let mut labels: Vec<_> = key
                .key()
                .labels()
                .map(|label| (label.key().to_owned(), label.value().to_owned()))
                .collect();
            labels.sort();
            ((key.kind(), key.key().name().to_owned(), labels), value)
        })
        .collect()
}

fn labels(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut labels: Vec<_> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    labels.sort();
    labels
}

fn dropped(snapshot: &HashMap<MetricId, DebugValue>, app: &str, event_type: &str) -> Option<u64> {
    let id = (
        MetricKind::Counter,
        "noctis_events_dropped_total".to_owned(),
        labels(&[("app", app), ("event_type", event_type)]),
    );
    match snapshot.get(&id) {
        Some(DebugValue::Counter(n)) => Some(*n),
        _ => None,
    }
}

fn total_dropped(snapshot: &HashMap<MetricId, DebugValue>) -> u64 {
    snapshot
        .iter()
        .filter(|((kind, name, _), _)| {
            *kind == MetricKind::Counter && name == "noctis_events_dropped_total"
        })
        .map(|(_, value)| match value {
            DebugValue::Counter(n) => *n,
            _ => 0,
        })
        .sum()
}

/// The gauges `(interaction, event, overflow)` of `app`.
fn gauges(snapshot: &HashMap<MetricId, DebugValue>, app: &str) -> (f64, f64, f64) {
    let read = |name: &str, pairs: &[(&str, &str)]| match snapshot.get(&(
        MetricKind::Gauge,
        name.to_owned(),
        labels(pairs),
    )) {
        Some(DebugValue::Gauge(value)) => value.0,
        other => panic!("gauge {name} {pairs:?} missing: {other:?}"),
    };
    (
        read(
            "noctis_lane_depth",
            &[("app", app), ("lane", "interaction")],
        ),
        read("noctis_lane_depth", &[("app", app), ("lane", "event")]),
        read("noctis_event_overflow_depth", &[("app", app)]),
    )
}

/// A cloneable in-memory log sink.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Logs {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    fn errors(&self) -> Vec<String> {
        self.text()
            .lines()
            .filter(|line| line.contains("ERROR"))
            .map(str::to_owned)
            .collect()
    }
}

/// Runs `f` with a metrics recorder and a TRACE log capture on this thread.
fn observed<T>(f: impl FnOnce(&Snapshotter) -> T) -> (T, Logs) {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let out = tracing::subscriber::with_default(subscriber, || {
        // A callsite first hit on another test thread while no subscriber
        // was alive may have cached `Interest::never`; recompute it now
        // that this one is.
        tracing::callsite::rebuild_interest_cache();
        metrics::with_local_recorder(&recorder, || f(&snapshotter))
    });
    (out, logs)
}

#[test]
fn row01_empty() {
    observed(|metrics| {
        let (_sender, mut receiver) = lanes(config(3, 2));
        assert!(receiver.try_recv().is_none());
        assert_eq!(receiver.depths(), LaneDepths::default());
        let snap = snapshot(metrics);
        assert_eq!(gauges(&snap, "test"), (0.0, 0.0, 0.0));
        assert_eq!(snap.len(), 3, "{snap:?}");
    });
}

#[test]
fn row02_fifo() {
    observed(|_| {
        let (sender, mut receiver) = lanes(config(3, 2));
        push_all(&sender, [m(1), m(2), m(3)]);
        assert_eq!(receiver.depths(), depths(0, 3, 0));
        assert_eq!(drain(&mut receiver), ["M1", "M2", "M3"]);
    });
}

#[test]
fn row03_priority() {
    observed(|_| {
        let (sender, mut receiver) = lanes(config(8, 2));
        push_all(&sender, [m(1), m(2), i(1), m(3), i(2)]);
        assert_eq!(drain(&mut receiver), ["I1", "I2", "M1", "M2", "M3"]);
    });
}

#[test]
fn row04_evict_oldest_droppable() {
    observed(|metrics| {
        let (sender, receiver) = lanes(config(3, 2));
        push_all(&sender, [t(1), m(1), p(1), t(2)]);
        assert_eq!(event_tags(&receiver), ["M1", "P1", "T2"]);
        let snap = snapshot(metrics);
        assert_eq!(dropped(&snap, "test", "TYPING_START"), Some(1));
        assert_eq!(total_dropped(&snap), 1);
        assert_eq!(receiver.depths().overflow, 0);
    });
}

#[test]
fn row05_evict_a_different_type() {
    observed(|metrics| {
        let (sender, receiver) = lanes(config(3, 2));
        push_all(&sender, [m(1), p(1), t(1), t(2)]);
        assert_eq!(event_tags(&receiver), ["M1", "T1", "T2"]);
        let snap = snapshot(metrics);
        assert_eq!(dropped(&snap, "test", "PRESENCE_UPDATE"), Some(1));
        assert_eq!(total_dropped(&snap), 1);
    });
}

#[test]
fn row06_no_droppable_queued() {
    let ((), logs) = observed(|metrics| {
        let (sender, receiver) = lanes(config(3, 2));
        push_all(&sender, [m(1), m(2), m(3), t(1)]);
        assert_eq!(event_tags(&receiver), ["M1", "M2", "M3"]);
        assert_eq!(receiver.depths(), depths(0, 3, 0));
        let snap = snapshot(metrics);
        assert_eq!(dropped(&snap, "test", "TYPING_START"), Some(1));
        assert_eq!(total_dropped(&snap), 1);
    });
    assert!(logs.errors().is_empty(), "{}", logs.text());
    assert!(logs.text().contains("TYPING_START"), "{}", logs.text());
}

/// Rows 7 and 8: overflow, its hard cap, then FIFO refill.
#[test]
fn row07_08_overflow_cap_and_refill() {
    let ((), logs) = observed(|metrics| {
        let (sender, mut receiver) = lanes(config(3, 2));
        push_all(&sender, [m(1), m(2), m(3), m(4), m(5)]);
        assert_eq!(overflow_tags(&receiver), ["M4", "M5"]);
        assert_eq!(gauges(&snapshot(metrics), "test").2, 2.0);
        sender.push(m(6));
        assert_eq!(overflow_tags(&receiver), ["M4", "M5"]);
        let snap = snapshot(metrics);
        assert_eq!(dropped(&snap, "test", "MESSAGE_DELETE"), Some(1));
        assert_eq!(total_dropped(&snap), 1);

        let first = receiver.try_recv().unwrap();
        assert_eq!(tag(&first), "M1");
        assert_eq!(receiver.depths(), depths(0, 3, 1));
        assert_eq!(event_tags(&receiver), ["M2", "M3", "M4"]);
        assert_eq!(drain(&mut receiver), ["M2", "M3", "M4", "M5"]);
        assert_eq!(receiver.depths(), LaneDepths::default());
    });
    let errors = logs.errors();
    assert_eq!(errors.len(), 1, "{}", logs.text());
    assert!(
        errors[0].contains("event_type=\"MESSAGE_DELETE\"")
            || errors[0].contains("event_type=MESSAGE_DELETE"),
        "{}",
        errors[0]
    );
    assert!(errors[0].contains("overflow_capacity=2"), "{}", errors[0]);
    assert!(!logs.text().contains(SENTINEL));
}

#[test]
fn row09_droppable_while_overflow_non_empty() {
    observed(|_| {
        let (sender, mut receiver) = lanes(config(2, 2));
        push_all(&sender, [t(1), m(1), m(2)]);
        assert_eq!(overflow_tags(&receiver), ["M2"]);
        sender.push(t(2));
        assert_eq!(event_tags(&receiver), ["M1", "T2"]);
        assert_eq!(overflow_tags(&receiver), ["M2"]);
        assert_eq!(drain(&mut receiver), ["M1", "T2", "M2"]);
    });
}

#[test]
fn row10_non_droppables_never_evict() {
    observed(|metrics| {
        let (sender, receiver) = lanes(config(2, 1));
        push_all(&sender, [t(1), t(2), m(1)]);
        assert_eq!(event_tags(&receiver), ["T1", "T2"]);
        assert_eq!(overflow_tags(&receiver), ["M1"]);
        assert_eq!(total_dropped(&snapshot(metrics)), 0);
    });
}

#[test]
fn row11_zero_overflow() {
    let ((), logs) = observed(|metrics| {
        let (sender, receiver) = lanes(config(1, 0));
        push_all(&sender, [m(1), m(2)]);
        assert_eq!(event_tags(&receiver), ["M1"]);
        assert_eq!(receiver.depths(), depths(0, 1, 0));
        assert_eq!(
            dropped(&snapshot(metrics), "test", "MESSAGE_DELETE"),
            Some(1)
        );
    });
    assert_eq!(logs.errors().len(), 1, "{}", logs.text());
}

#[test]
fn row12_capacity_one() {
    observed(|_| {
        let (sender, receiver) = lanes(config(1, 2));
        push_all(&sender, [m(1), m(2)]);
        assert_eq!(event_tags(&receiver), ["M1"]);
        assert_eq!(overflow_tags(&receiver), ["M2"]);
    });
}

#[test]
fn row13_empty_droppable_set() {
    let ((), logs) = observed(|metrics| {
        let config = LaneConfig {
            droppable: EventTypeFlags::empty(),
            ..config(1, 1)
        };
        let (sender, receiver) = lanes(config);
        push_all(&sender, [t(1), t(2), t(3)]);
        assert_eq!(event_tags(&receiver), ["T1"]);
        assert_eq!(overflow_tags(&receiver), ["T2"]);
        let snap = snapshot(metrics);
        assert_eq!(dropped(&snap, "test", "TYPING_START"), Some(1));
        assert_eq!(total_dropped(&snap), 1);
    });
    assert_eq!(logs.errors().len(), 1, "{}", logs.text());
}

#[test]
fn row14_custom_droppable() {
    observed(|metrics| {
        let config = LaneConfig {
            droppable: EventTypeFlags::MESSAGE_DELETE,
            ..config(1, 1)
        };
        let (sender, receiver) = lanes(config);
        push_all(&sender, [m(1), m(2)]);
        assert_eq!(event_tags(&receiver), ["M2"]);
        assert_eq!(overflow_tags(&receiver), Vec::<String>::new());
        assert_eq!(
            dropped(&snapshot(metrics), "test", "MESSAGE_DELETE"),
            Some(1)
        );
    });
}

#[test]
fn row15_non_dispatch_kind() {
    observed(|metrics| {
        let (sender, receiver) = lanes(config(1, 0));
        sender.push(m(1));
        sender.push(item(InboundKind::Event(Box::new(
            Event::GatewayHeartbeatAck,
        ))));
        assert_eq!(event_tags(&receiver), ["M1"]);
        let snap = snapshot(metrics);
        assert_eq!(dropped(&snap, "test", "UNKNOWN"), Some(1));
        assert_eq!(total_dropped(&snap), 1);
    });
}

#[test]
fn row16_interactions_never_dropped() {
    observed(|metrics| {
        let (sender, mut receiver) = lanes(config(1, 1));
        push_all(&sender, [m(1), m(2)]);
        for n in 1..=10_000 {
            sender.push(i(n));
        }
        assert_eq!(receiver.depths(), depths(10_000, 1, 1));
        assert_eq!(total_dropped(&snapshot(metrics)), 0);
        let popped = drain(&mut receiver);
        assert_eq!(popped.len(), 10_002);
        assert!(popped[..10_000].iter().all(|tag| tag.starts_with('I')));
        assert_eq!(popped[10_000..], ["M1", "M2"]);
    });
}

#[tokio::test(flavor = "current_thread")]
async fn row17_close_with_items() {
    let (sender, mut receiver) = lanes(config(3, 2));
    push_all(&sender, [i(1), m(1)]);
    drop(sender);
    assert_eq!(tag(&receiver.recv().await.unwrap()), "I1");
    assert_eq!(tag(&receiver.recv().await.unwrap()), "M1");
    assert!(receiver.recv().await.is_none());
    assert!(receiver.try_recv().is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn row18_close_while_waiting() {
    let (sender, mut receiver) = lanes(config(3, 2));
    let clone = sender.clone();
    let waiting = tokio::spawn(async move { receiver.recv().await.is_none() });
    tokio::task::yield_now().await;
    drop(sender);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished());
    drop(clone);
    let closed = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(closed);
}

#[tokio::test(flavor = "current_thread")]
async fn row19_wake() {
    let (sender, mut receiver) = lanes(config(3, 2));
    let waiting = tokio::spawn(async move { receiver.recv().await.map(|inbound| tag(&inbound)) });
    tokio::task::yield_now().await;
    sender.push(m(1));
    let got = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.as_deref(), Some("M1"));
}

#[test]
fn row20_receiver_gone() {
    observed(|metrics| {
        let (sender, receiver) = lanes(config(1, 0));
        drop(receiver);
        let before = snapshot(metrics);
        assert_eq!(gauges(&before, "test"), (0.0, 0.0, 0.0));
        push_all(&sender, [i(1), m(1), m(2)]);
        let after = snapshot(metrics);
        assert_eq!(total_dropped(&after), 0);
        assert_eq!(gauges(&after, "test"), (0.0, 0.0, 0.0));
        assert_eq!(sender.shared.depths(), LaneDepths::default());
    });
}

#[test]
fn row21_gauges_track() {
    observed(|metrics| {
        let (sender, mut receiver) = lanes(config(1, 1));
        push_all(&sender, [i(1), m(1), m(2)]);
        assert_eq!(gauges(&snapshot(metrics), "test"), (1.0, 1.0, 1.0));
        assert_eq!(tag(&receiver.try_recv().unwrap()), "I1");
        assert_eq!(gauges(&snapshot(metrics), "test"), (0.0, 1.0, 1.0));
        assert_eq!(tag(&receiver.try_recv().unwrap()), "M1");
        assert_eq!(gauges(&snapshot(metrics), "test"), (0.0, 1.0, 0.0));
    });
}

#[test]
fn row22_unicode_label() {
    observed(|metrics| {
        let config = LaneConfig {
            app: Arc::from("bot-ü🙂"),
            ..config(1, 0)
        };
        let (sender, _receiver) = lanes(config);
        push_all(&sender, [m(1), m(2)]);
        let snap = snapshot(metrics);
        assert_eq!(gauges(&snap, "bot-ü🙂"), (0.0, 1.0, 0.0));
        assert_eq!(dropped(&snap, "bot-ü🙂", "MESSAGE_DELETE"), Some(1));
    });
}

#[test]
fn row23_concurrency() {
    const THREADS: u64 = 8;
    const PER_THREAD: u64 = 1000;
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let (sender, mut receiver) = metrics::with_local_recorder(&recorder, || lanes(config(64, 64)));
    let pushed_interactions = AtomicUsize::new(0);
    let done = AtomicBool::new(false);
    let started = std::time::Instant::now();
    let (popped, popped_interactions) = std::thread::scope(|scope| {
        let consumer = scope.spawn(|| {
            let mut popped = 0_u64;
            let mut interactions = 0_usize;
            loop {
                assert!(started.elapsed() < Duration::from_secs(10), "deadlock");
                match receiver.try_recv() {
                    Some(inbound) => {
                        popped += 1;
                        if matches!(inbound.kind, InboundKind::Interaction(..)) {
                            interactions += 1;
                        }
                    }
                    None if done.load(Ordering::SeqCst) => {
                        if receiver.depths() == LaneDepths::default() {
                            break (popped, interactions);
                        }
                    }
                    None => std::thread::yield_now(),
                }
            }
        });
        let pushers: Vec<_> = (0..THREADS)
            .map(|thread| {
                let sender = sender.clone();
                let recorder = &recorder;
                let pushed_interactions = &pushed_interactions;
                scope.spawn(move || {
                    metrics::with_local_recorder(recorder, || {
                        for n in 0..PER_THREAD {
                            let id = thread * PER_THREAD + n + 1;
                            match n % 3 {
                                0 => {
                                    sender.push(i(id));
                                    pushed_interactions.fetch_add(1, Ordering::SeqCst);
                                }
                                1 => sender.push(m(id)),
                                _ => sender.push(t(id)),
                            }
                        }
                    });
                })
            })
            .collect();
        for pusher in pushers {
            pusher.join().unwrap();
        }
        done.store(true, Ordering::SeqCst);
        consumer.join().unwrap()
    });
    let dropped = total_dropped(&snapshot(&snapshotter));
    assert_eq!(popped + dropped, THREADS * PER_THREAD);
    assert_eq!(
        popped_interactions,
        pushed_interactions.load(Ordering::SeqCst)
    );
    drop(sender);
}

#[tokio::test(start_paused = true)]
async fn row24_interaction_deadline_under_full_lanes() {
    let (sender, mut receiver) = lanes(config(3, 2));
    push_all(&sender, (1..=5).map(m));
    assert_eq!(receiver.depths(), depths(0, 3, 2));
    sender.push(i(1));
    let m_ended = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut started_tx = Some(started_tx);
    let flag = Arc::clone(&m_ended);
    let consumer = tokio::spawn(async move {
        while let Some(inbound) = receiver.recv().await {
            if matches!(inbound.kind, InboundKind::Interaction(..)) {
                let started_tx = started_tx.take();
                let flag = Arc::clone(&flag);
                tokio::spawn(async move {
                    let waited = inbound.received_at.elapsed();
                    let any_ended = flag.load(Ordering::SeqCst);
                    if let Some(tx) = started_tx {
                        let _ = tx.send((waited, any_ended));
                    }
                });
            } else {
                let flag = Arc::clone(&flag);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    flag.store(true, Ordering::SeqCst);
                    drop(inbound);
                });
            }
        }
    });
    let (waited, any_ended) = started_rx.await.unwrap();
    assert!(waited <= Duration::from_millis(2500), "{waited:?}");
    assert!(!any_ended);
    drop(sender);
    consumer.await.unwrap();
}

#[test]
fn row25_debug_redaction() {
    let (sender, receiver) = lanes(config(3, 2));
    sender.push(i(1));
    let sender_debug = format!("{sender:?}");
    let receiver_debug = format!("{receiver:?}");
    for debug in [&sender_debug, &receiver_debug] {
        assert!(!debug.contains(SENTINEL), "{debug}");
        assert!(debug.contains("interaction: 1"), "{debug}");
    }
    assert!(sender_debug.starts_with("LaneSender"));
    assert!(receiver_debug.starts_with("LaneReceiver"));
}

/// Story 5.6 Matrix 39: `recv_interaction` takes only interactions.
#[tokio::test(flavor = "current_thread")]
async fn recv_interaction_skips_events() {
    let (sender, mut receiver) = lanes(config(3, 2));
    push_all(&sender, [m(1), i(1)]);
    let got = receiver.recv_interaction().await.unwrap();
    assert_eq!(tag(&got), "I1");
    assert_eq!(event_tags(&receiver), ["M1"]);
    drop(sender);
}

/// Story 5.6 Matrix 39: closed with only events queued returns `None`.
#[tokio::test(flavor = "current_thread")]
async fn recv_interaction_closed_with_events() {
    let (sender, mut receiver) = lanes(config(3, 2));
    sender.push(m(1));
    drop(sender);
    assert!(receiver.recv_interaction().await.is_none());
    assert_eq!(tag(&receiver.recv().await.unwrap()), "M1");
    assert!(receiver.recv().await.is_none());
}

/// Story 5.6 Matrix 39: a pending call wakes for an interaction and stays
/// pending for an event, which `recv` still pops.
#[tokio::test(flavor = "current_thread")]
async fn recv_interaction_pending() {
    let (sender, mut receiver) = lanes(config(3, 2));
    {
        let waiting = receiver.recv_interaction();
        tokio::pin!(waiting);
        assert!(futures_util::poll!(waiting.as_mut()).is_pending());
        sender.push(m(1));
        tokio::task::yield_now().await;
        assert!(futures_util::poll!(waiting.as_mut()).is_pending());
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(futures_util::poll!(waiting.as_mut()).is_pending());
        sender.push(i(1));
        let got = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tag(&got), "I1");
    }
    let got = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tag(&got), "M1");
    drop(sender);
}

/// Records every `overflow_dropped` call as `(shard, tag)`.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<(u32, String, Option<u64>)>>,
}

impl DropObserver for Recorder {
    fn overflow_dropped(&self, shard: u32, event: &Event) {
        let tag = tag(&item(InboundKind::Event(Box::new(event.clone()))));
        let guild = event.guild_id().map(Id::get);
        self.calls.lock().unwrap().push((shard, tag, guild));
    }
}

fn observed_config(
    event_capacity: usize,
    overflow_capacity: usize,
    observer: &Arc<Recorder>,
) -> LaneConfig {
    LaneConfig {
        drop_observer: Some(Arc::clone(observer) as Arc<dyn DropObserver>),
        ..config(event_capacity, overflow_capacity)
    }
}

/// A non-droppable `MESSAGE_DELETE` tagged `n` on shard 2 in guild `guild`.
fn m_on(n: u64, guild: u64) -> Inbound {
    Inbound {
        shard: ShardId::new(2, 4),
        received_at: Instant::now(),
        kind: InboundKind::Event(Box::new(Event::MessageDelete(MessageDelete {
            channel_id: Id::new(1),
            guild_id: Some(Id::new(guild)),
            id: Id::new(n),
        }))),
    }
}

/// Story 5.7 Matrix 20: a full overflow hands the drop to the observer
/// instead of the lane's own `error` log.
#[test]
fn observer_receives_overflow_drop() {
    let observer = Arc::new(Recorder::default());
    let ((), logs) = observed(|metrics| {
        let (sender, receiver) = lanes(observed_config(1, 0, &observer));
        push_all(&sender, [m_on(1, 77), m_on(2, 77)]);
        assert_eq!(event_tags(&receiver), ["M1"]);
        assert_eq!(
            dropped(&snapshot(metrics), "test", "MESSAGE_DELETE"),
            Some(1)
        );
    });
    assert_eq!(
        *observer.calls.lock().unwrap(),
        [(2, "M2".to_owned(), Some(77))]
    );
    assert!(logs.errors().is_empty(), "{}", logs.text());
}

/// Story 5.7 Matrix 21: without an observer the 5.5 `error` line stays.
#[test]
fn no_observer_logs_overflow_drop() {
    let ((), logs) = observed(|_| {
        let (sender, _receiver) = lanes(config(1, 0));
        push_all(&sender, [m_on(1, 77), m_on(2, 77)]);
    });
    let errors = logs.errors();
    assert_eq!(errors.len(), 1, "{}", logs.text());
    assert!(errors[0].contains("noctis::lanes"), "{}", errors[0]);
    assert!(errors[0].contains("event overflow is full; event dropped"));
}

/// Story 5.7 Matrix 22: an overflow cap above zero calls the observer once
/// per event beyond it.
#[test]
fn observer_called_per_drop_beyond_cap() {
    let observer = Arc::new(Recorder::default());
    observed(|metrics| {
        let (sender, receiver) = lanes(observed_config(1, 2, &observer));
        push_all(&sender, [m(10), m(1), m(2), m(3), m(4)]);
        assert_eq!(event_tags(&receiver), ["M10"]);
        assert_eq!(overflow_tags(&receiver), ["M1", "M2"]);
        assert_eq!(
            dropped(&snapshot(metrics), "test", "MESSAGE_DELETE"),
            Some(2)
        );
    });
    let calls = observer.calls.lock().unwrap();
    let tags: Vec<_> = calls.iter().map(|(_, tag, _)| tag.as_str()).collect();
    assert_eq!(tags, ["M3", "M4"]);
}

/// Story 5.7 Matrix 23: droppable evictions never reach the observer.
#[test]
fn observer_skips_evictions() {
    let observer = Arc::new(Recorder::default());
    observed(|metrics| {
        let (sender, receiver) = lanes(observed_config(1, 0, &observer));
        push_all(&sender, [t(1), t(2)]);
        assert_eq!(event_tags(&receiver), ["T2"]);
        assert_eq!(dropped(&snapshot(metrics), "test", "TYPING_START"), Some(1));
    });
    assert!(observer.calls.lock().unwrap().is_empty());
}

/// Clones and drops a `LaneSender` it holds from inside the callback.
#[derive(Default)]
struct Reentrant {
    sender: std::sync::OnceLock<LaneSender>,
    calls: AtomicUsize,
}

impl DropObserver for Reentrant {
    fn overflow_dropped(&self, _shard: u32, _event: &Event) {
        if let Some(sender) = self.sender.get() {
            drop(sender.clone());
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

/// Story 5.7 Matrix 26: the observer runs after the lock is released, so
/// locking the lanes from inside it does not deadlock.
#[test]
fn observer_runs_outside_the_lock() {
    let observer = Arc::new(Reentrant::default());
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let thread_observer = Arc::clone(&observer);
    std::thread::spawn(move || {
        let (sender, receiver) = lanes(LaneConfig {
            drop_observer: Some(Arc::clone(&thread_observer) as Arc<dyn DropObserver>),
            ..config(1, 0)
        });
        assert!(thread_observer.sender.set(sender.clone()).is_ok());
        push_all(&sender, [m(1), m(2), m(3)]);
        drop(receiver);
        let _ = done_tx.send(());
    });
    done_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("the observer deadlocked");
    assert_eq!(observer.calls.load(Ordering::SeqCst), 2);
}
