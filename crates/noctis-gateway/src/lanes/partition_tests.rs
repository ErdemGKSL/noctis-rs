//! Story 16.2: partitioned lanes (partition index, per-shard FIFO, global
//! bounds, per-partition O(1) eviction, batched pops, the unsplit receiver
//! over several partitions, closing).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use metrics::{
    Counter, Gauge, GaugeFn, Histogram, Key, KeyName, Metadata, Recorder, SharedString, Unit,
};
use metrics_util::MetricKind;
use metrics_util::debugging::{DebugValue, DebuggingRecorder};
use serde_json::json;
use tokio::time::Instant;
use twilight_gateway::{Event, ShardId};
use twilight_model::gateway::payload::incoming::{MessageDelete, TypingStart};
use twilight_model::id::Id;

use super::*;

fn n(count: usize) -> NonZeroUsize {
    NonZeroUsize::new(count).unwrap()
}

fn config(event_capacity: usize, overflow_capacity: usize) -> LaneConfig {
    LaneConfig {
        app: Arc::from("partitions"),
        event_capacity: n(event_capacity),
        overflow_capacity,
        droppable: DROPPABLE_EVENTS,
        drop_observer: None,
    }
}

fn on(shard: u32, kind: InboundKind) -> Inbound {
    Inbound {
        shard: ShardId::new(shard, 64),
        received_at: Instant::now(),
        kind,
    }
}

/// A non-droppable `MESSAGE_DELETE` tagged `id` on `shard`.
fn m(shard: u32, id: u64) -> Inbound {
    on(
        shard,
        InboundKind::Event(Box::new(Event::MessageDelete(MessageDelete {
            channel_id: Id::new(1),
            guild_id: None,
            id: Id::new(id),
        }))),
    )
}

fn typing(id: u64) -> TypingStart {
    serde_json::from_value(json!({
        "channel_id": "1",
        "timestamp": 0,
        "user_id": id.to_string(),
    }))
    .unwrap()
}

/// A droppable `TYPING_START` tagged `id` on `shard`.
fn t(shard: u32, id: u64) -> Inbound {
    on(
        shard,
        InboundKind::Event(Box::new(Event::TypingStart(Box::new(typing(id))))),
    )
}

/// An interaction tagged `id` on `shard`.
fn i(shard: u32, id: u64) -> Inbound {
    let d = json!({
        "id": id.to_string(),
        "application_id": "1",
        "type": 1,
        "token": "token",
        "version": 1,
        "authorizing_integration_owners": {},
        "entitlements": [],
    });
    let raw = crate::RawJson::new(bytes::Bytes::from(d.to_string()));
    on(
        shard,
        InboundKind::Interaction(Box::new(serde_json::from_value(d).unwrap()), raw),
    )
}

/// `M1`, `T2`, `I3`.
fn tag(inbound: &Inbound) -> String {
    match &inbound.kind {
        InboundKind::Interaction(interaction, _) => format!("I{}", interaction.id),
        InboundKind::Event(event) => match &**event {
            Event::MessageDelete(delete) => format!("M{}", delete.id),
            Event::TypingStart(typing) => format!("T{}", typing.user_id),
            other => format!("{:?}", other.kind()),
        },
    }
}

fn tags(batch: &VecDeque<Inbound>) -> Vec<String> {
    batch.iter().map(tag).collect()
}

/// The queued event tags of partition `index`, oldest first.
fn event_tags(shared: &Shared, index: usize) -> Vec<String> {
    shared.partitions[index]
        .lock()
        .event
        .iter()
        .map(tag)
        .collect()
}

fn overflow_tags(shared: &Shared, index: usize) -> Vec<String> {
    shared.partitions[index]
        .lock()
        .overflow
        .iter()
        .map(tag)
        .collect()
}

/// Runs `f` under a scoped subscriber that discards its output.
///
/// A lane log callsite first hit while no subscriber is alive anywhere can
/// cache `Interest::never` for the whole process (a `tracing-core`
/// registration race), silencing it for the `lanes::tests` log assertions
/// that run in parallel; a live subscriber here keeps the cached interest
/// at `sometimes`.
fn quiet<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(std::io::sink)
        .finish();
    tracing::subscriber::with_default(subscriber, f)
}

/// Takes everything partition `p` holds now.
fn take_all(partition: &mut LanePartition) -> Vec<String> {
    let mut batch = VecDeque::new();
    while partition.try_recv_batch(&mut batch, n(1000)) > 0 {}
    tags(&batch)
}

#[test]
fn partition_index() {
    let (_sender, receiver) = lanes_with(config(8, 8), n(64));
    assert_eq!(receiver.shared.partition_of(u32::MAX), 63);
    assert_eq!(receiver.shared.partition_of(0), 0);
    assert_eq!(receiver.shared.partition_of(65), 1);
    let (_sender, receiver) = lanes(config(8, 8));
    for shard in [0, 1, 7, u32::MAX] {
        assert_eq!(receiver.shared.partition_of(shard), 0);
    }
    let (sender, receiver) = lanes_with(config(8, 8), n(4));
    sender.push(m(6, 1));
    sender.push(m(3, 2));
    let partitions = receiver.into_partitions();
    let events: Vec<_> = partitions.iter().map(|p| p.depths().event).collect();
    assert_eq!(events, [0, 0, 1, 1]);
}

#[test]
fn partition_count_is_clamped() {
    let (_sender, receiver) = lanes_with(config(8, 8), n(200));
    assert_eq!(receiver.into_partitions().len(), MAX_LANE_PARTITIONS);
    let (_sender, receiver) = lanes(config(8, 8));
    assert_eq!(receiver.into_partitions().len(), 1);
}

/// `lanes_with(.., 1)` behaves as `lanes`.
#[test]
fn one_partition_is_lanes() {
    let script = |sender: &LaneSender| {
        for item in [
            m(0, 1),
            t(5, 1),
            i(3, 1),
            m(2, 2),
            t(1, 2),
            m(9, 3),
            m(4, 4),
        ] {
            sender.push(item);
        }
    };
    let (a, mut ra) = lanes(config(3, 1));
    let (b, mut rb) = lanes_with(config(3, 1), NonZeroUsize::MIN);
    script(&a);
    script(&b);
    let drain = |r: &mut LaneReceiver| {
        std::iter::from_fn(|| r.try_recv())
            .map(|x| tag(&x))
            .collect::<Vec<_>>()
    };
    let expected = drain(&mut ra);
    assert_eq!(expected, ["I1", "M1", "M2", "T2", "M3"]);
    assert_eq!(drain(&mut rb), expected);
}

#[test]
fn per_shard_fifo() {
    let (sender, receiver) = lanes_with(config(1000, 0), n(4));
    for id in 1..=800_u64 {
        sender.push(m(u32::try_from(id % 8).unwrap(), id));
    }
    let mut partitions = receiver.into_partitions();
    let mut seen = Vec::new();
    for partition in &mut partitions {
        let mut last_by_shard: HashMap<u64, u64> = HashMap::new();
        for tag in take_all(partition) {
            let id: u64 = tag[1..].parse().unwrap();
            let shard = id % 8;
            assert_eq!(
                usize::try_from(shard).unwrap() % 4,
                partition.index(),
                "{tag}"
            );
            let last = last_by_shard.insert(shard, id);
            assert!(last.is_none_or(|last| last < id), "{tag} after {last:?}");
            seen.push(id);
        }
    }
    seen.sort_unstable();
    assert_eq!(seen, (1..=800).collect::<Vec<_>>());
}

/// The unsplit receiver over several partitions keeps each shard's order.
#[test]
fn unsplit_receiver_keeps_shard_order() {
    let (sender, mut receiver) = lanes_with(config(1000, 0), n(4));
    for id in 1..=400_u64 {
        sender.push(m(u32::try_from(id % 8).unwrap(), id));
    }
    sender.push(i(7, 1));
    sender.push(i(2, 2));
    let popped: Vec<_> = std::iter::from_fn(|| receiver.try_recv())
        .map(|x| tag(&x))
        .collect();
    assert_eq!(popped.len(), 402);
    let mut interactions = popped[..2].to_vec();
    interactions.sort();
    assert_eq!(interactions, ["I1", "I2"]);
    let mut last_by_shard: HashMap<u64, u64> = HashMap::new();
    for tag in &popped[2..] {
        let id: u64 = tag[1..].parse().unwrap();
        let last = last_by_shard.insert(id % 8, id);
        assert!(last.is_none_or(|last| last < id), "{tag}");
    }
    assert_eq!(receiver.depths(), LaneDepths::default());
}

#[tokio::test(flavor = "current_thread")]
async fn unsplit_receiver_wakes_on_any_partition() {
    let (sender, mut receiver) = lanes_with(config(8, 8), n(4));
    let waiting = tokio::spawn(async move {
        let first = receiver.recv().await.map(|x| tag(&x));
        let second = receiver.recv_interaction().await.map(|x| tag(&x));
        let closed = receiver.recv().await.is_none();
        (first, second, closed)
    });
    tokio::task::yield_now().await;
    sender.push(m(3, 1));
    tokio::task::yield_now().await;
    sender.push(i(2, 5));
    tokio::task::yield_now().await;
    drop(sender);
    let (first, second, closed) = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.as_deref(), Some("M1"));
    assert_eq!(second.as_deref(), Some("I5"));
    assert!(closed);
}

/// The bounds hold over all partitions under concurrent pushes.
#[test]
fn global_bound_concurrent() {
    const THREADS: u32 = 8;
    const PER_THREAD: u64 = 1000;
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let (sender, mut receiver) =
        metrics::with_local_recorder(&recorder, || lanes_with(config(4, 2), n(4)));
    let done = AtomicBool::new(false);
    let started = std::time::Instant::now();
    let popped = std::thread::scope(|scope| {
        let consumer = scope.spawn(|| {
            let mut popped = 0_u64;
            loop {
                assert!(started.elapsed() < Duration::from_secs(20), "deadlock");
                let (mut event, mut overflow) = (0, 0);
                for partition in &receiver.shared.partitions {
                    let queues = partition.lock();
                    event += queues.event.len();
                    overflow += queues.overflow.len();
                }
                assert!(event <= 4, "event lanes hold {event}");
                assert!(overflow <= 2, "overflows hold {overflow}");
                let depths = receiver.depths();
                assert!(depths.event <= 4 && depths.overflow <= 2, "{depths:?}");
                match receiver.try_recv() {
                    Some(_) => popped += 1,
                    None if done.load(Ordering::SeqCst) => {
                        if receiver.depths() == LaneDepths::default() {
                            break popped;
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
                scope.spawn(move || {
                    metrics::with_local_recorder(recorder, || {
                        for id in 0..PER_THREAD {
                            sender.push(m(thread, u64::from(thread) * PER_THREAD + id + 1));
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
    let dropped: u64 = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| {
            key.kind() == MetricKind::Counter && key.key().name() == "noctis_events_dropped_total"
        })
        .map(|(.., value)| match value {
            DebugValue::Counter(n) => n,
            _ => 0,
        })
        .sum();
    assert_eq!(popped + dropped, u64::from(THREADS) * PER_THREAD);
    drop(sender);
}

/// A droppable on a full lane evicts its own partition's oldest droppable,
/// or is dropped when its partition holds none.
#[test]
fn eviction_is_per_partition() {
    let (sender, receiver) = lanes_with(config(3, 2), n(2));
    let shared = Arc::clone(&receiver.shared);
    sender.push(t(0, 1));
    sender.push(m(0, 1));
    sender.push(m(1, 2));
    assert_eq!(shared.depths().event, 3);
    // Partition 1 holds no droppable: the pushed one is dropped.
    sender.push(t(1, 2));
    assert_eq!(event_tags(&shared, 1), ["M2"]);
    // Partition 0 holds T1: it is evicted, the survivors keep their order.
    sender.push(t(0, 3));
    assert_eq!(event_tags(&shared, 0), ["M1", "T3"]);
    assert_eq!(
        shared.depths(),
        LaneDepths {
            interaction: 0,
            event: 3,
            overflow: 0
        }
    );
    drop(receiver);
}

/// Eviction is O(1): droppables only at the back of a 200 000-item lane.
#[test]
fn eviction_complexity() {
    const CAPACITY: usize = 200_000;
    let (sender, receiver) = lanes(config(CAPACITY, 0));
    for id in 1..CAPACITY as u64 {
        sender.push(m(0, id));
    }
    sender.push(t(0, 1));
    let template = typing(7);
    let pushes: Vec<_> = (0..100_000)
        .map(|_| {
            on(
                0,
                InboundKind::Event(Box::new(Event::TypingStart(Box::new(template.clone())))),
            )
        })
        .collect();
    let started = std::time::Instant::now();
    for inbound in pushes {
        sender.push(inbound);
    }
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    assert_eq!(receiver.depths().event, CAPACITY);
    let queues = receiver.shared.lock();
    assert_eq!(queues.event.len(), CAPACITY);
    assert_eq!(queues.event.droppable.len(), 1);
}

/// A stalled consumer and endless droppables: the queue never grows.
#[test]
fn eviction_memory() {
    let (sender, receiver) = lanes(config(8, 0));
    for id in 1..=100_000 {
        sender.push(t(0, id));
    }
    let queues = receiver.shared.lock();
    assert_eq!(queues.event.len(), 8);
    assert!(
        queues.event.droppable.capacity() < 64,
        "{}",
        queues.event.droppable.capacity()
    );
}

/// A partition's overflow refills its own event lane; an event lane that
/// is empty still hands out its overflow; a partition without overflow
/// uses free capacity even while another partition's overflow is non-empty.
#[test]
fn refill_and_overflow_only_pop() {
    let (sender, receiver) = lanes_with(config(2, 4), n(2));
    let shared = Arc::clone(&receiver.shared);
    let mut partitions = receiver.into_partitions();
    sender.push(m(1, 1));
    sender.push(m(0, 2));
    sender.push(m(0, 3));
    assert_eq!(overflow_tags(&shared, 0), ["M3"]);
    let mut batch = VecDeque::new();
    assert_eq!(partitions[1].try_recv_batch(&mut batch, n(1)), 1);
    assert_eq!(tags(&batch), ["M1"]);
    assert_eq!(
        shared.depths(),
        LaneDepths {
            interaction: 0,
            event: 1,
            overflow: 1
        }
    );
    // Partition 1 has no overflow: the free slot takes its event.
    sender.push(m(1, 4));
    assert_eq!(event_tags(&shared, 1), ["M4"]);
    // Partition 0 keeps its FIFO behind its overflow.
    sender.push(m(0, 5));
    assert_eq!(overflow_tags(&shared, 0), ["M3", "M5"]);
    assert_eq!(take_all(&mut partitions[0]), ["M2", "M3", "M5"]);
    assert_eq!(take_all(&mut partitions[1]), ["M4"]);
    assert_eq!(shared.depths(), LaneDepths::default());

    // Overflow only: the event lane is full of another partition's items.
    sender.push(m(1, 6));
    sender.push(m(1, 7));
    sender.push(m(0, 8));
    assert_eq!(event_tags(&shared, 0), Vec::<String>::new());
    assert_eq!(overflow_tags(&shared, 0), ["M8"]);
    assert_eq!(take_all(&mut partitions[0]), ["M8"]);
    assert_eq!(take_all(&mut partitions[1]), ["M6", "M7"]);
}

/// Event capacity another partition frees drains this partition's overflow
/// into its event lane, oldest first, so its new events are not dropped.
#[test]
fn overflow_drains_into_freed_capacity() {
    let (sender, receiver) = lanes_with(config(3, 2), n(2));
    let shared = Arc::clone(&receiver.shared);
    let mut partitions = receiver.into_partitions();
    sender.push(m(1, 1));
    sender.push(m(1, 2));
    sender.push(m(0, 3));
    sender.push(m(0, 4));
    sender.push(m(0, 5));
    assert_eq!(event_tags(&shared, 0), ["M3"]);
    assert_eq!(overflow_tags(&shared, 0), ["M4", "M5"]);
    // Partition 1 frees two slots; partition 0's next pop moves its whole
    // overflow into them.
    assert_eq!(take_all(&mut partitions[1]), ["M1", "M2"]);
    let mut batch = VecDeque::new();
    assert_eq!(partitions[0].try_recv_batch(&mut batch, n(1)), 1);
    assert_eq!(tags(&batch), ["M3"]);
    assert_eq!(event_tags(&shared, 0), ["M4", "M5"]);
    assert_eq!(overflow_tags(&shared, 0), Vec::<String>::new());
    assert_eq!(
        shared.depths(),
        LaneDepths {
            interaction: 0,
            event: 2,
            overflow: 0
        }
    );
    // The overflow is empty: a new event takes the free slot.
    sender.push(m(0, 6));
    assert_eq!(event_tags(&shared, 0), ["M4", "M5", "M6"]);
    assert_eq!(take_all(&mut partitions[0]), ["M4", "M5", "M6"]);
    assert_eq!(shared.depths(), LaneDepths::default());
}

/// Counts the gauge `set` calls.
#[derive(Default)]
struct GaugeSets(AtomicUsize);

impl GaugeFn for GaugeSets {
    fn increment(&self, _: f64) {}

    fn decrement(&self, _: f64) {}

    fn set(&self, _: f64) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Counting(Arc<GaugeSets>);

impl Recorder for Counting {
    fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

    fn register_counter(&self, _: &Key, _: &Metadata<'_>) -> Counter {
        Counter::noop()
    }

    fn register_gauge(&self, _: &Key, _: &Metadata<'_>) -> Gauge {
        Gauge::from_arc(Arc::clone(&self.0))
    }

    fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
        Histogram::noop()
    }
}

/// One batch pop: interactions first, one lock, three gauge sets; a push
/// sets one gauge.
#[test]
fn batch_pop_and_gauge_sets() {
    let sets = Arc::new(GaugeSets::default());
    let recorder = Counting(Arc::clone(&sets));
    metrics::with_local_recorder(&recorder, || {
        let (sender, receiver) = lanes_with(config(200, 0), n(2));
        let mut partition = receiver.into_partitions().remove(0);
        for id in 1..=100 {
            sender.push(m(0, id));
        }
        for id in 1..=3 {
            sender.push(i(2, id));
        }
        sender.push(m(1, 999));
        let before = sets.0.load(Ordering::SeqCst);
        sender.push(m(0, 101));
        assert_eq!(
            sets.0.load(Ordering::SeqCst) - before,
            1,
            "one set per push"
        );
        let mut batch = VecDeque::with_capacity(32);
        let before = sets.0.load(Ordering::SeqCst);
        assert_eq!(partition.try_recv_batch(&mut batch, n(32)), 32);
        assert_eq!(
            sets.0.load(Ordering::SeqCst) - before,
            3,
            "three sets per batch"
        );
        let got = tags(&batch);
        assert_eq!(got[..3], ["I1", "I2", "I3"]);
        let expected: Vec<_> = (1..=29).map(|id| format!("M{id}")).collect();
        assert_eq!(got[3..], expected);
        assert_eq!(
            partition.depths(),
            LaneDepths {
                interaction: 0,
                event: 72,
                overflow: 0
            }
        );
    });
}

#[tokio::test(flavor = "current_thread")]
async fn batch_waits_then_closes() {
    let (sender, receiver) = lanes_with(config(8, 8), n(2));
    let mut partitions = receiver.into_partitions();
    let mut second = partitions.pop().unwrap();
    let waiting = tokio::spawn(async move {
        let mut batch = VecDeque::new();
        let first = second.recv_batch(&mut batch, n(8)).await;
        let closed = second.recv_batch(&mut batch, n(8)).await;
        (first, tags(&batch), closed)
    });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    // A push into partition 0 does not wake partition 1.
    sender.push(m(0, 1));
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    sender.push(m(1, 2));
    tokio::task::yield_now().await;
    drop(sender);
    let (first, got, closed) = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first, got, closed), (1, vec!["M2".to_owned()], 0));
    let mut batch = VecDeque::new();
    assert_eq!(partitions[0].recv_batch(&mut batch, n(8)).await, 1);
    assert_eq!(partitions[0].recv_batch(&mut batch, n(8)).await, 0);
    assert!(partitions[0].recv_interaction().await.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn partition_recv_interaction_skips_events() {
    let (sender, receiver) = lanes_with(config(8, 8), n(2));
    let mut partition = receiver.into_partitions().remove(1);
    sender.push(m(1, 1));
    sender.push(i(0, 9));
    sender.push(i(3, 2));
    let got = partition.recv_interaction().await.unwrap();
    assert_eq!(tag(&got), "I2");
    assert_eq!(
        partition.depths(),
        LaneDepths {
            interaction: 0,
            event: 1,
            overflow: 0
        }
    );
}

/// A dropped partition discards its items and later pushes into it; the
/// other partitions keep working; with every consumer gone the gauges are 0.
#[test]
fn dropped_partition_discards() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    metrics::with_local_recorder(&recorder, || {
        let (sender, receiver) = lanes_with(config(8, 8), n(2));
        let mut partitions = receiver.into_partitions();
        sender.push(m(0, 1));
        sender.push(m(1, 2));
        drop(partitions.remove(0));
        assert_eq!(
            sender.shared.depths(),
            LaneDepths {
                interaction: 0,
                event: 1,
                overflow: 0
            }
        );
        sender.push(m(0, 3));
        sender.push(i(0, 4));
        assert_eq!(sender.shared.depths().event, 1);
        sender.push(m(1, 5));
        assert_eq!(take_all(&mut partitions[0]), ["M2", "M5"]);
        drop(partitions);
        sender.push(m(1, 6));
        assert_eq!(sender.shared.depths(), LaneDepths::default());
    });
    let gauges: Vec<_> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.kind() == MetricKind::Gauge)
        .map(|(.., value)| value)
        .collect();
    assert_eq!(gauges.len(), 3);
    for value in gauges {
        assert!(
            matches!(value, DebugValue::Gauge(v) if v.0 == 0.0),
            "{value:?}"
        );
    }
    let dropped = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.kind() == MetricKind::Counter)
        .count();
    assert_eq!(dropped, 0);
}

/// The unsplit receiver's drop closes every partition.
#[test]
fn unsplit_drop_closes_all() {
    let (sender, receiver) = lanes_with(config(8, 8), n(3));
    sender.push(m(0, 1));
    sender.push(m(1, 2));
    sender.push(i(2, 3));
    drop(receiver);
    sender.push(m(2, 4));
    assert_eq!(sender.shared.depths(), LaneDepths::default());
}

/// A [`DebuggingRecorder`] that also counts each `register_counter` call
/// by key.
#[derive(Default)]
struct Registrations {
    inner: DebuggingRecorder,
    counters: Mutex<Vec<Key>>,
}

impl Recorder for Registrations {
    fn describe_counter(&self, name: KeyName, unit: Option<Unit>, text: SharedString) {
        self.inner.describe_counter(name, unit, text);
    }

    fn describe_gauge(&self, name: KeyName, unit: Option<Unit>, text: SharedString) {
        self.inner.describe_gauge(name, unit, text);
    }

    fn describe_histogram(&self, name: KeyName, unit: Option<Unit>, text: SharedString) {
        self.inner.describe_histogram(name, unit, text);
    }

    fn register_counter(&self, key: &Key, metadata: &Metadata<'_>) -> Counter {
        self.counters.lock().unwrap().push(key.clone());
        self.inner.register_counter(key, metadata)
    }

    fn register_gauge(&self, key: &Key, metadata: &Metadata<'_>) -> Gauge {
        self.inner.register_gauge(key, metadata)
    }

    fn register_histogram(&self, key: &Key, metadata: &Metadata<'_>) -> Histogram {
        self.inner.register_histogram(key, metadata)
    }
}

/// Story 16.3 D1: repeated drops of one event type register
/// `noctis_events_dropped_total` once for that type and reuse the handle;
/// names, labels and counts are unchanged.
#[test]
fn drop_counters_register_once_per_event_type() {
    let recorder = Registrations::default();
    let snapshotter = recorder.inner.snapshotter();
    quiet(|| {
        metrics::with_local_recorder(&recorder, || {
            let mut config = config(1, 0);
            config.droppable = DROPPABLE_EVENTS | EventTypeFlags::MESSAGE_DELETE;
            let (sender, receiver) = lanes_with(config, n(1));
            // t1 queued; t2..t5 and m1 each evict the oldest: 5 TYPING_START
            // drops; m2 and m3 evict m1 and m2: 2 MESSAGE_DELETE drops.
            for id in 1..=5 {
                sender.push(t(0, id));
            }
            for id in 1..=3 {
                sender.push(m(0, id));
            }
            assert_eq!(receiver.depths().event, 1);
        });
    });
    let registered = recorder.counters.lock().unwrap().clone();
    let labels = |key: &Key| {
        let mut labels: Vec<_> = key
            .labels()
            .map(|l| (l.key().to_owned(), l.value().to_owned()))
            .collect();
        labels.sort();
        labels
    };
    assert_eq!(registered.len(), 2, "{registered:?}");
    for (key, event_type) in registered.iter().zip(["TYPING_START", "MESSAGE_DELETE"]) {
        assert_eq!(key.name(), "noctis_events_dropped_total");
        assert_eq!(
            labels(key),
            [
                ("app".to_owned(), "partitions".to_owned()),
                ("event_type".to_owned(), event_type.to_owned())
            ]
        );
    }
    let mut counts: Vec<_> = snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .filter(|(key, ..)| key.key().name() == "noctis_events_dropped_total")
        .map(|(key, .., value)| {
            let event_type = labels(key.key())[1].1.clone();
            match value {
                DebugValue::Counter(n) => (event_type, n),
                other => panic!("{other:?}"),
            }
        })
        .collect();
    counts.sort();
    assert_eq!(
        counts,
        [
            ("MESSAGE_DELETE".to_owned(), 2),
            ("TYPING_START".to_owned(), 5)
        ]
    );
}

/// Story 16.3: a gateway-only kind without a name is labeled `UNKNOWN`
/// and cached under its `EventType`.
#[test]
fn unnamed_drop_counter_is_cached() {
    let recorder = Registrations::default();
    metrics::with_local_recorder(&recorder, || {
        let (sender, _receiver) = lanes_with(config(1, 0), n(1));
        assert_eq!(EventType::GatewayHeartbeatAck.name(), None);
        sender.shared.count_drop(EventType::GatewayHeartbeatAck);
        sender.shared.count_drop(EventType::GatewayHeartbeatAck);
    });
    let registered = recorder.counters.lock().unwrap().clone();
    assert_eq!(registered.len(), 1, "{registered:?}");
    assert!(
        registered[0]
            .labels()
            .any(|l| l.key() == "event_type" && l.value() == "UNKNOWN")
    );
}
