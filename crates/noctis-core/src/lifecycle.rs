//! Lifecycle Hooks (FR-54, AD-21): the App's own lifecycle events, as typed
//! payloads a `#[derive(Hook)]` struct's `#[handler]` method takes.
//!
//! | Payload | Fires | Data |
//! |---|---|---|
//! | [`AppStarted`] | once per run, after the locale overlays and the startup Module-state load, before the startup Publish | `publisher` |
//! | [`PublishCompleted`] | whenever an `App::publish(..)` resolves `Ok` (startup, CLI modes, author code; dry runs and clears included) | `scope`, `clear`, `report` (its targets' `diff()`) |
//! | [`PublishFailed`] | whenever an `App::publish(..)` resolves `Err` | `scope`, `dry_run`, `clear`, `error` |
//! | [`ShardReady`] | a shard received READY | `shard` |
//! | [`ShardResumed`] | a shard received RESUMED | `shard` |
//! | [`ShardDisconnected`] | a shard's connection closed; it reconnects | `shard`, `close_code` (`None` without a close frame) |
//! | [`ShardFatal`] | a shard stopped for good (a fatal close) | `shard`, `close_code` |
//! | [`AllShardsReady`] | once per run, when every shard of the range was ready | `shards`, `total` |
//! | [`ModuleToggled`] | `Modules::set_enabled`/`reset` through this App changed a record | `module`, `scope`, `enabled` (its own state in `scope` after the change), `reset` |
//! | [`ShutdownStarted`] | once per run in which [`AppStarted`] fired, as the shutdown begins (also when startup fails after it) | `reason` |
//!
//! While `App::run` runs, every occurrence goes into one ordered queue:
//! one task runs, per event in arrival order, every hook registered for it
//! in Registry fold order (pre-order, `add` order, plugins included), one
//! at a time. Outside a run (`App::publish` or `App::modules()` before
//! `run`, the CLI `--publish*` modes), the hooks run inline and the call
//! returns after them. Hooks run no Middleware, no locale stage and no rate
//! limit, and run whether or not their Module is enabled at runtime; Boot
//! Flags filter them like every registration. An `Err` or a panic goes to
//! the [`ErrorOrigin::Hook`] Error Sink and the App keeps running. At the
//! end of a run the queue is drained until `shutdown_grace` after
//! [`ShutdownStarted`] (at least 1 s); then the running hook is cancelled
//! (reported as `Cancelled`) and the queued events are discarded.

use std::fmt;
use std::future::Future;
use std::ops::Range;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_util::FutureExt as _;
use futures_util::future::BoxFuture;
use noctis_gateway::ShardEvent;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::Instrument as _;

use crate::handler::{AppShared, HandlerResult};
use crate::module_state::ModuleScope;
use crate::publish::{PublishError, PublishReport, PublishScope};
use crate::sink::{ErrorOrigin, ErrorReport, ReportKind};
use crate::state::State;
use crate::worker::{AbortOnDrop, panic_message};

/// How many lifecycle events a slow receiver of the crate-internal tap may
/// fall behind before it sees `Lagged`.
pub(crate) const LIFECYCLE_CAPACITY: usize = 256;

/// The least time the end of a run waits for the queued hooks.
const DRAIN_AT_LEAST: Duration = Duration::from_secs(1);

/// How long a cut runner may take to report and stop.
const CUT_WAIT: Duration = Duration::from_secs(1);

pub(crate) mod sealed {
    /// The lifecycle event kinds, as indices of the hook index.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HookKind {
        AppStarted,
        ShardReady,
        ShardResumed,
        ShardDisconnected,
        ShardFatal,
        AllShardsReady,
        PublishCompleted,
        PublishFailed,
        ModuleToggled,
        ShutdownStarted,
    }

    impl HookKind {
        /// How many kinds there are.
        pub const COUNT: usize = 10;

        /// Its index in `0..COUNT`.
        pub const fn index(self) -> usize {
            self as usize
        }
    }

    /// One fired lifecycle event.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Fired {
        AppStarted(super::AppStarted),
        ShardReady(super::ShardReady),
        ShardResumed(super::ShardResumed),
        ShardDisconnected(super::ShardDisconnected),
        ShardFatal(super::ShardFatal),
        AllShardsReady(super::AllShardsReady),
        PublishCompleted(super::PublishCompleted),
        PublishFailed(super::PublishFailed),
        ModuleToggled(super::ModuleToggled),
        ShutdownStarted(super::ShutdownStarted),
    }

    /// What a lifecycle payload tells the hook index.
    pub trait Sealed: Sized {
        /// Its kind.
        const KIND: HookKind;
        /// Wraps it.
        fn into_fired(self) -> Fired;
        /// The payload of `fired`, if it is this type's.
        fn from_fired(fired: &Fired) -> Option<&Self>;
    }
}

use sealed::{Fired, HookKind};

impl Fired {
    /// Its kind.
    pub(crate) fn kind(&self) -> HookKind {
        match self {
            Self::AppStarted(_) => HookKind::AppStarted,
            Self::ShardReady(_) => HookKind::ShardReady,
            Self::ShardResumed(_) => HookKind::ShardResumed,
            Self::ShardDisconnected(_) => HookKind::ShardDisconnected,
            Self::ShardFatal(_) => HookKind::ShardFatal,
            Self::AllShardsReady(_) => HookKind::AllShardsReady,
            Self::PublishCompleted(_) => HookKind::PublishCompleted,
            Self::PublishFailed(_) => HookKind::PublishFailed,
            Self::ModuleToggled(_) => HookKind::ModuleToggled,
            Self::ShutdownStarted(_) => HookKind::ShutdownStarted,
        }
    }

    /// Its `NAME` (the `hook_event` span and report field).
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::AppStarted(_) => AppStarted::NAME,
            Self::ShardReady(_) => ShardReady::NAME,
            Self::ShardResumed(_) => ShardResumed::NAME,
            Self::ShardDisconnected(_) => ShardDisconnected::NAME,
            Self::ShardFatal(_) => ShardFatal::NAME,
            Self::AllShardsReady(_) => AllShardsReady::NAME,
            Self::PublishCompleted(_) => PublishCompleted::NAME,
            Self::PublishFailed(_) => PublishFailed::NAME,
            Self::ModuleToggled(_) => ModuleToggled::NAME,
            Self::ShutdownStarted(_) => ShutdownStarted::NAME,
        }
    }

    /// The shard of a shard event.
    pub(crate) fn shard(&self) -> Option<u32> {
        match self {
            Self::ShardReady(e) => Some(e.shard),
            Self::ShardResumed(e) => Some(e.shard),
            Self::ShardDisconnected(e) => Some(e.shard),
            Self::ShardFatal(e) => Some(e.shard),
            _ => None,
        }
    }

    /// The hook event of a shard event; `Closed` (a requested stop) has
    /// none.
    fn from_shard(event: &ShardEvent) -> Option<Self> {
        Some(match *event {
            ShardEvent::Ready { shard } => Self::ShardReady(ShardReady { shard }),
            ShardEvent::Resumed { shard } => Self::ShardResumed(ShardResumed { shard }),
            ShardEvent::Disconnected { shard, close_code } => {
                Self::ShardDisconnected(ShardDisconnected { shard, close_code })
            }
            ShardEvent::Ended { shard, close_code } => {
                Self::ShardFatal(ShardFatal { shard, close_code })
            }
            _ => return None,
        })
    }
}

/// A lifecycle event payload a `#[derive(Hook)]` struct's `#[handler]`
/// method takes as its event: one of the types in
/// [`noctis_core::lifecycle`](self). Sealed.
///
/// ```
/// use noctis_core::lifecycle::{AppStarted, HookEvent};
///
/// assert_eq!(AppStarted::NAME, "APP_STARTED");
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a lifecycle event",
    label = "not a lifecycle event",
    note = "use a type from `noctis::lifecycle`; gateway events are handled by `#[derive(EventHandler)]`"
)]
pub trait HookEvent: sealed::Sealed + Clone + Send + Sync + 'static {
    /// The event's name (`"APP_STARTED"`): the `hook_event` span field and
    /// the `event_type` of an `ErrorOrigin::Hook` report.
    const NAME: &'static str;
}

/// Declares each payload's sealed impl and name.
macro_rules! hook_events {
    ($($ty:ident => $name:literal;)*) => {$(
        impl sealed::Sealed for $ty {
            const KIND: HookKind = HookKind::$ty;

            fn into_fired(self) -> Fired {
                Fired::$ty(self)
            }

            fn from_fired(fired: &Fired) -> Option<&Self> {
                match fired {
                    Fired::$ty(e) => Some(e),
                    _ => None,
                }
            }
        }

        impl HookEvent for $ty {
            const NAME: &'static str = $name;
        }
    )*};
}

/// The App started running: the locale overlays are installed and the
/// Module state is loaded; the startup Publish (if any) and the shards
/// follow.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStarted {
    /// Whether this App publishes globally at startup
    /// (`AppBuilder::publisher`).
    pub publisher: bool,
}

/// A shard received READY.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardReady {
    /// The shard id.
    pub shard: u32,
}

/// A shard received RESUMED.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardResumed {
    /// The shard id.
    pub shard: u32,
}

/// A shard's connection closed; it reconnects (and resumes when it can).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardDisconnected {
    /// The shard id.
    pub shard: u32,
    /// The close code; `None` when the connection dropped without a close
    /// frame.
    pub close_code: Option<u16>,
}

/// A shard stopped for good: a fatal close (such as 4014, disallowed
/// intents) or its retries ran out.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardFatal {
    /// The shard id.
    pub shard: u32,
    /// The close code, if the shard received one.
    pub close_code: Option<u16>,
}

/// Every shard of this process's Shard Range was ready at least once;
/// fires once per run.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllShardsReady {
    /// The Shard Range this process runs.
    pub shards: Range<u32>,
    /// The App's shard total.
    pub total: u32,
}

/// An `App::publish(..)` resolved `Ok`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishCompleted {
    /// The Publish scope.
    pub scope: PublishScope,
    /// Whether it was a clear (`--publish-clear`).
    pub clear: bool,
    /// The report: `report.is_dry_run()`, and each target's `diff()`.
    pub report: PublishReport,
}

/// An `App::publish(..)` resolved `Err`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishFailed {
    /// The Publish scope.
    pub scope: PublishScope,
    /// Whether it was a dry run.
    pub dry_run: bool,
    /// Whether it was a clear.
    pub clear: bool,
    /// Why it failed.
    pub error: PublishError,
}

/// `Modules::set_enabled` or `Modules::reset` through this App changed a
/// Module's record (an unchanged write fires nothing; changes other Apps
/// wrote are never reported here).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleToggled {
    /// The Module id.
    pub module: &'static str,
    /// The scope written.
    pub scope: ModuleScope,
    /// The Module's own state in `scope` after the change (ancestors not
    /// counted).
    pub enabled: bool,
    /// Whether it was a `reset`.
    pub reset: bool,
}

/// The shutdown sequence started.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownStarted {
    /// Why the App shuts down.
    pub reason: ShutdownReason,
}

hook_events! {
    AppStarted => "APP_STARTED";
    ShardReady => "SHARD_READY";
    ShardResumed => "SHARD_RESUMED";
    ShardDisconnected => "SHARD_DISCONNECTED";
    ShardFatal => "SHARD_FATAL";
    AllShardsReady => "ALL_SHARDS_READY";
    PublishCompleted => "PUBLISH_COMPLETED";
    PublishFailed => "PUBLISH_FAILED";
    ModuleToggled => "MODULE_TOGGLED";
    ShutdownStarted => "SHUTDOWN_STARTED";
}

/// Why a run shuts down.
///
/// ```
/// use noctis_core::lifecycle::ShutdownReason;
///
/// assert_eq!(ShutdownReason::Signal.as_str(), "signal");
/// assert_eq!(ShutdownReason::AllShardsEnded.to_string(), "all shards ended");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShutdownReason {
    /// SIGINT or SIGTERM (`App::run`).
    Signal,
    /// The `run_until` future resolved.
    Requested,
    /// Every shard ended on its own.
    AllShardsEnded,
    /// Startup failed after the App started.
    StartupFailed,
}

impl ShutdownReason {
    /// `"signal"`, `"requested"`, `"all shards ended"` or
    /// `"startup failed"` (the `reason` field of the `shutdown started`
    /// log line).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Signal => "signal",
            Self::Requested => "requested",
            Self::AllShardsEnded => "all shards ended",
            Self::StartupFailed => "startup failed",
        }
    }
}

impl fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The context a lifecycle hook receives: its Module, the App's id, typed
/// state and handles. Cheap to clone (one `Arc`). `Debug` prints the Module
/// and the states, never the HTTP client.
///
/// ```
/// use noctis_core::lifecycle::{AllShardsReady, HookCtx};
/// use noctis_core::HandlerResult;
///
/// async fn online(ctx: HookCtx, ev: AllShardsReady) -> HandlerResult<()> {
///     tracing::info!(module = ctx.module(), app = ctx.app_id(), total = ev.total, "online");
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct HookCtx(Arc<HookInner>);

struct HookInner {
    module: &'static str,
    app: Arc<AppShared>,
}

impl HookCtx {
    fn new(module: &'static str, app: Arc<AppShared>) -> Self {
        Self(Arc::new(HookInner { module, app }))
    }

    /// The id of the Module the hook was added to.
    pub fn module(&self) -> &'static str {
        self.0.module
    }

    /// The App's id (the `app` span field and metric label).
    pub fn app_id(&self) -> &str {
        &self.0.app.app
    }

    /// The state registered for `T` with `AppBuilder::state`, if any.
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        self.0.app.states.get::<T>()
    }

    /// The App's one Discord HTTP client.
    pub fn http(&self) -> &Arc<twilight_http::Client> {
        &self.0.app.http
    }

    /// The App's gateway command handle (detached outside a run).
    pub fn gateway(&self) -> noctis_gateway::GatewayHandle {
        self.0.app.gateway.clone()
    }

    /// The App's typed key-value handle over its namespaced `CacheStore`.
    pub fn kv(&self) -> crate::Kv {
        crate::Kv::of(Some(&self.0.app))
    }

    /// The App's Discord resource cache.
    pub fn cache(&self) -> crate::DiscordCacheReader {
        crate::DiscordCacheReader::of(Some(&self.0.app))
    }

    /// The App's runtime Module switches; a toggle from a hook fires
    /// [`ModuleToggled`] after the running hooks (in a run) or nested
    /// (outside one).
    pub fn modules(&self) -> crate::Modules {
        crate::Modules::of(Some(&self.0.app))
    }

    /// The App's webhook factory.
    pub fn webhooks(&self) -> crate::Webhooks {
        crate::Webhooks::of(Some(&self.0.app))
    }
}

impl fmt::Debug for HookCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookCtx")
            .field("module", &self.0.module)
            .field("states", &self.0.app.states)
            .finish_non_exhaustive()
    }
}

/// The erased hook: `None` when the event is not its payload type.
type ErasedHook =
    dyn Fn(HookCtx, &Fired) -> Option<BoxFuture<'static, HandlerResult<()>>> + Send + Sync;

/// One lifecycle hook, as `#[derive(Hook)]` and its `#[handler]` method
/// build it; register the struct on a [`Module`](crate::Module) with
/// `add::<T>()`.
///
/// Opaque and cheap to clone. `Debug` prints the handler path and the
/// event name.
#[derive(Clone)]
pub struct LifecycleHook {
    name: &'static str,
    kind: HookKind,
    event: &'static str,
    call: Arc<ErasedHook>,
}

impl LifecycleHook {
    /// Builds a hook of `E` named `handler`; what the `#[handler]` method of
    /// a `#[derive(Hook)]` struct expands to.
    #[doc(hidden)]
    pub fn __new<E, F, Fut>(handler: &'static str, f: F) -> Self
    where
        E: HookEvent,
        F: Fn(HookCtx, E) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<()>> + Send + 'static,
    {
        let call = move |ctx: HookCtx, fired: &Fired| {
            E::from_fired(fired).map(|event| {
                let fut: BoxFuture<'static, HandlerResult<()>> = Box::pin(f(ctx, event.clone()));
                fut
            })
        };
        Self {
            name: handler,
            kind: E::KIND,
            event: E::NAME,
            call: Arc::new(call),
        }
    }

    /// The handler's path (`module_path!()::name`).
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    /// Its event kind.
    pub(crate) fn kind(&self) -> HookKind {
        self.kind
    }
}

impl fmt::Debug for LifecycleHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LifecycleHook")
            .field("handler", &self.name)
            .field("event", &self.event)
            .finish_non_exhaustive()
    }
}

/// One folded hook: its Module and the hook.
#[derive(Debug, Clone)]
pub(crate) struct HookEntry {
    pub(crate) module: &'static str,
    pub(crate) hook: LifecycleHook,
}

/// Every folded hook by event kind, in fold order.
#[derive(Debug, Default)]
pub(crate) struct Hooks {
    by_kind: [Vec<HookEntry>; HookKind::COUNT],
}

impl Hooks {
    /// Appends `entry` to its kind's list.
    pub(crate) fn push(&mut self, entry: HookEntry) {
        self.by_kind[entry.hook.kind().index()].push(entry);
    }

    /// The hooks of `kind`, in fold order.
    pub(crate) fn of(&self, kind: HookKind) -> &[HookEntry] {
        &self.by_kind[kind.index()]
    }

    /// Whether no hook is registered at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.by_kind.iter().all(Vec::is_empty)
    }

    /// How many hooks are registered.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.by_kind.iter().map(Vec::len).sum()
    }
}

/// The crate-internal lifecycle tap (a `broadcast` the tests read) of the
/// shard events and the shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleEvent {
    /// A shard received READY.
    ShardReady { shard: u32 },
    /// A shard received RESUMED.
    ShardResumed { shard: u32 },
    /// A shard's connection closed; twilight reconnects.
    ShardDisconnected { shard: u32, close_code: Option<u16> },
    /// A shard stopped on its own for good (a fatal close).
    ShardFatal { shard: u32, close_code: Option<u16> },
    /// The shutdown sequence started.
    ShutdownStarted,
}

impl LifecycleEvent {
    /// The lifecycle event of a shard event; `Closed` (a requested stop)
    /// has none.
    pub(crate) fn from_shard(event: &ShardEvent) -> Option<Self> {
        Some(match *event {
            ShardEvent::Ready { shard } => Self::ShardReady { shard },
            ShardEvent::Resumed { shard } => Self::ShardResumed { shard },
            ShardEvent::Disconnected { shard, close_code } => {
                Self::ShardDisconnected { shard, close_code }
            }
            ShardEvent::Ended { shard, close_code } => Self::ShardFatal { shard, close_code },
            _ => return None,
        })
    }
}

/// The App's lifecycle: the internal tap, the hook index and, while a run
/// runs, the sender of its hook queue. Sending never blocks or fails.
#[derive(Debug)]
pub(crate) struct Lifecycle {
    sender: broadcast::Sender<LifecycleEvent>,
    hooks: Arc<Hooks>,
    /// The hook queue of the running run; `None` outside a run (hooks then
    /// run inline).
    queue: Mutex<Option<mpsc::UnboundedSender<Fired>>>,
}

impl Lifecycle {
    /// A lifecycle without hooks.
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_hooks(Arc::default())
    }

    /// A lifecycle firing `hooks`.
    pub(crate) fn with_hooks(hooks: Arc<Hooks>) -> Self {
        Self {
            sender: broadcast::channel(LIFECYCLE_CAPACITY).0,
            hooks,
            queue: Mutex::new(None),
        }
    }

    /// The hook index.
    pub(crate) fn hooks(&self) -> &Hooks {
        &self.hooks
    }

    /// Broadcasts `event` on the tap; without receivers it is dropped.
    pub(crate) fn send(&self, event: LifecycleEvent) {
        let _ = self.sender.send(event);
    }

    /// Broadcasts the lifecycle event of `event`, if it has one, and
    /// enqueues its hook event.
    pub(crate) fn shard(&self, event: &ShardEvent) {
        if let Some(tap) = LifecycleEvent::from_shard(event) {
            self.send(tap);
        }
        if let Some(fired) = Fired::from_shard(event)
            && self.wants(fired.kind())
        {
            let _ = self.enqueue(fired);
        }
    }

    /// Whether a hook is registered for `kind`.
    fn wants(&self, kind: HookKind) -> bool {
        !self.hooks.of(kind).is_empty()
    }

    /// Enqueues `event` for the running run's hooks; the event back when no
    /// run runs.
    fn enqueue(&self, event: Fired) -> Result<(), Fired> {
        let queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        match queue.as_ref() {
            Some(sender) => sender.send(event).map_err(|e| e.0),
            None => Err(event),
        }
    }

    /// Enqueues the event `make` builds when a hook wants it; outside a run
    /// it is dropped. What the run's own firing points call.
    pub(crate) fn emit<E: HookEvent>(&self, make: impl FnOnce() -> E) {
        if self.wants(E::KIND) {
            let _ = self.enqueue(make().into_fired());
        }
    }

    /// A new receiver of the tap.
    #[cfg(test)]
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<LifecycleEvent> {
        self.sender.subscribe()
    }

    fn attach(&self, sender: mpsc::UnboundedSender<Fired>) {
        *self.queue.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    }

    fn detach(&self) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

/// Fires the event `make` builds for `app`'s hooks: enqueued while a run
/// runs, else run inline before this returns. Builds nothing when no hook
/// wants the event.
pub(crate) async fn fire<E: HookEvent>(app: &Arc<AppShared>, make: impl FnOnce() -> E) {
    if !app.lifecycle.wants(E::KIND) {
        return;
    }
    if let Err(event) = app.lifecycle.enqueue(make().into_fired()) {
        run_inline(Arc::clone(app), event).await;
    }
}

/// Runs every hook of `event` in order; boxed, so a hook that fires again
/// (a nested toggle) recurses safely.
fn run_inline(app: Arc<AppShared>, event: Fired) -> BoxFuture<'static, ()> {
    Box::pin(async move {
        for entry in app.lifecycle.hooks().of(event.kind()) {
            run_hook(&app, entry, &event, None).await;
        }
    })
}

/// How a hook run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ran {
    /// It returned or panicked (reported).
    Done,
    /// The end-of-run cut cancelled it (reported).
    Cut,
}

/// Runs one hook in span `noctis.hook` under `catch_unwind`, recording its
/// duration and reporting an error, a panic or a cut to the `Hook` sink.
async fn run_hook(
    app: &Arc<AppShared>,
    entry: &HookEntry,
    event: &Fired,
    cut: Option<&CancellationToken>,
) -> Ran {
    let handler = entry.hook.name();
    let hook_event = event.name();
    let shard = event.shard();
    let span = tracing::info_span!(
        "noctis.hook",
        app = &*app.app,
        module = entry.module,
        handler,
        hook_event,
        shard,
    );
    let ctx = HookCtx::new(entry.module, Arc::clone(app));
    let started_at = Instant::now();
    let started = span
        .in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| (entry.hook.call)(ctx, event))));
    let outcome = match started {
        Ok(Some(fut)) => {
            let run = AssertUnwindSafe(fut)
                .catch_unwind()
                .instrument(span.clone());
            let cut = async {
                match cut {
                    Some(cut) => cut.cancelled().await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                biased;
                () = cut => None,
                result = run => Some(result),
            }
        }
        Ok(None) => return Ran::Done,
        Err(panic) => Some(Err(panic)),
    };
    let base = ErrorReport {
        origin: ErrorOrigin::Hook,
        kind: ReportKind::Cancelled,
        app: &app.app,
        shard,
        guild_id: None,
        module: Some(entry.module),
        handler: Some(handler),
        event_type: Some(hook_event),
        extensions: None,
        middleware: None,
    };
    let ran = if outcome.is_some() {
        Ran::Done
    } else {
        Ran::Cut
    };
    if ran == Ran::Done {
        record_duration(app, started_at);
    }
    let kind = match &outcome {
        Some(Ok(Ok(()))) => return ran,
        Some(Ok(Err(error))) => ReportKind::Error(error),
        Some(Err(panic)) => ReportKind::Panic(panic_message(&**panic)),
        None => ReportKind::Cancelled,
    };
    count_error(app);
    span.in_scope(|| app.sinks.report(ErrorReport { kind, ..base }));
    ran
}

/// Records a hook's duration since `started_at`.
fn record_duration(app: &AppShared, started_at: Instant) {
    metrics::histogram!(
        "noctis_handler_duration_seconds",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Hook.as_str()
    )
    .record(started_at.elapsed().as_secs_f64());
}

/// Counts one hook failure.
fn count_error(app: &AppShared) {
    metrics::counter!(
        "noctis_handler_errors_total",
        "app" => Arc::clone(&app.app),
        "kind" => ErrorOrigin::Hook.as_str()
    )
    .increment(1);
}

/// The queue's reader: runs every event's hooks in arrival order, one at a
/// time, until the queue closes or `cut` fires.
async fn runner(
    app: Arc<AppShared>,
    mut queue: mpsc::UnboundedReceiver<Fired>,
    cut: CancellationToken,
) {
    while let Some(event) = queue.recv().await {
        let mut started = false;
        for entry in app.lifecycle.hooks().of(event.kind()) {
            if cut.is_cancelled() {
                break;
            }
            started = true;
            if run_hook(&app, entry, &event, Some(&cut)).await == Ran::Cut {
                break;
            }
        }
        if cut.is_cancelled() {
            queue.close();
            let mut events = usize::from(!started);
            while queue.try_recv().is_ok() {
                events += 1;
            }
            if events > 0 {
                tracing::warn!(
                    target: "noctis::hook",
                    app = &*app.app,
                    events,
                    "lifecycle hooks discarded at shutdown"
                );
            }
            return;
        }
    }
}

/// One run's hook queue and the bookkeeping of its `AppStarted` /
/// `ShutdownStarted` pairing. Dropping it aborts the runner and detaches
/// the queue (hooks then run inline again).
pub(crate) struct HookRun {
    app: Arc<AppShared>,
    task: Option<AbortOnDrop>,
    cut: CancellationToken,
    started: std::sync::atomic::AtomicBool,
    /// When `ShutdownStarted` fired.
    shutdown_at: Mutex<Option<Instant>>,
}

impl HookRun {
    /// Attaches a queue and starts its runner; without hooks, neither.
    pub(crate) fn start(app: &Arc<AppShared>) -> Self {
        let cut = CancellationToken::new();
        let task = (!app.lifecycle.hooks().is_empty()).then(|| {
            let (sender, receiver) = mpsc::unbounded_channel();
            app.lifecycle.attach(sender);
            AbortOnDrop(tokio::spawn(runner(Arc::clone(app), receiver, cut.clone())))
        });
        Self {
            app: Arc::clone(app),
            task,
            cut,
            started: std::sync::atomic::AtomicBool::new(false),
            shutdown_at: Mutex::new(None),
        }
    }

    /// Fires `AppStarted` (once).
    pub(crate) fn app_started(&self, publisher: bool) {
        use std::sync::atomic::Ordering;
        if !self.started.swap(true, Ordering::AcqRel) {
            self.app.lifecycle.emit(|| AppStarted { publisher });
        }
    }

    /// Whether `AppStarted` fired.
    pub(crate) fn has_started(&self) -> bool {
        self.started.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Fires `ShutdownStarted` once, and only after `AppStarted`.
    pub(crate) fn shutdown_started(&self, reason: ShutdownReason) {
        if !self.has_started() {
            return;
        }
        let mut at = self
            .shutdown_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if at.is_none() {
            *at = Some(Instant::now());
            self.app.lifecycle.emit(|| ShutdownStarted { reason });
        }
    }

    /// Closes the queue and waits for the runner until `grace` after
    /// `ShutdownStarted` (at least 1 s from now); then cuts the running
    /// hook and discards the rest.
    pub(crate) async fn finish(mut self, grace: Duration) {
        self.app.lifecycle.detach();
        let Some(mut task) = self.task.take() else {
            return;
        };
        let now = Instant::now();
        let fired = *self
            .shutdown_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let deadline = fired.map_or(now, |at| at + grace).max(now + DRAIN_AT_LEAST);
        if tokio::time::timeout_at(deadline, &mut task.0).await.is_ok() {
            return;
        }
        self.cut.cancel();
        let _ = tokio::time::timeout(CUT_WAIT, &mut task.0).await;
    }
}

impl Drop for HookRun {
    fn drop(&mut self) {
        if self.task.is_some() {
            self.app.lifecycle.detach();
        }
    }
}

impl fmt::Debug for HookRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HookRun")
            .field("running", &self.task.is_some())
            .field("started", &self.has_started())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
