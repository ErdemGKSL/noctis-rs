//! Typed Event Handlers: `HandlerError`, `EventCtx` and the erased
//! `EventHandler` descriptor `#[event]` expands to (FR-6, AD-7, AD-13).

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use noctis_gateway::EventTypeFlags;
use noctis_rules::{MessageKey, Translate};
use twilight_model::gateway::Intents;
use twilight_model::gateway::event::{Event, EventType};
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::State;
use crate::events::EventPayload;
use crate::pipeline::Dispatcher;
use crate::registry::Registry;
use crate::sink::ErrorSinks;
use crate::state::StateMap;

/// What every handler returns: `Ok(T)` or a type-erased [`HandlerError`].
///
/// ```
/// use noctis_core::HandlerResult;
///
/// fn parse(raw: &str) -> HandlerResult<u32> {
///     Ok(raw.parse::<u32>()?)
/// }
///
/// assert_eq!(parse("7").unwrap(), 7);
/// assert!(parse("x").is_err());
/// ```
pub type HandlerResult<T> = Result<T, HandlerError>;

/// A type-erased handler error: any `Error + Send + Sync + 'static`
/// converts into it with `?`.
///
/// It may carry a [`MessageKey`] the user sees instead of the generic
/// `noctis.error.generic` notice ([`with_message`](Self::with_message)), or be
/// a known user condition ([`user`](Self::user)) that only produces the notice.
///
/// `Display` and `Debug` print the inner error. It does not implement
/// [`std::error::Error`] itself (which keeps the blanket `From` coherent);
/// [`as_error`](Self::as_error) returns the inner error, for example to
/// downcast it.
///
/// ```
/// use std::io;
///
/// use noctis_core::HandlerError;
///
/// let err = HandlerError::new(io::Error::other("disk full"));
/// assert_eq!(err.to_string(), "disk full");
/// assert!(err.as_error().downcast_ref::<io::Error>().is_some());
/// assert_eq!(err.user_message(), None);
/// ```
pub struct HandlerError {
    error: Box<dyn Error + Send + Sync + 'static>,
    key: Option<MessageKey>,
    user: bool,
}

/// The inner error of [`HandlerError::user`].
#[derive(Debug)]
struct UserCondition(MessageKey);

impl fmt::Display for UserCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "user notice `{}`", self.0)
    }
}

impl Error for UserCondition {}

impl HandlerError {
    /// Wraps `error`.
    ///
    /// ```
    /// let err = noctis_core::HandlerError::new(std::fmt::Error);
    /// assert_eq!(err.to_string(), std::fmt::Error.to_string());
    /// assert_eq!(err.user_message(), None);
    /// ```
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self {
            error: Box::new(error),
            key: None,
            user: false,
        }
    }

    /// A known, user-facing condition (such as "guild only"), not a bug: the
    /// user sees `key`'s localized ephemeral notice, the pipeline logs it at
    /// `debug`, and it never reaches an Error Sink.
    ///
    /// ```
    /// use noctis_core::HandlerError;
    /// use noctis_rules::MessageKey;
    ///
    /// const GUILD_ONLY: MessageKey = MessageKey::new("common.guild_only");
    /// let err = HandlerError::user(GUILD_ONLY);
    /// assert_eq!(err.user_message(), Some(GUILD_ONLY));
    /// assert_eq!(err.to_string(), "user notice `common.guild_only`");
    /// ```
    pub fn user(key: MessageKey) -> Self {
        Self {
            error: Box::new(UserCondition(key)),
            key: Some(key),
            user: true,
        }
    }

    /// Shows `key`'s localized text to the user instead of the generic
    /// notice. On a [`new`](Self::new) error the error itself still reaches
    /// the Error Sink; on a [`user`](Self::user) error only the key is
    /// replaced and it stays a user condition (no sink report).
    ///
    /// ```
    /// use std::io;
    ///
    /// use noctis_core::HandlerError;
    /// use noctis_rules::MessageKey;
    ///
    /// const SAVE_FAILED: MessageKey = MessageKey::new("shop.save_failed");
    /// let err = HandlerError::new(io::Error::other("db down")).with_message(SAVE_FAILED);
    /// assert_eq!(err.user_message(), Some(SAVE_FAILED));
    /// assert_eq!(err.to_string(), "db down");
    /// ```
    #[must_use]
    pub fn with_message(mut self, key: MessageKey) -> Self {
        self.key = Some(key);
        self
    }

    /// The key of the notice the user sees, if one was given.
    ///
    /// ```
    /// use noctis_core::HandlerError;
    /// use noctis_rules::MessageKey;
    ///
    /// let a = MessageKey::new("shop.a");
    /// let b = MessageKey::new("shop.b");
    /// assert_eq!(HandlerError::user(a).with_message(b).user_message(), Some(b));
    /// assert_eq!(HandlerError::new(std::fmt::Error).user_message(), None);
    /// ```
    pub fn user_message(&self) -> Option<MessageKey> {
        self.key
    }

    /// The inner error.
    ///
    /// ```
    /// let err = noctis_core::HandlerError::new(std::fmt::Error);
    /// assert!(err.as_error().is::<std::fmt::Error>());
    /// ```
    pub fn as_error(&self) -> &(dyn Error + Send + Sync + 'static) {
        &*self.error
    }

    /// A user condition that keeps `error` as its inner error (a failed
    /// option validator keeps its `InvalidOption`).
    pub(crate) fn user_condition(
        error: impl Error + Send + Sync + 'static,
        key: MessageKey,
    ) -> Self {
        Self {
            error: Box::new(error),
            key: Some(key),
            user: true,
        }
    }

    /// Whether this is a user condition (no Error Sink, `debug` log only).
    pub(crate) fn is_user(&self) -> bool {
        self.user
    }
}

impl<E: Error + Send + Sync + 'static> From<E> for HandlerError {
    fn from(error: E) -> Self {
        Self::new(error)
    }
}

impl fmt::Display for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl fmt::Debug for HandlerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.error, f)
    }
}

/// What every handler invocation shares with its App: its name, the one
/// HTTP client, the typed state, the Error Sinks, the Limits and the
/// Registry.
pub(crate) struct AppShared {
    /// The `app` span field, metric label and report field (`"default"`
    /// until `AppId`, Story 7.4).
    pub(crate) app: Arc<str>,
    pub(crate) sinks: ErrorSinks,
    pub(crate) http: Arc<twilight_http::Client>,
    pub(crate) states: StateMap,
    /// The Discord HTTP edge the client sends through; dropping the last
    /// `AppShared` stops it.
    pub(crate) edge: crate::edge::Edge,
    /// The Limits every Responder validates against (`AppBuilder::limits`).
    pub(crate) limits: noctis_rules::Limits,
    /// The folded Registry (the Dispatcher resolves Commands in it).
    pub(crate) registry: Arc<Registry>,
    /// When the auto-defer acknowledges a pending interaction, after
    /// `received_at` (`AppBuilder::ack_deadline`, INT-01).
    pub(crate) ack_deadline: std::time::Duration,
    /// The strings every user-facing notice is rendered from (`noctis-i18n`
    /// `Builtin` until Epic 11 merges bot catalogs).
    pub(crate) strings: Arc<dyn Translate>,
}

/// The context an event handler receives: the shard, the guild, the
/// handler's Module, the App's typed state, its HTTP client and its
/// Command [`Dispatcher`].
///
/// Cheap to clone (one `Arc`). `Debug` leaves the HTTP client out.
///
/// ```
/// use noctis_core::events::{EventCtx, MessageCreate};
/// use noctis_core::HandlerResult;
///
/// async fn log_message(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
///     tracing::info!(shard = ctx.shard(), module = ctx.module(), guild = ?ctx.guild_id(), "message {}", msg.id);
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct EventCtx(Arc<CtxInner>);

struct CtxInner {
    shard: u32,
    guild_id: Option<Id<GuildMarker>>,
    module: &'static str,
    app: Arc<AppShared>,
    dispatcher: Dispatcher,
}

impl EventCtx {
    pub(crate) fn new(
        shard: u32,
        guild_id: Option<Id<GuildMarker>>,
        module: &'static str,
        app: Arc<AppShared>,
    ) -> Self {
        let dispatcher = Dispatcher::new(Arc::clone(&app));
        Self(Arc::new(CtxInner {
            shard,
            guild_id,
            module,
            app,
            dispatcher,
        }))
    }

    /// The number of the shard that received the event.
    pub fn shard(&self) -> u32 {
        self.0.shard
    }

    /// The event's guild, if it has one (twilight's `Event::guild_id`).
    pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
        self.0.guild_id
    }

    /// The id of the Module the handler was added to.
    pub fn module(&self) -> &'static str {
        self.0.module
    }

    /// The state registered for `T` with `AppBuilder::state`, if any.
    pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
        self.0.app.states.get::<T>()
    }

    /// The App's one Discord HTTP client (the `Arc` `App::http` returns).
    pub fn http(&self) -> &Arc<twilight_http::Client> {
        &self.0.app.http
    }

    /// The App's Command [`Dispatcher`]: runs a registered Command from
    /// outside an interaction, such as a prefix command.
    pub fn dispatcher(&self) -> &Dispatcher {
        &self.0.dispatcher
    }
}

impl fmt::Debug for EventCtx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventCtx")
            .field("shard", &self.0.shard)
            .field("guild_id", &self.0.guild_id)
            .field("module", &self.0.module)
            .field("states", &self.0.app.states)
            .finish_non_exhaustive()
    }
}

/// The erased handler: `None` when the event is not its payload type.
type ErasedHandler =
    dyn Fn(EventCtx, Event) -> Option<BoxFuture<'static, HandlerResult<()>>> + Send + Sync;

/// One typed event handler, as `#[event]` builds it; add it to a
/// [`Module`](crate::Module).
///
/// Opaque and cheap to clone. `Debug` prints the handler path, the event
/// name and the `ordered`/`droppable` flags.
///
/// Handlers come from `#[event]`: annotating `async fn on_delete` gives a
/// `fn on_delete()` that returns its `EventHandler`.
///
/// ```
/// # use noctis_core::events::{EventCtx, EventHandler, MessageDelete};
/// # use noctis_core::{HandlerResult, Module};
/// # async fn on_delete_fn(_: EventCtx, _: MessageDelete) -> HandlerResult<()> {
/// #     Ok(())
/// # }
/// # fn on_delete() -> EventHandler {
/// #     EventHandler::__new("bot::on_delete", false, false, on_delete_fn)
/// # }
/// let handler = on_delete();
/// assert!(format!("{handler:?}").contains("MESSAGE_DELETE"));
/// let module = Module::new("audit").__add(handler);
/// ```
#[derive(Clone)]
pub struct EventHandler {
    name: &'static str,
    kind: EventType,
    dispatch: EventType,
    parse: EventTypeFlags,
    intent: Intents,
    ordered: bool,
    droppable: bool,
    call: Arc<ErasedHandler>,
}

impl EventHandler {
    /// Builds a handler of `P` named `handler`; what `#[event]` expands to.
    #[doc(hidden)]
    pub fn __new<P, F, Fut>(handler: &'static str, ordered: bool, droppable: bool, f: F) -> Self
    where
        P: EventPayload,
        F: Fn(EventCtx, P) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<()>> + Send + 'static,
    {
        let call = move |ctx: EventCtx, event: Event| {
            P::extract(event).map(|payload| {
                let fut: BoxFuture<'static, HandlerResult<()>> = Box::pin(f(ctx, payload));
                fut
            })
        };
        Self {
            name: handler,
            kind: P::KIND,
            dispatch: P::DISPATCH,
            parse: P::PARSE,
            intent: P::INTENT,
            ordered,
            droppable,
            call: Arc::new(call),
        }
    }

    /// The handler's path (`module_path!()::name`).
    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    /// The event type its payload is named after (logs, GW-05).
    pub(crate) fn kind(&self) -> EventType {
        self.kind
    }

    /// The event type it is dispatched on: the type whose flag is
    /// [`parse`](Self::parse). Equals [`kind`](Self::kind) except for
    /// `UnavailableGuild` (`GUILD_DELETE`).
    pub(crate) fn dispatch(&self) -> EventType {
        self.dispatch
    }

    /// The event types the gateway must parse for it.
    pub(crate) fn parse(&self) -> EventTypeFlags {
        self.parse
    }

    /// The intent its event needs (empty for none).
    pub(crate) fn intent(&self) -> Intents {
        self.intent
    }

    /// Whether it runs before the concurrent handlers, in per-guild order.
    pub(crate) fn ordered(&self) -> bool {
        self.ordered
    }

    /// Whether its event may be dropped under load.
    pub(crate) fn droppable(&self) -> bool {
        self.droppable
    }

    /// Starts the handler on `event`; `None` when `event` is not its payload.
    pub(crate) fn call(
        &self,
        ctx: EventCtx,
        event: Event,
    ) -> Option<BoxFuture<'static, HandlerResult<()>>> {
        (self.call)(ctx, event)
    }
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "the parse flags and intent follow from the event; the closure has no Debug"
)]
impl fmt::Debug for EventHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventHandler")
            .field("handler", &self.name)
            .field("event", &self.kind.name().unwrap_or("UNKNOWN"))
            .field("ordered", &self.ordered)
            .field("droppable", &self.droppable)
            .finish()
    }
}

#[cfg(test)]
mod tests;
