//! [`Gateway`]: one tokio task per shard (FR-2, GW-01).

use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::Arc;

use noctis_ports::IdentifyQueue;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use twilight_gateway::{EventTypeFlags, Intents, Shard, ShardId};

use crate::inbound::{InboundSink, ShardEvent};
use crate::queue::PortQueue;
use crate::{ShardConfigError, shard, shard_config};

/// Everything [`Gateway::start`] needs. `Debug` redacts the token.
///
/// ```
/// use std::num::{NonZeroU16, NonZeroU32};
/// use std::sync::Arc;
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
///     total: NonZeroU32::MIN,
///     max_concurrency: NonZeroU16::MIN,
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
    /// The shard total.
    pub total: NonZeroU32,
    /// Discord's `session_start_limit.max_concurrency` (identify buckets).
    pub max_concurrency: NonZeroU16,
}

impl fmt::Debug for GatewayOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayOptions")
            .field("app", &self.app)
            .field("token", &"<redacted>")
            .field("intents", &self.intents)
            .field("gateway_url", &self.gateway_url)
            .field("event_types", &self.event_types)
            .field("total", &self.total)
            .field("max_concurrency", &self.max_concurrency)
            .finish_non_exhaustive()
    }
}

/// The running shards: one tokio task each. Dropping it aborts every task.
///
/// ```no_run
/// use std::num::{NonZeroU16, NonZeroU32};
/// use std::sync::Arc;
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
///     total: NonZeroU32::MIN,
///     max_concurrency: NonZeroU16::MIN,
/// };
/// let (gateway, mut events) = Gateway::start(options, Arc::new(Discard))?;
/// while let Some(event) = events.recv().await {
///     println!("{event:?}");
/// }
/// # Ok(()) }
/// ```
pub struct Gateway {
    tasks: JoinSet<()>,
    total: u32,
}

impl Gateway {
    /// Starts one task per shard `0..total`, each built through
    /// [`shard_config`] with an identify queue over `options.identify_queue`
    /// (bucket `shard % max_concurrency`).
    ///
    /// Each task pushes its parsed items into `sink` and reports
    /// [`ShardEvent`]s on the returned channel. Only the tasks hold senders,
    /// so the channel closes once every task has ended.
    ///
    /// # Errors
    ///
    /// [`ShardConfigError`] when [`shard_config`] rejects the settings; no
    /// task is started then.
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
    /// #     total: NonZeroU32::MIN,
    /// #     max_concurrency: NonZeroU16::MIN,
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
        let queue = PortQueue::new(options.identify_queue, options.max_concurrency);
        let config = shard_config(
            &options.token,
            options.intents,
            options.gateway_url.as_deref(),
            queue,
        )?;
        let total = options.total.get();
        let (tx, rx) = mpsc::unbounded_channel();
        let mut tasks = JoinSet::new();
        for id in (0..total).filter_map(|number| ShardId::new_checked(number, total)) {
            let shard = Shard::with_config(id, config.clone());
            tasks.spawn(shard::run(
                shard,
                Arc::clone(&options.app),
                options.event_types,
                Arc::clone(&sink),
                tx.clone(),
            ));
        }
        Ok((Self { tasks, total }, rx))
    }

    /// The number of shards started.
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
    /// #     total: NonZeroU32::new(4).unwrap(),
    /// #     max_concurrency: NonZeroU16::MIN,
    /// # };
    /// let (gateway, _events) = Gateway::start(options, Arc::new(Discard))?;
    /// assert_eq!(gateway.shard_total(), 4);
    /// # Ok(()) }
    /// ```
    pub fn shard_total(&self) -> u32 {
        self.total
    }
}

impl fmt::Debug for Gateway {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gateway")
            .field("total", &self.total)
            .field("running", &self.tasks.len())
            .finish()
    }
}
