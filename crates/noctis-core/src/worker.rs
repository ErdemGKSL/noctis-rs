//! The worker set: the drain task, the event semaphore, the per-guild
//! ordered sub-queues and panic-safe handler runs (FR-5, FR-6, AD-10,
//! NFR-4).

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt;
use futures_util::future::join_all;
use noctis_gateway::{Inbound, InboundKind, LaneReceiver};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::task::JoinHandle;
use twilight_model::gateway::event::Event;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::events::EventCtx;
use crate::handler::AppShared;
use crate::registry::{Entry, Handlers, Registry};

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
}

impl Workers {
    /// Builds the worker set; spawns the sub-queue tasks when `registry` has
    /// an ordered handler. Must run inside a tokio runtime.
    pub(crate) fn new(
        registry: Arc<Registry>,
        app: Arc<AppShared>,
        handler_concurrency: usize,
    ) -> Self {
        let ordered = if registry.has_ordered() {
            (0..SUBQUEUES)
                .map(|_| {
                    let (tx, rx) = mpsc::unbounded_channel();
                    tokio::spawn(run_subqueue(rx, Arc::clone(&app)));
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
        }
    }

    /// How many sub-queues run.
    #[cfg(test)]
    pub(crate) fn subqueues(&self) -> usize {
        self.ordered.len()
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
            tokio::spawn(run_concurrent(job, Arc::clone(&self.app)));
        } else if let Some(tx) = self.ordered.get(subqueue(guild_id)) {
            // A closed sub-queue only happens during runtime shutdown.
            let _ = tx.send(job);
        }
    }
}

/// Takes every [`Inbound`] from the lanes until every sender has dropped.
///
/// An interaction never takes a permit. An event waits for one permit,
/// serving interactions that arrive meanwhile first.
pub(crate) async fn drain(mut receiver: LaneReceiver, workers: Workers) {
    while let Some(inbound) = receiver.recv().await {
        let shard = inbound.shard.number();
        match inbound.kind {
            InboundKind::Event(event) => {
                let permit = loop {
                    tokio::select! {
                        biased;
                        Some(interaction) = receiver.recv_interaction() => interaction_unhandled(interaction),
                        permit = Arc::clone(&workers.semaphore).acquire_owned() => break permit,
                    }
                };
                // The semaphore is never closed.
                let Ok(permit) = permit else { return };
                workers.dispatch(shard, *event, permit);
            }
            _ => interaction_unhandled(inbound),
        }
    }
}

/// No interaction handler exists before Epic 6: traced and dropped.
fn interaction_unhandled(inbound: Inbound) {
    tracing::trace!(
        target: "noctis::app",
        shard = inbound.shard.number(),
        kind = ?inbound.kind,
        "no handler registered"
    );
    drop(inbound);
}

/// Runs each job's ordered handlers in arrival order, then spawns its
/// concurrent part.
async fn run_subqueue(mut jobs: mpsc::UnboundedReceiver<Job>, app: Arc<AppShared>) {
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
            run_handler(entry, shard, guild_id, next, &app).await;
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
                tokio::spawn(run_concurrent(job, Arc::clone(&app)));
            }
            _ => drop(permit),
        }
    }
}

/// Runs every concurrent handler of `job` at once; the permit is released
/// when all have ended.
async fn run_concurrent(job: Job, app: Arc<AppShared>) {
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
            Some(run_handler(entry, shard, guild_id, next, &app))
        });
    join_all(runs.collect::<Vec<_>>()).await;
    drop(permit);
}

/// The original event for the last handler, a clone for the others.
fn take_event(event: &mut Option<Event>, last: bool) -> Option<Event> {
    if last { event.take() } else { event.clone() }
}

/// Runs one handler, catching its panic; logs an error or a panic.
async fn run_handler(
    entry: &Entry,
    shard: u32,
    guild_id: Option<Id<GuildMarker>>,
    event: Event,
    app: &Arc<AppShared>,
) {
    let handler = &entry.handler;
    let ctx = EventCtx::new(shard, guild_id, entry.module, Arc::clone(app));
    let event_type = handler.kind().name().unwrap_or("UNKNOWN");
    let started = std::panic::catch_unwind(AssertUnwindSafe(|| handler.call(ctx, event)));
    let outcome = match started {
        Ok(Some(fut)) => AssertUnwindSafe(fut).catch_unwind().await,
        Ok(None) => return,
        Err(panic) => Err(panic),
    };
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::error!(
            target: "noctis::event",
            module = entry.module,
            handler = handler.name(),
            event_type,
            error = %error,
            "event handler failed"
        ),
        Err(panic) => tracing::error!(
            target: "noctis::event",
            module = entry.module,
            handler = handler.name(),
            event_type,
            panic = panic_message(&*panic),
            "event handler panicked"
        ),
    }
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
