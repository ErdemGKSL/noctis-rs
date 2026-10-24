//! Middleware and typed Extensions (FR-20; AD-23 stages 7 and 10, AD-9).
//!
//! A Module registers function Middleware as values:
//! [`Module::before`](crate::Module::before) takes a [`BeforeMiddleware`]
//! (built with [`Middleware::before`]) and
//! [`Module::after`](crate::Module::after) an [`AfterMiddleware`] (built
//! with [`Middleware::after`]). `App::build` folds one chain per Module:
//! its parent's Middleware first, then its own, in registration order.
//!
//! A handler run (a slash Command, a context menu, a `Dispatcher` Command, a
//! component, a modal submit or an event handler) runs its chain around the
//! handler:
//!
//! 1. the before-Middleware, in chain order, each returning [`Flow`]: they
//!    may [`insert`](BeforeCtx::insert) typed Extensions and stop the run
//!    with [`Flow::Stop`];
//! 2. the Extensions freeze, and the handler runs (its `Ext<T>` parameters
//!    read them);
//! 3. the after-Middleware, in reverse chain order, for every outcome
//!    except a shutdown cancellation, with the [`RunOutcome`] and the frozen
//!    [`Extensions`].
//!
//! No Middleware runs for autocomplete, the Expired Handler chain, a
//! rejected interaction or the detached test seams. Extensions live for one
//! run: they are never serialized, logged by value or written to a port.

use std::any::{Any, TypeId};
use std::fmt;
use std::future::Future;
use std::ops::Deref;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use futures_util::FutureExt;
use futures_util::future::BoxFuture;
use noctis_rules::{Locale, MessageKey};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::State;
use crate::handler::{AppShared, HandlerError, HandlerResult};
use crate::locale::{LocaleCell, Locales};

/// The host of the Middleware constructors; it has no values.
///
/// ```
/// use noctis_core::{BeforeCtx, AfterCtx, Flow, HandlerResult, Middleware, Module};
///
/// #[derive(Debug)]
/// struct Settings { premium: bool }
///
/// async fn load(ctx: BeforeCtx) -> HandlerResult<Flow> {
///     ctx.insert(Settings { premium: false });
///     Ok(Flow::Continue)
/// }
///
/// async fn audit(ctx: AfterCtx) {
///     tracing::info!(completed = ctx.outcome().is_completed(), "run ended");
/// }
///
/// let module = Module::new("shop")
///     .before(Middleware::before(load).provides::<Settings>())
///     .after(Middleware::after(audit));
/// assert!(format!("{module:?}").contains("load"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Middleware {}

/// Which handler kinds a Middleware runs for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// Every handler (the default).
    All,
    /// Only `#[derive(EventHandler)]` handlers.
    EventsOnly,
    /// Every handler except events.
    SkipEvents,
}

impl Scope {
    /// Whether the scope covers a handler that is (`event`) or is not an
    /// event handler.
    pub(crate) const fn covers(self, event: bool) -> bool {
        match self {
            Self::All => true,
            Self::EventsOnly => event,
            Self::SkipEvents => !event,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::EventsOnly => "events_only",
            Self::SkipEvents => "skip_events",
        }
    }
}

/// What a before-Middleware decides.
///
/// ```
/// use noctis_core::Flow;
/// use noctis_rules::MessageKey;
///
/// let deny = Flow::Stop(Some(MessageKey::new("common.guild_only")));
/// assert_ne!(deny, Flow::Continue);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Run the next Middleware, then the handler.
    Continue,
    /// Skip the remaining before-Middleware and the handler. An interaction
    /// shows `Some(key)`'s localized ephemeral notice (a public reply for a
    /// `Dispatcher` Command); `None` sends nothing (and deletes a type-5
    /// deferred original). Events send nothing either way.
    Stop(Option<MessageKey>),
}

/// What a handler run answers: the invoked Command, context menu,
/// component, modal or event.
///
/// ```
/// use noctis_core::Invocation;
///
/// let invocation = Invocation::Command { path: "config set" };
/// assert_eq!(invocation, Invocation::Command { path: "config set" });
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    /// A slash Command, or a Command run by the `Dispatcher`.
    Command {
        /// The Command Path (`"config set prefix"`).
        path: &'static str,
    },
    /// A user or message context menu.
    ContextMenu {
        /// The menu name.
        name: &'static str,
    },
    /// A component click.
    Component {
        /// The Route id (`"shop.buy"`).
        route: &'static str,
    },
    /// A modal submit.
    Modal {
        /// The Route id.
        route: &'static str,
    },
    /// A gateway event.
    Event {
        /// The Discord event name (`"MESSAGE_CREATE"`).
        event_type: &'static str,
    },
    /// An autocomplete interaction; seen only by a `LocaleResolver`
    /// (autocomplete runs no Middleware).
    Autocomplete {
        /// The Command Path whose option is autocompleted.
        path: &'static str,
    },
}

/// How a handler run ended, as the after-Middleware see it.
///
/// ```
/// use noctis_core::RunOutcome;
///
/// assert!(RunOutcome::Completed.is_completed());
/// assert!(!RunOutcome::Stopped.is_completed());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The handler returned `Ok` and its response was sent.
    Completed,
    /// A before-Middleware returned [`Flow::Stop`].
    Stopped,
    /// A `HandlerError::user` or a failed option validator, from the handler
    /// or a before-Middleware.
    UserError,
    /// Any other error: from the handler or a before-Middleware, a failed
    /// send, or an [`ExtensionMissing`].
    Failed,
    /// The handler or a before-Middleware panicked.
    Panicked,
    /// A Route Guard (`owner`, `permissions` or `guard = path`) refused the
    /// interaction: its notice was sent and the handler did not run.
    Denied,
}

impl RunOutcome {
    /// `true` only for [`Completed`](Self::Completed).
    pub const fn is_completed(self) -> bool {
        matches!(self, Self::Completed)
    }

    /// The outcome of a run that returned `result`.
    pub(crate) fn of(result: &HandlerResult<()>) -> Self {
        match result {
            Ok(()) => Self::Completed,
            Err(error) if error.is_user() => Self::UserError,
            Err(_) => Self::Failed,
        }
    }
}

/// A typed Extension a before-Middleware inserted: a cheap, shared handle
/// to the value.
///
/// A `#[handler]` method takes it as a parameter after `ctx`
/// (`settings: Ext<GuildSettings>`), or as `Option<Ext<T>>` to accept a run
/// where nothing inserted `T`.
///
/// ```
/// use noctis_core::{BeforeCtx, Ext, Flow, HandlerResult};
///
/// #[derive(Debug, PartialEq)]
/// struct Limit(u32);
///
/// fn read(limit: &Ext<Limit>) -> u32 {
///     limit.0 // `Deref<Target = Limit>`
/// }
/// # let _ = read;
/// ```
pub struct Ext<T>(Arc<T>);

impl<T> Ext<T> {
    /// The shared value.
    pub fn into_arc(self) -> Arc<T> {
        self.0
    }
}

impl<T> Deref for Ext<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> AsRef<T> for Ext<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> Clone for Ext<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: fmt::Debug> fmt::Debug for Ext<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Ext").field(&*self.0).finish()
    }
}

/// One inserted Extension.
struct Slot {
    type_id: TypeId,
    type_name: &'static str,
    /// The Middleware that inserted it.
    by: &'static str,
    value: Arc<dyn Any + Send + Sync>,
}

/// The typed Extensions of one handler run: at most one value per type,
/// inserted by its before-Middleware and frozen before the handler runs.
///
/// Not constructible outside noctis. `Debug` prints the type names only,
/// never a value.
///
/// ```
/// use noctis_core::{AfterCtx, Extensions};
///
/// async fn audit(ctx: AfterCtx) {
///     let extensions: &Extensions = ctx.extensions();
///     let names: Vec<&str> = extensions.type_names().collect();
///     tracing::debug!(?names, "extensions of the run");
/// }
/// # let _ = audit;
/// ```
pub struct Extensions {
    slots: Vec<Slot>,
}

/// The Extensions of a run without Middleware.
static EMPTY: Extensions = Extensions::new();

impl Extensions {
    /// An empty map; it allocates nothing.
    const fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// The static empty map.
    pub(crate) fn empty() -> &'static Self {
        &EMPTY
    }

    /// The value of type `T`, if one was inserted.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.slot::<T>()?.value.downcast_ref::<T>()
    }

    /// Whether a value of type `T` was inserted.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.slot::<T>().is_some()
    }

    /// How many values it holds.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether it holds no value.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The type names of its values, in first-insert order.
    pub fn type_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.slots.iter().map(|slot| slot.type_name)
    }

    /// A shared handle to the value of type `T`, if one was inserted.
    pub(crate) fn ext<T: Send + Sync + 'static>(&self) -> Option<Ext<T>> {
        let value = Arc::clone(&self.slot::<T>()?.value);
        value.downcast::<T>().ok().map(Ext)
    }

    fn slot<T: 'static>(&self) -> Option<&Slot> {
        let id = TypeId::of::<T>();
        self.slots.iter().find(|slot| slot.type_id == id)
    }

    /// Inserts `value`, inserted by the Middleware `by`; replacing a value
    /// of the same type is one `debug` naming both Middleware.
    fn insert<T: Send + Sync + 'static>(&mut self, value: T, by: &'static str) {
        let type_name = std::any::type_name::<T>();
        let value: Arc<dyn Any + Send + Sync> = Arc::new(value);
        let id = TypeId::of::<T>();
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.type_id == id) {
            tracing::debug!(
                target: "noctis::middleware",
                extension = type_name,
                previous = slot.by,
                by,
                "extension replaced"
            );
            slot.by = by;
            slot.value = value;
        } else {
            self.slots.push(Slot {
                type_id: id,
                type_name,
                by,
                value,
            });
        }
    }
}

#[cfg(test)]
impl Extensions {
    /// A map holding `value`, inserted by `test`.
    pub(crate) fn of<T: Send + Sync + 'static>(value: T) -> Self {
        let mut extensions = Self::new();
        extensions.insert(value, "test");
        extensions
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.type_names()).finish()
    }
}

/// A handler took `Ext<T>`, but no before-Middleware inserted `T` in this
/// run (a declared provider that did not insert it, or a detached test
/// seam). The handler does not run; this is its error.
///
/// ```
/// # use noctis_core::ExtensionMissing;
/// fn describe(error: &ExtensionMissing) -> String {
///     format!("{} needs {}", error.handler(), error.extension())
/// }
/// # let _ = describe;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("handler `{handler}` takes `Ext<{extension}>`, but no Middleware inserted it in this run")]
pub struct ExtensionMissing {
    extension: &'static str,
    handler: &'static str,
}

impl ExtensionMissing {
    /// The missing Extension's type name.
    pub fn extension(&self) -> &'static str {
        self.extension
    }

    /// The handler's path.
    pub fn handler(&self) -> &'static str {
        self.handler
    }

    /// The error of `handler` missing `T`.
    pub(crate) fn of<T: ?Sized>(handler: &'static str) -> Self {
        Self {
            extension: std::any::type_name::<T>(),
            handler,
        }
    }
}

/// The erased before call.
type BeforeFn = dyn Fn(BeforeCtx) -> BoxFuture<'static, HandlerResult<Flow>> + Send + Sync;
/// The erased after call.
type AfterFn = dyn Fn(AfterCtx) -> BoxFuture<'static, ()> + Send + Sync;

/// A provided Extension type.
#[derive(Clone, Copy)]
struct Provided {
    type_id: TypeId,
    type_name: &'static str,
}

/// A before-Middleware: what [`Middleware::before`] builds and
/// [`Module::before`](crate::Module::before) registers.
///
/// Cheap to clone. `Debug` shows the name (the function's type name), the
/// scope and the provided types.
#[derive(Clone)]
#[must_use = "register the Middleware with `Module::before`"]
pub struct BeforeMiddleware {
    name: &'static str,
    scope: Scope,
    provides: Vec<Provided>,
    call: Arc<BeforeFn>,
}

/// An after-Middleware: what [`Middleware::after`] builds and
/// [`Module::after`](crate::Module::after) registers.
///
/// Cheap to clone. `Debug` shows the name and the scope.
#[derive(Clone)]
#[must_use = "register the Middleware with `Module::after`"]
pub struct AfterMiddleware {
    name: &'static str,
    scope: Scope,
    call: Arc<AfterFn>,
}

impl Middleware {
    /// A before-Middleware running `f`: it runs before the handler of every
    /// run on its Module's path and returns [`Flow`].
    ///
    /// ```
    /// use noctis_core::{BeforeCtx, Flow, HandlerResult, Middleware};
    ///
    /// async fn guild_only(ctx: BeforeCtx) -> HandlerResult<Flow> {
    ///     Ok(match ctx.guild_id() {
    ///         Some(_) => Flow::Continue,
    ///         None => Flow::Stop(None),
    ///     })
    /// }
    ///
    /// let mw = Middleware::before(guild_only).skip_events();
    /// assert!(mw.name().ends_with("guild_only"));
    /// ```
    pub fn before<F, Fut>(f: F) -> BeforeMiddleware
    where
        F: Fn(BeforeCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Flow>> + Send + 'static,
    {
        let call =
            move |ctx: BeforeCtx| -> BoxFuture<'static, HandlerResult<Flow>> { Box::pin(f(ctx)) };
        BeforeMiddleware {
            name: std::any::type_name::<F>(),
            scope: Scope::All,
            provides: Vec::new(),
            call: Arc::new(call),
        }
    }

    /// An after-Middleware running `f`: it runs after every run on its
    /// Module's path (stopped and failed ones included, not a shutdown
    /// cancellation).
    ///
    /// ```
    /// use noctis_core::{AfterCtx, Middleware};
    ///
    /// async fn audit(ctx: AfterCtx) {
    ///     tracing::info!(outcome = ?ctx.outcome(), handler = ctx.handler(), "ran");
    /// }
    ///
    /// let mw = Middleware::after(audit);
    /// assert!(mw.name().ends_with("audit"));
    /// ```
    pub fn after<F, Fut>(f: F) -> AfterMiddleware
    where
        F: Fn(AfterCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let call = move |ctx: AfterCtx| -> BoxFuture<'static, ()> { Box::pin(f(ctx)) };
        AfterMiddleware {
            name: std::any::type_name::<F>(),
            scope: Scope::All,
            call: Arc::new(call),
        }
    }
}

impl BeforeMiddleware {
    /// Declares that this Middleware inserts `T`: `App::build` accepts an
    /// `Ext<T>` handler parameter only when a Middleware on the handler's
    /// Module path declares it (a repeated `T` is kept once).
    pub fn provides<T: Send + Sync + 'static>(mut self) -> Self {
        let type_id = TypeId::of::<T>();
        if !self.provides.iter().any(|p| p.type_id == type_id) {
            self.provides.push(Provided {
                type_id,
                type_name: std::any::type_name::<T>(),
            });
        }
        self
    }

    /// Runs only for `#[derive(EventHandler)]` handlers (the last scope call
    /// wins).
    pub fn events_only(mut self) -> Self {
        self.scope = Scope::EventsOnly;
        self
    }

    /// Runs for every handler except events (the last scope call wins).
    pub fn skip_events(mut self) -> Self {
        self.scope = Scope::SkipEvents;
        self
    }

    /// The Middleware's name: the type name of its function.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Whether it declares `type_id` for a handler that is (`event`) or is
    /// not an event handler.
    pub(crate) fn provides_for(&self, type_id: TypeId, event: bool) -> bool {
        self.scope.covers(event) && self.provides.iter().any(|p| p.type_id == type_id)
    }
}

impl fmt::Debug for BeforeMiddleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let provides: Vec<&str> = self.provides.iter().map(|p| p.type_name).collect();
        f.debug_struct("BeforeMiddleware")
            .field("name", &self.name)
            .field("scope", &self.scope.as_str())
            .field("provides", &provides)
            .finish_non_exhaustive()
    }
}

impl AfterMiddleware {
    /// Runs only for `#[derive(EventHandler)]` handlers (the last scope call
    /// wins).
    pub fn events_only(mut self) -> Self {
        self.scope = Scope::EventsOnly;
        self
    }

    /// Runs for every handler except events (the last scope call wins).
    pub fn skip_events(mut self) -> Self {
        self.scope = Scope::SkipEvents;
        self
    }

    /// The Middleware's name: the type name of its function.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl fmt::Debug for AfterMiddleware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AfterMiddleware")
            .field("name", &self.name)
            .field("scope", &self.scope.as_str())
            .field("provides", &(&[] as &[&str]))
            .finish_non_exhaustive()
    }
}

/// The Middleware chain of one folded Module: its ancestors' Middleware,
/// root first, then its own, each in registration order. Shared by every
/// entry of the Module.
#[derive(Clone, Default)]
pub(crate) struct Chain {
    pub(crate) before: Arc<[BeforeMiddleware]>,
    pub(crate) after: Arc<[AfterMiddleware]>,
}

impl Chain {
    /// The chain of a Module whose parent's chain is `self`, with its own
    /// `before` and `after` lists.
    pub(crate) fn extend(&self, before: &[BeforeMiddleware], after: &[AfterMiddleware]) -> Self {
        Self {
            before: join(&self.before, before),
            after: join(&self.after, after),
        }
    }

    /// Whether any Middleware of the chain runs for a handler that is
    /// (`event`) or is not an event handler.
    pub(crate) fn covers(&self, event: bool) -> bool {
        self.before.iter().any(|m| m.scope.covers(event))
            || self.after.iter().any(|m| m.scope.covers(event))
    }
}

/// `parent` followed by `own`; `parent` itself (shared) when `own` is
/// empty.
fn join<T: Clone>(parent: &Arc<[T]>, own: &[T]) -> Arc<[T]> {
    if own.is_empty() {
        Arc::clone(parent)
    } else {
        parent.iter().chain(own).cloned().collect()
    }
}

/// What a run tells its Middleware: the invocation, the handler, where it
/// came from and the App.
pub(crate) struct RunInfo {
    pub(crate) invocation: Invocation,
    pub(crate) module: &'static str,
    pub(crate) handler: &'static str,
    pub(crate) shard: u32,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    pub(crate) user_id: Option<Id<UserMarker>>,
    pub(crate) app: Option<Arc<AppShared>>,
    /// The run's locale cell, shared with the handler's context, the
    /// Responder and the notices.
    pub(crate) locales: Arc<LocaleCell>,
}

/// The mutable part of a run: the Extensions being inserted and the
/// Middleware inserting them; `None` once frozen.
struct Building {
    extensions: Extensions,
    current: &'static str,
}

/// One handler run with Middleware: what [`BeforeCtx`] and [`AfterCtx`]
/// share and what the handler's context reads its Extensions from.
pub(crate) struct Run {
    info: RunInfo,
    building: Mutex<Option<Building>>,
    frozen: OnceLock<Extensions>,
}

impl Run {
    pub(crate) fn new(info: RunInfo) -> Arc<Self> {
        Arc::new(Self {
            info,
            building: Mutex::new(Some(Building {
                extensions: Extensions::new(),
                current: "",
            })),
            frozen: OnceLock::new(),
        })
    }

    /// The frozen Extensions, or the static empty map before the freeze.
    pub(crate) fn extensions(&self) -> &Extensions {
        self.frozen.get().unwrap_or(&EMPTY)
    }

    fn building(&self) -> std::sync::MutexGuard<'_, Option<Building>> {
        self.building.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_current(&self, name: &'static str) {
        if let Some(building) = self.building().as_mut() {
            building.current = name;
        }
    }

    fn insert<T: Send + Sync + 'static>(&self, value: T) {
        let mut guard = self.building();
        if let Some(building) = guard.as_mut() {
            let by = building.current;
            building.extensions.insert(value, by);
        } else {
            drop(guard);
            tracing::debug!(
                target: "noctis::middleware",
                extension = std::any::type_name::<T>(),
                handler = self.info.handler,
                "extension inserted after the before-Middleware ended; ignored"
            );
        }
    }

    /// Applies `f` to the run's locales while the before chain runs; a
    /// later call is ignored with one `debug`.
    fn set_locales(&self, f: impl Fn(Locales) -> Locales) {
        let guard = self.building();
        if guard.is_some() {
            // Under the lock: the freeze cannot slip in between.
            self.info.locales.update(f);
        } else {
            drop(guard);
            tracing::debug!(
                target: "noctis::middleware",
                handler = self.info.handler,
                "locale set after the before-Middleware ended; ignored"
            );
        }
    }

    fn get<T: Send + Sync + 'static>(&self) -> Option<Ext<T>> {
        match self.building().as_ref() {
            Some(building) => building.extensions.ext::<T>(),
            None => self.extensions().ext::<T>(),
        }
    }

    /// Ends the before phase: later inserts are dropped, and the handler,
    /// the after-Middleware and the report read the frozen map.
    pub(crate) fn freeze(&self) {
        let building = self.building().take();
        if let Some(building) = building {
            let _ = self.frozen.set(building.extensions);
        }
    }
}

/// The read accessors [`BeforeCtx`] and [`AfterCtx`] share.
macro_rules! run_accessors {
    ($ty:ident) => {
        impl $ty {
            /// What the run answers.
            pub fn invocation(&self) -> Invocation {
                self.run.info.invocation
            }

            /// The id of the Module the handler was added to.
            pub fn module(&self) -> &'static str {
                self.run.info.module
            }

            /// The handler's path (`module_path!()::Struct`).
            pub fn handler(&self) -> &'static str {
                self.run.info.handler
            }

            /// The number of the shard that received the interaction or
            /// event.
            pub fn shard(&self) -> u32 {
                self.run.info.shard
            }

            /// The guild, if there is one.
            pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
                self.run.info.guild_id
            }

            /// The channel, if Discord sent it (`None` for events).
            pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
                self.run.info.channel_id
            }

            /// The invoking user (`None` for events).
            pub fn user_id(&self) -> Option<Id<UserMarker>> {
                self.run.info.user_id
            }

            /// The run's resolved User Locale (FR-43): stage 4's
            /// resolution (and the App's `LocaleResolver`), with every
            /// before-Middleware override so far. An event's User Locale is
            /// its Guild Locale.
            pub fn user_locale(&self) -> Locale {
                self.run.info.locales.get().user
            }

            /// The run's resolved Guild Locale (FR-43), with every
            /// before-Middleware override so far.
            pub fn guild_locale(&self) -> Locale {
                self.run.info.locales.get().guild
            }

            /// The state registered for `T` with `AppBuilder::state`, if any.
            pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
                self.run.info.app.as_ref()?.states.get::<T>()
            }

            /// The App's typed key-value handle over its namespaced
            /// `CacheStore` (a cheap clone).
            pub fn kv(&self) -> crate::Kv {
                crate::Kv::of(self.run.info.app.as_ref())
            }

            /// The App's Discord resource cache (a cheap clone): owned values,
            /// `Ok(None)` when not cached, [`CacheError::Disabled`](crate::CacheError::Disabled)
            /// when the resource is not configured or without an App.
            pub fn cache(&self) -> crate::DiscordCacheReader {
                crate::DiscordCacheReader::of(self.run.info.app.as_ref())
            }

            /// The App's runtime Module switches: `is_enabled` reads the cached
            /// snapshot, writes go through the `ModuleStateStore` (a cheap clone).
            pub fn modules(&self) -> crate::Modules {
                crate::Modules::of(self.run.info.app.as_ref())
            }

            /// The App's one Discord HTTP client.
            pub fn http(&self) -> &Arc<twilight_http::Client> {
                match &self.run.info.app {
                    Some(app) => &app.http,
                    None => crate::command::detached_http(),
                }
            }
        }
    };
}

/// The context a before-Middleware receives: the run's invocation, handler,
/// shard, guild, channel and user, its resolved locales (to read and
/// override with [`set_locale`](Self::set_locale) and friends), the App's
/// state, key-value handle and HTTP client, and the run's Extensions to
/// insert into.
///
/// An owned, cheap clone (one `Arc`). An insert after the run's
/// before-Middleware ended (from a clone kept in a spawned task) is
/// ignored with a `debug`.
///
/// ```
/// use noctis_core::{BeforeCtx, Flow, HandlerResult};
///
/// struct Caller(u64);
///
/// async fn caller(ctx: BeforeCtx) -> HandlerResult<Flow> {
///     if let Some(user) = ctx.user_id() {
///         ctx.insert(Caller(user.get()));
///     }
///     Ok(Flow::Continue)
/// }
/// # let _ = caller;
/// ```
#[derive(Clone)]
pub struct BeforeCtx {
    run: Arc<Run>,
}

run_accessors!(BeforeCtx);

impl BeforeCtx {
    /// Inserts the Extension `value`; a value of the same type is replaced
    /// (the last insert wins, with one `debug` naming both Middleware).
    pub fn insert<T: Send + Sync + 'static>(&self, value: T) {
        self.run.insert(value);
    }

    /// The Extension of type `T` inserted so far, if any.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Ext<T>> {
        self.run.get::<T>()
    }

    /// Whether an Extension of type `T` was inserted so far.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.get::<T>().is_some()
    }

    /// Overrides the run's User Locale: the handler's `user_locale()`, its
    /// ephemeral responses and its notices use `locale`. The last write
    /// wins and a later Middleware reads it; a call after the before chain
    /// ended is ignored with one `debug`.
    pub fn set_user_locale(&self, locale: Locale) {
        self.run.set_locales(|l| Locales { user: locale, ..l });
    }

    /// Overrides the run's Guild Locale: the handler's `guild_locale()`
    /// and its public responses use `locale` (see
    /// [`set_user_locale`](Self::set_user_locale) for the write rules).
    ///
    /// ```
    /// use noctis_core::{BeforeCtx, Flow, HandlerResult};
    /// use noctis_rules::Locale;
    ///
    /// // A per-guild language setting read from a store would go here.
    /// async fn turkish_guilds(ctx: BeforeCtx) -> HandlerResult<Flow> {
    ///     if ctx.guild_id().is_some_and(|id| id.get() == 42) {
    ///         ctx.set_guild_locale(Locale::Tr);
    ///     }
    ///     Ok(Flow::Continue)
    /// }
    /// # let _ = turkish_guilds;
    /// ```
    pub fn set_guild_locale(&self, locale: Locale) {
        self.run.set_locales(|l| Locales { guild: locale, ..l });
    }

    /// Overrides both of the run's locales with `locale` (see
    /// [`set_user_locale`](Self::set_user_locale) for the write rules).
    pub fn set_locale(&self, locale: Locale) {
        self.run.set_locales(|_| Locales::both(locale));
    }
}

impl fmt::Debug for BeforeCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BeforeCtx")
            .field("invocation", &self.run.info.invocation)
            .field("module", &self.run.info.module)
            .field("handler", &self.run.info.handler)
            .finish_non_exhaustive()
    }
}

/// The context an after-Middleware receives: the read accessors of
/// [`BeforeCtx`], the run's [`RunOutcome`] and its frozen [`Extensions`].
///
/// An owned, cheap clone (one `Arc`).
///
/// ```
/// use noctis_core::{AfterCtx, RunOutcome};
///
/// async fn count(ctx: AfterCtx) {
///     if ctx.outcome() == RunOutcome::Failed {
///         tracing::warn!(handler = ctx.handler(), "failed");
///     }
/// }
/// # let _ = count;
/// ```
#[derive(Clone)]
pub struct AfterCtx {
    run: Arc<Run>,
    outcome: RunOutcome,
}

run_accessors!(AfterCtx);

impl AfterCtx {
    /// How the run ended.
    pub fn outcome(&self) -> RunOutcome {
        self.outcome
    }

    /// The run's frozen Extensions (whatever was inserted when a
    /// before-Middleware stopped or failed).
    pub fn extensions(&self) -> &Extensions {
        self.run.extensions()
    }

    /// The Extension of type `T`, if one was inserted.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Ext<T>> {
        self.run.extensions().ext::<T>()
    }
}

impl fmt::Debug for AfterCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AfterCtx")
            .field("invocation", &self.run.info.invocation)
            .field("module", &self.run.info.module)
            .field("handler", &self.run.info.handler)
            .field("outcome", &self.outcome)
            .field("extensions", self.run.extensions())
            .finish_non_exhaustive()
    }
}

/// How the before chain ended.
pub(crate) enum Before {
    /// Every Middleware continued: run the handler.
    Continue,
    /// A Middleware stopped with this notice.
    Stop(Option<MessageKey>),
    /// The Middleware `.1` returned an error.
    Failed(HandlerError, &'static str),
    /// The Middleware `.1` panicked.
    Panicked(Box<dyn Any + Send>, &'static str),
    /// The cancel token fired.
    Cancelled,
}

/// The Middleware of one handler run: its Module's chain and, when any of
/// it covers the handler's kind, the shared [`Run`]. A run whose chain
/// covers nothing allocates nothing.
pub(crate) struct Around {
    chain: Chain,
    event: bool,
    run: Option<Arc<Run>>,
}

impl Around {
    /// The Middleware of a run of `chain` described by `info` (built only
    /// when some Middleware covers its kind).
    pub(crate) fn new(chain: &Chain, info: impl FnOnce() -> RunInfo, event: bool) -> Self {
        let run = chain.covers(event).then(|| Run::new(info()));
        Self {
            chain: chain.clone(),
            event,
            run,
        }
    }

    /// The shared run the handler's context reads its Extensions from.
    pub(crate) fn run(&self) -> Option<Arc<Run>> {
        self.run.clone()
    }

    /// The frozen Extensions (empty without Middleware or before the
    /// freeze).
    pub(crate) fn extensions(&self) -> &Extensions {
        match &self.run {
            Some(run) => run.extensions(),
            None => &EMPTY,
        }
    }

    /// Freezes the Extensions (a run that ends without its before chain:
    /// a start panic or a cancellation).
    pub(crate) fn freeze(&self) {
        if let Some(run) = &self.run {
            run.freeze();
        }
    }

    /// Whether a before-Middleware runs.
    pub(crate) fn has_before(&self) -> bool {
        self.run.is_some() && self.chain.before.iter().any(|m| m.scope.covers(self.event))
    }

    /// Runs the before chain in order (each under `catch_unwind`, in span
    /// `noctis.middleware{phase = "before"}` below `span`, raced against
    /// `cancel`), then freezes the Extensions.
    pub(crate) async fn before(&self, span: &Span, cancel: Option<&CancellationToken>) -> Before {
        let Some(run) = &self.run else {
            return Before::Continue;
        };
        let result = self.run_before(run, span, cancel).await;
        run.freeze();
        result
    }

    async fn run_before(
        &self,
        run: &Arc<Run>,
        span: &Span,
        cancel: Option<&CancellationToken>,
    ) -> Before {
        for mw in self
            .chain
            .before
            .iter()
            .filter(|m| m.scope.covers(self.event))
        {
            let name = mw.name;
            run.set_current(name);
            let mw_span = middleware_span(span, run, name, "before");
            let ctx = BeforeCtx {
                run: Arc::clone(run),
            };
            let started =
                mw_span.in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| (mw.call)(ctx))));
            let fut = match started {
                Ok(fut) => fut,
                Err(panic) => return Before::Panicked(panic, name),
            };
            let polled = AssertUnwindSafe(fut).catch_unwind().instrument(mw_span);
            let result = match cancel {
                Some(cancel) => tokio::select! {
                    biased;
                    () = cancel.cancelled() => return Before::Cancelled,
                    result = polled => result,
                },
                None => polled.await,
            };
            match result {
                Ok(Ok(Flow::Continue)) => {}
                Ok(Ok(Flow::Stop(key))) => {
                    span.in_scope(|| {
                        tracing::debug!(
                            target: "noctis::middleware",
                            middleware = name,
                            handler = run.info.handler,
                            "stopped"
                        );
                    });
                    return Before::Stop(key);
                }
                Ok(Err(error)) => return Before::Failed(error, name),
                Err(panic) => return Before::Panicked(panic, name),
            }
        }
        Before::Continue
    }

    /// Runs the after chain in reverse order with `outcome` (each under
    /// `catch_unwind`, in span `noctis.middleware{phase = "after"}` below
    /// `span`); a panic is handed to `panicked` with the Middleware's name,
    /// and the rest still run. A fired `cancel` ends it.
    pub(crate) async fn after(
        &self,
        span: &Span,
        cancel: Option<&CancellationToken>,
        outcome: RunOutcome,
        panicked: impl Fn(&'static str, &str),
    ) {
        let Some(run) = &self.run else {
            return;
        };
        run.freeze();
        let chain = self.chain.after.iter().rev();
        for mw in chain.filter(|m| m.scope.covers(self.event)) {
            let name = mw.name;
            let mw_span = middleware_span(span, run, name, "after");
            let ctx = AfterCtx {
                run: Arc::clone(run),
                outcome,
            };
            let started =
                mw_span.in_scope(|| std::panic::catch_unwind(AssertUnwindSafe(|| (mw.call)(ctx))));
            let result = match started {
                Ok(fut) => {
                    let polled = AssertUnwindSafe(fut).catch_unwind().instrument(mw_span);
                    match cancel {
                        Some(cancel) => tokio::select! {
                            biased;
                            () = cancel.cancelled() => return,
                            result = polled => result,
                        },
                        None => polled.await,
                    }
                }
                Err(panic) => Err(panic),
            };
            if let Err(panic) = result {
                span.in_scope(|| panicked(name, crate::worker::panic_message(&*panic)));
            }
        }
    }
}

/// The span of one Middleware call: `noctis.middleware{app, middleware,
/// phase}` below `parent`.
fn middleware_span(parent: &Span, run: &Run, name: &'static str, phase: &'static str) -> Span {
    let app = run.info.app.as_ref().map_or("default", |app| &*app.app);
    tracing::info_span!(
        parent: parent,
        "noctis.middleware",
        app,
        middleware = name,
        phase
    )
}

#[cfg(test)]
mod tests;
