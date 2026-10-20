//! Shard tasks, lanes, `Inbound`, identify and the `SessionStore` writer.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
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

mod shard_config;

pub use shard_config::{ShardConfigError, shard_config};
