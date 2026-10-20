//! What shard tasks hand to core: [`Inbound`] items through an
//! [`InboundSink`], and [`ShardEvent`]s (AD-10, FR-2).

use std::fmt;

use bytes::Bytes;
use tokio::time::Instant;
use twilight_gateway::{Event, ShardId};
use twilight_model::application::interaction::Interaction;

/// One gateway item a shard task parsed, stamped with when its frame arrived.
///
/// `Debug` never prints the interaction or event payload.
///
/// ```
/// use noctis_gateway::{Inbound, InboundKind};
/// use twilight_gateway::{Event, ShardId};
///
/// let inbound = Inbound {
///     shard: ShardId::ONE,
///     received_at: tokio::time::Instant::now(),
///     kind: InboundKind::Event(Box::new(Event::GatewayHeartbeatAck)),
/// };
/// assert_eq!(inbound.shard.number(), 0);
/// ```
pub struct Inbound {
    /// The shard that received it.
    pub shard: ShardId,
    /// When its gateway frame was read, before parsing.
    pub received_at: Instant,
    /// The interaction or event.
    pub kind: InboundKind,
}

impl fmt::Debug for Inbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inbound")
            .field("shard", &self.shard)
            .field("received_at", &self.received_at)
            .field("kind", &self.kind)
            .finish()
    }
}

/// What an [`Inbound`] carries.
///
/// `Debug` prints only the variant and the event type, never the payload.
///
/// ```
/// use noctis_gateway::InboundKind;
/// use twilight_gateway::Event;
///
/// let kind = InboundKind::Event(Box::new(Event::GatewayHeartbeatAck));
/// assert!(format!("{kind:?}").contains("GatewayHeartbeatAck"));
/// ```
#[non_exhaustive]
pub enum InboundKind {
    /// An `INTERACTION_CREATE`, with the exact raw bytes of its `d` field.
    Interaction(Box<Interaction>, RawJson),
    /// Any other wanted dispatch event.
    Event(Box<Event>),
}

impl fmt::Debug for InboundKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interaction(_, raw) => f.debug_tuple("Interaction").field(raw).finish(),
            Self::Event(event) => f.debug_tuple("Event").field(&event.kind()).finish(),
        }
    }
}

/// The raw JSON bytes of a gateway payload's `d` field.
///
/// `Debug` prints only the length, never the content.
///
/// ```
/// use bytes::Bytes;
/// use noctis_gateway::RawJson;
///
/// let raw = RawJson::new(Bytes::from_static(br#"{"id":"1"}"#));
/// assert_eq!(raw.as_bytes(), br#"{"id":"1"}"#);
/// assert_eq!(format!("{raw:?}"), "RawJson { len: 10 }");
/// assert_eq!(raw.into_bytes().len(), 10);
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct RawJson(Bytes);

impl RawJson {
    /// Wraps `bytes`.
    ///
    /// ```
    /// let raw = noctis_gateway::RawJson::new(bytes::Bytes::from_static(b"{}"));
    /// ```
    pub fn new(bytes: Bytes) -> Self {
        Self(bytes)
    }

    /// The bytes.
    ///
    /// ```
    /// let raw = noctis_gateway::RawJson::new(bytes::Bytes::from_static(b"{}"));
    /// assert_eq!(raw.as_bytes(), b"{}");
    /// ```
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The bytes, by value.
    ///
    /// ```
    /// let raw = noctis_gateway::RawJson::new(bytes::Bytes::from_static(b"{}"));
    /// assert_eq!(raw.into_bytes(), bytes::Bytes::from_static(b"{}"));
    /// ```
    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl fmt::Debug for RawJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawJson")
            .field("len", &self.0.len())
            .finish()
    }
}

/// Receives every [`Inbound`] the shard tasks parse.
///
/// `push` runs on the shard task that read the frame, between socket reads
/// and heartbeats. It must never block or await: hand the item to a bounded
/// queue with `try_send` (or drop it), and never wait for capacity or for a
/// handler.
///
/// ```
/// use noctis_gateway::{Inbound, InboundSink};
///
/// struct Discard;
///
/// impl InboundSink for Discard {
///     fn push(&self, _inbound: Inbound) {}
/// }
/// ```
pub trait InboundSink: Send + Sync + 'static {
    /// Takes one item; never blocks or awaits.
    fn push(&self, inbound: Inbound);
}

/// A shard's status change, reported on the channel [`Gateway::start`](crate::Gateway::start)
/// returns. Each shard task ends with exactly one `Ended` or `Closed`.
///
/// ```
/// use noctis_gateway::ShardEvent;
///
/// let event = ShardEvent::Ended { shard: 0, close_code: Some(4014) };
/// assert_ne!(event, ShardEvent::Ready { shard: 0 });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ShardEvent {
    /// The shard received READY (a new session).
    Ready {
        /// The shard number.
        shard: u32,
    },
    /// The shard received RESUMED.
    Resumed {
        /// The shard number.
        shard: u32,
    },
    /// The connection closed; twilight reconnects unless the code is fatal.
    Disconnected {
        /// The shard number.
        shard: u32,
        /// The close code, if the close frame carried one.
        close_code: Option<u16>,
    },
    /// The shard stopped on its own (a fatal close code, or a stream that
    /// ended); its task ended. Never sent after
    /// [`Gateway::close_all`](crate::Gateway::close_all) stopped the shard.
    Ended {
        /// The shard number.
        shard: u32,
        /// The last close code the shard saw, if any.
        close_code: Option<u16>,
    },
    /// The shard stopped because [`Gateway::close_all`](crate::Gateway::close_all)
    /// asked it to; its task ended. Sent instead of `Disconnected` and
    /// `Ended` for the requested close.
    Closed {
        /// The shard number.
        shard: u32,
    },
}
