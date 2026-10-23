//! [`Gateway`]: one tokio task per shard (FR-2, GW-01).

use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use noctis_ports::IdentifyQueue;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use twilight_gateway::{CloseFrame, EventTypeFlags, Intents, Shard, ShardId};

use crate::inbound::{InboundSink, ShardEvent};
use crate::queue::PortQueue;
use crate::{ShardConfigError, shard, shard_config};

/// How [`Gateway::close_all`] closes the shards.
///
/// ```
/// use noctis_gateway::CloseMode;
///
/// assert_eq!(CloseMode::default(), CloseMode::Resumable);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseMode {
    /// Close code 4000: Discord keeps the session, so a restarted process
    /// can resume it.
    #[default]
    Resumable,
    /// Close code 1000: Discord ends the session and shows the bot offline.
    Fresh,
}

impl CloseMode {
    /// The close frame this mode sends.
    pub(crate) const fn frame(self) -> CloseFrame<'static> {
        match self {
            Self::Resumable => CloseFrame::RESUME,
            Self::Fresh => CloseFrame::NORMAL,
        }
    }
}

/// Everything [`Gateway::start`] needs. `Debug` redacts the token.
///
/// ```
/// use std::num::{NonZeroU16, NonZeroU32};
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// use noctis_gateway::{EventTypeFlags, GatewayOptions};
/// use noctis_ports::memory::MemoryIdentifyQueue;
/// use twilight_gateway::Intents;
///
/// let options = GatewayOptions {
///     app: Arc::from("default"),
///     token: "SECRET".to_owned(),
///     intents: Intents::GUILDS,
///     gateway_url: None,
///     event_types: EventTypeFlags::INTERACTION_CREATE,
///     identify_queue: Arc::new(MemoryIdentifyQueue::default()),
///     shards: 0..1,
///     total: NonZeroU32::MIN,
///     max_concurrency: NonZeroU16::MIN,
///     close_timeout: Duration::from_secs(5),
/// };
/// assert!(!format!("{options:?}").contains("SECRET"));
/// ```
#[derive(Clone)]
pub struct GatewayOptions {
    /// The `app` field of every `noctis.shard` span.
    pub app: Arc<str>,
    /// The bot token (a `Bot ` prefix is optional).
    pub token: String,
    /// The intents every shard identifies with.
    pub intents: Intents,
    /// A gateway URL replacing Discord's (`ws[s]://host[:port]`), if any.
    pub gateway_url: Option<String>,
    /// The dispatch event types parsed into [`Inbound`](crate::Inbound) items.
    pub event_types: EventTypeFlags,
    /// The identify coordination port.
    pub identify_queue: Arc<dyn IdentifyQueue>,
    /// The shard ids this process runs, within `0..total` (half-open, never
    /// empty): `0..total` for a single process, one Cluster process's Shard
    /// Range otherwise. Ids are global, so a shard's bucket is always
    /// `id % max_concurrency`.
    pub shards: Range<u32>,
    /// The shard total, shared by every process of a Cluster.
    pub total: NonZeroU32,
    /// Discord's `session_start_limit.max_concurrency` (identify buckets).
    pub max_concurrency: NonZeroU16,
    /// How long a shard closed by [`Gateway::close_all`] waits for the
    /// gateway's close frame before it stops anyway.
    pub close_timeout: Duration,
}

impl fmt::Debug for GatewayOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayOptions")
            .field("app", &self.app)
            .field("token", &"<redacted>")
            .field("intents", &self.intents)
            .field("gateway_url", &self.gateway_url)
            .field("event_types", &self.event_types)
            .field("shards", &self.shards)
            .field("total", &self.total)
            .field("max_concurrency", &self.max_concurrency)
            .field("close_timeout", &self.close_timeout)
            .finish_non_exhaustive()
    }
}

/// The running shards: one tokio task each. Dropping it aborts every task.
///
/// [`close_all`](Self::close_all) asks every shard to stop and
/// [`closed`](Self::closed) waits until every shard task has ended.
///
/// ```no_run
/// use std::num::{NonZeroU16, NonZeroU32};
/// use std::sync::Arc;
/// use std::time::Duration;
///
/// use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink};
/// use noctis_ports::memory::MemoryIdentifyQueue;
/// use twilight_gateway::Intents;
///
/// struct Discard;
/// impl InboundSink for Discard {
///     fn push(&self, _: Inbound) {}
/// }
///
/// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
/// let options = GatewayOptions {
///     app: Arc::from("default"),
///     token: "token".to_owned(),
///     intents: Intents::GUILDS,
///     gateway_url: None,
///     event_types: EventTypeFlags::INTERACTION_CREATE,
///     identify_queue: Arc::new(MemoryIdentifyQueue::default()),
///     shards: 0..1,
///     total: NonZeroU32::MIN,
///     max_concurrency: NonZeroU16::MIN,
///     close_timeout: Duration::from_secs(5),
/// };
/// let (gateway, mut events) = Gateway::start(options, Arc::new(Discard))?;
/// while let Some(event) = events.recv().await {
///     println!("{event:?}");
/// }
/// # Ok(()) }
/// ```
pub struct Gateway {
    tasks: JoinSet<()>,
    shards: Range<u32>,
    total: u32,
    /// The requested close; `None` until [`Gateway::close_all`].
    close: watch::Sender<Option<CloseMode>>,
}

impl Gateway {
    /// Starts one task per shard in `options.shards`, each identifying as
    /// `[id, total]` and built through
    /// [`shard_config`] with an identify queue over `options.identify_queue`
    /// (bucket `shard % max_concurrency`).
    ///
    /// Each task pushes its parsed items into `sink` and reports
    /// [`ShardEvent`]s on the returned channel. Only the tasks hold senders,
    /// so the channel closes once every task has ended.
    ///
    /// # Errors
    ///
    /// [`ShardConfigError::InvalidShardRange`] when `options.shards` is empty
    /// or ends past `options.total`, and any other [`ShardConfigError`] when
    /// [`shard_config`] rejects the settings; no task is started then.
    ///
    /// ```no_run
    /// # use std::{num::{NonZeroU16, NonZeroU32}, sync::Arc};
    /// # use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink};
    /// # struct Discard;
    /// # impl InboundSink for Discard { fn push(&self, _: Inbound) {} }
    /// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
    /// # let options = GatewayOptions {
    /// #     app: Arc::from("default"),
    /// #     token: "token".to_owned(),
    /// #     intents: twilight_gateway::Intents::GUILDS,
    /// #     gateway_url: None,
    /// #     event_types: EventTypeFlags::all(),
    /// #     identify_queue: Arc::new(noctis_ports::memory::MemoryIdentifyQueue::default()),
    /// #     shards: 0..1,
    /// #     total: NonZeroU32::MIN,
    /// #     max_concurrency: NonZeroU16::MIN,
    /// #     close_timeout: std::time::Duration::from_secs(5),
    /// # };
    /// let (gateway, events) = Gateway::start(options, Arc::new(Discard))?;
    /// assert_eq!(gateway.shard_total(), 1);
    /// # Ok(()) }
    /// ```
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the Gateway takes the sink; each shard task holds a clone"
    )]
    pub fn start(
        options: GatewayOptions,
        sink: Arc<dyn InboundSink>,
    ) -> Result<(Self, mpsc::UnboundedReceiver<ShardEvent>), ShardConfigError> {
        let total = options.total.get();
        let shards = options.shards;
        if shards.is_empty() || shards.end > total {
            return Err(ShardConfigError::InvalidShardRange);
        }
        let queue = PortQueue::new(options.identify_queue, options.max_concurrency);
        let config = shard_config(
            &options.token,
            options.intents,
            options.gateway_url.as_deref(),
            queue,
        )?;
        let (tx, rx) = mpsc::unbounded_channel();
        let (close, close_rx) = watch::channel(None);
        let mut tasks = JoinSet::new();
        for id in shards
            .clone()
            .filter_map(|number| ShardId::new_checked(number, total))
        {
            let shard = Shard::with_config(id, config.clone());
            tasks.spawn(shard::run(
                shard,
                Arc::clone(&options.app),
                options.event_types,
                Arc::clone(&sink),
                tx.clone(),
                close_rx.clone(),
                options.close_timeout,
            ));
        }
        Ok((
            Self {
                tasks,
                shards,
                total,
                close,
            },
            rx,
        ))
    }

    /// The shard total every shard identifies with (`[id, total]`), which
    /// can exceed the number of shards this Gateway runs.
    ///
    /// ```no_run
    /// # use std::{num::{NonZeroU16, NonZeroU32}, sync::Arc};
    /// # use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink};
    /// # struct Discard;
    /// # impl InboundSink for Discard { fn push(&self, _: Inbound) {} }
    /// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
    /// # let options = GatewayOptions {
    /// #     app: Arc::from("default"),
    /// #     token: "token".to_owned(),
    /// #     intents: twilight_gateway::Intents::GUILDS,
    /// #     gateway_url: None,
    /// #     event_types: EventTypeFlags::all(),
    /// #     identify_queue: Arc::new(noctis_ports::memory::MemoryIdentifyQueue::default()),
    /// #     shards: 2..4,
    /// #     total: NonZeroU32::new(4).unwrap(),
    /// #     max_concurrency: NonZeroU16::MIN,
    /// #     close_timeout: std::time::Duration::from_secs(5),
    /// # };
    /// let (gateway, _events) = Gateway::start(options, Arc::new(Discard))?;
    /// assert_eq!(gateway.shard_total(), 4);
    /// # Ok(()) }
    /// ```
    pub fn shard_total(&self) -> u32 {
        self.total
    }

    /// The shard ids this Gateway runs ([`GatewayOptions::shards`]).
    ///
    /// ```no_run
    /// # use std::{num::{NonZeroU16, NonZeroU32}, sync::Arc};
    /// # use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink};
    /// # struct Discard;
    /// # impl InboundSink for Discard { fn push(&self, _: Inbound) {} }
    /// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
    /// # let options = GatewayOptions {
    /// #     app: Arc::from("default"),
    /// #     token: "token".to_owned(),
    /// #     intents: twilight_gateway::Intents::GUILDS,
    /// #     gateway_url: None,
    /// #     event_types: EventTypeFlags::all(),
    /// #     identify_queue: Arc::new(noctis_ports::memory::MemoryIdentifyQueue::default()),
    /// #     shards: 2..4,
    /// #     total: NonZeroU32::new(4).unwrap(),
    /// #     max_concurrency: NonZeroU16::MIN,
    /// #     close_timeout: std::time::Duration::from_secs(5),
    /// # };
    /// let (gateway, _events) = Gateway::start(options, Arc::new(Discard))?;
    /// assert_eq!(gateway.shard_range(), 2..4);
    /// # Ok(()) }
    /// ```
    pub fn shard_range(&self) -> Range<u32> {
        self.shards.clone()
    }

    /// Asks every shard to stop; never blocks or awaits. The first call
    /// wins: later calls (with any mode) change nothing.
    ///
    /// From the request on, no shard pushes another [`Inbound`](crate::Inbound)
    /// into the sink. An identified shard sends `mode`'s close frame (4000
    /// for [`CloseMode::Resumable`], 1000 for [`CloseMode::Fresh`]) and
    /// reads on until the gateway's close frame arrives or
    /// [`GatewayOptions::close_timeout`] passes; a shard that is not
    /// identified stops at once without a frame and never identifies or
    /// resumes again. Each stopped shard reports [`ShardEvent::Closed`].
    ///
    /// ```no_run
    /// # use std::{num::{NonZeroU16, NonZeroU32}, sync::Arc};
    /// # use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink};
    /// use noctis_gateway::CloseMode;
    /// # struct Discard;
    /// # impl InboundSink for Discard { fn push(&self, _: Inbound) {} }
    /// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
    /// # let options = GatewayOptions {
    /// #     app: Arc::from("default"),
    /// #     token: "token".to_owned(),
    /// #     intents: twilight_gateway::Intents::GUILDS,
    /// #     gateway_url: None,
    /// #     event_types: EventTypeFlags::all(),
    /// #     identify_queue: Arc::new(noctis_ports::memory::MemoryIdentifyQueue::default()),
    /// #     shards: 0..1,
    /// #     total: NonZeroU32::MIN,
    /// #     max_concurrency: NonZeroU16::MIN,
    /// #     close_timeout: std::time::Duration::from_secs(5),
    /// # };
    /// let (mut gateway, _events) = Gateway::start(options, Arc::new(Discard))?;
    /// gateway.close_all(CloseMode::Resumable);
    /// gateway.close_all(CloseMode::Fresh); // ignored: the first call wins
    /// gateway.closed().await;
    /// # Ok(()) }
    /// ```
    pub fn close_all(&self, mode: CloseMode) {
        self.close.send_if_modified(|requested| {
            if requested.is_some() {
                return false;
            }
            *requested = Some(mode);
            true
        });
    }

    /// Waits until every shard task has ended, whether closed by
    /// [`close_all`](Self::close_all) or stopped on its own.
    ///
    /// Cancel-safe: dropping the future loses nothing, and a later call
    /// waits for the remaining tasks. Once every task has ended it returns
    /// at once.
    ///
    /// ```no_run
    /// # use std::{num::{NonZeroU16, NonZeroU32}, sync::Arc};
    /// # use noctis_gateway::{EventTypeFlags, Gateway, GatewayOptions, Inbound, InboundSink};
    /// use noctis_gateway::CloseMode;
    /// # struct Discard;
    /// # impl InboundSink for Discard { fn push(&self, _: Inbound) {} }
    /// # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
    /// # let options = GatewayOptions {
    /// #     app: Arc::from("default"),
    /// #     token: "token".to_owned(),
    /// #     intents: twilight_gateway::Intents::GUILDS,
    /// #     gateway_url: None,
    /// #     event_types: EventTypeFlags::all(),
    /// #     identify_queue: Arc::new(noctis_ports::memory::MemoryIdentifyQueue::default()),
    /// #     shards: 0..1,
    /// #     total: NonZeroU32::MIN,
    /// #     max_concurrency: NonZeroU16::MIN,
    /// #     close_timeout: std::time::Duration::from_secs(5),
    /// # };
    /// let (mut gateway, _events) = Gateway::start(options, Arc::new(Discard))?;
    /// gateway.close_all(CloseMode::Resumable);
    /// let in_time = tokio::time::timeout(std::time::Duration::from_secs(6), gateway.closed()).await;
    /// assert!(in_time.is_ok());
    /// # Ok(()) }
    /// ```
    pub async fn closed(&mut self) {
        while self.tasks.join_next().await.is_some() {}
    }
}

impl fmt::Debug for Gateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gateway")
            .field("shards", &self.shards)
            .field("total", &self.total)
            .field("running", &self.tasks.len())
            .field("closing", &*self.close.borrow())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use noctis_ports::memory::MemoryIdentifyQueue;

    use super::*;
    use crate::Inbound;

    struct Discard;

    impl InboundSink for Discard {
        fn push(&self, _: Inbound) {}
    }

    fn options(shards: Range<u32>, total: u32) -> GatewayOptions {
        GatewayOptions {
            app: Arc::from("default"),
            token: "token".to_owned(),
            intents: Intents::GUILDS,
            gateway_url: None,
            event_types: EventTypeFlags::INTERACTION_CREATE,
            identify_queue: Arc::new(MemoryIdentifyQueue::default()),
            shards,
            total: NonZeroU32::new(total).unwrap(),
            max_concurrency: NonZeroU16::MIN,
            close_timeout: Duration::from_secs(5),
        }
    }

    /// G2: an empty range or one ending past the total starts no task (the
    /// check runs before anything else, so no runtime is needed).
    #[test]
    fn invalid_shard_range_starts_nothing() {
        #[expect(
            clippy::reversed_empty_ranges,
            reason = "the reversed range is the input under test"
        )]
        let reversed = 3..1;
        for (shards, total) in [(0..0, 4), (2..2, 4), (reversed, 4), (2..5, 4), (4..5, 4)] {
            let started = Gateway::start(options(shards.clone(), total), Arc::new(Discard));
            assert!(
                matches!(started, Err(ShardConfigError::InvalidShardRange)),
                "{shards:?} of {total}"
            );
        }
    }

    #[test]
    fn debug_shows_the_shard_range() {
        let text = format!("{:?}", options(2..4, 4));
        assert!(text.contains("shards: 2..4"), "{text}");
        assert!(!text.contains("token: \"token\""), "{text}");
    }
}
