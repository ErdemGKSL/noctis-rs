//! The Command pipeline (AD-23) and the public [`Dispatcher`] (AD-24,
//! FR-61; LOC-06, INT-12).
//!
//! An `APPLICATION_COMMAND` interaction runs these stages, in this order:
//!
//! 1. resolve the Command from the Registry's O(1) maps by its default
//!    (unlocalized) names, in the scope `data.guild_id` names (LOC-06);
//! 2. *Ref finalization: absent until Story 7.5;*
//! 3. *Module enablement checks: absent until Stories 13.2 and 13.3;*
//! 4. resolve the locale default (User and Guild Locale, FR-43);
//! 5. *rate limits: absent until Story 13.4;*
//! 6. arm the auto-defer (INT-01): a task that acknowledges the interaction
//!    with a type-5 callback at `received_at + ack_deadline` unless the
//!    handler answered first; an `opens_modal` Command gets none, only an
//!    error log when it has not answered by the 2.9 s ack deadline
//!    (INT-05). The task is aborted when the handler run ends;
//! 7. *before-Middleware: absent until Story 7.6;*
//! 8. *Guards: absent until Story 7.7;*
//! 9. run the handler under `catch_unwind`, raced against the shutdown
//!    cancel token, and send what it returned;
//! 10. route a failure to the Command Error Sink (after-Middleware arrives
//!     with Story 7.6).
//!
//! The [`Dispatcher`] runs stages 3 onward for a Command invoked from
//! outside an interaction; it arms no auto-defer (a message has no ack
//! deadline).

use std::any::Any;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use futures_util::FutureExt;
use noctis_gateway::{Inbound, InboundKind};
use noctis_rules::command::{MenuKind, Scope};
use tokio_util::sync::CancellationToken;
use tracing::field::Empty;
use tracing::{Instrument, Span};
use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::application_command::{
    CommandData, CommandOptionValue,
};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, UserMarker};

use crate::command::{CommandCtx, CtxInner, HandlerFuture, failed};
use crate::edge::callback::ACK_DEADLINE;
use crate::handler::{AppShared, HandlerError, HandlerResult};
use crate::menu::MenuInput;
use crate::options::CommandInput;
use crate::registry::Registry;
use crate::registry::commands::{CommandEntry, MenuEntry, ScopeCommands};
use crate::responder::{
    Ephemerality, InteractionTarget, MessageTarget, PENDING, Response, Shared, Transport,
};
use crate::sink::{ErrorOrigin, ErrorReport, ReportKind};
use crate::worker::panic_message;

/// The `event_type` of a Command report.
const INTERACTION_CREATE: &str = "INTERACTION_CREATE";

/// A resolved Command (stage 1).
enum Resolved {
    Command {
        entry: Arc<CommandEntry>,
        path: &'static str,
    },
    Menu(Arc<MenuEntry>),
}

impl Resolved {
    fn module(&self) -> &'static str {
        match self {
            Self::Command { entry, .. } => entry.module,
            Self::Menu(entry) => entry.module,
        }
    }

    fn path(&self) -> &'static str {
        match self {
            Self::Command { path, .. } => path,
            Self::Menu(entry) => entry.handler.menu_name(),
        }
    }

    fn handler(&self) -> &'static str {
        match self {
            Self::Command { entry, .. } => entry.handler.name(),
            Self::Menu(entry) => entry.handler.name(),
        }
    }

    fn ephemerality(&self) -> Ephemerality {
        match self {
            Self::Command { entry, .. } => entry.handler.ephemerality(),
            Self::Menu(entry) => entry.handler.ephemerality(),
        }
    }

    /// Whether the Command is declared `opens_modal` (menus never are).
    fn opens_modal(&self) -> bool {
        match self {
            Self::Command { entry, .. } => entry.handler.opens_modal(),
            Self::Menu(_) => false,
        }
    }
}

/// Aborts a spawned task when dropped: the auto-defer never outlives the
/// handler run.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The most follow-ups a user-install-only interaction may send (INT-11).
const USER_INSTALL_FOLLOWUPS: u8 = 5;

/// Stage 6: spawns the auto-defer task of an interaction received at
/// `received_at` (INT-01), or for an `opens_modal` Command the task that
/// logs a missed ack deadline (INT-05).
fn arm(
    app: &AppShared,
    shared: &Arc<Shared>,
    resolved: &Resolved,
    received_at: tokio::time::Instant,
    span: &Span,
) -> AbortOnDrop {
    let shared = Arc::clone(shared);
    let handler = resolved.handler();
    let task = if resolved.opens_modal() {
        let module = resolved.module();
        let deadline = received_at + ACK_DEADLINE;
        tokio::spawn(
            async move {
                tokio::time::sleep_until(deadline).await;
                if shared.state() == PENDING {
                    tracing::error!(
                        target: "noctis::dispatch",
                        handler,
                        module,
                        "an `opens_modal` handler did not answer within the 3 s ack deadline"
                    );
                }
            }
            .instrument(span.clone()),
        )
    } else {
        let deadline = received_at + app.ack_deadline;
        tokio::spawn(
            async move {
                tokio::time::sleep_until(deadline).await;
                if !shared.auto_defer().await {
                    tracing::warn!(target: "noctis::dispatch", handler, "auto-defer failed");
                }
            }
            .instrument(span.clone()),
        )
    };
    AbortOnDrop(task)
}

/// The Command Path of chat-input `data`: its name, then the subcommand
/// group and subcommand it carries.
fn command_path(data: &CommandData) -> String {
    let mut path = data.name.clone();
    let mut options = &data.options;
    while let Some(first) = options.first() {
        match &first.value {
            CommandOptionValue::SubCommandGroup(children)
            | CommandOptionValue::SubCommand(children) => {
                path.push(' ');
                path.push_str(&first.name);
                options = children;
            }
            _ => break,
        }
    }
    path
}

/// The chat-input entry registered at `path` in `scope`, with its
/// `'static` path.
fn command_at(scope: Option<&ScopeCommands>, path: &str) -> Option<Resolved> {
    let entry = scope?.command(path)?;
    let path = entry.handler.paths().iter().find(|p| **p == path)?;
    Some(Resolved::Command {
        entry: Arc::clone(entry),
        path,
    })
}

/// Stage 1: the Command of `data`, in its scope only (no fallback).
fn resolve(registry: &Registry, data: &CommandData) -> Result<Resolved, String> {
    let scope = data
        .guild_id
        .map_or(Scope::Global, |id| Scope::Guild(id.get()));
    let commands = registry.commands().scope(scope);
    let menu = |kind| {
        commands
            .and_then(|c| c.menu(kind, &data.name))
            .map(|entry| Resolved::Menu(Arc::clone(entry)))
            .ok_or_else(|| data.name.clone())
    };
    match data.kind {
        CommandType::ChatInput => {
            let path = command_path(data);
            command_at(commands, &path).ok_or(path)
        }
        CommandType::User => menu(MenuKind::User),
        CommandType::Message => menu(MenuKind::Message),
        _ => Err(data.name.clone()),
    }
}

/// Where a handler run is reported.
struct Report<'a> {
    app: &'a Arc<AppShared>,
    shard: Option<u32>,
    guild_id: Option<Id<GuildMarker>>,
    module: &'static str,
    handler: &'static str,
    event_type: Option<&'static str>,
}

/// How one handler run ended.
enum Outcome {
    Returned(HandlerResult<()>),
    Panicked(Box<dyn Any + Send>),
    Cancelled,
}

/// Stages 9 and 10: starts the handler with a Pending responder over
/// `shared` (catching a panic at the call), sends what it returned
/// (a returned message is the reply while Pending, else an edit of the
/// original, INT-12), races the whole run against `cancel`, records the
/// duration and reports an error, a panic or a cancellation to the Command
/// Error Sink. `timer` (the auto-defer) is aborted as soon as the run ends.
async fn execute(
    span: &Span,
    cancel: Option<&CancellationToken>,
    shared: Arc<Shared>,
    start: impl FnOnce(Arc<Shared>) -> HandlerFuture,
    timer: Option<AbortOnDrop>,
    report: Report<'_>,
) {
    let app = report.app;
    let kind = ErrorOrigin::Command.as_str();
    let outcome = if cancel.is_some_and(CancellationToken::is_cancelled) {
        Outcome::Cancelled
    } else {
        let started_at = Instant::now();
        let handle = Arc::clone(&shared);
        let started =
            span.in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| start(handle))));
        let outcome = match started {
            Ok(fut) => {
                let run = AssertUnwindSafe(async move {
                    match fut.await?.0 {
                        Response::Done => Ok(()),
                        Response::Message(msg) => {
                            shared.finish(msg).await.map_err(HandlerError::new)
                        }
                    }
                })
                .catch_unwind()
                .instrument(span.clone());
                let result = match cancel {
                    Some(cancel) => tokio::select! {
                        biased;
                        () = cancel.cancelled() => None,
                        result = run => Some(result),
                    },
                    None => Some(run.await),
                };
                match result {
                    None => Outcome::Cancelled,
                    Some(Ok(result)) => Outcome::Returned(result),
                    Some(Err(panic)) => Outcome::Panicked(panic),
                }
            }
            Err(panic) => Outcome::Panicked(panic),
        };
        metrics::histogram!(
            "noctis_handler_duration_seconds",
            "app" => Arc::clone(&app.app),
            "kind" => kind
        )
        .record(started_at.elapsed().as_secs_f64());
        outcome
    };
    drop(timer);
    let report_kind = match &outcome {
        Outcome::Returned(Ok(())) => return,
        Outcome::Returned(Err(error)) => ReportKind::Error(error),
        Outcome::Panicked(panic) => ReportKind::Panic(panic_message(&**panic)),
        Outcome::Cancelled => ReportKind::Cancelled,
    };
    metrics::counter!(
        "noctis_handler_errors_total",
        "app" => Arc::clone(&app.app),
        "kind" => kind
    )
    .increment(1);
    span.in_scope(|| {
        app.sinks.report(ErrorReport {
            origin: ErrorOrigin::Command,
            kind: report_kind,
            app: &app.app,
            shard: report.shard,
            guild_id: report.guild_id,
            module: Some(report.module),
            handler: Some(report.handler),
            event_type: report.event_type,
        });
    });
}

/// Starts the resolved handler: decodes the options or the menu target
/// from `raw` (a failure is the handler's error; it does not run).
fn start(
    resolved: &Resolved,
    raw: &[u8],
    parts: CtxInner,
) -> impl FnOnce(Arc<Shared>) -> HandlerFuture {
    let call: Box<dyn FnOnce(Arc<Shared>) -> HandlerFuture + Send> = match resolved {
        Resolved::Command { entry, path } => {
            let entry = Arc::clone(entry);
            let path: &'static str = path;
            match CommandInput::from_json(raw) {
                Ok(input) => Box::new(move |r| {
                    let ctx = CommandCtx::from_parts(CtxInner {
                        path,
                        input,
                        ..parts
                    });
                    entry.handler.call(ctx, r)
                }),
                Err(error) => Box::new(move |_| failed(HandlerError::new(error))),
            }
        }
        Resolved::Menu(entry) => {
            let entry = Arc::clone(entry);
            match MenuInput::from_json(raw).and_then(|input| input.target()) {
                Ok(target) => Box::new(move |r| {
                    let path = entry.handler.menu_name();
                    let ctx = CommandCtx::from_parts(CtxInner {
                        path,
                        input: CommandInput::empty(path),
                        ..parts
                    });
                    entry.handler.call(ctx, target, r)
                }),
                Err(error) => Box::new(move |_| failed(HandlerError::new(error))),
            }
        }
    };
    call
}

/// Runs one `APPLICATION_COMMAND` interaction through the pipeline (see
/// the module docs) in span `noctis.interaction`.
pub(crate) async fn run_interaction(
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    inbound: Inbound,
    cancel: CancellationToken,
) {
    let shard = inbound.shard.number();
    let received_at = inbound.received_at;
    let InboundKind::Interaction(interaction, raw) = inbound.kind else {
        return;
    };
    let span = tracing::info_span!(
        "noctis.interaction",
        app = &*app.app,
        shard,
        guild_id = interaction.guild_id.map(Id::get),
        module = Empty,
        path = Empty,
    );
    // Stage 1: resolve.
    let resolved = match &interaction.data {
        Some(InteractionData::ApplicationCommand(data)) => resolve(&registry, data),
        _ => Err(String::new()),
    };
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(path) => {
            span.in_scope(|| {
                tracing::warn!(target: "noctis::dispatch", shard, path, "unknown command");
            });
            return;
        }
    };
    span.record("module", resolved.module());
    span.record("path", resolved.path());
    let Some(user_id) = interaction.author_id() else {
        span.in_scope(|| {
            tracing::warn!(target: "noctis::dispatch", shard, "command interaction without an author");
        });
        return;
    };
    // Stage 2 (Ref finalization) lands with Story 7.5; stage 3 (Module
    // enablement) with Stories 13.2 and 13.3.
    // Stage 4: the locale default.
    let Interaction {
        application_id,
        id: interaction_id,
        token,
        locale,
        guild_locale,
        guild_id,
        channel,
        authorizing_integration_owners: owners,
        ..
    } = *interaction;
    let ephemerality = resolved.ephemerality();
    let channel_id = channel.as_ref().map(|c| c.id);
    let parts = CtxInner {
        module: resolved.module(),
        shard,
        guild_id,
        channel_id,
        user_id,
        user_locale: locale,
        guild_locale,
        app: Some(Arc::clone(&app)),
        ..CtxInner::detached_parts(ephemerality)
    };
    // Stage 5 (rate limits) lands with Story 13.4.
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(&app),
        application_id,
        interaction_id,
        token,
        received_at,
        channel_id,
    });
    // A user-install-only interaction may send 5 follow-ups (INT-11).
    let cap = (owners.guild.is_none() && owners.user.is_some()).then_some(USER_INSTALL_FOLLOWUPS);
    let state = Shared::with_followup_cap(transport, ephemerality, app.limits.clone(), cap);
    // Stage 6: the auto-defer.
    let timer = arm(&app, &state, &resolved, received_at, &span);
    // Stages 7 (before-Middleware, Story 7.6) and 8 (Guards, Story 7.7).
    let report = Report {
        app: &app,
        shard: Some(shard),
        guild_id,
        module: resolved.module(),
        handler: resolved.handler(),
        event_type: Some(INTERACTION_CREATE),
    };
    let start = start(&resolved, raw.as_bytes(), parts);
    execute(&span, Some(&cancel), state, start, Some(timer), report).await;
}

/// Runs registered Commands from outside an interaction, such as a prefix
/// command (AD-24, FR-61); get it with `EventCtx::dispatcher`.
///
/// It resolves the path in the guild's scope, then globally, and runs the
/// pipeline from stage 3 on (FR-21). The handler answers with a reply to
/// the invoking message: `defer` shows the typing indicator, and an
/// ephemeral response is [`ResponderError::EphemeralUnsupported`](crate::ResponderError::EphemeralUnsupported).
/// Handler errors and panics go to the Command Error Sink, not into the
/// result.
///
/// It enforces none of the Command's `default_member_permissions`,
/// `contexts` or `nsfw` metadata (Discord enforces those only for real
/// interactions): the caller must check them before `run_command`.
///
/// ```
/// use noctis_core::events::{EventCtx, MessageCreate};
/// use noctis_core::{CommandInput, CommandInvocation, HandlerResult, OptionValue};
///
/// async fn prefix(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
///     if msg.author.bot {
///         return Ok(());
///     }
///     let Some(text) = msg.content.strip_prefix('!') else { return Ok(()) };
///     // `!warn 3`: the first word is the Command Path, the rest its arguments.
///     let mut words = text.split_whitespace();
///     let Some(path) = words.next() else { return Ok(()) };
///     let mut input = CommandInput::new(path);
///     if let Some(days) = words.next().and_then(|w| w.parse().ok()) {
///         input = input.with_option("days", OptionValue::Integer(days));
///     }
///     let invocation = CommandInvocation::new(input, msg.channel_id, msg.id, msg.author.id)
///         .guild_id(msg.guild_id)
///         .shard(ctx.shard());
///     ctx.dispatcher().run_command(invocation).await?;
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct Dispatcher {
    registry: Arc<Registry>,
    app: Arc<AppShared>,
}

impl fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dispatcher")
            .field("app", &self.app.app)
            .finish_non_exhaustive()
    }
}

/// Why [`Dispatcher::run_command`] did not run a handler.
///
/// ```
/// use noctis_core::DispatchError;
///
/// let err = DispatchError::UnknownCommand { path: "nope".into() };
/// assert_eq!(err.to_string(), "no Command is registered at `nope`");
/// ```
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    /// No Command is registered at the path in the guild's scope or
    /// globally; nothing was sent.
    #[error("no Command is registered at `{path}`")]
    UnknownCommand {
        /// The requested Command Path.
        path: Box<str>,
    },
}

/// One Command invocation for [`Dispatcher::run_command`]: the decoded
/// input, the invoking message and its author, and optionally the guild,
/// the shard and the user's locale.
///
/// ```
/// use noctis_core::{CommandInput, CommandInvocation, OptionValue};
/// use twilight_model::id::Id;
///
/// let input = CommandInput::new("warn").with_option("days", OptionValue::Integer(3));
/// let invocation = CommandInvocation::new(input, Id::new(1), Id::new(2), Id::new(3))
///     .guild_id(Some(Id::new(4)))
///     .shard(0)
///     .locale("tr");
/// assert_eq!(invocation.input().path(), "warn");
/// ```
#[derive(Clone, Debug)]
pub struct CommandInvocation {
    input: CommandInput,
    channel_id: Id<ChannelMarker>,
    message_id: Id<MessageMarker>,
    user_id: Id<UserMarker>,
    guild_id: Option<Id<GuildMarker>>,
    shard: u32,
    locale: Option<String>,
}

impl CommandInvocation {
    /// An invocation of `input` from the message `message_id` in
    /// `channel_id`, written by `user_id`; no guild, shard 0, no locale.
    pub fn new(
        input: CommandInput,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        user_id: Id<UserMarker>,
    ) -> Self {
        Self {
            input,
            channel_id,
            message_id,
            user_id,
            guild_id: None,
            shard: 0,
            locale: None,
        }
    }

    /// Sets the guild: its scope is searched before the global one.
    #[must_use]
    pub fn guild_id(mut self, guild_id: Option<Id<GuildMarker>>) -> Self {
        self.guild_id = guild_id;
        self
    }

    /// Sets the shard the handler's `ctx.shard()` reports.
    #[must_use]
    pub fn shard(mut self, shard: u32) -> Self {
        self.shard = shard;
        self
    }

    /// Sets the User Locale (`ctx.user_locale()`).
    #[must_use]
    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    /// The input.
    pub fn input(&self) -> &CommandInput {
        &self.input
    }
}

impl Dispatcher {
    pub(crate) fn new(app: Arc<AppShared>) -> Self {
        Self {
            registry: Arc::clone(&app.registry),
            app,
        }
    }

    /// Runs the Command registered at the invocation's path, in span
    /// `noctis.dispatch`; its response is a reply to the invoking message.
    ///
    /// # Errors
    ///
    /// [`DispatchError::UnknownCommand`] when no Command is registered at
    /// the path in the guild's scope or globally. A failed handler is not
    /// an error here: it goes to the Command Error Sink.
    pub async fn run_command(&self, invocation: CommandInvocation) -> Result<(), DispatchError> {
        let CommandInvocation {
            input,
            channel_id,
            message_id,
            user_id,
            guild_id,
            shard,
            locale,
        } = invocation;
        let span = tracing::info_span!(
            "noctis.dispatch",
            app = &*self.app.app,
            guild_id = guild_id.map(Id::get),
            module = Empty,
            path = Empty,
        );
        let commands = self.registry.commands();
        let in_guild = guild_id
            .and_then(|id| command_at(commands.scope(Scope::Guild(id.get())), input.path()));
        let Some(resolved) =
            in_guild.or_else(|| command_at(commands.scope(Scope::Global), input.path()))
        else {
            return Err(DispatchError::UnknownCommand {
                path: input.path().into(),
            });
        };
        span.record("module", resolved.module());
        span.record("path", resolved.path());
        let Resolved::Command { entry, path } = &resolved else {
            return Ok(());
        };
        let ephemerality = entry.handler.ephemerality();
        let parts = CtxInner {
            path,
            input,
            module: entry.module,
            shard,
            guild_id,
            channel_id: Some(channel_id),
            user_id,
            user_locale: locale,
            guild_locale: None,
            ephemerality,
            app: Some(Arc::clone(&self.app)),
        };
        let transport = Transport::Message(MessageTarget {
            app: Arc::clone(&self.app),
            channel_id,
            message_id,
        });
        let shared = Shared::new(transport, ephemerality, self.app.limits.clone());
        let report = Report {
            app: &self.app,
            shard: Some(shard),
            guild_id,
            module: entry.module,
            handler: entry.handler.name(),
            event_type: None,
        };
        let entry = Arc::clone(entry);
        let start = move |shared| entry.handler.call(CommandCtx::from_parts(parts), shared);
        execute(&span, None, shared, start, None, report).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
