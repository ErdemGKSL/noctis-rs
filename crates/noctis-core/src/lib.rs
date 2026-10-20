//! `App`, `Runtime`, `Plugin`, `Dispatcher`, `Module`, `Registry`, pipeline, workers, `Responder`, HTTP client and publish.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! # App
//!
//! [`App::builder`] or [`App::from_env`] returns a mutable [`AppBuilder`];
//! [`AppBuilder::build`] installs the [`Plugin`]s, runs the App-build checks
//! and returns an [`App`] or one [`BuildError`] listing every violation.
//! [`App::run`] checks the tokio runtime, the rustls crypto provider and the
//! [`Token`], reads `GET /gateway/bot`, plans the shard total and identify
//! buckets (GW-01..03) and starts one task per shard, returning a typed
//! [`StartError`] instead of panicking when any step fails. Inbound items
//! flow from the shard tasks through the lanes (the never-dropped
//! Interaction Lane and the bounded event lane) to one core drain task,
//! which takes interactions first.
//!
//! # Modules and events
//!
//! A [`Module`] groups typed event handlers ([`EventHandler`], written with
//! `#[event]` in `noctis`) and nests other Modules;
//! [`AppBuilder::module`] mounts a tree. `build()` folds every tree once,
//! parent before child, into an immutable Registry: the gateway then parses
//! only interactions and the handled event types, and the handlers'
//! non-privileged intents are added to the requested set (a missing
//! privileged one is [`BuildViolation::MissingIntent`], GW-05). At run time
//! a semaphore bounds how many events run their handlers at once
//! ([`AppBuilder::handler_concurrency`]); interactions never wait for it.
//! Ordered handlers run first, in per-guild order; every handler future
//! runs under `catch_unwind`, so a panic is logged and the shard keeps
//! running. The payload types are in [`events`].
//!
//! ```no_run
//! use noctis_core::{App, AppBuilder, Error, State};
//!
//! struct Greeting(&'static str);
//!
//! fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
//!     builder.state(Greeting("hello"))
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     let app = configure(&mut App::from_env()?).build()?;
//!     let greeting: State<Greeting> = app.state().expect("registered");
//!     assert_eq!(greeting.0, "hello");
//!     app.run().await
//! }
//! ```

mod app;
mod authority;
mod error;
pub mod events;
mod gateway_url;
mod handler;
mod module;
mod plugin;
mod ports;
mod proxy;
mod registry;
mod runtime;
mod sharding;
mod startup;
mod state;
#[cfg(test)]
mod test_support;
mod token;
mod worker;

pub use app::{App, AppBuilder};
pub use error::{BuildError, BuildViolation, EnvError, Error, StartError};
pub use events::{EventCtx, EventHandler, EventPayload};
pub use gateway_url::GatewayUrlError;
pub use handler::{HandlerError, HandlerResult};
pub use module::{Descriptor, Module};
pub use plugin::Plugin;
pub use proxy::HttpProxyError;
pub use runtime::{Resource, ResourceConflict, Runtime};
pub use state::State;
pub use token::{Token, TokenError};
