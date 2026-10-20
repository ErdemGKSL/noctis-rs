//! The Interaction Lane and the bounded event lane between shard tasks and
//! core (FR-5, AD-10, NFR-5).
//!
//! [`lanes`] returns a [`LaneSender`], the [`InboundSink`] shard tasks push
//! into, and a single-consumer [`LaneReceiver`] that always drains
//! interactions first. One `std::sync::Mutex` guards three queues; `push`
//! never awaits, and metrics and logs are emitted after the lock is released.

use std::collections::VecDeque;
use std::fmt;
use std::mem;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use metrics::Gauge;
use tokio::sync::Notify;
use twilight_gateway::{EventType, EventTypeFlags};

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

/// How [`lanes`] bounds the event lane and labels its metrics.
///
/// ```
/// use noctis_gateway::{DEFAULT_EVENT_CAPACITY, DROPPABLE_EVENTS, LaneConfig};
///
/// let config = LaneConfig::default();
/// assert_eq!(&*config.app, "default");
/// assert_eq!(config.event_capacity, DEFAULT_EVENT_CAPACITY);
/// assert_eq!(config.overflow_capacity, 16384);
/// assert_eq!(config.droppable, DROPPABLE_EVENTS);
/// ```
#[derive(Debug, Clone)]
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
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            app: Arc::from("default"),
            event_capacity: DEFAULT_EVENT_CAPACITY,
            overflow_capacity: DEFAULT_OVERFLOW_CAPACITY,
            droppable: DROPPABLE_EVENTS,
        }
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

/// Creates the lanes: a [`LaneSender`] for the shard tasks and the
/// [`LaneReceiver`] core drains.
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
    let app = Arc::clone(&config.app);
    let gauges = Gauges {
        interaction: metrics::gauge!("noctis_lane_depth", "app" => Arc::clone(&app), "lane" => "interaction"),
        event: metrics::gauge!("noctis_lane_depth", "app" => Arc::clone(&app), "lane" => "event"),
        overflow: metrics::gauge!("noctis_event_overflow_depth", "app" => app),
    };
    gauges.set(LaneDepths::default());
    let shared = Arc::new(Shared {
        queues: Mutex::new(Queues {
            interaction: VecDeque::new(),
            event: VecDeque::new(),
            overflow: VecDeque::new(),
            senders: 1,
            receiver_alive: true,
        }),
        notify: Notify::new(),
        config,
        gauges,
    });
    (
        LaneSender {
            shared: Arc::clone(&shared),
        },
        LaneReceiver { shared },
    )
}

/// The three queues and the liveness of both ends.
struct Queues {
    interaction: VecDeque<Inbound>,
    event: VecDeque<Inbound>,
    overflow: VecDeque<Inbound>,
    senders: usize,
    receiver_alive: bool,
}

impl Queues {
    fn depths(&self) -> LaneDepths {
        LaneDepths {
            interaction: self.interaction.len(),
            event: self.event.len(),
            overflow: self.overflow.len(),
        }
    }
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

/// State shared by the sender clones and the receiver.
struct Shared {
    queues: Mutex<Queues>,
    notify: Notify,
    config: LaneConfig,
    gauges: Gauges,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Queues> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn depths(&self) -> LaneDepths {
        self.lock().depths()
    }

    fn is_droppable(&self, event_type: EventType) -> bool {
        self.config
            .droppable
            .intersects(EventTypeFlags::from(event_type))
    }

    /// Counts one dropped event of `event_type`.
    fn count_drop(&self, event_type: EventType) {
        metrics::counter!(
            "noctis_events_dropped_total",
            "app" => Arc::clone(&self.config.app),
            "event_type" => type_label(event_type)
        )
        .increment(1);
    }
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
    /// The item was enqueued.
    Queued,
    /// The item was enqueued after evicting a droppable event.
    Evicted(Inbound),
    /// The droppable item was dropped: no droppable event was queued.
    DroppedDroppable(EventType),
    /// The non-droppable item was dropped: the overflow is full.
    OverflowFull(EventType),
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

impl LaneSender {
    /// Enqueues `inbound` under the lock and reports what happened.
    fn enqueue(&self, inbound: Inbound) -> (Pushed, LaneDepths) {
        let config = &self.shared.config;
        let mut queues = self.shared.lock();
        if !queues.receiver_alive {
            return (Pushed::Closed, LaneDepths::default());
        }
        let pushed = match &inbound.kind {
            InboundKind::Interaction(..) => {
                queues.interaction.push_back(inbound);
                Pushed::Queued
            }
            InboundKind::Event(event) => {
                let kind = event.kind();
                let full = !queues.overflow.is_empty()
                    || queues.event.len() >= config.event_capacity.get();
                if !full {
                    queues.event.push_back(inbound);
                    Pushed::Queued
                } else if self.shared.is_droppable(kind) {
                    let oldest = queues.event.iter().position(|queued| {
                        event_type(queued).is_some_and(|t| self.shared.is_droppable(t))
                    });
                    match oldest.and_then(|index| queues.event.remove(index)) {
                        Some(evicted) => {
                            queues.event.push_back(inbound);
                            Pushed::Evicted(evicted)
                        }
                        None => Pushed::DroppedDroppable(kind),
                    }
                } else if queues.overflow.len() < config.overflow_capacity {
                    queues.overflow.push_back(inbound);
                    Pushed::Queued
                } else {
                    Pushed::OverflowFull(kind)
                }
            }
        };
        (pushed, queues.depths())
    }
}

impl InboundSink for LaneSender {
    fn push(&self, inbound: Inbound) {
        let (pushed, depths) = self.enqueue(inbound);
        let shared = &*self.shared;
        match pushed {
            Pushed::Closed => return,
            Pushed::Queued => {}
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
            Pushed::OverflowFull(kind) => {
                shared.count_drop(kind);
                tracing::error!(
                    target: "noctis::lanes",
                    event_type = type_label(kind),
                    overflow_capacity = shared.config.overflow_capacity,
                    "event overflow is full; event dropped"
                );
                return;
            }
        }
        shared.gauges.set(depths);
        shared.notify.notify_one();
    }
}

impl Clone for LaneSender {
    fn clone(&self) -> Self {
        self.shared.lock().senders += 1;
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for LaneSender {
    fn drop(&mut self) {
        let last = {
            let mut queues = self.shared.lock();
            queues.senders = queues.senders.saturating_sub(1);
            queues.senders == 0
        };
        if last {
            self.shared.notify.notify_one();
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
/// Dropping it discards every queued item, and later pushes are discarded
/// without being counted. `Debug` prints only the depths.
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
        loop {
            match self.pop() {
                Popped::Item(inbound) => return Some(inbound),
                Popped::Closed => return None,
                Popped::Empty => self.shared.notify.notified().await,
            }
        }
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
        loop {
            match self.pop_interaction() {
                Popped::Item(inbound) => return Some(inbound),
                Popped::Closed => return None,
                Popped::Empty => self.shared.notify.notified().await,
            }
        }
    }

    /// Takes the next item without waiting, if one is queued.
    ///
    /// ```
    /// let (_sender, mut receiver) = noctis_gateway::lanes(Default::default());
    /// assert!(receiver.try_recv().is_none());
    /// ```
    pub fn try_recv(&mut self) -> Option<Inbound> {
        match self.pop() {
            Popped::Item(inbound) => Some(inbound),
            Popped::Empty | Popped::Closed => None,
        }
    }

    /// How many items each lane holds now.
    ///
    /// ```
    /// let (_sender, receiver) = noctis_gateway::lanes(Default::default());
    /// assert_eq!(receiver.depths(), noctis_gateway::LaneDepths::default());
    /// ```
    pub fn depths(&self) -> LaneDepths {
        self.shared.depths()
    }

    fn pop_interaction(&self) -> Popped {
        let (item, depths) = {
            let mut queues = self.shared.lock();
            let Some(item) = queues.interaction.pop_front() else {
                return if queues.senders == 0 {
                    Popped::Closed
                } else {
                    Popped::Empty
                };
            };
            (item, queues.depths())
        };
        self.shared.gauges.set(depths);
        Popped::Item(item)
    }

    fn pop(&self) -> Popped {
        let (item, depths) = {
            let mut queues = self.shared.lock();
            let item = if let Some(inbound) = queues.interaction.pop_front() {
                inbound
            } else if let Some(inbound) = queues.event.pop_front() {
                if let Some(refill) = queues.overflow.pop_front() {
                    queues.event.push_back(refill);
                }
                inbound
            } else if queues.senders == 0 {
                return Popped::Closed;
            } else {
                return Popped::Empty;
            };
            (item, queues.depths())
        };
        self.shared.gauges.set(depths);
        Popped::Item(item)
    }
}

impl Drop for LaneReceiver {
    fn drop(&mut self) {
        let queued = {
            let mut queues = self.shared.lock();
            queues.receiver_alive = false;
            (
                mem::take(&mut queues.interaction),
                mem::take(&mut queues.event),
                mem::take(&mut queues.overflow),
            )
        };
        self.shared.gauges.set(LaneDepths::default());
        drop(queued);
    }
}

impl fmt::Debug for LaneReceiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneReceiver")
            .field("depths", &self.shared.depths())
            .finish()
    }
}

#[cfg(test)]
mod tests;
