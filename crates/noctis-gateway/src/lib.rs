//! Shard tasks, lanes, `Inbound`, identify and the `SessionStore` writer.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! # Gateway
//!
//! [`Gateway::start`] spawns one tokio task per shard in
//! [`GatewayOptions::shards`] (the whole `0..total`, or one Cluster process's
//! Shard Range; ids stay global). Every shard identifies
//! through the `IdentifyQueue` port in bucket `shard_id % max_concurrency`
//! (GW-01). Each task reads raw gateway messages, parses them with
//! `twilight_gateway::parse`, pushes [`Inbound`] items into a non-blocking
//! [`InboundSink`] and reports [`ShardEvent`]s on a channel; it never waits
//! for core or a handler.
//!
//! [`Gateway::close_all`] asks every shard to stop (the first call wins): an
//! identified shard sends the [`CloseMode`]'s close frame (4000 resumable,
//! 1000 fresh) and reads until the gateway's close frame or
//! [`GatewayOptions::close_timeout`], a shard that is not identified stops at
//! once, and no shard pushes another [`Inbound`] after the request. Each
//! such shard reports [`ShardEvent::Closed`]; a shard that stops on its own
//! (a fatal close code) reports [`ShardEvent::Ended`].
//! [`Gateway::closed`] waits until every shard task has ended.
//!
//! # Sessions
//!
//! This crate is the only reader and writer of the `SessionStore` port
//! ([`GatewayOptions::session_store`], AD-21). A shard's entry lives under
//! `shard:{id}:{total}` (the App's port adds its `<app>:session:` prefix)
//! and holds the session id, the last received sequence and the resume URL.
//!
//! - **Read** once at shard start, bounded by 5 s. A valid entry makes the
//!   shard send RESUME instead of IDENTIFY, without an `IdentifyQueue`
//!   lease; a miss, a store error or a timeout makes it identify. An entry
//!   that does not decode or fails validation is deleted.
//! - **Written** only after a requested [`CloseMode::Resumable`] close of an
//!   identified shard is acknowledged by the gateway's close frame, within
//!   the same [`GatewayOptions::close_timeout`]; the entry has no TTL.
//! - **Deleted** when the gateway rejects the stored session (op 9 with
//!   `d: false`) or a READY replaces it, and on a [`CloseMode::Fresh`]
//!   close.
//!
//! Nothing is written on READY, on in-process reconnects or periodically,
//! so a crash leaves no entry. A resume replays only what Discord still
//! holds; events dropped under back-pressure are not replayed
//! (at-most-once).
//!
//! # Lanes
//!
//! [`lanes`] builds the two queues between the shard tasks and core
//! (AD-10, FR-5). The [`LaneSender`] is the [`InboundSink`] the shard tasks
//! push into; it never awaits. Interactions go to the Interaction Lane,
//! which is never dropped and has no bound. Other events go to the event
//! lane, bounded by [`LaneConfig::event_capacity`]: when it is full, a
//! droppable event ([`DROPPABLE_EVENTS`] by default) evicts the oldest
//! queued droppable event, and any other event waits in a capped overflow,
//! beyond which it is dropped with an `error` log, or handed to the
//! [`DropObserver`] set in [`LaneConfig::drop_observer`]. Every drop is counted in
//! `noctis_events_dropped_total{app, event_type}`, and the depths are the
//! gauges `noctis_lane_depth{app, lane}` and `noctis_event_overflow_depth{app}`.
//! The single [`LaneReceiver`] always takes interactions first.
//!
//! ```
//! use noctis_gateway::{LaneConfig, lanes};
//!
//! let (sender, mut receiver) = lanes(LaneConfig::default());
//! drop(sender);
//! assert!(receiver.try_recv().is_none());
//! ```
//!
//! # Shard configuration
//!
//! [`shard_config`] is the one place that turns App settings (token, intents,
//! gateway URL, identify queue) into a twilight [`Config`](twilight_gateway::Config).
//! Core's shard startup (Story 5.4) must build every shard through it, so a
//! gateway URL set with `AppBuilder::gateway_url` reaches every shard through
//! twilight's `proxy_url`, and invalid input is a typed [`ShardConfigError`]
//! instead of a panic inside twilight.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use noctis_gateway::shard_config;
//! use twilight_gateway::{queue::InMemoryQueue, Intents, Shard, ShardId};
//!
//! # #[tokio::main] async fn main() -> Result<(), noctis_gateway::ShardConfigError> {
//! let queue = InMemoryQueue::new(1, 1000, Duration::from_secs(5), 1000);
//! let config = shard_config("token", Intents::GUILDS, Some("ws://127.0.0.1:3000"), queue)?;
//! let shard = Shard::with_config(ShardId::ONE, config);
//! # Ok(()) }
//! ```

mod gateway;
mod inbound;
mod lanes;
mod queue;
mod session;
mod shard;
mod shard_config;

pub use gateway::{CloseMode, Gateway, GatewayOptions};
pub use inbound::{Inbound, InboundKind, InboundSink, RawJson, ShardEvent};
pub use lanes::{
    DEFAULT_EVENT_CAPACITY, DEFAULT_OVERFLOW_CAPACITY, DROPPABLE_EVENTS, DropObserver, LaneConfig,
    LaneDepths, LaneReceiver, LaneSender, lanes,
};
pub use shard_config::{ShardConfigError, shard_config};
/// Which gateway event types shard tasks parse (twilight's flags,
/// re-exported so core needs no `twilight-gateway` dependency).
///
/// ```
/// let wanted = noctis_gateway::EventTypeFlags::INTERACTION_CREATE;
/// assert!(wanted.contains(noctis_gateway::EventTypeFlags::INTERACTION_CREATE));
/// ```
pub use twilight_gateway::EventTypeFlags;
