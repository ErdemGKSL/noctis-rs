//! The Command pipeline (AD-23) and the public [`Dispatcher`] (AD-24,
//! FR-61; LOC-06, INT-12).
//!
//! An `APPLICATION_COMMAND` interaction runs these stages, in this order:
//!
//! 1. resolve the Command from the Registry's O(1) maps by its default
//!    (unlocalized) names, in the scope `data.guild_id` names (LOC-06);
//! 2. Ref resolution: none, since a Command has no Ref (components and
//!    modals resolve theirs, `pipeline::component`);
//! 3. the App's `GuildAccessList` (FR-47), then Module enablement (FR-45).
//!    An interaction from a guild the App does not serve gets one type-4
//!    ephemeral `noctis.module.disabled` notice in the built-in locale
//!    resolution and one `debug` on `noctis::dispatch` (`guild not
//!    served`); a DM (no guild) is always served and a panicking list
//!    counts as not served (fail closed, one `error`). Then the handler's
//!    Module and every ancestor must be enabled in the interaction's guild
//!    (globally in a DM), read from the cached Module-state snapshot with no
//!    port call; a disabled one gets the same immediate localized ephemeral
//!    type-4 `noctis.module.disabled` notice, with one `debug`. Either way
//!    nothing later runs (no locale resolver, rate limit, auto-defer,
//!    Middleware, handler, report or duration sample), and the recorded
//!    Guild Locale is not refreshed (stage 4 never runs). Boot Flags and
//!    App restrictions need no runtime check: their pruned registrations
//!    never reach the Registry;
//! 4. resolve the locales (FR-43): the User Locale as Discord's `locale`,
//!    then `guild_locale`, then the App's default; the Guild Locale as
//!    `guild_locale`, then `locale`, then the default (each step the
//!    code's own locale file or its sibling's), then the App's
//!    `LocaleResolver` once. The run's one locale cell is shared by the
//!    context, the Responder, the notices and the Middleware;
//! 5. rate limits (FR-46, INT-14): every `rate_limit(..)` the handler
//!    declares is checked in declared order in the App's namespaced
//!    `RateLimitStore` (auto limits counted, `manual` limits read); the
//!    first one over its count stops the run before
//!    the auto-defer with one type-4 ephemeral notice (the struct's
//!    `#[on_rate_limited]`, else the App's `rate_limited_handler`, else
//!    `noctis.rate_limited` with `{retry}`), one `debug` on
//!    `noctis::rate_limit` and `noctis_rate_limited_total{app, kind}`; a store
//!    error fails open with one warning;
//! 6. arm the auto-defer (INT-01): a task that acknowledges the interaction
//!    with a type-5 callback at `received_at + ack_deadline` unless the
//!    handler answered first; an `auto_defer = false` handler gets none, only an
//!    error log when it has not answered by the 2.9 s ack deadline
//!    (INT-05). The task is aborted when the handler run ends;
//! 7. run the before-Middleware of the handler's Module path (parent's
//!    first, each Module's in registration order) under `catch_unwind`,
//!    raced against the cancel token, in a `noctis.middleware` span: each
//!    may insert typed Extensions and override the run's locales; a
//!    `Flow::Stop` skips the rest and the
//!    handler (its key's ephemeral notice, or nothing for `Stop(None)`,
//!    which deletes a type-5 deferred original); an error or a panic is
//!    handled like the handler's, naming the Middleware. The Extensions are
//!    then frozen;
//! 8. Route Guards: none for Commands; components and modals run their
//!    Route's guards (`owner`, `permissions`, `guard = path`, in that
//!    order) after the before-Middleware, and a denial sends its localized
//!    ephemeral notice instead of running the handler (`component`);
//! 9. run the handler under `catch_unwind`, raced against the shutdown
//!    cancel token, and send what it returned;
//! 10. run the after-Middleware (child's first, each Module's in reverse
//!     registration order) for every outcome but a cancellation, after the
//!     notice and before the report; an after-Middleware panic is its own
//!     `Panic` report. Send a localized ephemeral notice, then route the
//!     failure to the Command Error Sink: an
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
//! deadline), and its stage-3 Module notice is a public reply to the
//! message. A guild it does not serve is
//! [`DispatchError::GuildNotServed`], with nothing sent.
//!
//! # Autocomplete
//!
//! An `APPLICATION_COMMAND_AUTOCOMPLETE` interaction (INT-06, INT-07,
//! OPT-12) resolves its Command like stage 1, decodes the partial options
//! leniently and finds the focused option's `#[autocomplete(field)]`
//! method through the Command's struct. A Module disabled where it runs
//! (stage 3) answers an empty list without running the method, with one
//! `debug`. The method runs under
//! `catch_unwind`, a deadline of `received_at + 2.5 s` and the shutdown
//! token; it never runs once the deadline has passed. Its suggestions are
//! truncated to 25, their `MessageKey` labels rendered in the user's locale,
//! labels over 100 characters cut and invalid kept suggestions dropped, each
//! with a warning on target `noctis::autocomplete`, and exactly one type-8
//! callback is sent. From a guild the App does not serve (stage 3) it sends
//! an empty list without running the method. Any failure sends an empty list
//! and goes to the Autocomplete Error Sink; an unknown Command or an option
//! without a handler sends an empty list with a warning only. The
//! [`Dispatcher`] has no autocomplete.
//!
//! # Components and modals
//!
//! A `MESSAGE_COMPONENT` or `MODAL_SUBMIT` interaction runs through
//! [`component`] (Stories 7.1, 7.2 and 7.3; CID-04, CID-06, INT-01): its
//! `custom_id` is decoded once and its Route looked up in the Registry's
//! Route map with the borrowed id (AD-25); a `signed` Route's tag is
//! verified with the App's payload key ring. Stage 3 (the App's
//! `GuildAccessList`) runs once the Route is resolved and before any Ref
//! lookup, so a denied click never takes a `one_shot` Ref and a stale id
//! from a denied guild gets the `noctis.module.disabled` notice. A stale or
//! Ref id runs the
//! Expired Handler chain (Route `#[on_stale]`, Route `#[on_expired]`, App
//! `expired_handler`, default `noctis.expired` notice) like a handler; any
//! other input that reaches no handler is one `warn` with a `reason` label
//! and the generic ephemeral notice. The handler then runs through the
//! same stages 6 to 10 as a Command, with its Route Guards at stage 8: the auto-defer follows its
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
use noctis_rules::MessageKey;
use noctis_rules::command::{MenuKind, Scope};
use noctis_ui::V2Message;
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
use crate::guard::GuardDenied;
use crate::handler::{AppShared, HandlerError, HandlerResult};
use crate::locale::{self, LocaleCell, Locales, Stage4};
use crate::menu::MenuInput;
use crate::middleware::{Around, Before, Chain, Invocation, RunInfo, RunOutcome};
use crate::notice;
use crate::options::CommandInput;
use crate::rate_limits::{RateIds, RateLimitHit, RateLimitedCtx, RateLimitedFn, RateRun, Stage5};
use crate::registry::Registry;
use crate::registry::commands::{CommandEntry, MenuEntry, ScopeCommands};
use crate::responder::{
    Ephemerality, GuardPhase, InteractionTarget, MessageTarget, OutgoingMessage, PENDING,
    ResponderError, Response, ResponseValue, Shared, Transport,
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

    /// The `kind` of the stage-3 `debug` line.
    const fn kind(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::Menu(_) => "context_menu",
        }
    }

    fn ephemerality(&self) -> Ephemerality {
        match self {
            Self::Command { entry, .. } => entry.handler.ephemerality(),
            Self::Menu(entry) => entry.handler.ephemerality(),
        }
    }

    /// The Middleware chain of the handler's Module.
    fn chain(&self) -> &Chain {
        match self {
            Self::Command { entry, .. } => &entry.chain,
            Self::Menu(entry) => &entry.chain,
        }
    }

    /// The stage 7/10 Middleware run of the handler for an interaction.
    fn around(
        &self,
        app: &Arc<AppShared>,
        shard: u32,
        guild_id: Option<Id<GuildMarker>>,
        channel_id: Option<Id<ChannelMarker>>,
        user_id: Id<UserMarker>,
        locales: &Arc<LocaleCell>,
    ) -> Around {
        Around::new(
            self.chain(),
            || RunInfo {
                invocation: self.invocation(),
                module: self.module(),
                handler: self.handler(),
                shard,
                guild_id,
                channel_id,
                user_id: Some(user_id),
                app: Some(Arc::clone(app)),
                locales: Arc::clone(locales),
            },
            false,
        )
    }

    /// What the run answers.
    fn invocation(&self) -> Invocation {
        match self {
            Self::Command { path, .. } => Invocation::Command { path },
            Self::Menu(entry) => Invocation::ContextMenu {
                name: entry.handler.menu_name(),
            },
        }
    }

    /// The declared rate limits.
    fn rate_limits(&self) -> &'static [crate::RateLimit] {
        match self {
            Self::Command { entry, .. } => entry.handler.rate_limits(),
            Self::Menu(entry) => entry.handler.rate_limits(),
        }
    }

    /// The struct's `#[on_rate_limited]` method.
    fn on_rate_limited(&self) -> Option<Arc<RateLimitedFn>> {
        match self {
            Self::Command { entry, .. } => entry.handler.on_rate_limited().cloned(),
            Self::Menu(entry) => entry.handler.on_rate_limited().cloned(),
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

/// What [`arm`] arms the timer for: the handler, its Module and whether it
/// is auto-deferred.
pub(crate) struct Armed {
    pub(crate) handler: &'static str,
    pub(crate) module: &'static str,
    pub(crate) auto_defer: bool,
}

/// Stage 6 of the command pipeline and stage 5 of the component pipeline:
/// spawns the auto-defer task
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

/// Stage 1: the Command of `data`, in its scope: a guild invocation looks in that
/// guild's scope first, then in the Global scope, since `--publish-guild` mirrors the
/// global Commands into the dev guild.
fn resolve(registry: &Registry, data: &CommandData) -> Result<Resolved, String> {
    let commands = registry.commands();
    let global = commands.scope(Scope::Global);
    let guild = data
        .guild_id
        .and_then(|id| commands.scope(Scope::Guild(id.get())));
    let scopes = [guild, global];
    let menu = |kind| {
        scopes
            .iter()
            .flatten()
            .find_map(|c| c.menu(kind, &data.name))
            .map(|entry| Resolved::Menu(Arc::clone(entry)))
            .ok_or_else(|| data.name.clone())
    };
    match data.kind {
        CommandType::ChatInput => {
            let path = command_path(data);
            scopes
                .into_iter()
                .flatten()
                .find_map(|c| command_at(Some(c), &path))
                .ok_or(path)
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
    /// The run's locale cell: its notices use the User Locale, read when
    /// they are sent (a Middleware override reaches them).
    pub(crate) locales: Arc<LocaleCell>,
    /// The handler run's Middleware; `None` for the Expired Handler chain
    /// and autocomplete (no Middleware, no Extensions in the report).
    pub(crate) around: Option<Around>,
    /// The record of a `one_shot` Ref taken at stage 2, put back when the
    /// Guard phase ends without the handler body starting; `None`
    /// everywhere else.
    pub(crate) restore: Option<Restore>,
}

/// A `one_shot` Ref's taken record and the TTL to put it back with.
pub(crate) struct Restore {
    pub(crate) r: noctis_codec::Ref,
    pub(crate) record: bytes::Bytes,
    pub(crate) ttl: std::time::Duration,
}

/// Puts the taken record of `restore` back under its Ref after the Guard
/// phase ended without the handler body (`put_if_absent`); a failure is one
/// `warn` without the Ref.
async fn restore_ref(app: &AppShared, restore: &Restore) {
    let put = app
        .refs
        .put_if_absent(
            restore.r.as_str(),
            restore.record.clone(),
            Some(restore.ttl),
        )
        .await;
    if !matches!(put, Ok(true)) {
        tracing::warn!(
            target: "noctis::refs",
            "could not restore the one-shot Ref after a refused run"
        );
    }
}

/// How one handler run ended.
pub(crate) enum Outcome {
    Returned(HandlerResult<()>),
    Panicked(Box<dyn Any + Send>),
    /// A before-Middleware stopped the run, with this notice.
    Stopped(Option<MessageKey>),
    /// A Route Guard of `route` refused the run with this notice.
    Denied {
        route: &'static str,
        guard: &'static str,
        key: MessageKey,
    },
    Cancelled,
}

impl Outcome {
    /// The after-Middleware's view of the outcome; `None` for a
    /// cancellation (no after-Middleware runs).
    fn run_outcome(&self) -> Option<RunOutcome> {
        match self {
            Self::Returned(result) => Some(RunOutcome::of(result)),
            Self::Panicked(_) => Some(RunOutcome::Panicked),
            Self::Stopped(_) => Some(RunOutcome::Stopped),
            Self::Denied { .. } => Some(RunOutcome::Denied),
            Self::Cancelled => None,
        }
    }
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

/// Runs `fut`, sends the message it returned (if any) through `shared`,
/// then applies its [`RefOps`](crate::RefOps) to `refs`: only after a
/// successful send (or a `Responded`).
async fn answer(
    fut: HandlerFuture,
    shared: Arc<Shared>,
    refs: Arc<dyn noctis_ports::RefStore>,
) -> HandlerResult<()> {
    let ResponseValue(response, ops) = fut.await?;
    if let Response::Message(msg) = response {
        shared.finish(msg).await.map_err(HandlerError::new)?;
    }
    if !ops.is_empty() {
        crate::refs::apply(&*refs, &ops).await;
    }
    Ok(())
}

/// Runs the handler future `fut` (stage 9): sends what it returned, under
/// `catch_unwind`, raced against `cancel`.
async fn run_handler(
    span: &Span,
    cancel: Option<&CancellationToken>,
    shared: &Arc<Shared>,
    app: &AppShared,
    fut: HandlerFuture,
) -> Outcome {
    let run = AssertUnwindSafe(answer(fut, Arc::clone(shared), Arc::clone(&app.refs)))
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

/// Stages 7 to 9 of a started handler: the before-Middleware, then the
/// handler unless one stopped, failed or panicked. Returns the outcome,
/// when the handler's duration started (`None`: it did not run), and the
/// failing Middleware.
async fn before_and_handler(
    span: &Span,
    cancel: Option<&CancellationToken>,
    shared: &Arc<Shared>,
    app: &AppShared,
    started: Option<Started>,
    around: Option<&Around>,
) -> (Outcome, Option<Instant>, Option<&'static str>) {
    let (at, fut) = match started {
        None => return (Outcome::Cancelled, None, None),
        Some(Started {
            at,
            future: Err(panic),
        }) => return (Outcome::Panicked(panic), Some(at), None),
        Some(Started {
            at,
            future: Ok(fut),
        }) => (at, fut),
    };
    let ran = around.is_some_and(Around::has_before);
    let before = match around {
        Some(around) if ran => around.before(span, cancel).await,
        _ => Before::Continue,
    };
    match before {
        Before::Continue => {
            // Stage 8 (a guarded Route's Guards) runs at the start of `fut`;
            // a denial is its `GuardDenied` error. The handler's duration
            // starts at stage 9: an outcome that ended in the Guard phase
            // (deny, Guard error, panic or cancel) records none.
            let at = if ran { Instant::now() } else { at };
            let outcome = run_handler(span, cancel, shared, app, fut).await;
            let at = (shared.guard_phase() != GuardPhase::GUARDING).then_some(at);
            match outcome {
                Outcome::Returned(Err(error)) => match GuardDenied::of(&error) {
                    Some(d) => (
                        Outcome::Denied {
                            route: d.route,
                            guard: d.guard,
                            key: d.key,
                        },
                        None,
                        None,
                    ),
                    None => (Outcome::Returned(Err(error)), at, None),
                },
                outcome => (outcome, at, None),
            }
        }
        Before::Stop(key) => (Outcome::Stopped(key), None, None),
        Before::Failed(error, name) => (Outcome::Returned(Err(error)), None, Some(name)),
        Before::Panicked(panic, name) => (Outcome::Panicked(panic), None, Some(name)),
        Before::Cancelled => (Outcome::Cancelled, None, None),
    }
}

/// Stage 10's after-Middleware of `report`'s run, if it has any: a panic
/// is its own `Panic` report naming the Middleware, plus one
/// `noctis_handler_errors_total`.
async fn run_after(
    span: &Span,
    cancel: Option<&CancellationToken>,
    report: &Report<'_>,
    outcome: RunOutcome,
) {
    let Some(around) = &report.around else {
        return;
    };
    let app = report.app;
    let panicked = |name: &'static str, panic: &str| {
        metrics::counter!(
            "noctis_handler_errors_total",
            "app" => Arc::clone(&app.app),
            "kind" => report.origin.as_str()
        )
        .increment(1);
        report_logged(
            app,
            ErrorReport {
                kind: ReportKind::Panic(panic),
                middleware: Some(name),
                ..report.error_report(ReportKind::Cancelled)
            },
        );
    };
    around.after(span, cancel, outcome, panicked).await;
}

/// Stages 7 to 10 of a started handler (`None`: cancelled before the
/// start): runs the before-Middleware (stage 7; a stop, error or panic
/// skips the handler), freezes the Extensions, then sends what the handler
/// returned (a returned message is the reply or `update` while Pending,
/// else an edit of the original, INT-12), races the whole run against
/// `cancel` and records the handler's duration (handler plus the send). A
/// stop shows its notice (or, without one, deletes a type-5 deferred
/// original); an error or a panic first sends the user a localized
/// ephemeral notice (FR-22, AD-23); then `timer` (the auto-defer) is
/// aborted and the after-Middleware run (stage 10, not after a
/// cancellation or a start panic). A Guard denial (stage 8) is one `debug`
/// on `noctis::guard`, shows its notice and runs the after-Middleware with
/// `RunOutcome::Denied`, with no duration, counter or report. Whenever the
/// Guard phase ended without the handler body starting (a denial, a Guard
/// error or panic, a cancel during the Guards), a taken `one_shot` record
/// is put back first. A user condition is logged at `debug`; anything else (a
/// failed send of a returned message included) is counted and reported to
/// the origin's Error Sink (a cancellation without a notice).
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
    // No Middleware runs for a start panic: neither chain saw the run.
    let start_panicked = matches!(&started, Some(Started { future: Err(_), .. }));
    let (outcome, at, middleware) =
        before_and_handler(span, cancel, &shared, app, started, report.around.as_ref()).await;
    if let Some(at) = at {
        metrics::histogram!(
            "noctis_handler_duration_seconds",
            "app" => Arc::clone(&app.app),
            "kind" => kind
        )
        .record(at.elapsed().as_secs_f64());
    }
    if let Some(around) = &report.around {
        around.freeze();
    }
    // The handler body never started: the taken `one_shot` record goes
    // back before the notice, whatever ended the Guard phase.
    if shared.guard_phase() == GuardPhase::GUARDING
        && let Some(restore) = &report.restore
    {
        restore_ref(app, restore).instrument(span.clone()).await;
    }
    // Sent before the auto-defer is dropped: the gate orders the two.
    match &outcome {
        Outcome::Stopped(Some(key)) => send_notice(span, &shared, &report, *key).await,
        Outcome::Denied { route, guard, key } => {
            span.in_scope(|| {
                tracing::debug!(
                    target: "noctis::guard",
                    route,
                    handler = report.handler,
                    guard,
                    key = key.as_str(),
                    "denied"
                );
            });
            send_notice(span, &shared, &report, *key).await;
        }
        Outcome::Stopped(None) => shared.stop_silently().instrument(span.clone()).await,
        outcome => {
            if let Some(key) = notice_key(outcome) {
                send_notice(span, &shared, &report, key).await;
            }
        }
    }
    drop(timer);
    if !start_panicked && let Some(run_outcome) = outcome.run_outcome() {
        run_after(span, cancel, &report, run_outcome).await;
    }
    let report_kind = match &outcome {
        Outcome::Returned(Ok(())) | Outcome::Stopped(_) | Outcome::Denied { .. } => return,
        Outcome::Returned(Err(error)) if error.is_user() => {
            let key = error.user_message().map(MessageKey::as_str);
            span.in_scope(|| {
                if report.origin == ErrorOrigin::Component {
                    tracing::debug!(
                        target: "noctis::component",
                        handler = report.handler,
                        middleware,
                        key,
                        "user condition"
                    );
                } else {
                    tracing::debug!(
                        target: "noctis::command",
                        handler = report.handler,
                        middleware,
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
                middleware,
                ..report.error_report(report_kind)
            },
        );
    });
}

impl<'a> Report<'a> {
    /// The report of `kind` for this run.
    fn error_report<'r>(&'r self, kind: ReportKind<'r>) -> ErrorReport<'r>
    where
        'a: 'r,
    {
        ErrorReport {
            origin: self.origin,
            kind,
            app: &self.app.app,
            shard: self.shard,
            guild_id: self.guild_id,
            module: Some(self.module),
            handler: Some(self.handler),
            event_type: self.event_type,
            extensions: self.around.as_ref().map(Around::extensions),
            middleware: None,
        }
    }
}

/// AD-23 stage 3 (FR-47): whether the App serves `guild` by its
/// `GuildAccessList`. No guild (a DM, a guildless event) is always served
/// and the list is not consulted. A panic inside the list is caught,
/// logged once at `error` on `noctis::dispatch` naming `what` (the handler
/// or the event type), and counts as not served (fail closed). Synchronous,
/// no port I/O.
pub(crate) fn serves(app: &AppShared, guild: Option<Id<GuildMarker>>, what: &str) -> bool {
    let Some(guild) = guild else {
        return true;
    };
    if let Ok(allowed) =
        std::panic::catch_unwind(AssertUnwindSafe(|| app.guild_access.allows(guild)))
    {
        allowed
    } else {
        tracing::error!(
            target: "noctis::dispatch",
            handler = what,
            "the GuildAccessList panicked: the guild is not served"
        );
        false
    }
}

/// Answers a Command or context menu stage 3 refused: one `debug` on
/// `noctis::dispatch` (`handler` and `kind` only: no user id, no options),
/// then the `noctis.module.disabled` ephemeral notice over `shared` in
/// `locale` (the built-in resolution); a failed send is one `warn`.
/// Components and modals answer through their `Rejection::GuildNotServed`.
async fn not_served(
    span: &Span,
    app: &AppShared,
    shared: &Arc<Shared>,
    locale: noctis_rules::Locale,
    (handler, kind): (&'static str, &'static str),
) {
    span.in_scope(|| {
        tracing::debug!(
            target: "noctis::dispatch",
            handler,
            kind,
            "guild not served"
        );
    });
    let text = span.in_scope(|| notice::text(&*app.strings, locale, keys::MODULE_DISABLED));
    if let Err(error) = shared.notice(text).instrument(span.clone()).await {
        span.in_scope(|| {
            tracing::warn!(
                target: "noctis::dispatch",
                %error,
                "could not send the guild-not-served notice"
            );
        });
    }
}

/// Stage 4 (FR-43, AD-23): the run's locale cell: the built-in resolution
/// of `request`, then the App's `LocaleResolver` once (for `handler`); an
/// interaction's parseable `guild_locale` also refreshes an existing guild
/// record.
pub(crate) fn stage4(app: &AppShared, request: Stage4, handler: &'static str) -> Arc<LocaleCell> {
    if request.user_id.is_some() {
        app.locales.refresh(request.guild_id, request.discord_guild);
    }
    LocaleCell::new(app.locales.resolve_run(request, handler))
}

/// Stage 3 (AD-23): whether `module` is enabled where the run happens
/// (`guild_id`, else globally), read from the cached Module-state
/// snapshot; a disabled one is one `debug`.
pub(crate) fn stage3(
    app: &AppShared,
    span: &Span,
    module: &'static str,
    path: &str,
    guild_id: Option<Id<GuildMarker>>,
) -> bool {
    if app.module_state.is_enabled(module, guild_id.into()) {
        return true;
    }
    span.in_scope(|| {
        tracing::debug!(target: "noctis::dispatch", module, path, "module disabled here");
    });
    false
}

/// Sends the stage-3 `noctis.module.disabled` notice in `locale` over
/// `shared`; a failure to send it is one `warn`.
pub(crate) async fn disabled_notice(
    app: &AppShared,
    span: &Span,
    shared: &Arc<Shared>,
    locale: noctis_rules::Locale,
) {
    let text = span.in_scope(|| notice::text(&*app.strings, locale, keys::MODULE_DISABLED));
    if let Err(error) = shared.notice(text).instrument(span.clone()).await {
        span.in_scope(|| {
            tracing::warn!(
                target: "noctis::dispatch",
                %error,
                "could not send the module-disabled notice"
            );
        });
    }
}

/// The notice key of a failed run: an error's key (see
/// `notice::key_for`), the generic key for a panic, none for a success or
/// a cancellation.
fn notice_key(outcome: &Outcome) -> Option<MessageKey> {
    match outcome {
        Outcome::Returned(Err(error)) => Some(notice::key_for(error)),
        Outcome::Panicked(_) => Some(keys::ERROR_GENERIC),
        Outcome::Returned(Ok(()))
        | Outcome::Stopped(_)
        | Outcome::Denied { .. }
        | Outcome::Cancelled => None,
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
    let locale = report.locales.get().user;
    let text = span.in_scope(|| notice::text(&*report.app.strings, locale, key));
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

/// What stage 5 needs to answer a refused run: the responder state (no
/// callback sent yet), the run's report, the parts a [`RateLimitedCtx`]
/// reads, the struct's `#[on_rate_limited]` method, and the override's
/// deadline and cancel token.
pub(crate) struct Limited<'a> {
    pub(crate) shared: &'a Arc<Shared>,
    pub(crate) report: &'a Report<'a>,
    pub(crate) parts: Arc<CtxInner>,
    pub(crate) hook: Option<Arc<RateLimitedFn>>,
    pub(crate) deadline: tokio::time::Instant,
    pub(crate) cancel: Option<&'a CancellationToken>,
}

/// AD-23 stage 5 (FR-46, INT-14): counts `rate`'s declared limits; on a
/// hit, one `debug` on `noctis::rate_limit` (no ids), one
/// `noctis_rate_limited_total{app, kind}`, a taken `one_shot` record put
/// back, and the answer: the struct's `#[on_rate_limited]` message, else
/// the App's `rate_limited_handler` message, else the localized
/// `noctis.rate_limited` notice, always ephemeral. Returns whether the run
/// was refused (no Middleware, no handler, no report then).
pub(crate) async fn rate_limit(span: &Span, rate: &RateRun, limited: Limited<'_>) -> bool {
    if rate.is_empty() {
        return false;
    }
    let hit = match rate.stage5().instrument(span.clone()).await {
        Stage5::Pass => return false,
        Stage5::Hit(hit) => hit,
    };
    let report = limited.report;
    let app = report.app;
    span.in_scope(|| {
        tracing::debug!(
            target: "noctis::rate_limit",
            handler = report.handler,
            scope = hit.scope().as_str(),
            limit = hit.limit().max_uses(),
            window_secs = hit.limit().window().as_secs(),
            retry_ms = u64::try_from(hit.retry_after().as_millis()).unwrap_or(u64::MAX),
            "rate limited"
        );
    });
    metrics::counter!(
        "noctis_rate_limited_total",
        "app" => Arc::clone(&app.app),
        "kind" => report.origin.as_str()
    )
    .increment(1);
    if let Some(restore) = &report.restore {
        restore_ref(app, restore).instrument(span.clone()).await;
    }
    let hook = limited.hook.clone().or_else(|| app.rate_limited.clone());
    let custom = match hook {
        Some(hook) => run_rate_limited(span, &hook, &limited, hit).await,
        None => None,
    };
    let sent = if let Some(msg) = custom {
        limited
            .shared
            .notice_message(OutgoingMessage::V2(msg))
            .instrument(span.clone())
            .await
    } else {
        let locale = report.locales.get().user;
        let retry = hit.relative_timestamp();
        let text = span.in_scope(|| {
            notice::text_with(
                &*app.strings,
                locale,
                keys::RATE_LIMITED,
                &[("retry", &retry)],
            )
        });
        limited.shared.notice(text).instrument(span.clone()).await
    };
    if let Err(error) = sent {
        span.in_scope(|| {
            tracing::warn!(
                target: "noctis::rate_limit",
                handler = report.handler,
                %error,
                "could not send the rate-limit notice"
            );
        });
    }
    true
}

/// Runs a rate-limited override under `catch_unwind`, `limited.deadline`
/// and the cancel token: its message, or `None` for the default notice (an
/// error or a panic reported to the origin's Error Sink, a missed deadline
/// or a cancellation one `warn`).
async fn run_rate_limited(
    span: &Span,
    hook: &Arc<RateLimitedFn>,
    limited: &Limited<'_>,
    hit: RateLimitHit,
) -> Option<V2Message> {
    let report = limited.report;
    let ctx = RateLimitedCtx::new(Arc::clone(&limited.parts));
    let started = span.in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| hook(ctx, hit))));
    let fut = match started {
        Ok(fut) => fut,
        Err(panic) => {
            let message = panic_message(&*panic);
            span.in_scope(|| {
                report_logged(report.app, report.error_report(ReportKind::Panic(message)));
            });
            return None;
        }
    };
    let run = AssertUnwindSafe(fut)
        .catch_unwind()
        .instrument(span.clone());
    let sleep = tokio::time::sleep_until(limited.deadline);
    let outcome = if let Some(cancel) = limited.cancel {
        tokio::select! {
            biased;
            () = cancel.cancelled() => None,
            () = sleep => None,
            result = run => Some(result),
        }
    } else {
        tokio::select! {
            biased;
            () = sleep => None,
            result = run => Some(result),
        }
    };
    match outcome {
        Some(Ok(Ok(msg))) => Some(msg),
        Some(Ok(Err(error))) => {
            span.in_scope(|| {
                report_logged(report.app, report.error_report(ReportKind::Error(&error)));
            });
            None
        }
        Some(Err(panic)) => {
            let message = panic_message(&*panic);
            span.in_scope(|| {
                report_logged(report.app, report.error_report(ReportKind::Panic(message)));
            });
            None
        }
        None => {
            span.in_scope(|| {
                tracing::warn!(
                    target: "noctis::rate_limit",
                    handler = report.handler,
                    "the rate-limited override missed its deadline: sent the default notice"
                );
            });
            None
        }
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
                        shared: Some(Arc::clone(&r)),
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
                        shared: Some(Arc::clone(&r)),
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
#[expect(
    clippy::too_many_lines,
    reason = "one linear pass over the stages, stage 4 included"
)]
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
    // A message command's target is its `message` rate-limit key.
    let target_message = match (&resolved, &interaction.data) {
        (Resolved::Menu(entry), Some(InteractionData::ApplicationCommand(data)))
            if entry.handler.kind() == MenuKind::Message =>
        {
            data.target_id.map(Id::cast::<MessageMarker>)
        }
        _ => None,
    };
    // Stage 2 (Ref resolution) has nothing to do: a Command has no Ref.
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
    let transport = Transport::Interaction(InteractionTarget {
        app: Arc::clone(&app),
        application_id,
        interaction_id,
        token,
        received_at,
        channel_id,
        file_limit: crate::responder::file_limit(raw.as_bytes()),
    });
    // A user-install-only interaction may send 5 follow-ups (INT-11).
    let cap = (owners.guild.is_none() && owners.user.is_some()).then_some(USER_INSTALL_FOLLOWUPS);
    // Stage 3: the App's GuildAccessList, then Module enablement (cached
    // state only).
    if !span.in_scope(|| serves(&app, guild_id, resolved.handler())) {
        let builtin = app
            .locales
            .builtin_raw(locale.as_deref(), guild_locale.as_deref());
        let state =
            Shared::with_followup_cap(transport, Ephemerality::Ephemeral, app.limits.clone(), cap);
        let what = (resolved.handler(), resolved.kind());
        return not_served(&span, &app, &state, builtin.user, what).await;
    }
    let (discord_user, discord_guild) = (
        locale::parse(locale.as_deref()),
        locale::parse(guild_locale.as_deref()),
    );
    if !stage3(&app, &span, resolved.module(), resolved.path(), guild_id) {
        // Before stage 4: the built-in locale resolution (no resolver).
        let builtin = LocaleCell::new(app.locales.builtin(discord_user, discord_guild));
        let state =
            Shared::with_followup_cap(transport, Ephemerality::Ephemeral, app.limits.clone(), cap);
        state.set_i18n(Arc::clone(&app.strings), Arc::clone(&builtin));
        return disabled_notice(&app, &span, &state, builtin.get().user).await;
    }
    // Stage 4: resolve the locales.
    let locales = stage4(
        &app,
        Stage4 {
            invocation: resolved.invocation(),
            guild_id,
            user_id: Some(user_id),
            discord_user,
            discord_guild,
        },
        resolved.handler(),
    );
    let around = resolved.around(&app, shard, guild_id, channel_id, user_id, &locales);
    let rate = Arc::new(RateRun::new(
        Arc::clone(&app.rate_limits),
        resolved.handler(),
        resolved.rate_limits(),
        RateIds {
            user: user_id,
            guild: guild_id,
            channel: channel_id,
            message: target_message,
        },
    ));
    let parts = CtxInner {
        path: resolved.path(),
        module: resolved.module(),
        shard,
        guild_id,
        channel_id,
        user_id,
        locales: Arc::clone(&locales),
        app: Some(Arc::clone(&app)),
        run: around.run(),
        rate: Some(Arc::clone(&rate)),
        ..CtxInner::detached_parts(ephemerality)
    };
    let state = Shared::with_followup_cap(transport, ephemerality, app.limits.clone(), cap);
    state.set_i18n(Arc::clone(&app.strings), Arc::clone(&locales));
    let report = Report {
        app: &app,
        origin: ErrorOrigin::Command,
        shard: Some(shard),
        guild_id,
        module: resolved.module(),
        handler: resolved.handler(),
        event_type: Some(INTERACTION_CREATE),
        locales,
        around: Some(around),
        restore: None,
    };
    // Stage 5: the declared rate limits; a refused run is answered here.
    let limited = Limited {
        shared: &state,
        report: &report,
        parts: Arc::new(parts.rate_limited_parts()),
        hook: resolved.on_rate_limited(),
        deadline: received_at + app.ack_deadline,
        cancel: Some(&cancel),
    };
    if rate_limit(&span, &rate, limited).await {
        return;
    }
    // Stage 6: the auto-defer.
    let armed = Armed {
        handler: resolved.handler(),
        module: resolved.module(),
        auto_defer: resolved.auto_defer(),
    };
    let timer = arm(&app, &state, &armed, received_at, &span);
    // Stage 7 (before-Middleware) runs in `run_started`, once the handler
    // future exists; Commands have no stage 8 (Guards).
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
        .callback_json(&callback_body(choices), false)
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
#[expect(
    clippy::too_many_lines,
    reason = "one linear pass over the stages, stage 4 included"
)]
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
        // Autocomplete answers with choices, never files.
        file_limit: None,
    };
    let report_for = |module, handler, locales| Report {
        app: &app,
        origin: ErrorOrigin::Autocomplete,
        shard: Some(shard),
        guild_id,
        module,
        handler,
        event_type: Some(INTERACTION_CREATE),
        // Autocomplete sends no notice.
        locales,
        // Autocomplete runs no Middleware.
        around: None,
        restore: None,
    };
    // Stage 3: the App's GuildAccessList; a guild it does not serve gets
    // an empty list, with no handler run and no report.
    if let Ok(resolved) = &resolved
        && !span.in_scope(|| serves(&app, guild_id, resolved.handler()))
    {
        let sent = send_choices(&target, &[]).instrument(span.clone()).await;
        if let Err(error) = sent {
            span.in_scope(|| {
                tracing::warn!(target: "noctis::autocomplete", %error, "autocomplete callback failed");
            });
        }
        return;
    }
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
                let locales = LocaleCell::new(Locales::both(app.locales.default_locale()));
                let report = report_for(module, handler, locales);
                report_autocomplete(&span, &report, ReportKind::Error(error));
            }
            return;
        }
    };
    // Stage 3: a disabled Module's method never runs; empty choices.
    if !stage3(&app, &span, found.entry.module, found.path, guild_id) {
        if let Err(error) = send_choices(&target, &[]).instrument(span.clone()).await {
            span.in_scope(|| {
                tracing::warn!(target: "noctis::autocomplete", %error, "autocomplete callback failed");
            });
        }
        return;
    }
    // Stage 4: resolve the locales.
    let locales = stage4(
        &app,
        Stage4 {
            invocation: Invocation::Autocomplete { path: found.path },
            guild_id,
            user_id: Some(found.user_id),
            discord_user: locale::parse(locale.as_deref()),
            discord_guild: locale::parse(guild_locale.as_deref()),
        },
        found.handler.name(),
    );
    let report = report_for(
        found.entry.module,
        found.handler.name(),
        Arc::clone(&locales),
    );
    let kind = found.focused.kind();
    let ctx = AutocompleteCtx::new(AutocompleteParts {
        path: found.path,
        module: found.entry.module,
        shard,
        guild_id,
        channel_id,
        user_id: found.user_id,
        locales,
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
                extensions: None,
                middleware: None,
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
/// The shipped caller is `noctis::prefix::PrefixPlugin` (the `prefix`
/// feature), which parses `!warn <@5> 3` against the Command's options.
/// A caller that fails to decode marks the input with
/// [`CommandInput::with_error`]: the handler then does not run, and the
/// user gets the same validation notice as for a failed slash decode.
///
/// A hand-written caller looks like this:
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
    /// The App's `GuildAccessList` does not serve the invocation's guild
    /// (AD-23 stage 3); nothing was sent and no handler ran.
    ///
    /// ```
    /// use noctis_core::DispatchError;
    /// use twilight_model::id::Id;
    ///
    /// let err = DispatchError::GuildNotServed { guild_id: Id::new(7) };
    /// assert_eq!(err.to_string(), "guild `7` is not served by this App");
    /// ```
    #[error("guild `{guild_id}` is not served by this App")]
    GuildNotServed {
        /// The invocation's guild.
        guild_id: Id<GuildMarker>,
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

    /// Sets the user's raw locale code, as Discord would send it (`"tr"`,
    /// `"en-GB"`): stage 4 parses it exactly and resolves it like an
    /// interaction's `locale` (an unknown code counts as absent). The Guild
    /// Locale comes from the guild's recorded `preferred_locale`.
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

    /// The responder state of a message-transport run on `target` (channel,
    /// message), whose locale keys resolve in the run's `locales`.
    fn message_shared(
        &self,
        (channel_id, message_id): (Id<ChannelMarker>, Id<MessageMarker>),
        ephemerality: Ephemerality,
        locales: Arc<LocaleCell>,
    ) -> Arc<Shared> {
        let transport = Transport::Message(MessageTarget {
            app: Arc::clone(&self.app),
            channel_id,
            message_id,
        });
        let shared = Shared::new(transport, ephemerality, self.app.limits.clone());
        shared.set_i18n(Arc::clone(&self.app.strings), locales);
        shared
    }

    /// Runs the Command registered at the invocation's path, in span
    /// `noctis.dispatch`; its response is a reply to the invoking message.
    ///
    /// # Errors
    ///
    /// [`DispatchError::UnknownCommand`] when no Command is registered at
    /// the path in the guild's scope or globally;
    /// [`DispatchError::GuildNotServed`] when the App's `GuildAccessList`
    /// does not serve the invocation's guild (nothing is sent). A failed
    /// handler is not an error here: it goes to the Command Error Sink.
    #[expect(
        clippy::too_many_lines,
        reason = "one linear pass over the stages, stage 4 included"
    )]
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
        // Stage 3: the App's GuildAccessList; silent, so two white-label
        // Apps sharing a prefix never both answer.
        if let Some(guild_id) = guild_id
            && !span.in_scope(|| serves(&self.app, Some(guild_id), entry.handler.name()))
        {
            return Err(DispatchError::GuildNotServed { guild_id });
        }
        let ephemerality = entry.handler.ephemerality();
        // Stage 3: a disabled Module answers a public notice reply.
        if !stage3(&self.app, &span, entry.module, path, guild_id) {
            let builtin = LocaleCell::new(self.app.locales.builtin(
                locale::parse(locale.as_deref()),
                self.app.locales.guild(guild_id),
            ));
            let user = builtin.get().user;
            let shared = self.message_shared((channel_id, message_id), ephemerality, builtin);
            disabled_notice(&self.app, &span, &shared, user).await;
            return Ok(());
        }
        // Stage 4: the User Locale the caller gave, the guild's recorded one;
        // only interactions refresh the record.
        let locales = LocaleCell::new(self.app.locales.resolve_run(
            Stage4 {
                invocation: Invocation::Command { path },
                guild_id,
                user_id: Some(user_id),
                discord_user: locale::parse(locale.as_deref()),
                discord_guild: self.app.locales.guild(guild_id),
            },
            entry.handler.name(),
        ));
        let around = Around::new(
            &entry.chain,
            || RunInfo {
                invocation: Invocation::Command { path },
                module: entry.module,
                handler: entry.handler.name(),
                shard,
                guild_id,
                channel_id: Some(channel_id),
                user_id: Some(user_id),
                app: Some(Arc::clone(&self.app)),
                locales: Arc::clone(&locales),
            },
            false,
        );
        let rate = Arc::new(RateRun::new(
            Arc::clone(&self.app.rate_limits),
            entry.handler.name(),
            entry.handler.rate_limits(),
            RateIds {
                user: user_id,
                guild: guild_id,
                channel: Some(channel_id),
                message: None,
            },
        ));
        let parts = CtxInner {
            path,
            input,
            module: entry.module,
            shard,
            guild_id,
            channel_id: Some(channel_id),
            user_id,
            locales: Arc::clone(&locales),
            ephemerality,
            app: Some(Arc::clone(&self.app)),
            current_ref: None,
            run: around.run(),
            member_permissions: None,
            shared: None,
            values: crate::component::SelectedValues::default(),
            unknown: Vec::new(),
            rate: Some(Arc::clone(&rate)),
        };
        let target = (channel_id, message_id);
        let shared = self.message_shared(target, ephemerality, Arc::clone(&locales));
        let report = Report {
            app: &self.app,
            origin: ErrorOrigin::Command,
            shard: Some(shard),
            guild_id,
            module: entry.module,
            handler: entry.handler.name(),
            event_type: None,
            locales,
            around: Some(around),
            restore: None,
        };
        // Stage 5: the declared rate limits (a slash Command has no
        // `message` scope, so the invoking message keys nothing).
        let limited = Limited {
            shared: &shared,
            report: &report,
            parts: Arc::new(parts.rate_limited_parts()),
            hook: entry.handler.on_rate_limited().cloned(),
            deadline: tokio::time::Instant::now() + self.app.ack_deadline,
            cancel: None,
        };
        if rate_limit(&span, &rate, limited).await {
            return Ok(());
        }
        let entry = Arc::clone(entry);
        let start = move |shared: Arc<Shared>| {
            let parts = CtxInner {
                shared: Some(Arc::clone(&shared)),
                ..parts
            };
            entry.handler.call(CommandCtx::from_parts(parts), shared)
        };
        execute(&span, None, shared, start, None, report).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
