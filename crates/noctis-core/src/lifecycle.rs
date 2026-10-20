//! The crate-internal lifecycle seam (AD-21): core turns the gateway's
//! `ShardEvent`s and its own shutdown into [`LifecycleEvent`]s on a
//! `broadcast` channel. Story 13.5 attaches the public Lifecycle Hooks here.

use noctis_gateway::ShardEvent;
use tokio::sync::broadcast;

/// How many lifecycle events a slow receiver may fall behind before it
/// sees `Lagged`.
pub(crate) const LIFECYCLE_CAPACITY: usize = 256;

/// One App lifecycle change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleEvent {
    /// A shard received READY.
    ShardReady { shard: u32 },
    /// A shard received RESUMED.
    ShardResumed { shard: u32 },
    /// A shard's connection closed; twilight reconnects.
    ShardDisconnected { shard: u32, close_code: Option<u16> },
    /// A shard stopped on its own for good (a fatal close).
    ShardFatal { shard: u32, close_code: Option<u16> },
    /// The shutdown sequence started.
    ShutdownStarted,
}

impl LifecycleEvent {
    /// The lifecycle event of a shard event; `Closed` (a requested stop)
    /// has none.
    pub(crate) fn from_shard(event: &ShardEvent) -> Option<Self> {
        Some(match *event {
            ShardEvent::Ready { shard } => Self::ShardReady { shard },
            ShardEvent::Resumed { shard } => Self::ShardResumed { shard },
            ShardEvent::Disconnected { shard, close_code } => {
                Self::ShardDisconnected { shard, close_code }
            }
            ShardEvent::Ended { shard, close_code } => Self::ShardFatal { shard, close_code },
            _ => return None,
        })
    }
}

/// The App's lifecycle broadcast; sending never blocks or fails.
#[derive(Debug, Clone)]
pub(crate) struct Lifecycle {
    sender: broadcast::Sender<LifecycleEvent>,
}

impl Lifecycle {
    pub(crate) fn new() -> Self {
        Self {
            sender: broadcast::channel(LIFECYCLE_CAPACITY).0,
        }
    }

    /// Broadcasts `event`; without receivers it is dropped.
    pub(crate) fn send(&self, event: LifecycleEvent) {
        let _ = self.sender.send(event);
    }

    /// Broadcasts the lifecycle event of `event`, if it has one.
    pub(crate) fn shard(&self, event: &ShardEvent) {
        if let Some(event) = LifecycleEvent::from_shard(event) {
            self.send(event);
        }
    }

    /// A new receiver.
    #[cfg(test)]
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<LifecycleEvent> {
        self.sender.subscribe()
    }
}
