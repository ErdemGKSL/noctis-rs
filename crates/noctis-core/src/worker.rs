//! The worker set: one drain task per lane partition, the event
//! semaphore, the per-guild ordered queues and panic-safe handler runs
//! (FR-5, FR-6, AD-10, NFR-4).

use std::any::Any;
use std::collections::hash_map::Entry as MapEntry;
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

use futures_util::FutureExt;
use futures_util::future::join_all;
use metrics::{Counter, Histogram};
use noctis_gateway::{Inbound, InboundKind, LanePartition, LaneReceiver};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
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
use crate::locale::Stage4;
use crate::middleware::{Around, Before, Invocation, RunInfo, RunOutcome};
use crate::pipeline::component::run_component;
use crate::pipeline::{run_autocomplete, run_interaction};
use crate::registry::{Entry, Handlers, Registry};
use crate::sink::{ErrorOrigin, ErrorReport, ReportKind};

/// How many items a partition's drain takes from the lanes under one lock.
pub(crate) const DRAIN_BATCH: NonZeroUsize = match NonZeroUsize::new(32) {
    Some(batch) => batch,
    None => panic!("32 is not zero"),
};

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
    /// `None` when none of `handlers` takes it (the rest of an ordered
    /// job whose last accepting handler was ordered).
    event: Option<Event>,
    handlers: Arc<Handlers>,
    permit: OwnedSemaphorePermit,
}

/// The per-guild FIFOs of ordered jobs, created on demand.
///
/// An entry exists only while its guild (`None`: guildless events) has an
/// ordered job queued or running, and one runner task per entry runs its
/// jobs in arrival order, then removes the entry. Every job holds a worker
/// permit, so the map never holds more than `handler_concurrency` entries.
#[derive(Default)]
struct OrderedQueues {
    // keyed-map: only guilds with a queued or running ordered job, each holding a permit (<= handler_concurrency entries)
    queues: Mutex<HashMap<Option<Id<GuildMarker>>, VecDeque<Job>>>,
}

impl OrderedQueues {
    fn lock(&self) -> MutexGuard<'_, HashMap<Option<Id<GuildMarker>>, VecDeque<Job>>> {
        self.queues.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues `job` behind its guild's running job; returns it when its
    /// guild had none, so the caller starts a runner for it.
    fn enqueue(&self, job: Job) -> Option<Job> {
        let mut queues = self.lock();
        match queues.entry(job.guild_id) {
            MapEntry::Occupied(mut running) => {
                running.get_mut().push_back(job);
                None
            }
            MapEntry::Vacant(vacant) => {
                vacant.insert(VecDeque::new());
                Some(job)
            }
        }
    }

    /// The next job of `guild_id`, or `None` after removing its entry.
    fn next(&self, guild_id: Option<Id<GuildMarker>>) -> Option<Job> {
        let mut queues = self.lock();
        let next = queues.get_mut(&guild_id).and_then(VecDeque::pop_front);
        if next.is_none() {
            queues.remove(&guild_id);
        }
        next
    }
}

/// Removes its guild's entry (dropping the queued jobs and their permits)
/// when a runner ends without emptying it: an aborted task.
struct RunnerGuard {
    ordered: Arc<OrderedQueues>,
    guild_id: Option<Id<GuildMarker>>,
    armed: bool,
}

impl Drop for RunnerGuard {
    fn drop(&mut self) {
        if self.armed {
            let removed = self.ordered.lock().remove(&self.guild_id);
            drop(removed);
        }
    }
}

/// Dispatches events to their handlers, at most `handler_concurrency` at
/// a time.
pub(crate) struct Workers {
    registry: Arc<Registry>,
    app: Arc<AppShared>,
    semaphore: Arc<Semaphore>,
    ordered: Arc<OrderedQueues>,
    shutdown: Shutdown,
}

impl Workers {
    /// Builds the worker set.
    pub(crate) fn new(
        registry: Arc<Registry>,
        app: Arc<AppShared>,
        handler_concurrency: usize,
        shutdown: Shutdown,
    ) -> Self {
        Self {
            registry,
            app,
            semaphore: Arc::new(Semaphore::new(handler_concurrency)),
            ordered: Arc::default(),
            shutdown,
        }
    }

    /// How many guilds have an ordered job queued or running.
    #[cfg(test)]
    pub(crate) fn subqueues(&self) -> usize {
        self.ordered.lock().len()
    }

    /// Hands one command interaction to the Command pipeline, one
    /// autocomplete interaction to the autocomplete stage and one component
    /// click or modal submit to the component pipeline, each as one task on
    /// the tracker (never taking a permit); any other interaction (a ping)
    /// is traced and dropped. Each task first feeds the interaction to the
    /// Discord cache (pings are not fed).
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
                    async move {
                        feed_interaction(&app, &inbound).await;
                        run_interaction(registry, app, inbound, cancel).await;
                    },
                    observed,
                ));
            }
            Some(InteractionType::ApplicationCommandAutocomplete) => {
                self.shutdown.tracker.spawn(observe(
                    async move {
                        feed_interaction(&app, &inbound).await;
                        run_autocomplete(registry, app, inbound, cancel).await;
                    },
                    observed,
                ));
            }
            Some(InteractionType::MessageComponent | InteractionType::ModalSubmit) => {
                self.shutdown.tracker.spawn(observe(
                    async move {
                        feed_interaction(&app, &inbound).await;
                        run_component(registry, app, inbound, cancel).await;
                    },
                    observed,
                ));
            }
            _ => {
                interaction_unhandled(inbound, &self.app.app);
                drop(observed);
            }
        }
    }

    /// Hands one event to `handlers`, the Registry's handlers of its type;
    /// never awaits.
    fn dispatch(
        &self,
        shard: u32,
        event: Event,
        handlers: Arc<Handlers>,
        permit: OwnedSemaphorePermit,
    ) {
        let kind = event.kind();
        let guild_id = event.guild_id();
        // AD-23 stage 3: no handler runs for a guild the App does not serve
        // (the Discord cache and the guild record already saw the event).
        if !crate::pipeline::serves(&self.app, guild_id, kind.name().unwrap_or("UNKNOWN")) {
            tracing::trace!(target: "noctis::dispatch", shard, kind = ?kind, "guild not served");
            drop(permit);
            return;
        }
        let job = Job {
            shard,
            guild_id,
            event: Some(event),
            handlers,
            permit,
        };
        if job.handlers.ordered.is_empty() {
            self.shutdown.tracker.spawn(run_concurrent(
                job,
                Arc::clone(&self.app),
                self.shutdown.cancel.clone(),
            ));
        } else if let Some(job) = self.ordered.enqueue(job) {
            self.shutdown.tracker.spawn(run_ordered(
                job,
                Arc::clone(&self.ordered),
                Arc::clone(&self.app),
                self.shutdown.clone(),
            ));
        }
    }
}

/// Feeds `inbound`'s interaction to the Discord cache when the configured
/// resources read interactions (inside the interaction's task, before its
/// pipeline; the ack deadline still counts from `received_at`).
async fn feed_interaction(app: &AppShared, inbound: &Inbound) {
    if let InboundKind::Interaction(interaction, _) = &inbound.kind {
        app.discord_cache.interaction(&app.app, interaction).await;
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

/// What the cancel token discarded: queued, batched and permit-waiting
/// lane items.
#[derive(Debug, Default, Clone, Copy)]
struct Discarded {
    interactions: usize,
    events: usize,
}

impl Discarded {
    /// The items of `partition`, of the drain's local `batch` and the
    /// `waiting` event (0 or 1) that waited for a permit.
    fn of(partition: &LanePartition, batch: &VecDeque<Inbound>, waiting: usize) -> Self {
        let depths = partition.depths();
        let batched = batch
            .iter()
            .filter(|inbound| matches!(inbound.kind, InboundKind::Event(..)))
            .count();
        Self {
            interactions: depths.interaction + batch.len() - batched,
            events: depths.event + depths.overflow + batched + waiting,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            interactions: self.interactions + other.interactions,
            events: self.events + other.events,
        }
    }

    /// Logs the one summary warning, if anything was discarded.
    fn warn(self) {
        let Self {
            interactions,
            events,
        } = self;
        if interactions + events > 0 {
            tracing::warn!(
                target: "noctis::app",
                interactions,
                events,
                "lanes discarded at shutdown"
            );
        }
    }
}

/// Takes every [`Inbound`] from the lanes until every sender has dropped,
/// or until the shutdown token fires and the lanes are empty.
///
/// Each lane partition has its own drain: one partition is drained inline,
/// several by one task each in a `JoinSet` (aborted with this future). An
/// interaction never takes a permit. An event waits for one permit, serving
/// its partition's interactions that arrive meanwhile first. When the
/// cancel token fires, every queued item is discarded with one summary
/// warning over all partitions.
pub(crate) async fn drain(receiver: LaneReceiver, workers: Workers) {
    drain_shared(receiver, Arc::new(workers)).await;
}

/// [`drain`] over a worker set the caller may also hold.
pub(crate) async fn drain_shared(receiver: LaneReceiver, workers: Arc<Workers>) {
    let mut partitions = receiver.into_partitions();
    let discarded = match partitions.pop() {
        Some(partition) if partitions.is_empty() => drain_partition(partition, &workers).await,
        Some(last) => {
            let mut drains = JoinSet::new();
            for partition in partitions.into_iter().chain([last]) {
                let workers = Arc::clone(&workers);
                drains.spawn(async move { drain_partition(partition, &workers).await });
            }
            let mut total = Discarded::default();
            while let Some(done) = drains.join_next().await {
                // A drain task only fails when the runtime shuts down.
                if let Ok(discarded) = done {
                    total = total.add(discarded);
                }
            }
            total
        }
        None => Discarded::default(),
    };
    discarded.warn();
}

/// What a partition's drain waits for.
enum Wait {
    Cancelled,
    Shutdown,
    Popped(usize),
}

/// What a permit wait ends with.
enum PermitWait {
    Cancelled,
    Interaction(Inbound),
    Permit(Result<OwnedSemaphorePermit, tokio::sync::AcquireError>),
}

/// Drains one lane partition in batches of [`DRAIN_BATCH`] (the batch
/// buffer is allocated once); returns what the cancel token discarded.
async fn drain_partition(mut partition: LanePartition, workers: &Workers) -> Discarded {
    let shutdown = workers.shutdown.shutdown.clone();
    let cancel = workers.shutdown.cancel.clone();
    let mut batch = VecDeque::with_capacity(DRAIN_BATCH.get());
    loop {
        if cancel.is_cancelled() {
            return Discarded::of(&partition, &batch, 0);
        }
        if batch.is_empty() {
            // Let the tasks this batch spawned on this worker thread run
            // before the next batch: every worker may be running a drain.
            tokio::task::yield_now().await;
            let popped = if shutdown.is_cancelled() {
                partition.try_recv_batch(&mut batch, DRAIN_BATCH)
            } else {
                let wait = tokio::select! {
                    biased;
                    () = cancel.cancelled() => Wait::Cancelled,
                    () = shutdown.cancelled() => Wait::Shutdown,
                    popped = partition.recv_batch(&mut batch, DRAIN_BATCH) => Wait::Popped(popped),
                };
                match wait {
                    Wait::Cancelled | Wait::Shutdown => continue,
                    Wait::Popped(popped) => popped,
                }
            };
            if popped == 0 {
                return Discarded::default();
            }
        }
        let Some(inbound) = batch.pop_front() else {
            continue;
        };
        let shard = inbound.shard.number();
        match inbound.kind {
            InboundKind::Event(event) => {
                // Interactions pushed since this batch was popped never wait
                // behind its events.
                while let Some(Some(interaction)) = partition.recv_interaction().now_or_never() {
                    workers.interaction(interaction);
                }
                // The guild record follows every event, handled or not.
                workers.app.locales.observe(&event);
                // The Discord cache sees a cached event type before any of
                // its handlers (bounded; a failure never stops the event).
                workers
                    .app
                    .discord_cache
                    .event(&workers.app.app, &event)
                    .await;
                // An unhandled event never waits on a permit.
                let Some(handlers) = workers.registry.handlers(event.kind()) else {
                    let kind = event.kind();
                    tracing::trace!(target: "noctis::app", shard, kind = ?kind, "no handler registered");
                    continue;
                };
                let handlers = Arc::clone(handlers);
                let permit = loop {
                    let wait = tokio::select! {
                        biased;
                        () = cancel.cancelled() => PermitWait::Cancelled,
                        Some(interaction) = partition.recv_interaction() => PermitWait::Interaction(interaction),
                        permit = Arc::clone(&workers.semaphore).acquire_owned() => PermitWait::Permit(permit),
                    };
                    match wait {
                        PermitWait::Cancelled => return Discarded::of(&partition, &batch, 1),
                        PermitWait::Interaction(interaction) => workers.interaction(interaction),
                        PermitWait::Permit(permit) => break permit,
                    }
                };
                // The semaphore is never closed.
                let Ok(permit) = permit else {
                    return Discarded::default();
                };
                workers.dispatch(shard, *event, handlers, permit);
            }
            _ => workers.interaction(inbound),
        }
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

/// Runs the ordered jobs of one guild (`first`, then each job queued
/// behind it) in arrival order, then removes the guild's entry.
async fn run_ordered(
    first: Job,
    ordered: Arc<OrderedQueues>,
    app: Arc<AppShared>,
    shutdown: Shutdown,
) {
    let mut guard = RunnerGuard {
        ordered,
        guild_id: first.guild_id,
        armed: true,
    };
    let mut job = first;
    loop {
        run_ordered_job(job, &app, &shutdown).await;
        match guard.ordered.next(guard.guild_id) {
            Some(next) => job = next,
            None => break,
        }
    }
    guard.armed = false;
}

/// Runs one job's ordered handlers in fold order, then spawns its
/// concurrent part (with the event when a concurrent handler takes it).
async fn run_ordered_job(job: Job, app: &Arc<AppShared>, shutdown: &Shutdown) {
    let Job {
        shard,
        guild_id,
        event,
        handlers,
        permit,
    } = job;
    let total = handlers.ordered.len() + handlers.concurrent.len();
    let mut share = Share::new(event, total, |index, event| {
        handlers
            .ordered
            .iter()
            .chain(&handlers.concurrent)
            .nth(index)
            .is_some_and(|entry| entry.handler.accepts(event))
    });
    for (index, entry) in handlers.ordered.iter().enumerate() {
        let event = share.take(index, |event| entry.handler.accepts(event));
        run_entry(entry, shard, guild_id, event, app, &shutdown.cancel).await;
    }
    if handlers.concurrent.is_empty() {
        drop(permit);
    } else {
        let job = Job {
            shard,
            guild_id,
            event: share.into_rest(),
            handlers,
            permit,
        };
        shutdown.tracker.spawn(run_concurrent(
            job,
            Arc::clone(app),
            shutdown.cancel.clone(),
        ));
    }
}

/// Runs every concurrent handler of `job` at once; the permit is released
/// when all have ended. One handler is awaited inline; several are joined
/// through one allocation (D4, Story 16.3: `join_all` over an exact-size
/// iterator collects one boxed slice up to 30 futures, and switches to a
/// `FuturesOrdered` above).
async fn run_concurrent(job: Job, app: Arc<AppShared>, cancel: CancellationToken) {
    let Job {
        shard,
        guild_id,
        event,
        handlers,
        permit,
    } = job;
    let concurrent = handlers.concurrent.as_slice();
    let mut share = Share::new(event, concurrent.len(), |index, event| {
        concurrent
            .get(index)
            .is_some_and(|entry| entry.handler.accepts(event))
    });
    match concurrent {
        [] => {}
        [entry] => {
            let event = share.take(0, |event| entry.handler.accepts(event));
            run_entry(entry, shard, guild_id, event, &app, &cancel).await;
        }
        entries => {
            let runs = entries.iter().enumerate().map(|(index, entry)| {
                let event = share.take(index, |event| entry.handler.accepts(event));
                run_entry(entry, shard, guild_id, event, &app, &cancel)
            });
            join_all(runs).await;
        }
    }
    drop(permit);
}

/// One event shared among the handlers of a job (D3, Story 16.3): the
/// last handler whose payload filter accepts it gets the original, the
/// earlier accepting ones a clone each, the rejecting ones nothing.
struct Share<T> {
    /// The original, until the last accepting handler takes it; `None`
    /// from the start when no handler accepts it.
    value: Option<T>,
    /// The index of the last accepting handler.
    last: Option<usize>,
}

impl<T: Clone> Share<T> {
    /// Shares `value` among `count` handlers; `accepts(index, value)` is
    /// handler `index`'s payload filter.
    fn new(value: Option<T>, count: usize, accepts: impl Fn(usize, &T) -> bool) -> Self {
        let last = value
            .as_ref()
            .and_then(|value| (0..count).rev().find(|&index| accepts(index, value)));
        Self {
            value: value.filter(|_| last.is_some()),
            last,
        }
    }

    /// What handler `index` (whose payload filter is `accepts`) gets, in
    /// handler order: the original when it is the last accepting one, a
    /// clone when it accepts earlier, `None` when it rejects.
    fn take(&mut self, index: usize, accepts: impl FnOnce(&T) -> bool) -> Option<T> {
        let value = self.value.as_ref()?;
        if !accepts(value) {
            None
        } else if self.last == Some(index) {
            self.value.take()
        } else {
            Some(value.clone())
        }
    }

    /// The original when no handler took it yet: a later handler (the
    /// job's concurrent part) is the last accepting one.
    fn into_rest(self) -> Option<T> {
        self.value
    }
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

/// Runs one handler on `event` ([`run_entry`] with the event).
#[cfg(test)]
pub(crate) async fn run_handler(
    entry: &Entry,
    shard: u32,
    guild_id: Option<Id<GuildMarker>>,
    event: Event,
    app: &Arc<AppShared>,
    cancel: &CancellationToken,
) {
    run_entry(entry, shard, guild_id, Some(event), app, cancel).await;
}

/// Runs one handler in span `noctis.event`: its before-Middleware, then the
/// handler, then its after-Middleware, catching panics and racing the
/// cancel token (cancel first); records the handler's duration when it
/// ran, and reports an error, a panic or a cancellation to the `Event`
/// sink. Every run ends in exactly one outcome; a filtered event runs no
/// Middleware. A handler whose Module is disabled in the event's guild
/// (globally for a guildless event) does not run at all (AD-23 stage 3).
///
/// `event` is `None` when the handler's payload filter rejected the event
/// (D3, Story 16.3): the run passes every stage up to the call, then ends
/// as a filtered one. Without event Middleware no Middleware state is
/// built and the run shares the App's locale cell of its guild record
/// (D5, D6); a disabled span is neither entered nor attached (D7).
#[expect(
    clippy::too_many_lines,
    reason = "one linear pass over the stages of an event handler run"
)]
async fn run_entry(
    entry: &Entry,
    shard: u32,
    guild_id: Option<Id<GuildMarker>>,
    event: Option<Event>,
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
    // Stage 3: a disabled Module's handler is skipped silently (cached
    // state only): no Middleware, no report.
    if !app.module_state.is_enabled(entry.module, guild_id.into()) {
        in_span(&span, || {
            tracing::trace!(target: "noctis::dispatch", module = entry.module, "module disabled here");
        });
        return;
    }
    let invocation = Invocation::Event { event_type };
    // Stage 4: the guild's recorded locale, then the App's default. Only a
    // before-Middleware writes a run's cell, so a run without event
    // Middleware (and an App without a resolver) shares one.
    let discord_guild = app.locales.guild(guild_id);
    let shared_cell = if entry.event_middleware {
        None
    } else {
        app.locales.event_cell(discord_guild)
    };
    let locales = shared_cell.unwrap_or_else(|| {
        crate::pipeline::stage4(
            app,
            Stage4 {
                invocation,
                guild_id,
                user_id: None,
                discord_user: None,
                discord_guild,
            },
            handler.name(),
        )
    });
    let around = entry.event_middleware.then(|| {
        Around::new(
            &entry.chain,
            || RunInfo {
                invocation,
                module: entry.module,
                handler: handler.name(),
                shard,
                guild_id,
                channel_id: None,
                user_id: None,
                app: Some(Arc::clone(app)),
                locales: Arc::clone(&locales),
            },
            true,
        )
    });
    let mut middleware = None;
    // No Middleware runs for a start panic: neither chain saw the run.
    let mut start_panicked = false;
    let outcome = if cancel.is_cancelled() {
        Outcome::Cancelled
    } else {
        // A rejected payload ends like a filtered call.
        let Some(event) = event else {
            return;
        };
        let ctx = EventCtx::new(
            shard,
            guild_id,
            entry.module,
            Arc::clone(app),
            around.as_ref().and_then(Around::run),
            locales,
        );
        let started_at = Instant::now();
        let started = in_span(&span, || {
            std::panic::catch_unwind(AssertUnwindSafe(|| handler.call(ctx, event)))
        });
        match started {
            Ok(Some(fut)) => {
                let before = match &around {
                    Some(around) => around.before(&span, Some(cancel)).await,
                    None => Before::Continue,
                };
                match before {
                    Before::Continue => {
                        let started_at = if around.as_ref().is_some_and(Around::has_before) {
                            Instant::now()
                        } else {
                            started_at
                        };
                        let run = AssertUnwindSafe(fut).catch_unwind();
                        let outcome = if span.is_disabled() {
                            race(cancel, run).await
                        } else {
                            race(cancel, run.instrument(span.clone())).await
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
                }
            }
            Ok(None) => return,
            Err(panic) => {
                record_duration(app, started_at);
                start_panicked = true;
                Outcome::Panicked(panic)
            }
        }
    };
    if let Some(around) = &around {
        around.freeze();
    }
    let base = ErrorReport {
        origin: ErrorOrigin::Event,
        kind: ReportKind::Cancelled,
        app: &app.app,
        shard: Some(shard),
        guild_id,
        module: Some(entry.module),
        handler: Some(handler.name()),
        event_type: Some(event_type),
        extensions: Some(match &around {
            Some(around) => around.extensions(),
            None => crate::Extensions::empty(),
        }),
        middleware: None,
    };
    let run_outcome = match &outcome {
        _ if start_panicked => None,
        Outcome::Returned(result) => Some(RunOutcome::of(result)),
        Outcome::Panicked(_) => Some(RunOutcome::Panicked),
        Outcome::Stopped => Some(RunOutcome::Stopped),
        Outcome::Cancelled => None,
    };
    if let (Some(around), Some(run_outcome)) = (&around, run_outcome) {
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
    in_span(&span, || {
        app.sinks.report(ErrorReport {
            kind: report_kind,
            middleware,
            ..base
        });
    });
}

/// Polls a started handler `run` against `cancel` (cancel first).
async fn race<F>(cancel: &CancellationToken, run: F) -> Outcome
where
    F: Future<Output = std::thread::Result<HandlerResult<()>>>,
{
    tokio::select! {
        biased;
        () = cancel.cancelled() => Outcome::Cancelled,
        result = run => match result {
            Ok(result) => Outcome::Returned(result),
            Err(panic) => Outcome::Panicked(panic),
        },
    }
}

/// `f` inside `span`, or plainly when no subscriber is interested in the
/// span (entering a disabled span does nothing).
fn in_span<R>(span: &tracing::Span, f: impl FnOnce() -> R) -> R {
    if span.is_disabled() {
        f()
    } else {
        span.in_scope(f)
    }
}

/// The event-handler metric handles of one App
/// (`noctis_handler_duration_seconds` and `noctis_handler_errors_total`,
/// labels `{app, kind = "event"}`), each registered with the global (or
/// thread-local) recorder on its first use and reused afterwards (D1,
/// Story 16.3): a recorder installed after the App's first event run
/// misses them, so it is installed before `App::run`.
#[derive(Default)]
pub(crate) struct EventMetrics {
    duration: OnceLock<Histogram>,
    errors: OnceLock<Counter>,
}

impl EventMetrics {
    /// The duration histogram of App `app`.
    fn duration(&self, app: &Arc<str>) -> &Histogram {
        self.duration.get_or_init(|| {
            metrics::histogram!(
                "noctis_handler_duration_seconds",
                "app" => Arc::clone(app),
                "kind" => ErrorOrigin::Event.as_str()
            )
        })
    }

    /// The error counter of App `app`.
    fn errors(&self, app: &Arc<str>) -> &Counter {
        self.errors.get_or_init(|| {
            metrics::counter!(
                "noctis_handler_errors_total",
                "app" => Arc::clone(app),
                "kind" => ErrorOrigin::Event.as_str()
            )
        })
    }
}

/// Records an event handler's duration since `started_at`.
fn record_duration(app: &AppShared, started_at: Instant) {
    app.event_metrics
        .duration(&app.app)
        .record(started_at.elapsed().as_secs_f64());
}

/// Counts one event handler failure.
fn count_error(app: &AppShared) {
    app.event_metrics.errors(&app.app).increment(1);
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
