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
//! A [`Module`] groups registrations and nests other Modules; every
//! interaction is one struct with a kind derive and a `#[handler]` method in
//! `noctis`, registered by type with [`Module::add::<T>()`](Module::add)
//! ([`Register`]). Typed event handlers ([`EventHandler`]) come from
//! `#[derive(EventHandler)]`;
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
//! A [`CommandHandler`] (a `#[derive(SlashCommand)]` struct in `noctis`)
//! is one slash Command handler for a Command Path or Pattern, whose paths
//! the derive checked at compile time; [`Module::add`] registers it like an
//! event handler.
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
//! call returns a [`ResponderError`] on failure. A failure or a panic first
//! sends the user a localized ephemeral notice (the [`HandlerError`]'s key,
//! `noctis.validation.failed` for options, else `noctis.error.generic`;
//! after a defer the deferred original is deleted and the notice is an
//! ephemeral follow-up), then goes to the `Command` Error Sink; a
//! [`HandlerError::user`] condition or a failed validator only logs at
//! `debug`. The [`Dispatcher`] (from `EventCtx::dispatcher`)
//! runs a registered Command outside an interaction, replying to a message.
//!
//! # Command metadata and context menus
//!
//! A [`CommandHandler`] carries the [`CommandMetadata`] its
//! `#[command("…", ..)]` attribute declared (`default_member_permissions` as [`Permissions`], `contexts` as
//! [`InteractionContext`]s, `integration_types` as [`IntegrationType`]s,
//! `nsfw`, and the guilds of a guild-scoped command); unset fields stay
//! `None`. A [`ContextMenuHandler`] (a `#[derive(UserCommand)]` or
//! `#[derive(MessageCommand)]` struct in `noctis`) is one `USER` or `MESSAGE` command
//! ([`MenuKind`]): a call decodes the interaction's target from its resolved
//! data into a `ResolvedUser` or `ResolvedMessage`, or fails with a
//! [`MenuTargetError`] without running the handler.
//!
//! # Options
//!
//! A slash Command struct's named fields are its options: it implements
//! [`Options`] (`#[derive(SlashCommand)]` in `noctis`), and each field
//! implements [`CommandOption`]. A call decodes the interaction into a
//! [`CommandInput`], builds the struct from its leaf options, runs the field
//! validators, and only then runs the `#[handler]` method on it. An option
//! flagged `autocomplete` is answered by the struct's `#[autocomplete(field)]`
//! method, which returns [`Suggestions`]. A
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
//! # Publishing commands
//!
//! [`App::publish`] writes the Registry's commands to Discord, only when
//! they differ from the remote ones ([`publish`]): it reads the
//! application, builds the JSON from the Registry alone, runs every check
//! (META-04, and REG-03/REG-02 on the `--publish-guild` mirror) before any
//! write, then sends one bulk-overwrite `PUT` per changed target. A dry run
//! reports the JSON, the diff and the create count (REG-05) and writes
//! nothing. [`App::run`] parses the built-in command line ([`cli`]):
//! `--publish`, `--publish-guild [<id>]`, `--publish-clear [--guild <id>]`
//! and `--dry-run` publish, print the report and return without connecting
//! a shard; [`AppBuilder::publisher`] publishes once at startup instead.
//!
//! ```no_run
//! use noctis_core::publish::PublishScope;
//! use twilight_model::id::Id;
//!
//! # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let app = noctis_core::App::from_env()?.build()?;
//! let report = app.publish(PublishScope::Guild(Id::new(42))).dry_run(true).await?;
//! println!("{report}");
//! # Ok(()) }
//! ```
//!
//! # Observability and Error Sinks
//!
//! Handler errors, handler panics, handlers cancelled at shutdown and events
//! dropped by a full overflow go to the [`ErrorSink`] of their [`ErrorOrigin`]
//! ([`AppBuilder::error_sink`]); unset origins use [`LogSink`], which logs
//! at `error`. A custom `Command` or `Autocomplete` sink is called after
//! [`LogSink`] has logged the report, so the full error is logged once
//! either way. Spans (`noctis.shard`, `noctis.event`,
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
mod autocomplete;
pub mod cli;
mod command;
mod component;
mod edge;
mod error;
pub mod events;
mod gateway_url;
mod guard;
mod handler;
mod kind;
mod kv;
mod lifecycle;
mod menu;
mod metadata;
mod middleware;
mod module;
mod namespace;
mod notice;
pub mod observe;
mod options;
mod pipeline;
mod plugin;
mod ports;
mod proxy;
pub mod publish;
mod raw_http;
mod refs;
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
mod webhook;
mod worker;

pub use app::{App, AppBuilder};
#[doc(hidden)]
pub use autocomplete::{
    __AutocompleteField, __AutocompleteMethod, __SuggestionsFor, __autocomplete_method,
};
pub use autocomplete::{
    AutocompleteChoice, AutocompleteCtx, AutocompleteError, AutocompleteValue, Focused, Partial,
    PartialOptions, SuggestionLabel, SuggestionValue, Suggestions,
};
pub use command::{CommandCtx, CommandHandler, CommandPath, UnknownCommandPath};
#[doc(hidden)]
pub use component::{__Absent, __ExpiredHook, __HookCtx, __StaleHook};
pub use component::{
    ComponentCtx, ComponentHandler, ComponentKind, Expired, ExpiredCtx, ModalHandler,
    ModalSubmitCtx, SelectedValues,
};
pub use edge::config::{HttpConfig, HttpConfigError};
pub use error::{
    AckDeadlineMissed, BuildError, BuildViolation, EnvError, Error, RunError, StartError,
};
pub use events::{EventCtx, EventHandler, EventPayload};
pub use gateway_url::GatewayUrlError;
pub use guard::Access;
#[doc(hidden)]
pub use guard::{
    __Denial, __Guards, __OwnerField, __check_custom, __check_owner, __check_permissions,
    __owner_id,
};
pub use handler::{HandlerError, HandlerResult};
#[doc(hidden)]
pub use kind::{
    __AttachHooks, __BindExpired, __BindStale, __H, __HE, __HER, __HR, __HookKind, __NoHookCtx,
    __NoHookSlot, __NoHooks, __NotAHookStruct,
};
#[doc(hidden)]
pub use kind::{
    __Bind, __C, __CR, __CX, __CXR, __Component, __ComponentKind, __ComponentParams, __ContextMenu,
    __EventHandler, __EventKind, __EventParams, __Kind, __ManualIsDeclared, __ManualTakesManual,
    __MenuKind, __MenuTarget, __Modal, __ModalKind, __ModalParams, __NoHandler, __NoResponder,
    __PatternPaths, __Response, __SinglePath, __SlashCommand, __SlashKind, __TakesTarget,
    __Unbound, __assert_no_responder, __assert_responder,
};
#[doc(hidden)]
pub use kind::{__ExtType, __Extensions, __RequireExtensions, __ext, __ext_opt};
pub use kv::Kv;
pub use menu::{ContextMenuHandler, MenuTargetError};
pub use metadata::{BaseMeta, CommandMetadata};
pub use middleware::{
    AfterCtx, AfterMiddleware, BeforeCtx, BeforeMiddleware, Ext, ExtensionMissing, Extensions,
    Flow, Invocation, Middleware, RunOutcome,
};
#[doc(hidden)]
pub use module::Descriptor;
pub use module::{Module, Register};
pub use namespace::{AppId, AppIdError, IntoPort, Namespaced, NamespacedPort, Shared};
#[doc(hidden)]
pub use options::__options;
pub use options::{CommandInput, CommandOption, InvalidOption, OptionError, OptionValue, Options};
pub use pipeline::{CommandInvocation, DispatchError, Dispatcher};
pub use plugin::Plugin;
pub use proxy::HttpProxyError;
pub use raw_http::{Method, RawHttp, RawHttpError, RawRequest, RawResponse};
pub use refs::RefOps;
pub use responder::{
    Acked, AutoDefer, CallbackResource, CanShowModal, Command, CommandResponder, Component,
    ComponentResponder, DeferMode, Ephemerality, FollowupHandle, IntoEdit, IntoResponse, Manual,
    ModalResponder, ModalSubmit, NoticeText, OutgoingMessage, Pending, ReplyText, Responded,
    Responder, ResponderError, ResponderKind, ResponderState, Visibility,
};
pub use runtime::{Resource, ResourceConflict, Runtime};
pub use sink::{ErrorOrigin, ErrorReport, ErrorSink, LogSink, ReportKind};
pub use state::State;
pub use token::{Token, TokenError};
pub use webhook::{
    DeleteWebhookMessage, EditWebhookMessage, ExecuteWebhook, Foreign, ForeignWebhook, Owned,
    OwnedWebhook, Ownership, Webhook, WebhookChannel, WebhookError, WebhookMessage, WebhookThread,
    Webhooks,
};

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

    use std::future::Future;

    use crate::{
        App, BuildViolation, CommandHandler, ComponentHandler, ComponentKind, ContextMenuHandler,
        HandlerResult, ModalHandler, Module, Responded,
    };

    /// Installs `observer` as the App's run-end observer: it is called with
    /// the interaction id when that interaction's pipeline task ends
    /// (returned, panicked or cancelled; for an interaction without a
    /// pipeline, once it has been traced). It only observes and changes no
    /// dispatch behaviour; the last call wins.
    pub fn observe_runs(
        builder: &mut crate::AppBuilder,
        observer: std::sync::Arc<
            dyn Fn(twilight_model::id::Id<twilight_model::id::marker::InteractionMarker>)
                + Send
                + Sync,
        >,
    ) {
        builder.set_run_observer(observer);
    }

    /// A read-only view of a built App for the testkit: its command
    /// definitions, its built-in notice texts, and the codec's `custom_id`
    /// masking and Route lookup. It only reads.
    #[derive(Clone)]
    pub struct AppProbe {
        shared: std::sync::Arc<crate::handler::AppShared>,
        registry: std::sync::Arc<crate::registry::Registry>,
    }

    impl std::fmt::Debug for AppProbe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AppProbe")
                .field("app", &self.shared.app)
                .finish_non_exhaustive()
        }
    }

    impl AppProbe {
        /// The probe of `app`.
        pub fn of(app: &App) -> Self {
            Self {
                shared: std::sync::Arc::clone(app.shared()),
                registry: std::sync::Arc::clone(app.registry()),
            }
        }

        /// Every command object Publish would send, with its scope (`None`
        /// for global): each scope's chat-input bases, then its context
        /// menus, in Registry order.
        pub fn commands(
            &self,
        ) -> Vec<(
            Option<twilight_model::id::Id<twilight_model::id::marker::GuildMarker>>,
            serde_json::Value,
        )> {
            use noctis_rules::command::Scope;

            let mut out = Vec::new();
            for scope in self.registry.commands().scopes() {
                let guild = match scope.scope {
                    Scope::Global => None,
                    Scope::Guild(id) => match twilight_model::id::Id::new_checked(id) {
                        Some(id) => Some(id),
                        None => continue,
                    },
                };
                out.extend(
                    crate::publish::json::local(scope)
                        .into_iter()
                        .map(|command| (guild, command)),
                );
            }
            out
        }

        /// The text of the built-in notice `key` (a `noctis.*` key) in the
        /// locale a failed run's notice uses: the User Locale, then the
        /// Guild Locale, else `en-US`. A key no catalog has renders the
        /// generic error text, as at run time.
        pub fn notice(
            &self,
            key: &'static str,
            user_locale: Option<&str>,
            guild_locale: Option<&str>,
        ) -> String {
            crate::notice::text(
                &*self.shared.strings,
                crate::notice::locale(user_locale, guild_locale),
                noctis_rules::MessageKey::new(key),
            )
        }
    }

    /// `raw` with its per-send parts masked (select nonce `NN`, Ref body
    /// `~RRRRRRRRRRR`, integrity tag `TTTTTT`); unchanged for an inline
    /// unsigned id or a `custom_id` that is not Noctis's.
    pub fn mask_custom_id(raw: &str) -> std::borrow::Cow<'_, str> {
        noctis_codec::mask_volatile(raw)
            .map_or(std::borrow::Cow::Borrowed(raw), std::borrow::Cow::Owned)
    }

    /// The Route id of the Noctis `custom_id` `raw`, or `None` when it is
    /// not one.
    pub fn route_of(raw: &str) -> Option<&str> {
        noctis_codec::decode(raw)
            .ok()
            .map(|decoded| decoded.route())
    }

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

    /// Decodes `json` (an autocomplete interaction) and runs the
    /// `#[autocomplete(field)]` method `handler` binds to its focused option
    /// over a detached context: an `OptionError` for bad JSON, an
    /// `UnknownCommandPath` for a path the handler does not declare,
    /// `Ok(None)` when no method is bound, else the choices the pipeline
    /// would send (lowered and filtered; `MessageKey` labels in `en-US`).
    pub fn call_autocomplete(
        handler: &CommandHandler,
        json: &[u8],
    ) -> BoxFuture<'static, HandlerResult<Option<Vec<crate::AutocompleteChoice>>>> {
        crate::autocomplete::call_autocomplete(handler, json)
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

    /// Runs the component `handler` on `value` (its struct) over a detached
    /// context: an error when `value` is not the handler's struct type.
    pub fn call_component<S: Send + 'static>(
        handler: &ComponentHandler,
        value: S,
    ) -> BoxFuture<'static, HandlerResult<Responded>> {
        crate::component::call_component(handler, value)
    }

    /// Runs the modal `handler` on `value` (its struct) over a detached
    /// context: an error when `value` is not the handler's struct type.
    pub fn call_modal<S: Send + 'static>(
        handler: &ModalHandler,
        value: S,
    ) -> BoxFuture<'static, HandlerResult<Responded>> {
        crate::component::call_modal(handler, value)
    }

    /// A [`Responded`] proof without a response, for handlers in tests
    /// that answer nothing.
    pub fn responded() -> Responded {
        Responded::new()
    }

    /// The Route map `build()` folds from `modules` (the allocation
    /// proof's seam).
    pub struct RouteTable(crate::registry::Registry);

    impl std::fmt::Debug for RouteTable {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RouteTable")
                .field("len", &self.0.len())
                .finish_non_exhaustive()
        }
    }

    /// Folds `modules` as `build()` does (with no privileged intents
    /// requested): the Route map, or every violation.
    ///
    /// # Errors
    ///
    /// The fold's violations, such as `DuplicateRoute`.
    pub fn route_table(modules: &[Module]) -> Result<RouteTable, Vec<BuildViolation>> {
        let (registry, violations) =
            crate::registry::Registry::fold(modules, twilight_model::gateway::Intents::empty());
        if violations.is_empty() {
            Ok(RouteTable(registry))
        } else {
            Err(violations)
        }
    }

    impl RouteTable {
        /// Runs dispatch stage 2 for a click on a component of `kind` with
        /// `custom_id`: decode, borrowed-key lookup, kind check and the
        /// struct decode, without a handler. Returns the handler's name, or
        /// the rejection label; it never allocates.
        ///
        /// # Errors
        ///
        /// The rejection label (`malformed`, `unknown_route`, …).
        pub fn probe(
            &self,
            custom_id: &str,
            kind: ComponentKind,
        ) -> Result<&'static str, &'static str> {
            use crate::pipeline::component::{Family, Rejection, resolve};
            use crate::registry::RouteHandler;

            let decoded =
                noctis_codec::decode(custom_id).map_err(|_| Rejection::Malformed.as_str())?;
            let entry = resolve(&self.0, &decoded, Family::Component(Some(kind)))
                .map_err(Rejection::as_str)?;
            let RouteHandler::Component(handler) = &entry.handler else {
                return Err(Rejection::UnknownRoute.as_str());
            };
            handler
                .probe(&decoded)
                .map_err(|e| Rejection::from_decode(&e).as_str())?;
            if matches!(decoded.body(), noctis_codec::RawBody::Ref(_)) {
                return Err(Rejection::RefUnresolved.as_str());
            }
            Ok(handler.name())
        }
    }

    /// Parses `raw` (an `INTERACTION_CREATE` `d` object) and runs it
    /// through the component pipeline the worker spawns for clicks and
    /// modal submits, on shard 0, received now, with a fresh cancel token.
    /// Unparsable JSON is one `warn` (`malformed`); another interaction
    /// type does nothing.
    pub fn dispatch(app: &App, raw: &str) -> impl Future<Output = ()> + Send + use<> {
        dispatch_over(app, raw, crate::pipeline::component::Wire::Interaction)
    }

    /// As [`dispatch`], with every response sent nowhere (a detached
    /// transport): the pipeline, the responder state and the auto-defer
    /// task run, no request is made.
    pub fn dispatch_detached(app: &App, raw: &str) -> impl Future<Output = ()> + Send + use<> {
        dispatch_over(app, raw, crate::pipeline::component::Wire::Detached)
    }

    fn dispatch_over(
        app: &App,
        raw: &str,
        wire: crate::pipeline::component::Wire,
    ) -> impl Future<Output = ()> + Send + use<> {
        use noctis_gateway::{Inbound, InboundKind, RawJson};

        let registry = std::sync::Arc::clone(app.registry());
        let shared = std::sync::Arc::clone(app.shared());
        let parsed =
            serde_json::from_str::<twilight_model::application::interaction::Interaction>(raw);
        let inbound = parsed.ok().map(|interaction| Inbound {
            shard: twilight_model::gateway::ShardId::new(0, 1),
            received_at: tokio::time::Instant::now(),
            kind: InboundKind::Interaction(
                Box::new(interaction),
                RawJson::new(bytes::Bytes::from(raw.to_owned())),
            ),
        });
        if inbound.is_none() {
            tracing::warn!(
                target: "noctis::dispatch",
                reason = "malformed",
                shard = 0_u32,
                "component interaction rejected"
            );
        }
        async move {
            if let Some(inbound) = inbound {
                crate::pipeline::component::run_component_over(
                    registry,
                    shared,
                    inbound,
                    tokio_util::sync::CancellationToken::new(),
                    wire,
                )
                .await;
            }
        }
    }
}
