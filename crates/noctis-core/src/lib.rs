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
//! which takes interactions first. `run` stops on Ctrl-C/SIGTERM and
//! [`App::run_until`] when a future resolves: the shards close (resumable),
//! queued events and running handlers get [`AppBuilder::shutdown_grace`],
//! and handlers still running then are cancelled and reported as
//! [`ReportKind::Cancelled`]. A shard closing fatally after startup is
//! logged with its cause; when every shard has ended, `run` returns
//! [`RunError::AllShardsEnded`].
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
//! # Commands
//!
//! A [`CommandHandler`] (written with `#[command]` in `noctis`) is one slash
//! Command handler for a Command Path or Pattern, whose paths the macro
//! checked at compile time; [`Module::add`] takes it like an event handler.
//! A pattern with a group gives the handler a [`CommandPath`] enum naming
//! the invoked path; an invocation with a path the handler does not declare
//! fails with [`UnknownCommandPath`]. The handler receives a [`CommandCtx`].
//! `App::build` merges the Commands of every mounted Module (a base split
//! over several Modules becomes one base command), checks the merged
//! Registry with the same rule engine as the macros and reports every
//! conflict as a [`BuildViolation::Command`] naming the Modules and the
//! scope; [`Module::base`] and [`Module::group`] declare a base's
//! description and [`BaseMeta`], or a group's description. A node nobody
//! describes takes the description of a handler below it from the
//! lexicographically first Module id, with a warning.
//!
//! # The Command pipeline and the Responder
//!
//! An `APPLICATION_COMMAND` interaction runs as one task (never taking a
//! worker permit): its Command resolves by default names in its scope, the
//! handler runs under `catch_unwind` and what it returned ([`IntoResponse`])
//! is sent as the reply, or as an edit of the original once it deferred or
//! replied. A handler taking a [`CommandResponder`] answers through the
//! typestate [`Responder`] ([`Pending`] → [`Acked`] → [`Responded`]); every
//! call returns a [`ResponderError`] on failure. A failure goes to the
//! `Command` Error Sink. The [`Dispatcher`] (from `EventCtx::dispatcher`)
//! runs a registered Command outside an interaction, replying to a message.
//!
//! # Command metadata and context menus
//!
//! A [`CommandHandler`] carries the [`CommandMetadata`] its `#[command]`
//! declared (`default_member_permissions` as [`Permissions`], `contexts` as
//! [`InteractionContext`]s, `integration_types` as [`IntegrationType`]s,
//! `nsfw`, and the guilds of a guild-scoped command); unset fields stay
//! `None`. A [`ContextMenuHandler`] (written with `#[user_command]` or
//! `#[message_command]` in `noctis`) is one `USER` or `MESSAGE` command
//! ([`MenuKind`]): a call decodes the interaction's target from its resolved
//! data into a `ResolvedUser` or `ResolvedMessage`, or fails with a
//! [`MenuTargetError`] without running the handler.
//!
//! # Options
//!
//! A handler may take an options struct as its last parameter: a type
//! implementing [`Options`] (written with `#[derive(Options)]` in `noctis`),
//! whose fields implement [`CommandOption`]. A call decodes the
//! interaction into a [`CommandInput`], builds the struct from its leaf
//! options, runs the field validators, and only then runs the handler. A
//! missing, mistyped or out-of-range value is an [`OptionError`] and a
//! failed validator an [`InvalidOption`] carrying its `MessageKey`; neither
//! panics, and the handler does not run.
//!
//! # Discord HTTP
//!
//! Every request of the App's one HTTP client passes a loopback edge that
//! `build()` starts on the current tokio runtime: it forwards only
//! `/api/v10/` requests, byte for byte, to Discord (or to
//! [`AppBuilder::http_proxy`]) over one tuned upstream connection. `run`
//! warms that connection before `GET /gateway/bot` and keeps it warm with
//! HTTP/2 PING frames (never with API requests), redialing it with back-off
//! when it closes, a PING goes unanswered or a shard loses the network.
//! [`AppBuilder::http`] tunes it with an [`HttpConfig`]. A twilight
//! `Arc<Client>` kept after the App is dropped gets connection errors.
//!
//! # Observability and Error Sinks
//!
//! Handler errors, handler panics, handlers cancelled at shutdown and events
//! dropped by a full overflow go to the [`ErrorSink`] of their [`ErrorOrigin`]
//! ([`AppBuilder::error_sink`]); unset origins use [`LogSink`], which logs
//! at `error`. Spans (`noctis.shard`, `noctis.event`,
//! `noctis.interaction`) and metrics follow the contract in [`observe`].
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
mod command;
mod edge;
mod error;
pub mod events;
mod gateway_url;
mod handler;
mod lifecycle;
mod menu;
mod metadata;
mod module;
pub mod observe;
mod options;
mod pipeline;
mod plugin;
mod ports;
mod proxy;
mod registry;
mod responder;
mod runtime;
mod sharding;
mod sink;
mod startup;
mod state;
#[cfg(test)]
mod test_support;
mod token;
mod worker;

pub use app::{App, AppBuilder};
pub use command::{CommandCtx, CommandHandler, CommandPath, UnknownCommandPath};
pub use edge::config::{HttpConfig, HttpConfigError};
pub use error::{
    AckDeadlineMissed, BuildError, BuildViolation, EnvError, Error, RunError, StartError,
};
pub use events::{EventCtx, EventHandler, EventPayload};
pub use gateway_url::GatewayUrlError;
pub use handler::{HandlerError, HandlerResult};
pub use menu::{ContextMenuHandler, MenuTargetError};
pub use metadata::{BaseMeta, CommandMetadata};
pub use module::{Descriptor, Module};
#[doc(hidden)]
pub use options::__options;
pub use options::{CommandInput, CommandOption, InvalidOption, OptionError, OptionValue, Options};
pub use pipeline::{CommandInvocation, DispatchError, Dispatcher};
pub use plugin::Plugin;
pub use proxy::HttpProxyError;
pub use responder::{
    Acked, Command, CommandResponder, Ephemerality, FollowupHandle, IntoResponse, NoModal,
    OutgoingMessage, Pending, Responded, Responder, ResponderError, ResponderKind, ResponderState,
    Visibility,
};
pub use runtime::{Resource, ResourceConflict, Runtime};
pub use sink::{ErrorOrigin, ErrorReport, ErrorSink, LogSink, ReportKind};
pub use state::State;
pub use token::{Token, TokenError};

pub use noctis_rules::command::{IntegrationType, InteractionContext, MenuKind};
pub use twilight_model::guild::Permissions;

/// Test-only seams for the testkit's end-to-end tests; not part of the API.
#[doc(hidden)]
pub mod __testing {
    pub use crate::edge::testing::{
        CallbackAck, CallbackBody, CallbackError, CallbackRequest, KeepWarm, interaction_callback,
        keep_warm,
    };

    use futures_util::future::BoxFuture;

    use crate::{App, CommandHandler, ContextMenuHandler, HandlerResult};

    /// The merged command Registry of `app`, one line per scope, base,
    /// child and context menu (format in Story 6.4's spec); `""` when it has
    /// no commands.
    pub fn command_tree(app: &App) -> String {
        app.registry().commands().render()
    }

    /// Runs `handler` as if `path` had been invoked: its handler with the
    /// path's variant, or an `UnknownCommandPath` error.
    pub fn call_command(
        handler: &CommandHandler,
        path: &'static str,
    ) -> BoxFuture<'static, HandlerResult<()>> {
        crate::command::call_command(handler, path)
    }

    /// Decodes `json` (an interaction object) and runs `handler` for its
    /// Command Path: an `OptionError` for bad JSON, an `UnknownCommandPath`
    /// for a path the handler does not declare, else the options pipeline
    /// (decode, validators, handler).
    pub fn call_command_json(
        handler: &CommandHandler,
        json: &[u8],
    ) -> BoxFuture<'static, HandlerResult<()>> {
        crate::command::call_command_json(handler, json)
    }

    /// Decodes `json` (a USER or MESSAGE command interaction) and runs
    /// `handler` with its resolved target: a `MenuTargetError` for bad JSON,
    /// another interaction, the wrong kind or a missing target, an
    /// `UnknownCommandPath` for another menu name, else the handler.
    pub fn call_context_menu(
        handler: &ContextMenuHandler,
        json: &[u8],
    ) -> BoxFuture<'static, HandlerResult<()>> {
        crate::menu::call_context_menu(handler, json)
    }
}
