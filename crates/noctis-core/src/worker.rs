//! The worker set: the drain task, the event semaphore, the per-guild
//! ordered sub-queues and panic-safe handler runs (FR-5, FR-6, AD-10,
//! NFR-4).

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Instant;

use futures_util::FutureExt;
use futures_util::future::join_all;
use noctis_gateway::{Inbound, InboundKind, LaneReceiver};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::Instrument;
use tracing::field::Empty;
use twilight_model::application::interaction::InteractionType;
use twilight_model::gateway::event::Event;
use twilight_model::id::Id;
use twilight_model::id::marker::{GuildMarker, InteractionMarker};

use crate::events::EventCtx;
use crate::handler::{AppShared, HandlerResult};
use crate::middleware::{Around, Before, Invocation, RunInfo, RunOutcome};
use crate::pipeline::component::run_component;
use crate::pipeline::{run_autocomplete, run_interaction};
use crate::registry::{Entry, Handlers, Registry};
use crate::sink::{ErrorOrigin, ErrorReport, ReportKind};

/// How many ordered sub-queues exist (a power of two: `subqueue` takes the
/// top 6 bits of a multiplicative hash).
pub(crate) const SUBQUEUES: usize = 64;

/// The ordered sub-queue of `guild_id`; DMs and guildless events use 0.
pub(crate) fn subqueue(guild_id: Option<Id<GuildMarker>>) -> usize {
    match guild_id {
        None => 0,
        // The top 6 bits of the product always fit a `usize`.
        Some(id) => (id.get().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize,
    }
}

/// Aborts the task it holds when dropped, on every return path of `run`.
pub(crate) struct AbortOnDrop(pub(crate) JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The shutdown controls of a worker set.
#[derive(Debug, Clone, Default)]
#[expect(
    clippy::struct_field_names,
    reason = "`shutdown` names the AD-21 phase the token starts"
)]
pub(crate) struct Shutdown {
    /// Tracks every task the worker set spawns.
    pub(crate) tracker: TaskTracker,
    /// Fired when the shutdown starts: the drain empties the lanes and
    /// returns.
    pub(crate) shutdown: CancellationToken,
    /// Fired when the grace period ends: queued lane items are discarded and
    /// every handler still running or queued is cancelled.
    pub(crate) cancel: CancellationToken,
}

/// One event on its way to its handlers; holds its worker permit until the
/// concurrent part ends.
struct Job {
    shard: u32,
    guild_id: Option<Id<GuildMarker>>,
    event: Event,
    handlers: Arc<Handlers>,
    permit: OwnedSemaphorePermit,
}

/// Dispatches events to their handlers, at most `handler_concurrency` at
/// a time.
pub(crate) struct Workers {
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    semaphore: Arc<Semaphore>,
    /// One sender per ordered sub-queue; empty without ordered handlers.
    ordered: Vec<mpsc::UnboundedSender<Job>>,
    shutdown: Shutdown,
}

impl Workers {
    /// Builds the worker set; spawns the sub-queue tasks when `registry` has
    /// an ordered handler. Must run inside a tokio runtime.
    pub(crate) fn new(
        registry: Arc<Registry>,
        app: Arc<AppShared>,
        handler_concurrency: usize,
        shutdown: Shutdown,
    ) -> Self {
        let ordered = if registry.has_ordered() {
            (0..SUBQUEUES)
                .map(|_| {
                    let (tx, rx) = mpsc::unbounded_channel();
                    shutdown
                        .tracker
                        .spawn(run_subqueue(rx, Arc::clone(&app), shutdown.clone()));
                    tx
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            registry,
            app,
            semaphore: Arc::new(Semaphore::new(handler_concurrency)),
            ordered,
            shutdown,
        }
    }

    /// How many sub-queues run.
    #[cfg(test)]
    pub(crate) fn subqueues(&self) -> usize {
        self.ordered.len()
    }

    /// Hands one command interaction to the Command pipeline, one
    /// autocomplete interaction to the autocomplete stage and one component
    /// click or modal submit to the component pipeline, each as one task on
    /// the tracker (never taking a permit); any other interaction (a ping)
    /// is traced and dropped.
    fn interaction(&self, inbound: Inbound) {
        let (kind, id) = match &inbound.kind {
            InboundKind::Interaction(interaction, _) => {
                (Some(interaction.kind), Some(interaction.id))
            }
            _ => (None, None),
        };
        // Reports the run's end to the hidden observer, also when the task
        // panics or is cancelled (it is dropped either way).
        let observed = id.and_then(|id| {
            self.app
                .run_observer
                .clone()
                .map(|observer| RunEnd { observer, id })
        });
        let (registry, app, cancel) = (
            Arc::clone(&self.registry),
            Arc::clone(&self.app),
            self.shutdown.cancel.clone(),
        );
        match kind {
            Some(InteractionType::ApplicationCommand) => {
                self.shutdown.tracker.spawn(observe(
                    run_interaction(registry, app, inbound, cancel),
                    observed,
                ));
            }
            Some(InteractionType::ApplicationCommandAutocomplete) => {
                self.shutdown.tracker.spawn(observe(
                    run_autocomplete(registry, app, inbound, cancel),
                    observed,
                ));
            }
            Some(InteractionType::MessageComponent | InteractionType::ModalSubmit) => {
                self.shutdown.tracker.spawn(observe(
                    run_component(registry, app, inbound, cancel),
                    observed,
                ));
            }
            _ => {
                interaction_unhandled(inbound, &self.app.app);
                drop(observed);
            }
        }
    }

    /// Hands one event to its handlers; never awaits.
    fn dispatch(&self, shard: u32, event: Event, permit: OwnedSemaphorePermit) {
        let kind = event.kind();
        let Some(handlers) = self.registry.handlers(kind) else {
            tracing::trace!(target: "noctis::app", shard, kind = ?kind, "no handler registered");
            drop(permit);
            return;
        };
        let guild_id = event.guild_id();
        let job = Job {
            shard,
            guild_id,
            event,
            handlers: Arc::clone(handlers),
            permit,
        };
        if job.handlers.ordered.is_empty() {
            self.shutdown.tracker.spawn(run_concurrent(
                job,
                Arc::clone(&self.app),
                self.shutdown.cancel.clone(),
            ));
        } else if let Some(tx) = self.ordered.get(subqueue(guild_id)) {
            // A closed sub-queue only happens during runtime shutdown.
            let _ = tx.send(job);
        }
    }
}

/// Calls the run observer with its interaction id when dropped: when the
/// interaction's task returns, panics or is cancelled.
pub(crate) struct RunEnd {
    observer: crate::handler::RunObserver,
    id: Id<InteractionMarker>,
}

impl Drop for RunEnd {
    fn drop(&mut self) {
        (self.observer)(self.id);
    }
}

/// `run`, then the drop of `end` (if any), which reports the run's end.
pub(crate) async fn observe<F: Future<Output = ()>>(run: F, end: Option<RunEnd>) {
    let _end = end;
    run.await;
}

/// What the drain does next.
enum Next {
    Item(Inbound),
    Done,
    Cancelled,
}

/// Takes every [`Inbound`] from the lanes until every sender has dropped,
/// or until the shutdown token fires and the lanes are empty.
///
/// An interaction never takes a permit. An event waits for one permit,
/// serving interactions that arrive meanwhile first. When the cancel token
/// fires, every queued item is discarded with one summary warning.
pub(crate) async fn drain(mut receiver: LaneReceiver, workers: Workers) {
    let shutdown = workers.shutdown.shutdown.clone();
    let cancel = workers.shutdown.cancel.clone();
    loop {
        let next = if cancel.is_cancelled() {
            Next::Cancelled
        } else if shutdown.is_cancelled() {
            receiver.try_recv().map_or(Next::Done, Next::Item)
        } else {
            tokio::select! {
                biased;
                () = cancel.cancelled() => Next::Cancelled,
                () = shutdown.cancelled() => continue,
                item = receiver.recv() => item.map_or(Next::Done, Next::Item),
            }
        };
        let inbound = match next {
            Next::Item(inbound) => inbound,
            Next::Done => return,
            Next::Cancelled => return discard(&receiver, 0),
        };
        let shard = inbound.shard.number();
        match inbound.kind {
            InboundKind::Event(event) => {
                let permit = loop {
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => return discard(&receiver, 1),
                        Some(interaction) = receiver.recv_interaction() => workers.interaction(interaction),
                        permit = Arc::clone(&workers.semaphore).acquire_owned() => break permit,
                    }
                };
                // The semaphore is never closed.
                let Ok(permit) = permit else { return };
                workers.dispatch(shard, *event, permit);
            }
            _ => workers.interaction(inbound),
        }
    }
}

/// Logs the lane items the cancel token discards; `waiting` counts an event
/// that was waiting for a permit. The receiver's drop clears the lanes.
fn discard(receiver: &LaneReceiver, waiting: usize) {
    let depths = receiver.depths();
    let interactions = depths.interaction;
    let events = depths.event + depths.overflow + waiting;
    if interactions + events > 0 {
        tracing::warn!(
            target: "noctis::app",
            interactions,
            events,
            "lanes discarded at shutdown"
        );
    }
}

/// Handles one interaction without a pipeline yet (components, modals,
/// pings) in span `noctis.interaction`: traced and dropped.
fn interaction_unhandled(inbound: Inbound, app: &str) {
    let shard = inbound.shard.number();
    let guild_id = match &inbound.kind {
        InboundKind::Interaction(interaction, _) => interaction.guild_id.map(Id::get),
        InboundKind::Event(event) => event.guild_id().map(Id::get),
        _ => None,
    };
    let span = tracing::info_span!(
        "noctis.interaction",
        app,
        shard,
        guild_id,
        module = Empty,
        path = Empty,
    );
    span.in_scope(|| {
        tracing::trace!(
            target: "noctis::app",
            shard,
            kind = ?inbound.kind,
            "no handler registered"
        );
    });
    drop(inbound);
}

/// Runs each job's ordered handlers in arrival order, then spawns its
/// concurrent part.
async fn run_subqueue(
    mut jobs: mpsc::UnboundedReceiver<Job>,
    app: Arc<AppShared>,
    shutdown: Shutdown,
) {
    while let Some(Job {
        shard,
        guild_id,
        event,
        handlers,
        permit,
    }) = jobs.recv().await
    {
        let total = handlers.ordered.len() + handlers.concurrent.len();
        let mut event = Some(event);
        for (index, entry) in handlers.ordered.iter().enumerate() {
            let Some(next) = take_event(&mut event, index + 1 == total) else {
                break;
            };
            run_handler(entry, shard, guild_id, next, &app, &shutdown.cancel).await;
        }
        match event {
            Some(event) if !handlers.concurrent.is_empty() => {
                let job = Job {
                    shard,
                    guild_id,
                    event,
                    handlers,
                    permit,
                };
                shutdown.tracker.spawn(run_concurrent(
                    job,
                    Arc::clone(&app),
                    shutdown.cancel.clone(),
                ));
            }
            _ => drop(permit),
        }
    }
}

/// Runs every concurrent handler of `job` at once; the permit is released
/// when all have ended.
async fn run_concurrent(job: Job, app: Arc<AppShared>, cancel: CancellationToken) {
    let Job {
        shard,
        guild_id,
        event,
        handlers,
        permit,
    } = job;
    let total = handlers.concurrent.len();
    let mut event = Some(event);
    let runs = handlers
        .concurrent
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let next = take_event(&mut event, index + 1 == total)?;
            Some(run_handler(entry, shard, guild_id, next, &app, &cancel))
        });
    join_all(runs.collect::<Vec<_>>()).await;
    drop(permit);
}

/// The original event for the last handler, a clone for the others.
fn take_event(event: &mut Option<Event>, last: bool) -> Option<Event> {
    if last { event.take() } else { event.clone() }
}

/// How one handler run ended.
enum Outcome {
    /// The handler (or a before-Middleware) returned.
    Returned(HandlerResult<()>),
    /// The handler (or a before-Middleware) panicked.
    Panicked(Box<dyn Any + Send>),
    /// A before-Middleware stopped the run.
    Stopped,
    /// The cancel token fired first (or before the handler started).
    Cancelled,
}

/// Runs one handler in span `noctis.event`: its before-Middleware, then the
/// handler, then its after-Middleware, catching panics and racing the
/// cancel token (cancel first); records the handler's duration when it
/// ran, and reports an error, a panic or a cancellation to the `Event`
/// sink. Every run ends in exactly one outcome; a filtered event runs no
/// Middleware.
#[expect(
    clippy::too_many_lines,
    reason = "one linear pass over the stages of an event handler run"
)]
pub(crate) async fn run_handler(
    entry: &Entry,
    shard: u32,
    guild_id: Option<Id<GuildMarker>>,
    event: Event,
    app: &Arc<AppShared>,
    cancel: &CancellationToken,
) {
    let handler = &entry.handler;
    let event_type = handler.kind().name().unwrap_or("UNKNOWN");
    let span = tracing::info_span!(
        "noctis.event",
        app = &*app.app,
        shard,
        guild_id = guild_id.map(Id::get),
        module = entry.module,
        handler = handler.name(),
        event_type,
    );
    let around = Around::new(
        &entry.chain,
        || RunInfo {
            invocation: Invocation::Event { event_type },
            module: entry.module,
            handler: handler.name(),
            shard,
            guild_id,
            channel_id: None,
            user_id: None,
            app: Some(Arc::clone(app)),
        },
        true,
    );
    let mut middleware = None;
    // No Middleware runs for a start panic: neither chain saw the run.
    let mut start_panicked = false;
    let outcome = if cancel.is_cancelled() {
        Outcome::Cancelled
    } else {
        let ctx = EventCtx::new(shard, guild_id, entry.module, Arc::clone(app), around.run());
        let started_at = Instant::now();
        let started = span
            .in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| handler.call(ctx, event))));
        match started {
            Ok(Some(fut)) => match around.before(&span, Some(cancel)).await {
                Before::Continue => {
                    let started_at = if around.has_before() {
                        Instant::now()
                    } else {
                        started_at
                    };
                    let run = AssertUnwindSafe(fut)
                        .catch_unwind()
                        .instrument(span.clone());
                    let outcome = tokio::select! {
                        biased;
                        () = cancel.cancelled() => Outcome::Cancelled,
                        result = run => match result {
                            Ok(result) => Outcome::Returned(result),
                            Err(panic) => Outcome::Panicked(panic),
                        },
                    };
                    record_duration(app, started_at);
                    outcome
                }
                // An event sends nothing on a stop, whatever its key.
                Before::Stop(..) => Outcome::Stopped,
                Before::Failed(error, name) => {
                    middleware = Some(name);
                    Outcome::Returned(Err(error))
                }
                Before::Panicked(panic, name) => {
                    middleware = Some(name);
                    Outcome::Panicked(panic)
                }
                Before::Cancelled => Outcome::Cancelled,
            },
            Ok(None) => return,
            Err(panic) => {
                record_duration(app, started_at);
                start_panicked = true;
                Outcome::Panicked(panic)
            }
        }
    };
    around.freeze();
    let base = ErrorReport {
        origin: ErrorOrigin::Event,
        kind: ReportKind::Cancelled,
        app: &app.app,
        shard: Some(shard),
        guild_id,
        module: Some(entry.module),
        handler: Some(handler.name()),
        event_type: Some(event_type),
        extensions: Some(around.extensions()),
        middleware: None,
    };
    let run_outcome = match &outcome {
        _ if start_panicked => None,
        Outcome::Returned(result) => Some(RunOutcome::of(result)),
        Outcome::Panicked(_) => Some(RunOutcome::Panicked),
        Outcome::Stopped => Some(RunOutcome::Stopped),
        Outcome::Cancelled => None,
    };
    if let Some(run_outcome) = run_outcome {
        let panicked = |name: &'static str, panic: &str| {
            count_error(app);
            app.sinks.report(ErrorReport {
                kind: ReportKind::Panic(panic),
                middleware: Some(name),
                ..base
            });
        };
        around
            .after(&span, Some(cancel), run_outcome, panicked)
            .await;
    }
    let report_kind = match &outcome {
        Outcome::Returned(Ok(())) | Outcome::Stopped => return,
        Outcome::Returned(Err(error)) => ReportKind::Error(error),
        Outcome::Panicked(panic) => ReportKind::Panic(panic_message(&**panic)),
        Outcome::Cancelled => ReportKind::Cancelled,
    };
    count_error(app);
    span.in_scope(|| {
        app.sinks.report(ErrorReport {
            kind: report_kind,
            middleware,
            ..base
        });
    });
}

/// Records an event handler's duration since `started_at`.
fn record_duration(app: &AppShared, started_at: Instant) {
    metrics::histogram!(
        "noctis_handler_duration_seconds",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Event.as_str()
    )
    .record(started_at.elapsed().as_secs_f64());
}

/// Counts one event handler failure.
fn count_error(app: &AppShared) {
    metrics::counter!(
        "noctis_handler_errors_total",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Event.as_str()
    )
    .increment(1);
}

/// A panic payload's message: `&str` or `String`, else a placeholder.
pub(crate) fn panic_message(panic: &(dyn Any + Send)) -> &str {
    if let Some(message) = panic.downcast_ref::<&str>() {
        message
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message
    } else {
        "<non-string panic>"
    }
}

#[cfg(test)]
mod tests;
