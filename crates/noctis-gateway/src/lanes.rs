//! The Interaction Lane and the bounded event lane between shard tasks and
//! core (FR-5, AD-10, NFR-5).
//!
//! [`lanes_with`] returns a [`LaneSender`], the [`InboundSink`] shard tasks
//! push into, and a [`LaneReceiver`] that always drains interactions first.
//! The lanes are partitioned by shard: each partition has its own
//! `std::sync::Mutex` over its three queues and its own wake-up, while the
//! depths and bounds are global atomics. `push` never awaits, and metrics
//! and logs are emitted after the lock is released.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::mem;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::task::Poll;

use metrics::{Counter, Gauge};
use tokio::sync::Notify;
use tokio::sync::futures::Notified;
use twilight_gateway::{Event, EventType, EventTypeFlags};

use crate::inbound::{Inbound, InboundKind, InboundSink};

/// The event types the event lane drops first when it is full: presence
/// updates and typing starts.
///
/// ```
/// use noctis_gateway::{DROPPABLE_EVENTS, EventTypeFlags};
///
/// assert!(DROPPABLE_EVENTS.contains(EventTypeFlags::TYPING_START));
/// assert!(!DROPPABLE_EVENTS.contains(EventTypeFlags::MESSAGE_CREATE));
/// ```
pub const DROPPABLE_EVENTS: EventTypeFlags =
    EventTypeFlags::PRESENCE_UPDATE.union(EventTypeFlags::TYPING_START);

/// The default event lane capacity (4096 events).
///
/// ```
/// assert_eq!(noctis_gateway::DEFAULT_EVENT_CAPACITY.get(), 4096);
/// ```
pub const DEFAULT_EVENT_CAPACITY: NonZeroUsize = match NonZeroUsize::new(4096) {
    Some(capacity) => capacity,
    None => panic!("4096 is not zero"),
};

/// The default hard cap of the event overflow (16384 events).
///
/// ```
/// assert_eq!(noctis_gateway::DEFAULT_OVERFLOW_CAPACITY, 16384);
/// ```
pub const DEFAULT_OVERFLOW_CAPACITY: usize = 16384;

/// Observes non-droppable events dropped because the event overflow is full.
///
/// [`LaneConfig::drop_observer`] installs one. The lane calls
/// [`overflow_dropped`](Self::overflow_dropped) on the pushing shard task,
/// after it released its lock, and logs nothing itself; without an observer
/// it logs the drop at `error`. Droppable evictions never reach it. The call
/// must not block: the shard task reads the socket and heartbeats.
///
/// ```
/// use std::sync::Arc;
///
/// use noctis_gateway::{DropObserver, LaneConfig};
/// use twilight_gateway::Event;
///
/// struct Count;
/// impl DropObserver for Count {
///     fn overflow_dropped(&self, shard: u32, event: &Event) {
///         println!("shard {shard} dropped {:?}", event.kind());
///     }
/// }
///
/// let config = LaneConfig {
///     drop_observer: Some(Arc::new(Count)),
///     ..LaneConfig::default()
/// };
/// assert!(format!("{config:?}").contains("drop_observer: true"));
/// ```
pub trait DropObserver: Send + Sync + 'static {
    /// Called once per event dropped because the overflow was full.
    fn overflow_dropped(&self, shard: u32, event: &Event);
}

/// How [`lanes`] bounds the event lane and labels its metrics.
///
/// `Debug` shows only whether a [`DropObserver`] is set.
///
/// ```
/// use noctis_gateway::{DEFAULT_EVENT_CAPACITY, DROPPABLE_EVENTS, LaneConfig};
///
/// let config = LaneConfig::default();
/// assert_eq!(&*config.app, "default");
/// assert_eq!(config.event_capacity, DEFAULT_EVENT_CAPACITY);
/// assert_eq!(config.overflow_capacity, 16384);
/// assert_eq!(config.droppable, DROPPABLE_EVENTS);
/// assert!(config.drop_observer.is_none());
/// ```
#[derive(Clone)]
pub struct LaneConfig {
    /// The `app` label value of every lane metric (not validated).
    pub app: Arc<str>,
    /// How many events the event lane holds before it is full.
    pub event_capacity: NonZeroUsize,
    /// How many non-droppable events the overflow holds behind a full event
    /// lane; beyond it they are dropped with an `error` log.
    pub overflow_capacity: usize,
    /// The event types dropped (oldest first) when the event lane is full.
    pub droppable: EventTypeFlags,
    /// Receives every event dropped because the overflow is full, instead
    /// of the lane's own `error` log (default `None`).
    pub drop_observer: Option<Arc<dyn DropObserver>>,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            app: Arc::from("default"),
            event_capacity: DEFAULT_EVENT_CAPACITY,
            overflow_capacity: DEFAULT_OVERFLOW_CAPACITY,
            droppable: DROPPABLE_EVENTS,
            drop_observer: None,
        }
    }
}

impl fmt::Debug for LaneConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneConfig")
            .field("app", &self.app)
            .field("event_capacity", &self.event_capacity)
            .field("overflow_capacity", &self.overflow_capacity)
            .field("droppable", &self.droppable)
            .field("drop_observer", &self.drop_observer.is_some())
            .finish()
    }
}

/// How many items each lane holds.
///
/// ```
/// let depths = noctis_gateway::LaneDepths::default();
/// assert_eq!(depths.interaction + depths.event + depths.overflow, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaneDepths {
    /// Items in the Interaction Lane.
    pub interaction: usize,
    /// Items in the event lane.
    pub event: usize,
    /// Items in the event overflow.
    pub overflow: usize,
}

/// The most partitions [`lanes_with`] creates (64); a larger count is
/// clamped to it.
///
/// ```
/// assert_eq!(noctis_gateway::MAX_LANE_PARTITIONS, 64);
/// ```
pub const MAX_LANE_PARTITIONS: usize = 64;

/// Creates the lanes with one partition: a [`LaneSender`] for the shard
/// tasks and the [`LaneReceiver`] core drains. Same as
/// [`lanes_with`]`(config, 1)`.
///
/// Registers three gauges, each set to 0:
/// `noctis_lane_depth{app, lane="interaction"}`,
/// `noctis_lane_depth{app, lane="event"}` and
/// `noctis_event_overflow_depth{app}`. Drops count in
/// `noctis_events_dropped_total{app, event_type}`.
///
/// ```
/// use noctis_gateway::{LaneConfig, LaneDepths, lanes};
///
/// let (sender, mut receiver) = lanes(LaneConfig::default());
/// assert_eq!(receiver.depths(), LaneDepths::default());
/// assert!(receiver.try_recv().is_none());
/// drop(sender);
/// ```
pub fn lanes(config: LaneConfig) -> (LaneSender, LaneReceiver) {
    lanes_with(config, NonZeroUsize::MIN)
}

/// Creates the lanes split into `partitions` partitions (clamped to
/// [`MAX_LANE_PARTITIONS`]).
///
/// An item of shard `s` goes to partition `s % partitions`, so the items of
/// one shard keep their arrival order (interactions among themselves,
/// events among themselves). Each partition has its own lock and wake-up,
/// so shard tasks of different partitions never contend. The bounds of
/// [`LaneConfig`] stay global: the event lanes of all partitions together
/// hold at most `event_capacity` events and their overflows at most
/// `overflow_capacity`. A droppable event pushed into a full lane evicts the
/// oldest droppable event of its own partition, or is dropped when that
/// partition holds none. The gauges of [`lanes`] report the sums over all
/// partitions.
///
/// [`LaneReceiver::into_partitions`] hands each partition to its own
/// consumer; the [`LaneReceiver`] itself also drains every partition.
///
/// ```
/// use std::num::NonZeroUsize;
///
/// use noctis_gateway::{Inbound, InboundKind, InboundSink, LaneConfig, lanes_with};
/// use twilight_gateway::{Event, ShardId};
///
/// let (sender, receiver) = lanes_with(LaneConfig::default(), NonZeroUsize::new(4).unwrap());
/// sender.push(Inbound {
///     shard: ShardId::new(6, 8),
///     received_at: tokio::time::Instant::now(),
///     kind: InboundKind::Event(Box::new(Event::GatewayHeartbeatAck)),
/// });
/// assert_eq!(receiver.depths().event, 1);
/// let partitions = receiver.into_partitions();
/// assert_eq!(partitions.len(), 4);
/// assert_eq!(partitions[2].depths().event, 1);
/// ```
pub fn lanes_with(config: LaneConfig, partitions: NonZeroUsize) -> (LaneSender, LaneReceiver) {
    let count = partitions.get().min(MAX_LANE_PARTITIONS);
    let app = Arc::clone(&config.app);
    let gauges = Gauges {
        interaction: metrics::gauge!("noctis_lane_depth", "app" => Arc::clone(&app), "lane" => "interaction"),
        event: metrics::gauge!("noctis_lane_depth", "app" => Arc::clone(&app), "lane" => "event"),
        overflow: metrics::gauge!("noctis_event_overflow_depth", "app" => app),
    };
    gauges.set(LaneDepths::default());
    let shared = Arc::new(Shared {
        partitions: (0..count).map(|_| Partition::default()).collect(),
        // `count` is at most 64.
        modulus: u32::try_from(count).unwrap_or(1),
        totals: Totals::default(),
        senders: AtomicUsize::new(1),
        config,
        gauges,
        drop_counters: RwLock::default(),
    });
    (
        LaneSender {
            shared: Arc::clone(&shared),
        },
        LaneReceiver {
            shared,
            split: false,
            cursor: 0,
        },
    )
}

/// The event lane of one partition: droppable and other events in two
/// FIFOs, merged by the sequence number each item gets when it enters.
///
/// Evicting the oldest droppable event is a `pop_front` of the droppable
/// FIFO (O(1)), and leaves the order of the survivors unchanged.
#[derive(Default)]
struct EventQueue {
    droppable: VecDeque<(u64, Inbound)>,
    other: VecDeque<(u64, Inbound)>,
    next: u64,
}

impl EventQueue {
    fn push_back(&mut self, inbound: Inbound, droppable: bool) {
        let entry = (self.next, inbound);
        self.next = self.next.wrapping_add(1);
        if droppable {
            self.droppable.push_back(entry);
        } else {
            self.other.push_back(entry);
        }
    }

    /// The oldest event of either FIFO.
    fn pop_front(&mut self) -> Option<Inbound> {
        let from_droppable = match (self.droppable.front(), self.other.front()) {
            (Some((a, _)), Some((b, _))) => a < b,
            (Some(_), None) => true,
            (None, _) => false,
        };
        let queue = if from_droppable {
            &mut self.droppable
        } else {
            &mut self.other
        };
        queue.pop_front().map(|(_, inbound)| inbound)
    }

    fn evict_oldest_droppable(&mut self) -> Option<Inbound> {
        self.droppable.pop_front().map(|(_, inbound)| inbound)
    }

    fn len(&self) -> usize {
        self.droppable.len() + self.other.len()
    }

    /// Every queued event, oldest first.
    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = &Inbound> {
        let mut droppable = self.droppable.iter().peekable();
        let mut other = self.other.iter().peekable();
        std::iter::from_fn(move || {
            let take_droppable = match (droppable.peek(), other.peek()) {
                (Some((a, _)), Some((b, _))) => a < b,
                (Some(_), None) => true,
                (None, _) => false,
            };
            if take_droppable {
                droppable.next()
            } else {
                other.next()
            }
            .map(|(_, inbound)| inbound)
        })
    }
}

/// The three queues of one partition and whether its consumer is alive.
struct Queues {
    interaction: VecDeque<Inbound>,
    event: EventQueue,
    overflow: VecDeque<Inbound>,
    receiver_alive: bool,
}

impl Default for Queues {
    fn default() -> Self {
        Self {
            interaction: VecDeque::new(),
            event: EventQueue::default(),
            overflow: VecDeque::new(),
            receiver_alive: true,
        }
    }
}

impl Queues {
    fn depths(&self) -> LaneDepths {
        LaneDepths {
            interaction: self.interaction.len(),
            event: self.event.len(),
            overflow: self.overflow.len(),
        }
    }

    /// Takes the next item: interactions first, then events (refilling the
    /// event lane from this partition's overflow), then the overflow.
    /// Records in `taken` what left each lane.
    fn pop(&mut self, shared: &Shared, taken: &mut LaneDepths) -> Option<Inbound> {
        if let Some(inbound) = self.interaction.pop_front() {
            taken.interaction += 1;
            return Some(inbound);
        }
        let inbound = if let Some(inbound) = self.event.pop_front() {
            if let Some(refill) = self.overflow.pop_front() {
                self.refill_one(refill, shared);
                taken.overflow += 1;
            } else {
                taken.event += 1;
            }
            inbound
        } else {
            let inbound = self.overflow.pop_front()?;
            taken.overflow += 1;
            inbound
        };
        // Event capacity other partitions freed takes this partition's
        // overflow, oldest first.
        while !self.overflow.is_empty()
            && reserve(&shared.totals.event, shared.config.event_capacity.get())
        {
            if let Some(refill) = self.overflow.pop_front() {
                self.refill_one(refill, shared);
                taken.overflow += 1;
            }
        }
        Some(inbound)
    }

    /// Moves `refill`, taken off the overflow, to the back of the event lane.
    fn refill_one(&mut self, refill: Inbound, shared: &Shared) {
        let droppable = event_type(&refill).is_some_and(|t| shared.is_droppable(t));
        self.event.push_back(refill, droppable);
    }

    /// Takes the next interaction only.
    fn pop_interaction(&mut self, taken: &mut LaneDepths) -> Option<Inbound> {
        let inbound = self.interaction.pop_front()?;
        taken.interaction += 1;
        Some(inbound)
    }

    /// Marks the consumer gone and takes every queued item.
    fn close(&mut self) -> Self {
        let taken = mem::take(self);
        self.receiver_alive = false;
        taken
    }
}

/// One partition: its queues and the wake-up of its consumer.
#[derive(Default)]
struct Partition {
    queues: Mutex<Queues>,
    notify: Notify,
}

impl Partition {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The depths summed over every partition; the bounds are reserved here.
#[derive(Default)]
struct Totals {
    interaction: AtomicUsize,
    event: AtomicUsize,
    overflow: AtomicUsize,
}

impl Totals {
    fn load(&self) -> LaneDepths {
        LaneDepths {
            interaction: self.interaction.load(Ordering::Relaxed),
            event: self.event.load(Ordering::Relaxed),
            overflow: self.overflow.load(Ordering::Relaxed),
        }
    }

    /// Takes `taken` off the totals.
    fn remove(&self, taken: LaneDepths) {
        for (total, n) in [
            (&self.interaction, taken.interaction),
            (&self.event, taken.event),
            (&self.overflow, taken.overflow),
        ] {
            if n > 0 {
                total.fetch_sub(n, Ordering::Relaxed);
            }
        }
    }
}

/// Adds one to `total` if it stays at most `cap`; whether it did.
fn reserve(total: &AtomicUsize, cap: usize) -> bool {
    total
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < cap).then_some(n + 1)
        })
        .is_ok()
}

/// The cached depth gauge handles.
struct Gauges {
    interaction: Gauge,
    event: Gauge,
    overflow: Gauge,
}

impl Gauges {
    fn set(&self, depths: LaneDepths) {
        set_gauge(&self.interaction, depths.interaction);
        set_gauge(&self.event, depths.event);
        set_gauge(&self.overflow, depths.overflow);
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a queue depth above 2^53 cannot exist; gauges take f64"
)]
fn set_gauge(gauge: &Gauge, depth: usize) {
    gauge.set(depth as f64);
}

/// The lane one push touched.
#[derive(Clone, Copy)]
enum Lane {
    Interaction,
    Event,
    Overflow,
}

/// State shared by the sender clones and the receiver(s).
struct Shared {
    partitions: Box<[Partition]>,
    /// `partitions.len()`, as the shard number's modulus.
    modulus: u32,
    totals: Totals,
    senders: AtomicUsize,
    config: LaneConfig,
    gauges: Gauges,
    /// The `noctis_events_dropped_total` handle of each event type that
    /// dropped an event, registered on its first drop (bounded by the
    /// `EventType` variants).
    drop_counters: RwLock<HashMap<EventType, Counter>>,
}

impl Shared {
    /// The queues of partition 0 (the only one of [`lanes`]).
    #[cfg(test)]
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.partitions[0].lock()
    }

    fn depths(&self) -> LaneDepths {
        self.totals.load()
    }

    /// The partition of shard `shard`.
    fn partition_of(&self, shard: u32) -> usize {
        // The remainder is below 64.
        usize::try_from(shard % self.modulus).unwrap_or(0)
    }

    fn is_closed(&self) -> bool {
        self.senders.load(Ordering::SeqCst) == 0
    }

    fn is_droppable(&self, event_type: EventType) -> bool {
        self.config
            .droppable
            .intersects(EventTypeFlags::from(event_type))
    }

    /// Sets the gauge of `lane` from the totals.
    fn set_gauge(&self, lane: Lane) {
        let (gauge, total) = match lane {
            Lane::Interaction => (&self.gauges.interaction, &self.totals.interaction),
            Lane::Event => (&self.gauges.event, &self.totals.event),
            Lane::Overflow => (&self.gauges.overflow, &self.totals.overflow),
        };
        set_gauge(gauge, total.load(Ordering::Relaxed));
    }

    /// Sets every gauge from the totals.
    fn set_gauges(&self) {
        self.gauges.set(self.totals.load());
    }

    /// Counts one dropped event of `event_type` on its cached handle,
    /// registering the handle on the type's first drop (Story 16.3, D1);
    /// neither lock is held while the counter increments.
    fn count_drop(&self, event_type: EventType) {
        let cached = self
            .drop_counters
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&event_type)
            .cloned();
        let counter = cached.unwrap_or_else(|| {
            self.drop_counters
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(event_type)
                .or_insert_with(|| {
                    metrics::counter!(
                        "noctis_events_dropped_total",
                        "app" => Arc::clone(&self.config.app),
                        "event_type" => type_label(event_type)
                    )
                })
                .clone()
        });
        counter.increment(1);
    }

    /// Enqueues `inbound` into its partition under that partition's lock
    /// and reports what happened.
    fn enqueue(&self, inbound: Inbound) -> Pushed {
        let partition = &self.partitions[self.partition_of(inbound.shard.number())];
        let config = &self.config;
        let mut queues = partition.lock();
        if !queues.receiver_alive {
            return Pushed::Closed;
        }
        match &inbound.kind {
            InboundKind::Interaction(..) => {
                queues.interaction.push_back(inbound);
                self.totals.interaction.fetch_add(1, Ordering::Relaxed);
                Pushed::Queued(Lane::Interaction)
            }
            InboundKind::Event(event) => {
                let kind = event.kind();
                let droppable = self.is_droppable(kind);
                // A partition with overflow keeps its FIFO: new events queue
                // behind the overflow until it has refilled the lane.
                if queues.overflow.is_empty()
                    && reserve(&self.totals.event, config.event_capacity.get())
                {
                    queues.event.push_back(inbound, droppable);
                    Pushed::Queued(Lane::Event)
                } else if droppable {
                    match queues.event.evict_oldest_droppable() {
                        Some(evicted) => {
                            queues.event.push_back(inbound, true);
                            Pushed::Evicted(evicted)
                        }
                        None => Pushed::DroppedDroppable(kind),
                    }
                } else if reserve(&self.totals.overflow, config.overflow_capacity) {
                    queues.overflow.push_back(inbound);
                    Pushed::Queued(Lane::Overflow)
                } else {
                    Pushed::OverflowFull(kind, inbound)
                }
            }
        }
    }

    /// Takes up to `max` items of partition `index` into `batch` under one
    /// lock; `None` when it is empty and every sender has dropped.
    fn pop_batch(&self, index: usize, batch: &mut VecDeque<Inbound>, max: usize) -> Option<usize> {
        let closed = self.is_closed();
        let mut taken = LaneDepths::default();
        let popped = {
            let mut queues = self.partitions[index].lock();
            let mut popped = 0;
            while popped < max {
                let Some(inbound) = queues.pop(self, &mut taken) else {
                    break;
                };
                batch.push_back(inbound);
                popped += 1;
            }
            self.totals.remove(taken);
            popped
        };
        if popped > 0 {
            self.set_gauges();
            Some(popped)
        } else if closed {
            None
        } else {
            Some(0)
        }
    }

    /// Takes the next item of partition `index` (`interactions` only, or
    /// any).
    fn pop_one(&self, index: usize, take: Take) -> Option<Inbound> {
        let mut taken = LaneDepths::default();
        let item = {
            let mut queues = self.partitions[index].lock();
            let item = match take {
                Take::Interaction => queues.pop_interaction(&mut taken),
                Take::Any => queues.pop(self, &mut taken),
            };
            self.totals.remove(taken);
            item
        };
        if item.is_some() {
            self.set_gauges();
        }
        item
    }

    /// Closes partition `index`: later pushes into it are discarded, and
    /// its queued items are dropped (outside the lock).
    fn close(&self, index: usize) {
        let discarded = {
            let mut queues = self.partitions[index].lock();
            let discarded = queues.close();
            self.totals.remove(discarded.depths());
            discarded
        };
        self.set_gauges();
        drop(discarded);
    }
}

/// What a pop takes.
#[derive(Clone, Copy)]
enum Take {
    Interaction,
    Any,
}

/// The `event_type` label value of `event_type`.
fn type_label(event_type: EventType) -> &'static str {
    event_type.name().unwrap_or("UNKNOWN")
}

/// The event type of a queued event, `None` for anything else.
fn event_type(inbound: &Inbound) -> Option<EventType> {
    match &inbound.kind {
        InboundKind::Event(event) => Some(event.kind()),
        InboundKind::Interaction(..) => None,
    }
}

/// What one push did, reported after the lock is released.
enum Pushed {
    /// The receiver is gone; the item was discarded.
    Closed,
    /// The item was enqueued into this lane.
    Queued(Lane),
    /// The item was enqueued after evicting a droppable event.
    Evicted(Inbound),
    /// The droppable item was dropped: no droppable event was queued.
    DroppedDroppable(EventType),
    /// The non-droppable item was dropped: the overflow is full. It is
    /// carried out of the lock for the [`DropObserver`].
    OverflowFull(EventType, Inbound),
}

/// The shard-task end of the lanes; implements [`InboundSink`].
///
/// Cloning adds a sender; the receiver sees the end of the lanes once every
/// clone has dropped. `Debug` prints only the depths.
///
/// ```
/// use noctis_gateway::{Inbound, InboundKind, InboundSink, LaneConfig, lanes};
/// use twilight_gateway::{Event, ShardId};
///
/// let (sender, mut receiver) = lanes(LaneConfig::default());
/// sender.push(Inbound {
///     shard: ShardId::ONE,
///     received_at: tokio::time::Instant::now(),
///     kind: InboundKind::Event(Box::new(Event::GatewayHeartbeatAck)),
/// });
/// assert_eq!(receiver.depths().event, 1);
/// assert!(format!("{sender:?}").contains("event: 1"));
/// ```
pub struct LaneSender {
    shared: Arc<Shared>,
}

impl InboundSink for LaneSender {
    fn push(&self, inbound: Inbound) {
        let shared = &*self.shared;
        let index = shared.partition_of(inbound.shard.number());
        let lane = match shared.enqueue(inbound) {
            Pushed::Closed => return,
            Pushed::Queued(lane) => lane,
            Pushed::Evicted(evicted) => {
                if let Some(kind) = event_type(&evicted) {
                    shared.count_drop(kind);
                    tracing::debug!(
                        target: "noctis::lanes",
                        event_type = type_label(kind),
                        "event lane is full; oldest droppable event dropped"
                    );
                }
                drop(evicted);
                Lane::Event
            }
            Pushed::DroppedDroppable(kind) => {
                shared.count_drop(kind);
                tracing::debug!(
                    target: "noctis::lanes",
                    event_type = type_label(kind),
                    "event lane is full; droppable event dropped"
                );
                return;
            }
            Pushed::OverflowFull(kind, dropped) => {
                shared.count_drop(kind);
                if let (Some(observer), InboundKind::Event(event)) =
                    (&shared.config.drop_observer, &dropped.kind)
                {
                    observer.overflow_dropped(dropped.shard.number(), event);
                } else {
                    tracing::error!(
                        target: "noctis::lanes",
                        event_type = type_label(kind),
                        overflow_capacity = shared.config.overflow_capacity,
                        "event overflow is full; event dropped"
                    );
                }
                return;
            }
        };
        shared.set_gauge(lane);
        shared.partitions[index].notify.notify_one();
    }
}

impl Clone for LaneSender {
    fn clone(&self) -> Self {
        self.shared.senders.fetch_add(1, Ordering::SeqCst);
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for LaneSender {
    fn drop(&mut self) {
        if self.shared.senders.fetch_sub(1, Ordering::SeqCst) == 1 {
            for partition in &self.shared.partitions {
                partition.notify.notify_one();
            }
        }
    }
}

impl fmt::Debug for LaneSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneSender")
            .field("depths", &self.shared.depths())
            .finish()
    }
}

/// Core's end of the lanes: a single consumer that always takes
/// interactions before events.
///
/// With several partitions it takes the interactions of every partition
/// before any event, and keeps each partition's order;
/// [`into_partitions`](Self::into_partitions) splits it into one consumer
/// per partition instead. Dropping it discards every queued item, and later
/// pushes are discarded without being counted. `Debug` prints only the
/// depths.
///
/// ```
/// use noctis_gateway::{Inbound, InboundKind, InboundSink, LaneConfig, lanes};
/// use twilight_gateway::{Event, ShardId};
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
/// let (sender, mut receiver) = lanes(LaneConfig::default());
/// sender.push(Inbound {
///     shard: ShardId::ONE,
///     received_at: tokio::time::Instant::now(),
///     kind: InboundKind::Event(Box::new(Event::GatewayHeartbeatAck)),
/// });
/// drop(sender);
/// assert!(receiver.recv().await.is_some());
/// assert!(receiver.recv().await.is_none());
/// # }
/// ```
pub struct LaneReceiver {
    shared: Arc<Shared>,
    /// Set by `into_partitions`: the partitions own the queues now.
    split: bool,
    /// The partition whose events are taken first next time.
    cursor: usize,
}

/// One attempt to take an item.
enum Popped {
    Item(Inbound),
    Empty,
    Closed,
}

impl LaneReceiver {
    /// Waits for the next item: interactions first, then events in order.
    ///
    /// Returns `None` once every [`LaneSender`] has dropped and the lanes
    /// are empty. Cancel-safe: dropping the future loses no item.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let (sender, mut receiver) = noctis_gateway::lanes(Default::default());
    /// drop(sender);
    /// assert!(receiver.recv().await.is_none());
    /// # }
    /// ```
    pub async fn recv(&mut self) -> Option<Inbound> {
        self.recv_taking(Take::Any).await
    }

    /// Waits for the next interaction only; queued events stay queued.
    ///
    /// Returns `None` once every [`LaneSender`] has dropped and the
    /// Interaction Lane is empty, even while events remain. Cancel-safe:
    /// dropping the future loses no item. Core awaits it while it waits for
    /// a worker permit, so interactions never wait behind events.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let (sender, mut receiver) = noctis_gateway::lanes(Default::default());
    /// drop(sender);
    /// assert!(receiver.recv_interaction().await.is_none());
    /// # }
    /// ```
    pub async fn recv_interaction(&mut self) -> Option<Inbound> {
        self.recv_taking(Take::Interaction).await
    }

    /// Takes the next item without waiting, if one is queued.
    ///
    /// ```
    /// let (_sender, mut receiver) = noctis_gateway::lanes(Default::default());
    /// assert!(receiver.try_recv().is_none());
    /// ```
    pub fn try_recv(&mut self) -> Option<Inbound> {
        match self.pop(Take::Any) {
            Popped::Item(inbound) => Some(inbound),
            Popped::Empty | Popped::Closed => None,
        }
    }

    /// How many items each lane holds now, summed over every partition.
    ///
    /// ```
    /// let (_sender, receiver) = noctis_gateway::lanes(Default::default());
    /// assert_eq!(receiver.depths(), noctis_gateway::LaneDepths::default());
    /// ```
    pub fn depths(&self) -> LaneDepths {
        self.shared.depths()
    }

    /// Splits the receiver into one consumer per partition, in partition
    /// order; each takes the interactions of its partition before its
    /// events.
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    ///
    /// let (_sender, receiver) =
    ///     noctis_gateway::lanes_with(Default::default(), NonZeroUsize::new(3).unwrap());
    /// let partitions = receiver.into_partitions();
    /// assert_eq!(partitions.iter().map(|p| p.index()).collect::<Vec<_>>(), [0, 1, 2]);
    /// ```
    pub fn into_partitions(mut self) -> Vec<LanePartition> {
        self.split = true;
        (0..self.shared.partitions.len())
            .map(|index| LanePartition {
                shared: Arc::clone(&self.shared),
                index,
            })
            .collect()
    }

    async fn recv_taking(&mut self, take: Take) -> Option<Inbound> {
        loop {
            match self.pop(take) {
                Popped::Item(inbound) => return Some(inbound),
                Popped::Closed => return None,
                Popped::Empty => {}
            }
            let shared = Arc::clone(&self.shared);
            if let [partition] = &*shared.partitions {
                partition.notify.notified().await;
                continue;
            }
            // Register with every partition before checking again, so a
            // push between the check and the wait still wakes this task.
            let mut waits: Vec<Pin<Box<Notified<'_>>>> = shared
                .partitions
                .iter()
                .map(|partition| Box::pin(partition.notify.notified()))
                .collect();
            for wait in &mut waits {
                wait.as_mut().enable();
            }
            match self.pop(take) {
                Popped::Item(inbound) => return Some(inbound),
                Popped::Closed => return None,
                Popped::Empty => {}
            }
            std::future::poll_fn(|cx| {
                if waits
                    .iter_mut()
                    .any(|wait| wait.as_mut().poll(cx).is_ready())
                {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
    }

    /// Takes the interactions of every partition first, then the events of
    /// the partitions in turn, starting at the cursor.
    fn pop(&mut self, take: Take) -> Popped {
        let closed = self.shared.is_closed();
        let count = self.shared.partitions.len();
        if count == 1 {
            return match self.shared.pop_one(0, take) {
                Some(inbound) => Popped::Item(inbound),
                None if closed => Popped::Closed,
                None => Popped::Empty,
            };
        }
        for offset in 0..count {
            let index = (self.cursor + offset) % count;
            if let Some(inbound) = self.shared.pop_one(index, Take::Interaction) {
                return Popped::Item(inbound);
            }
        }
        if let Take::Any = take {
            for offset in 0..count {
                let index = (self.cursor + offset) % count;
                if let Some(inbound) = self.shared.pop_one(index, Take::Any) {
                    self.cursor = (index + 1) % count;
                    return Popped::Item(inbound);
                }
            }
        }
        if closed {
            Popped::Closed
        } else {
            Popped::Empty
        }
    }
}

impl Drop for LaneReceiver {
    fn drop(&mut self) {
        if self.split {
            return;
        }
        for index in 0..self.shared.partitions.len() {
            self.shared.close(index);
        }
    }
}

impl fmt::Debug for LaneReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneReceiver")
            .field("depths", &self.shared.depths())
            .finish_non_exhaustive()
    }
}

/// The consumer of one partition of the lanes, from
/// [`LaneReceiver::into_partitions`].
///
/// It takes the items of the shards `s` with `s % partitions == index`:
/// interactions first, then events in arrival order. Dropping it discards
/// the partition's queued items, and later pushes into the partition are
/// discarded without being counted. `Debug` prints the index and depths.
///
/// ```
/// use std::collections::{HashMap, VecDeque};
/// use std::num::NonZeroUsize;
///
/// use noctis_gateway::{Inbound, InboundKind, InboundSink, LaneConfig, lanes};
/// use twilight_gateway::{Event, ShardId};
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
/// let (sender, receiver) = lanes(LaneConfig::default());
/// sender.push(Inbound {
///     shard: ShardId::ONE,
///     received_at: tokio::time::Instant::now(),
///     kind: InboundKind::Event(Box::new(Event::GatewayHeartbeatAck)),
/// });
/// drop(sender);
/// let mut partition = receiver.into_partitions().remove(0);
/// let mut batch = VecDeque::new();
/// assert_eq!(partition.recv_batch(&mut batch, NonZeroUsize::new(32).unwrap()).await, 1);
/// assert_eq!(partition.recv_batch(&mut batch, NonZeroUsize::new(32).unwrap()).await, 0);
/// # }
/// ```
pub struct LanePartition {
    shared: Arc<Shared>,
    index: usize,
}

impl LanePartition {
    /// This partition's index.
    ///
    /// ```
    /// let (_sender, receiver) = noctis_gateway::lanes(Default::default());
    /// assert_eq!(receiver.into_partitions()[0].index(), 0);
    /// ```
    pub fn index(&self) -> usize {
        self.index
    }

    /// Waits until the partition holds an item, then appends up to `max`
    /// items to `batch` under one lock: interactions first, then events in
    /// order. Returns how many it appended; 0 once every [`LaneSender`] has
    /// dropped and the partition is empty. Cancel-safe: dropping the future
    /// loses no item.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// use std::collections::{HashMap, VecDeque};
    /// use std::num::NonZeroUsize;
    ///
    /// let (sender, receiver) = noctis_gateway::lanes(Default::default());
    /// drop(sender);
    /// let mut partition = receiver.into_partitions().remove(0);
    /// let mut batch = VecDeque::new();
    /// assert_eq!(partition.recv_batch(&mut batch, NonZeroUsize::MIN).await, 0);
    /// # }
    /// ```
    pub async fn recv_batch(&mut self, batch: &mut VecDeque<Inbound>, max: NonZeroUsize) -> usize {
        loop {
            match self.shared.pop_batch(self.index, batch, max.get()) {
                None => return 0,
                Some(0) => self.shared.partitions[self.index].notify.notified().await,
                Some(popped) => return popped,
            }
        }
    }

    /// Appends up to `max` queued items to `batch` without waiting, as
    /// [`recv_batch`](Self::recv_batch) takes them; returns how many.
    ///
    /// ```
    /// use std::collections::{HashMap, VecDeque};
    /// use std::num::NonZeroUsize;
    ///
    /// let (_sender, receiver) = noctis_gateway::lanes(Default::default());
    /// let mut partition = receiver.into_partitions().remove(0);
    /// let mut batch = VecDeque::new();
    /// assert_eq!(partition.try_recv_batch(&mut batch, NonZeroUsize::MIN), 0);
    /// ```
    pub fn try_recv_batch(&mut self, batch: &mut VecDeque<Inbound>, max: NonZeroUsize) -> usize {
        self.shared
            .pop_batch(self.index, batch, max.get())
            .unwrap_or(0)
    }

    /// Waits for the next interaction of this partition; queued events stay
    /// queued.
    ///
    /// Returns `None` once every [`LaneSender`] has dropped and the
    /// partition holds no interaction. Cancel-safe: dropping the future
    /// loses no item.
    ///
    /// ```
    /// # #[tokio::main(flavor = "current_thread")] async fn main() {
    /// let (sender, receiver) = noctis_gateway::lanes(Default::default());
    /// drop(sender);
    /// let mut partition = receiver.into_partitions().remove(0);
    /// assert!(partition.recv_interaction().await.is_none());
    /// # }
    /// ```
    pub async fn recv_interaction(&mut self) -> Option<Inbound> {
        loop {
            let closed = self.shared.is_closed();
            if let Some(inbound) = self.shared.pop_one(self.index, Take::Interaction) {
                return Some(inbound);
            }
            if closed {
                return None;
            }
            self.shared.partitions[self.index].notify.notified().await;
        }
    }

    /// How many items each lane of this partition holds now.
    ///
    /// ```
    /// let (_sender, receiver) = noctis_gateway::lanes(Default::default());
    /// let partition = receiver.into_partitions().remove(0);
    /// assert_eq!(partition.depths(), noctis_gateway::LaneDepths::default());
    /// ```
    pub fn depths(&self) -> LaneDepths {
        self.shared.partitions[self.index].lock().depths()
    }
}

impl Drop for LanePartition {
    fn drop(&mut self) {
        self.shared.close(self.index);
    }
}

impl fmt::Debug for LanePartition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LanePartition")
            .field("index", &self.index)
            .field("depths", &self.depths())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod partition_tests;
