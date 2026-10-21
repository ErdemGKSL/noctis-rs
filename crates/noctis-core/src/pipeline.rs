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
//!    handler answered first; an `auto_defer = false` handler gets none, only an
//!    error log when it has not answered by the 2.9 s ack deadline
//!    (INT-05). The task is aborted when the handler run ends;
//! 7. *before-Middleware: absent until Story 7.6;*
//! 8. *Guards: absent until Story 7.7;*
//! 9. run the handler under `catch_unwind`, raced against the shutdown
//!    cancel token, and send what it returned;
//! 10. send a localized ephemeral notice, then route the failure to the
//!     Command Error Sink (after-Middleware arrives with Story 7.6): an
//!     error or a panic first shows the user one notice (its
//!     `HandlerError` key, `noctis.validation.failed` for options, else
//!     `noctis.error.generic`) while the auto-defer gate still orders it;
//!     after a defer the deferred original is deleted and the notice is an
//!     ephemeral follow-up (AD-23). A user condition (`HandlerError::user`,
//!     a failed validator) is then logged at `debug`; anything else is
//!     counted and reported, through `LogSink` first when the sink is
//!     custom. A cancellation at shutdown is reported without a notice.
//!
//! The [`Dispatcher`] runs stages 3 onward for a Command invoked from
//! outside an interaction; it arms no auto-defer (a message has no ack
//! deadline).
//!
//! # Autocomplete
//!
//! An `APPLICATION_COMMAND_AUTOCOMPLETE` interaction (INT-06, INT-07,
//! OPT-12) resolves its Command like stage 1, decodes the partial options
//! leniently and finds the focused option's `#[autocomplete(field)]`
//! method through the Command's struct. The method runs under
//! `catch_unwind`, a deadline of `received_at + 2.5 s` and the shutdown
//! token; it never runs once the deadline has passed. Its suggestions are
//! truncated to 25, their `MessageKey` labels rendered in the user's locale,
//! labels over 100 characters cut and invalid kept suggestions dropped, each
//! with a warning on target `noctis::autocomplete`, and exactly one type-8
//! callback is sent. Any failure sends an empty list
//! and goes to the Autocomplete Error Sink; an unknown Command or an option
//! without a handler sends an empty list with a warning only. The
//! [`Dispatcher`] has no autocomplete.
//!
//! # Components and modals
//!
//! A `MESSAGE_COMPONENT` or `MODAL_SUBMIT` interaction runs through
//! [`component`] (Stories 7.1 and 7.2; CID-04, CID-06, INT-01): its
//! `custom_id` is decoded once and its Route looked up in the Registry's
//! Route map with the borrowed id (AD-25); a `signed` Route's tag is
//! verified with the App's payload key ring. Any input that reaches no
//! handler is one `warn` with a `reason` label and, except a stale or Ref
//! id, the generic ephemeral notice. The handler then runs through the
//! same stages 6, 9 and 10 as a Command: the auto-defer follows its
//! declared `response` (type 6 for `update` on a message, else type 5), a
//! returned message is sent per that response, and failures send the
//! notice and go to the Component Error Sink.

use std::any::Any;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use futures_util::FutureExt;
use noctis_gateway::{Inbound, InboundKind};
use noctis_i18n::keys;
use noctis_rules::command::{MenuKind, Scope};
use noctis_rules::{Locale, MessageKey};
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

use crate::autocomplete::{
    __AutocompleteMethod, AUTOCOMPLETE_DEADLINE, AutocompleteCtx, AutocompleteError,
    AutocompleteInput, AutocompleteParts, Labels, Raw, Warnings, callback_body, collect,
    filter_choices,
};
use crate::command::{CommandCtx, CtxInner, HandlerFuture, failed};
use crate::edge::callback::ACK_DEADLINE;
use crate::handler::{AppShared, HandlerError, HandlerResult};
use crate::menu::MenuInput;
use crate::notice;
use crate::options::CommandInput;
use crate::registry::Registry;
use crate::registry::commands::{CommandEntry, MenuEntry, ScopeCommands};
use crate::responder::{
    Ephemerality, InteractionTarget, MessageTarget, PENDING, ResponderError, Response, Shared,
    Transport,
};
use crate::sink::{ErrorOrigin, ErrorReport, ErrorSink as _, LogSink, ReportKind};
use crate::worker::panic_message;

pub(crate) mod component;

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

    /// Whether the handler is auto-deferred: `false` when it is declared
    /// `auto_defer = false` (INT-05).
    fn auto_defer(&self) -> bool {
        match self {
            Self::Command { entry, .. } => entry.handler.auto_defer(),
            Self::Menu(entry) => entry.handler.auto_defer(),
        }
    }
}

/// Aborts a spawned task when dropped: the auto-defer never outlives the
/// handler run.
pub(crate) struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The most follow-ups a user-install-only interaction may send (INT-11).
pub(crate) const USER_INSTALL_FOLLOWUPS: u8 = 5;

/// What stage 6 arms the timer for: the handler, its Module and whether it
/// is auto-deferred.
pub(crate) struct Armed {
    pub(crate) handler: &'static str,
    pub(crate) module: &'static str,
    pub(crate) auto_defer: bool,
}

/// Stage 6, shared by Commands and components: spawns the auto-defer task
/// of an interaction received at `received_at` (INT-01; its callback type
/// follows the declared response, see `Shared::auto_defer`), or for an
/// `auto_defer = false` handler the task that logs a missed ack deadline
/// (INT-05).
pub(crate) fn arm(
    app: &AppShared,
    shared: &Arc<Shared>,
    armed: &Armed,
    received_at: tokio::time::Instant,
    span: &Span,
) -> AbortOnDrop {
    let shared = Arc::clone(shared);
    let handler = armed.handler;
    let task = if armed.auto_defer {
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
    } else {
        let module = armed.module;
        let deadline = received_at + ACK_DEADLINE;
        tokio::spawn(
            async move {
                tokio::time::sleep_until(deadline).await;
                if shared.state() == PENDING {
                    tracing::error!(
                        target: "noctis::dispatch",
                        handler,
                        module,
                        "an `auto_defer = false` handler did not answer within the 3 s ack deadline"
                    );
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
pub(crate) struct Report<'a> {
    pub(crate) app: &'a Arc<AppShared>,
    /// The Error Sink and the `kind` label: Command or Component.
    pub(crate) origin: ErrorOrigin,
    pub(crate) shard: Option<u32>,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) module: &'static str,
    pub(crate) handler: &'static str,
    pub(crate) event_type: Option<&'static str>,
    /// The locale of the user-facing notice (User, then Guild Locale).
    pub(crate) locale: Locale,
}

/// How one handler run ended.
pub(crate) enum Outcome {
    Returned(HandlerResult<()>),
    Panicked(Box<dyn Any + Send>),
    Cancelled,
}

/// A handler start: when it began, and its future or the panic of the
/// call.
pub(crate) struct Started {
    pub(crate) at: Instant,
    pub(crate) future: Result<HandlerFuture, Box<dyn Any + Send>>,
}

/// Stages 9 and 10: starts the handler with a Pending responder over
/// `shared` (catching a panic at the call), then runs it (see
/// [`run_started`]); nothing starts once `cancel` fired.
async fn execute(
    span: &Span,
    cancel: Option<&CancellationToken>,
    shared: Arc<Shared>,
    start: impl FnOnce(Arc<Shared>) -> HandlerFuture,
    timer: Option<AbortOnDrop>,
    report: Report<'_>,
) {
    let started = if cancel.is_some_and(CancellationToken::is_cancelled) {
        None
    } else {
        let at = Instant::now();
        let handle = Arc::clone(&shared);
        let future = span.in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| start(handle))));
        Some(Started { at, future })
    };
    run_started(span, cancel, shared, started, timer, report).await;
}

/// Stages 9 and 10 of a started handler (`None`: cancelled before the
/// start): sends what it returned (a returned message is the reply or
/// `update` while Pending, else an edit of the original, INT-12), races the
/// whole run against `cancel` and records the duration (handler plus the
/// send). An error or a panic first sends the user a localized ephemeral
/// notice (FR-22, AD-23), then `timer` (the auto-defer) is aborted; a user
/// condition is logged at `debug`, anything else (a failed send of a
/// returned message included) is counted and reported to the origin's
/// Error Sink (a cancellation without a notice).
pub(crate) async fn run_started(
    span: &Span,
    cancel: Option<&CancellationToken>,
    shared: Arc<Shared>,
    started: Option<Started>,
    timer: Option<AbortOnDrop>,
    report: Report<'_>,
) {
    let app = report.app;
    let kind = report.origin.as_str();
    let outcome = if let Some(Started {
        at: started_at,
        future,
    }) = started
    {
        let outcome = match future {
            Ok(fut) => {
                let finisher = Arc::clone(&shared);
                let run = AssertUnwindSafe(async move {
                    match fut.await?.0 {
                        Response::Done => Ok(()),
                        Response::Message(msg) => {
                            finisher.finish(msg).await.map_err(HandlerError::new)
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
    } else {
        Outcome::Cancelled
    };
    // Sent before the auto-defer is dropped: the gate orders the two.
    if let Some(key) = notice_key(&outcome) {
        send_notice(span, &shared, &report, key).await;
    }
    drop(timer);
    let report_kind = match &outcome {
        Outcome::Returned(Ok(())) => return,
        Outcome::Returned(Err(error)) if error.is_user() => {
            let key = error.user_message().map(MessageKey::as_str);
            span.in_scope(|| {
                if report.origin == ErrorOrigin::Component {
                    tracing::debug!(
                        target: "noctis::component",
                        handler = report.handler,
                        key,
                        "user condition"
                    );
                } else {
                    tracing::debug!(
                        target: "noctis::command",
                        handler = report.handler,
                        key,
                        "user condition"
                    );
                }
            });
            return;
        }
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
        report_logged(
            app,
            ErrorReport {
                origin: report.origin,
                kind: report_kind,
                app: &app.app,
                shard: report.shard,
                guild_id: report.guild_id,
                module: Some(report.module),
                handler: Some(report.handler),
                event_type: report.event_type,
            },
        );
    });
}

/// The notice key of a failed run: an error's key (see
/// `notice::key_for`), the generic key for a panic, none for a success or
/// a cancellation.
fn notice_key(outcome: &Outcome) -> Option<MessageKey> {
    match outcome {
        Outcome::Returned(Err(error)) => Some(notice::key_for(error)),
        Outcome::Panicked(_) => Some(keys::ERROR_GENERIC),
        Outcome::Returned(Ok(())) | Outcome::Cancelled => None,
    }
}

/// Shows the user the notice of `key`. A failure to send it is one `warn`,
/// never a report.
pub(crate) async fn send_notice(
    span: &Span,
    shared: &Arc<Shared>,
    report: &Report<'_>,
    key: MessageKey,
) {
    let text = span.in_scope(|| notice::text(&*report.app.strings, report.locale, key));
    if let Err(error) = shared.notice(text).instrument(span.clone()).await {
        span.in_scope(|| {
            tracing::warn!(
                target: "noctis::dispatch",
                handler = report.handler,
                %error,
                "could not send the error notice"
            );
        });
    }
}

/// Hands `report` to its origin's sink. A custom sink replaces [`LogSink`],
/// so the full error chain is logged first, inside the current span, and
/// appears in the logs exactly once either way (FR-22).
fn report_logged(app: &AppShared, report: ErrorReport<'_>) {
    if app.sinks.is_custom(report.origin) {
        LogSink.report(report);
    }
    app.sinks.report(report);
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
    let notice_locale = notice::locale(locale.as_deref(), guild_locale.as_deref());
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
    let armed = Armed {
        handler: resolved.handler(),
        module: resolved.module(),
        auto_defer: resolved.auto_defer(),
    };
    let timer = arm(&app, &state, &armed, received_at, &span);
    // Stages 7 (before-Middleware, Story 7.6) and 8 (Guards, Story 7.7).
    let report = Report {
        app: &app,
        origin: ErrorOrigin::Command,
        shard: Some(shard),
        guild_id,
        module: resolved.module(),
        handler: resolved.handler(),
        event_type: Some(INTERACTION_CREATE),
        locale: notice_locale,
    };
    let start = start(&resolved, raw.as_bytes(), parts);
    execute(&span, Some(&cancel), state, start, Some(timer), report).await;
}

/// How an autocomplete handler run ended.
enum AutocompleteOutcome {
    /// It ran and returned its suggestions or an error.
    Answered(HandlerResult<Raw>),
    Panicked(Box<dyn Any + Send>),
    Cancelled,
    DeadlineMissed,
}

/// Sends the one type-8 callback of `choices`; a failure is returned for
/// the caller to report or log.
async fn send_choices(
    target: &InteractionTarget,
    choices: &[crate::AutocompleteChoice],
) -> Result<(), ResponderError> {
    target
        .callback(&callback_body(choices), false)
        .await
        .map(|_| ())
}

/// Logs the filter's warnings for `handler`.
fn log_warnings(warnings: &Warnings, handler: &'static str) {
    if let Some((returned, sent)) = warnings.truncated {
        tracing::warn!(
            target: "noctis::autocomplete",
            handler,
            returned,
            sent,
            "[INT-07] too many autocomplete choices: sent the first ones"
        );
    }
    if warnings.dropped > 0 {
        tracing::warn!(
            target: "noctis::autocomplete",
            handler,
            dropped = warnings.dropped,
            rules = %warnings.rule_list(),
            "invalid autocomplete choices dropped"
        );
    }
    if warnings.labels_cut > 0 {
        tracing::warn!(
            target: "noctis::autocomplete",
            handler,
            cut = warnings.labels_cut,
            "[OPT-09] autocomplete labels over 100 characters cut with `…`"
        );
    }
    if warnings.not_choosable {
        tracing::warn!(
            target: "noctis::autocomplete",
            handler,
            "[OPT-11] the focused option's type takes no autocomplete: sent no choices"
        );
    }
}

/// Runs one `APPLICATION_COMMAND_AUTOCOMPLETE` interaction (see the module
/// docs, "Autocomplete") in span `noctis.interaction`.
pub(crate) async fn run_autocomplete(
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    inbound: Inbound,
    cancel: CancellationToken,
) {
    let shard = inbound.shard.number();
    let received_at = inbound.received_at;
    let deadline = received_at + AUTOCOMPLETE_DEADLINE;
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
    let resolved = match &interaction.data {
        Some(InteractionData::ApplicationCommand(data)) => resolve(&registry, data),
        _ => Err(String::new()),
    };
    let author = interaction.author_id();
    let Interaction {
        application_id,
        id: interaction_id,
        token,
        locale,
        guild_locale,
        guild_id,
        channel,
        ..
    } = *interaction;
    let channel_id = channel.as_ref().map(|c| c.id);
    let target = InteractionTarget {
        app: Arc::clone(&app),
        application_id,
        interaction_id,
        token,
        received_at,
        channel_id,
    };
    let report_for = |module, handler| Report {
        app: &app,
        origin: ErrorOrigin::Autocomplete,
        shard: Some(shard),
        guild_id,
        module,
        handler,
        event_type: Some(INTERACTION_CREATE),
        // Autocomplete sends no notice.
        locale: Locale::EnUs,
    };
    let found = match find_autocomplete(&span, shard, resolved, raw.as_bytes(), author) {
        Ok(found) => found,
        Err(skip) => {
            let sent = send_choices(&target, &[]).instrument(span.clone()).await;
            if let Err(error) = sent {
                span.in_scope(|| {
                    tracing::warn!(target: "noctis::autocomplete", %error, "autocomplete callback failed");
                });
            }
            if let Some((module, handler, error)) = &skip {
                let report = report_for(module, handler);
                report_autocomplete(&span, &report, ReportKind::Error(error));
            }
            return;
        }
    };
    let report = report_for(found.entry.module, found.handler.name());
    let kind = found.focused.kind();
    let ctx = AutocompleteCtx::new(AutocompleteParts {
        path: found.path,
        module: found.entry.module,
        shard,
        guild_id,
        channel_id,
        user_id: found.user_id,
        user_locale: locale,
        guild_locale,
        focused: found.focused,
        app: Some(Arc::clone(&app)),
    });
    let labels = ctx.labels();
    let mut outcome =
        run_autocomplete_handler(&span, &cancel, deadline, &found.handler, ctx, &found.input).await;
    let raw = take_raw(&mut outcome);
    let choices = kept_choices(&span, raw, &labels, kind, &app.limits, report.handler);
    let sent = send_choices(&target, &choices)
        .instrument(span.clone())
        .await;
    finish_autocomplete(&span, &report, &outcome, sent);
}

/// What an autocomplete interaction resolved to.
struct Found {
    entry: Arc<CommandEntry>,
    path: &'static str,
    input: CommandInput,
    focused: crate::Focused,
    handler: __AutocompleteMethod,
    user_id: Id<UserMarker>,
}

/// Why no autocomplete handler runs: `None` after a warning (nothing to
/// blame), else the Module, the Command's handler and the error to report.
type Skip = Option<(&'static str, &'static str, HandlerError)>;

/// Resolves the Command (stage 1), decodes the partial input and finds the
/// focused option's handler, recording `module` and `path` on `span`.
fn find_autocomplete(
    span: &Span,
    shard: u32,
    resolved: Result<Resolved, String>,
    raw: &[u8],
    author: Option<Id<UserMarker>>,
) -> Result<Found, Skip> {
    let warn = |message: &'static str| {
        span.in_scope(|| tracing::warn!(target: "noctis::dispatch", shard, "{message}"));
    };
    let (entry, path) = match resolved {
        Ok(Resolved::Command { entry, path }) => (entry, path),
        Ok(Resolved::Menu(entry)) => {
            span.record("module", entry.module);
            span.record("path", entry.handler.menu_name());
            warn("no autocomplete handler");
            return Err(None);
        }
        Err(path) => {
            span.in_scope(|| {
                tracing::warn!(target: "noctis::dispatch", shard, path, "unknown command");
            });
            return Err(None);
        }
    };
    span.record("module", entry.module);
    span.record("path", path);
    let (input, focused) = AutocompleteInput::from_json(raw)
        .map_err(|e| Some((entry.module, entry.handler.name(), HandlerError::new(e))))?;
    let Some(handler) = entry.handler.autocomplete_for(focused.name()) else {
        warn("no autocomplete handler");
        return Err(None);
    };
    let Some(user_id) = author else {
        warn("autocomplete interaction without an author");
        return Err(None);
    };
    Ok(Found {
        entry,
        path,
        input,
        focused,
        handler,
        user_id,
    })
}

/// The suggestions of a successful run, taken out of `outcome` (which
/// keeps the error, panic or deadline for reporting).
fn take_raw(outcome: &mut AutocompleteOutcome) -> Option<Raw> {
    match outcome {
        AutocompleteOutcome::Answered(Ok(raw)) => Some(std::mem::take(raw)),
        _ => None,
    }
}

/// The choices to send after a run: its suggestions lowered and filtered
/// (warnings logged for `handler`), or none after a failure.
fn kept_choices(
    span: &Span,
    raw: Option<Raw>,
    labels: &Labels,
    kind: noctis_rules::command::OptionType,
    limits: &noctis_rules::Limits,
    handler: &'static str,
) -> Vec<crate::AutocompleteChoice> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let collected = collect(raw, labels, limits);
    let (choices, warnings) = filter_choices(collected, kind, limits);
    span.in_scope(|| log_warnings(&warnings, handler));
    choices
}

/// Runs an autocomplete handler under `catch_unwind`, the deadline and
/// `cancel` (biased to the cancel), and records its duration; it does not
/// start once the deadline has passed or `cancel` fired.
async fn run_autocomplete_handler(
    span: &Span,
    cancel: &CancellationToken,
    deadline: tokio::time::Instant,
    handler: &__AutocompleteMethod,
    ctx: AutocompleteCtx,
    input: &CommandInput,
) -> AutocompleteOutcome {
    if tokio::time::Instant::now() >= deadline {
        return AutocompleteOutcome::DeadlineMissed;
    }
    if cancel.is_cancelled() {
        return AutocompleteOutcome::Cancelled;
    }
    let app = ctx.app_name();
    let started_at = Instant::now();
    let started =
        span.in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| handler.call(ctx, input))));
    let outcome = match started {
        Ok(fut) => {
            let run = tokio::time::timeout_at(
                deadline,
                AssertUnwindSafe(fut)
                    .catch_unwind()
                    .instrument(span.clone()),
            );
            tokio::select! {
                biased;
                () = cancel.cancelled() => AutocompleteOutcome::Cancelled,
                result = run => match result {
                    Err(_) => AutocompleteOutcome::DeadlineMissed,
                    Ok(Ok(result)) => AutocompleteOutcome::Answered(result),
                    Ok(Err(panic)) => AutocompleteOutcome::Panicked(panic),
                },
            }
        }
        Err(panic) => AutocompleteOutcome::Panicked(panic),
    };
    if let Some(app) = app {
        metrics::histogram!(
            "noctis_handler_duration_seconds",
            "app" => app,
            "kind" => ErrorOrigin::Autocomplete.as_str()
        )
        .record(started_at.elapsed().as_secs_f64());
    }
    outcome
}

/// Reports how an autocomplete run ended once its callback was sent: a
/// handler failure goes to the Autocomplete Error Sink (a callback failure
/// after it is only a warning); after a success, a callback failure is
/// reported, except an edge deadline refusal, which is a warning.
fn finish_autocomplete(
    span: &Span,
    report: &Report<'_>,
    outcome: &AutocompleteOutcome,
    sent: Result<(), ResponderError>,
) {
    let warn = |error: &dyn fmt::Display| {
        span.in_scope(|| {
            tracing::warn!(target: "noctis::autocomplete", handler = report.handler, %error, "autocomplete callback failed");
        });
    };
    let error;
    let kind = match outcome {
        AutocompleteOutcome::Answered(Ok(_)) => match sent {
            Ok(()) => return,
            Err(ResponderError::AckDeadlineMissed(missed)) => {
                warn(&missed);
                return;
            }
            Err(e) => {
                error = HandlerError::new(e);
                ReportKind::Error(&error)
            }
        },
        failed => {
            if let Err(e) = sent {
                warn(&e);
            }
            match failed {
                AutocompleteOutcome::Answered(Err(e)) => ReportKind::Error(e),
                AutocompleteOutcome::Panicked(panic) => ReportKind::Panic(panic_message(&**panic)),
                AutocompleteOutcome::Cancelled => ReportKind::Cancelled,
                AutocompleteOutcome::DeadlineMissed | AutocompleteOutcome::Answered(Ok(_)) => {
                    error = HandlerError::new(AutocompleteError::DeadlineMissed {
                        deadline: AUTOCOMPLETE_DEADLINE,
                    });
                    ReportKind::Error(&error)
                }
            }
        }
    };
    report_autocomplete(span, report, kind);
}

/// Counts and reports an autocomplete failure to the Autocomplete Error
/// Sink.
fn report_autocomplete(span: &Span, report: &Report<'_>, kind: ReportKind<'_>) {
    let app = report.app;
    metrics::counter!(
        "noctis_handler_errors_total",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Autocomplete.as_str()
    )
    .increment(1);
    span.in_scope(|| {
        report_logged(
            app,
            ErrorReport {
                origin: ErrorOrigin::Autocomplete,
                kind,
                app: &app.app,
                shard: report.shard,
                guild_id: report.guild_id,
                module: Some(report.module),
                handler: Some(report.handler),
                event_type: report.event_type,
            },
        );
    });
}

/// Runs registered Commands from outside an interaction, such as a prefix
/// command (AD-24, FR-61); get it with `EventCtx::dispatcher`.
///
/// It resolves the path in the guild's scope, then globally, and runs the
/// pipeline from stage 3 on (FR-21). The handler answers with a reply to
/// the invoking message: `defer` shows the typing indicator, and an
/// ephemeral response is [`ResponderError::EphemeralUnsupported`](crate::ResponderError::EphemeralUnsupported).
/// Handler errors and panics send a public notice (a message has no
/// ephemeral form), then go to the Command Error Sink, not into the
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
        let notice_locale = notice::locale(locale.as_deref(), None);
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
            origin: ErrorOrigin::Command,
            shard: Some(shard),
            guild_id,
            module: entry.module,
            handler: entry.handler.name(),
            event_type: None,
            locale: notice_locale,
        };
        let entry = Arc::clone(entry);
        let start = move |shared| entry.handler.call(CommandCtx::from_parts(parts), shared);
        execute(&span, None, shared, start, None, report).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
