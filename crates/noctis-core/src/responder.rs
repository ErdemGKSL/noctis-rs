//! The typestate [`Responder`]: how a handler answers its interaction
//! (FR-19, FR-21, FR-61, AD-11, AD-24; INT-01, INT-02, INT-03, INT-04,
//! INT-05, INT-08, INT-09, INT-10, INT-11, INT-12, INT-13, CV2-01, CV2-02,
//! MSG-05).
//!
//! A [`CommandResponder`], [`ComponentResponder`] or [`ModalResponder`]
//! starts [`Pending`]. The discord.js flows consume it and give a
//! [`Responder<K, Acked, D>`](Responder), which only edits, fetches or
//! deletes the reply, sends follow-ups, and ends with
//! [`done`](Responder::done):
//!
//! | Method | Callback | Kinds |
//! | --- | --- | --- |
//! | `reply` / `reply_as` | type 4, a new message | all |
//! | `defer_reply` / `defer_reply_as` | type 5, "thinking…" | all |
//! | `update` | type 7, edits the clicked message | `Component`, `ModalSubmit` |
//! | `defer_update` | type 6, no visible change | `Component`, `ModalSubmit` |
//! | `show_modal` | type 9 | `Command`, `Component` (`Manual` only) |
//!
//! `reply_with_response`, `defer_reply_with_response`,
//! `update_with_response` and `defer_update_with_response` also return the
//! [`CallbackResource`] Discord answers with. A second initial response, an
//! `update` of a Command, a `show_modal` on an auto-deferred handler or of a
//! modal submit, and a flag set on `defer_reply_as` do not compile.
//!
//! At runtime one gate serializes every initial response, so at most one
//! interaction callback is ever sent (INT-03): when the auto-defer
//! answered first, a later `reply` becomes an edit of the deferred reply, or
//! a follow-up after a type-6 defer (the component's message is never
//! overwritten by a `reply`), and `update` becomes an edit. The context
//! methods (`ctx.respond`, `ctx.update`, `ctx.reply_text`) decide under the
//! same gate. Once the handler itself took the initial response, through
//! either API, a second initial response (any Pending method, or
//! `ctx.update`) is [`ResponderError::AlreadyResponded`] and sends nothing;
//! the auto-defer alone never counts as the handler's answer. After 15
//! minutes the interaction token is expired (INT-02): edits and deletes of
//! public messages fall back to the bot token, everything else is
//! [`ResponderError::TokenExpired`]. A user-installed app may send at most
//! 5 follow-ups ([`ResponderError::FollowupCap`], INT-11).
//!
//! ```compile_fail,E0599
//! use noctis_core::{CommandResponder, HandlerResult, Responded};
//! use noctis_ui::{Components, TextDisplay, V2Message};
//!
//! async fn twice(r: CommandResponder) -> HandlerResult<Responded> {
//!     let m = || V2Message::new(Components::new().push(TextDisplay::new("hi")));
//!     Ok(r.reply(m()).await?.reply(m()).await?.done()) // [INT-03]
//! }
//! ```
//!
//! ```compile_fail,E0599
//! use noctis_core::{CommandResponder, HandlerResult, Responded};
//! use noctis_ui::{Components, TextDisplay, V2Message};
//!
//! async fn update(r: CommandResponder) -> HandlerResult<Responded> {
//!     let m = V2Message::new(Components::new().push(TextDisplay::new("hi")));
//!     Ok(r.update(m).await?.done()) // [INT-04]
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use noctis_core::{CommandResponder, HandlerResult, Responded};
//! use noctis_ui::Modal;
//!
//! async fn modal(r: CommandResponder, m: Modal) -> HandlerResult<Responded> {
//!     Ok(r.show_modal(m).await?.done()) // [INT-05]: only `CommandResponder<Manual>`
//! }
//! ```
//!
//! ```compile_fail,E0277
//! use noctis_core::{ComponentResponder, HandlerResult, Responded};
//! use noctis_ui::Modal;
//!
//! async fn modal(r: ComponentResponder, m: Modal) -> HandlerResult<Responded> {
//!     Ok(r.show_modal(m).await?.done()) // [INT-05]: only `ComponentResponder<Manual>`
//! }
//! ```
//!
//! ```compile_fail,E0599
//! use noctis_core::{HandlerResult, ModalResponder, Responded};
//! use noctis_ui::Modal;
//!
//! async fn modal(r: ModalResponder, m: Modal) -> HandlerResult<Responded> {
//!     Ok(r.show_modal(m).await?.done()) // [INT-05]: a modal submit opens no modal
//! }
//! ```
//!
//! ```compile_fail,E0308
//! use noctis_core::{CommandResponder, HandlerResult, Responded};
//! use noctis_ui::CallbackFlags;
//!
//! async fn defer(r: CommandResponder) -> HandlerResult<Responded> {
//!     Ok(r.defer_reply_as(CallbackFlags::EPHEMERAL).await?.done()) // [INT-08]
//! }
//! ```
//!
//! ```compile_fail,E0061
//! use noctis_core::{CommandResponder, Ephemerality, HandlerResult, Responded};
//! use noctis_ui::{Components, TextDisplay, V2Message};
//!
//! async fn edit(r: CommandResponder) -> HandlerResult<Responded> {
//!     let m = || V2Message::new(Components::new().push(TextDisplay::new("hi")));
//!     let acked = r.reply(m()).await?;
//!     acked.edit_reply(m(), Ephemerality::Ephemeral).await?; // [INT-10]
//!     Ok(acked.done())
//! }
//! ```

pub(crate) mod ergonomics;
pub(crate) mod files;
mod nonce;
pub(crate) mod transport;

#[cfg(test)]
pub(crate) mod tests;

use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use std::collections::HashMap;
use std::time::Duration;

use noctis_codec::{PendingRef, Ref, SelectNonce, Sign};
use noctis_ports::{PortError, RefStore};
use noctis_rules::{Limits, MessageKey, Translate};
use noctis_ui::{
    CallbackFlags, Components, CreateFlags, CustomId, EditFlags, LegacyMessage, MessageBody,
    MessageFormat, Modal, TextDisplay, UiError, V2Message,
};
use serde::Deserialize as _;
use serde_json::{Value, json};
use twilight_model::channel::Message;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

pub use self::ergonomics::{NoticeText, ReplyText};
use self::files::{Body, Files};
pub(crate) use self::files::{KnownFiles, file_limit};
pub(crate) use self::nonce::NonceSet;

/// The nonce passed to `resolve_for_send` for an id without a select,
/// which ignores it: such a send draws no nonce (FR-25).
const PLACEHOLDER_NONCE: SelectNonce = match SelectNonce::new(0) {
    Some(nonce) => nonce,
    None => panic!("0 is a select nonce"),
};

/// A scripted `fill` for [`Shared`]'s nonce draw.
#[cfg(test)]
pub(crate) type ScriptedFill = Box<dyn FnMut(&mut [u8; 2]) -> Result<(), getrandom::Error> + Send>;
pub(crate) use self::transport::{InteractionTarget, MessageTarget, Transport};
use crate::AckDeadlineMissed;
use crate::handler::AppShared;
use crate::locale::{LocaleCell, Locales};
use crate::refs::RefOps;

pub(crate) mod sealed {
    /// Seals [`ResponderKind`](super::ResponderKind).
    pub trait Kind {}
    /// Seals [`ResponderState`](super::ResponderState).
    pub trait State {}
    /// Seals [`DeferMode`](super::DeferMode).
    pub trait Defer {}
    /// Seals [`CanShowModal`](super::CanShowModal).
    pub trait ShowModal {}
    /// Seals [`IntoResponse`](super::IntoResponse).
    pub trait Response {}
    /// Seals [`IntoEdit`](super::IntoEdit).
    pub trait Edit {}
}

/// The handler kind of a [`Responder`]: which Pending methods exist and
/// which values a handler may return (AD-11, INT-04). Sealed.
pub trait ResponderKind: sealed::Kind + Send + Sync + 'static {}

/// The state of a [`Responder`]: [`Pending`] or [`Acked`] (INT-03). Sealed.
pub trait ResponderState: sealed::State + Send + Sync + 'static {}

/// Whether a handler is auto-deferred: [`AutoDefer`] (the default) or
/// [`Manual`] (`auto_defer = false`, AD-11). Sealed.
pub trait DeferMode: sealed::Defer + Send + Sync + 'static {
    /// Whether the pipeline arms the auto-defer (INT-01).
    #[doc(hidden)]
    const AUTO_DEFER: bool;
}

/// The [`DeferMode`] of a handler the pipeline auto-defers when it has not
/// answered by the ack deadline (INT-01): the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoDefer {}

impl sealed::Defer for AutoDefer {}
impl DeferMode for AutoDefer {
    const AUTO_DEFER: bool = true;
}

/// The [`DeferMode`] of a handler declared `auto_defer = false`: no
/// auto-defer is armed, so the handler answers within 3 s itself, and
/// `show_modal` is available to Command and component handlers (INT-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Manual {}

impl sealed::Defer for Manual {}
impl DeferMode for Manual {
    const AUTO_DEFER: bool = false;
}

/// The [`DeferMode`]s whose responder may open a modal: only [`Manual`]
/// (INT-05). Sealed.
#[diagnostic::on_unimplemented(
    message = "[INT-05] show_modal needs auto_defer = false on this handler",
    label = "this handler auto-defers"
)]
pub trait CanShowModal: DeferMode + sealed::ShowModal {}

impl sealed::ShowModal for Manual {}
impl CanShowModal for Manual {}

/// The [`ResponderKind`] of slash and context-menu Commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {}

impl sealed::Kind for Command {}
impl ResponderKind for Command {}

/// The [`ResponderKind`] of button and select handlers: they may
/// `update` the clicked message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Component {}

impl sealed::Kind for Component {}
impl ResponderKind for Component {}

/// The [`ResponderKind`] of modal submit handlers: they may `update` the
/// message the modal was opened from, and never open a modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModalSubmit {}

impl sealed::Kind for ModalSubmit {}
impl ResponderKind for ModalSubmit {}

/// The state of a [`Responder`] that has not answered yet: it offers the
/// initial responses.
#[derive(Clone, Copy, Debug)]
pub struct Pending;

/// The state of a [`Responder`] whose interaction is answered: it offers
/// only edits, follow-ups and deletes (INT-03).
#[derive(Clone, Copy, Debug)]
pub struct Acked;

impl sealed::State for Pending {}
impl ResponderState for Pending {}
impl sealed::State for Acked {}
impl ResponderState for Acked {}

/// Whether a response is visible to everyone or only to the invoking user.
///
/// A handler declares it once (`#[command(.., response = ephemeral)]`,
/// `#[button(.., response = ephemeral)]`): [`reply`](Responder::reply),
/// [`defer_reply`](Responder::defer_reply), the auto-defer and a returned
/// message use the declared value (`public` for a component's default
/// `response = update`), [`reply_as`](Responder::reply_as) and
/// [`defer_reply_as`](Responder::defer_reply_as) take one explicitly, and
/// [`follow_up`](Responder::follow_up) takes a [`Visibility`]. Once the
/// reply is sent its ephemerality never changes (INT-10): no edit or
/// `update` takes one.
///
/// ```
/// use noctis_core::Ephemerality;
///
/// assert_eq!(Ephemerality::default(), Ephemerality::Public);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Ephemerality {
    /// Everyone in the channel sees it.
    #[default]
    Public,
    /// Only the invoking user sees it (`EPHEMERAL`).
    Ephemeral,
}

impl Ephemerality {
    /// The callback flags of this ephemerality (`EPHEMERAL` or none; INT-08).
    fn callback_flags(self) -> CallbackFlags {
        match self {
            Self::Public => CallbackFlags::empty(),
            Self::Ephemeral => CallbackFlags::EPHEMERAL,
        }
    }
}

/// The visibility of one follow-up message; the same values as
/// [`Ephemerality`].
pub type Visibility = Ephemerality;

/// A message a [`Responder`] sends: a Components V2 message or a legacy
/// one.
///
/// ```
/// use noctis_core::OutgoingMessage;
/// use noctis_ui::{Components, LegacyMessage, TextDisplay};
///
/// let v2: OutgoingMessage = Components::new().push(TextDisplay::new("hi")).into();
/// let legacy: OutgoingMessage = LegacyMessage::new("hi").into();
/// assert!(matches!(v2, OutgoingMessage::V2(_)));
/// assert!(matches!(legacy, OutgoingMessage::Legacy(_)));
/// ```
#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum OutgoingMessage {
    /// A Components V2 message (`IS_COMPONENTS_V2` is added, CV2-01).
    V2(V2Message),
    /// A legacy `content` message.
    Legacy(LegacyMessage),
}

impl OutgoingMessage {
    fn format(&self) -> MessageFormat {
        match self {
            Self::V2(_) => MessageFormat::V2,
            Self::Legacy(_) => MessageFormat::Legacy,
        }
    }
}

impl From<V2Message> for OutgoingMessage {
    fn from(message: V2Message) -> Self {
        Self::V2(message)
    }
}

impl From<LegacyMessage> for OutgoingMessage {
    fn from(message: LegacyMessage) -> Self {
        Self::Legacy(message)
    }
}

impl From<Components> for OutgoingMessage {
    fn from(components: Components) -> Self {
        Self::V2(V2Message::new(components))
    }
}

/// Why a [`Responder`] call failed.
///
/// ```
/// use noctis_core::ResponderError;
///
/// let err = ResponderError::Rejected { status: 404, code: Some(10015) };
/// assert_eq!(err.to_string(), "Discord rejected the response with HTTP 404 (code 10015)");
/// ```
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ResponderError {
    /// The message breaks a Discord Limit (Strict mode; the Upload Budget,
    /// ATT-01..03, in every mode), or an edit would turn a V2 message back
    /// into a legacy one; nothing was sent.
    #[error(transparent)]
    Invalid(#[from] UiError),
    /// The message could not be serialized: a Route `custom_id` could not
    /// be resolved (a select nonce or a Ref that could not be drawn, or an
    /// encode error), named in `reason`; nothing was sent.
    #[error("the message could not be encoded: {reason}")]
    Encode {
        /// What failed, naming the Route.
        reason: String,
    },
    /// A Ref minted for the message could not be stored in the App's
    /// `RefStore`; nothing was sent. Refs stored before the failure expire
    /// with their TTL.
    #[error("the Ref could not be stored: {0}")]
    RefStore(#[source] PortError),
    /// Three freshly minted Refs in a row were already taken in the App's
    /// `RefStore`; nothing was sent.
    #[error("could not mint a unique Ref after 3 attempts")]
    RefCollision,
    /// An ephemeral response over a transport that has none (a
    /// [`Dispatcher`](crate::Dispatcher) message reply); nothing was sent.
    #[error("this response cannot be ephemeral: a message reply is always public")]
    EphemeralUnsupported,
    /// The interaction token is not 1-512 characters of `[A-Za-z0-9._-]`;
    /// nothing was sent.
    #[error("the interaction token is not 1-512 characters of [A-Za-z0-9._-]")]
    InvalidToken,
    /// The initial response missed the ack deadline (INT-01).
    #[error(transparent)]
    AckDeadlineMissed(#[from] AckDeadlineMissed),
    /// Discord answered with an error status.
    #[error("Discord rejected the response with HTTP {status}{}", code.map(|c| format!(" (code {c})")).unwrap_or_default())]
    Rejected {
        /// The HTTP status.
        status: u16,
        /// Discord's JSON error code, when the body has one.
        code: Option<u32>,
    },
    /// The request did not get a usable response (for a
    /// `…_with_response` call: a body without `resource`).
    #[error("the response could not reach Discord")]
    Transport,
    /// The interaction token expired 15 minutes after the interaction
    /// arrived (INT-02): a follow-up, or an edit or delete of an ephemeral
    /// message or of one whose id or channel is unknown; nothing was sent.
    /// Public messages with a known id are edited and deleted with the bot
    /// token instead.
    #[error("the interaction token expired (15 minutes)")]
    TokenExpired,
    /// A user-installed app already sent 5 follow-ups for this interaction
    /// (INT-11); nothing was sent.
    #[error("a user-installed app may send at most 5 follow-ups here")]
    FollowupCap,
    /// There is no reply message to fetch: a
    /// [`Dispatcher`](crate::Dispatcher) handler has not replied yet, or
    /// the responder is detached; nothing was sent.
    #[error("there is no reply message to fetch")]
    NoReply,
    /// A modal was not the first response to an interaction, or the
    /// transport has none (a [`Dispatcher`](crate::Dispatcher) message
    /// reply); nothing was sent.
    #[error("a modal can only be the first response to an interaction")]
    ModalUnavailable,
    /// `update` or `defer_update` of a modal submit whose modal was opened
    /// by a slash command, not from a component message: there is no
    /// message to update; nothing was sent and the interaction is still
    /// unanswered.
    #[error("there is no message to update: this modal was not opened from a component message")]
    UpdateUnavailable,
    /// The handler already sent its initial response, through a
    /// context method (`ctx.respond`, `ctx.update`, `ctx.reply_text`) or a
    /// typestate [`Responder`], and then attempted a second one: a
    /// `ctx.update`, or any Pending method of a [`Responder`] (`reply`,
    /// `defer_reply`, `update`, `defer_update`, `show_modal`, …). Also a
    /// response after a modal or a Middleware stop. Nothing was sent
    /// (INT-03). `ctx.respond` and `ctx.reply_text` send a follow-up
    /// instead once answered.
    #[error("this handler already sent its initial response; nothing was sent")]
    AlreadyResponded,
}

/// What Discord answered to a `…_with_response` callback: the
/// `resource` of the interaction callback response.
///
/// ```
/// # use noctis_core::CallbackResource;
/// fn describe(resource: &CallbackResource) -> String {
///     match resource.message() {
///         Some(message) => format!("type {} created message {}", resource.kind(), message.id),
///         None => format!("type {} without a message", resource.kind()),
///     }
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CallbackResource {
    kind: u8,
    message: Option<Message>,
}

impl CallbackResource {
    /// The resource `type`: the callback type it answers (4, 5, 6 or 7).
    pub fn kind(&self) -> u8 {
        self.kind
    }

    /// The message the callback created or updated, when Discord sent one
    /// that parses.
    pub fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }

    /// The `resource` of a callback response body, or
    /// [`ResponderError::Transport`] when the body has none.
    fn parse(body: &[u8]) -> Result<Self, ResponderError> {
        let value: Value = serde_json::from_slice(body).map_err(|_| ResponderError::Transport)?;
        let resource = value.get("resource").ok_or(ResponderError::Transport)?;
        let kind = resource
            .get("type")
            .and_then(Value::as_u64)
            .and_then(|k| u8::try_from(k).ok())
            .ok_or(ResponderError::Transport)?;
        let message = resource.get("message").and_then(|m| {
            Message::deserialize(m)
                .map_err(|error| {
                    tracing::debug!(target: "noctis::responder", %error, "callback resource message does not parse");
                })
                .ok()
        });
        Ok(Self { kind, message })
    }
}

/// The proof that a handler answered through its [`Responder`]; return it
/// from the handler.
#[must_use = "return the `Responded` from the handler"]
#[derive(Debug)]
pub struct Responded {
    _private: (),
}

impl Responded {
    /// The proof itself; what `done()` and `AutocompleteCtx::respond`
    /// return.
    pub(crate) fn new() -> Self {
        Self { _private: () }
    }
}

/// A value a handler of kind `K` may return: a [`V2Message`],
/// [`Components`] or the [`Responded`] from [`done`](Responder::done); a
/// command handler may also return a [`LegacyMessage`], and a component or
/// modal handler any of its three paired with [`RefOps`] (Refs deleted
/// after the response is sent). Component and modal
/// handlers return V2 only (CV2-01): their returned message may update the
/// component's message, which can't turn back into a legacy message.
/// Sealed.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a response of this handler",
    label = "return a `V2Message`, `Components`, a `LegacyMessage` (commands only), or the `Responded` from `done()`",
    note = "[CV2-01] component and modal handlers return a `V2Message`/`Components` (or `Responded`); a `LegacyMessage` is a response of command handlers only"
)]
pub trait IntoResponse<K: ResponderKind>: sealed::Response + Send + 'static {
    /// The response the pipeline sends.
    #[doc(hidden)]
    fn __into_response(self) -> ResponseValue;
}

/// What a handler returned, as the pipeline sends it, and the Refs to
/// delete once it was sent.
#[doc(hidden)]
#[derive(Debug)]
pub struct ResponseValue(pub(crate) Response, pub(crate) RefOps);

impl ResponseValue {
    /// `response` without Ref operations.
    fn of(response: Response) -> Self {
        Self(response, RefOps::new())
    }
}

/// What a handler returned.
#[derive(Debug)]
pub(crate) enum Response {
    /// It answered through the Responder.
    Done,
    /// A message for the pipeline to send.
    Message(OutgoingMessage),
}

impl sealed::Response for Responded {}
impl sealed::Response for V2Message {}
impl sealed::Response for Components {}
impl sealed::Response for LegacyMessage {}

impl<K: ResponderKind> IntoResponse<K> for Responded {
    fn __into_response(self) -> ResponseValue {
        ResponseValue::of(Response::Done)
    }
}

/// The message responses of every kind.
macro_rules! message_responses {
    ($($kind:ident),*) => {$(
        impl IntoResponse<$kind> for V2Message {
            fn __into_response(self) -> ResponseValue {
                ResponseValue::of(Response::Message(self.into()))
            }
        }

        impl IntoResponse<$kind> for Components {
            fn __into_response(self) -> ResponseValue {
                ResponseValue::of(Response::Message(self.into()))
            }
        }

        impl IntoEdit<$kind> for V2Message {
            fn into_outgoing(self) -> OutgoingMessage {
                self.into()
            }
        }

        impl IntoEdit<$kind> for Components {
            fn into_outgoing(self) -> OutgoingMessage {
                self.into()
            }
        }
    )*};
}

message_responses!(Command, Component, ModalSubmit);

impl<T: sealed::Response> sealed::Response for (T, RefOps) {}

/// `(response, RefOps)` for the component and modal kinds.
macro_rules! ref_ops_responses {
    ($($kind:ident),*) => {$(
        impl IntoResponse<$kind> for (V2Message, RefOps) {
            fn __into_response(self) -> ResponseValue {
                ResponseValue(Response::Message(self.0.into()), self.1)
            }
        }

        impl IntoResponse<$kind> for (Components, RefOps) {
            fn __into_response(self) -> ResponseValue {
                ResponseValue(Response::Message(self.0.into()), self.1)
            }
        }

        impl IntoResponse<$kind> for (Responded, RefOps) {
            fn __into_response(self) -> ResponseValue {
                ResponseValue(Response::Done, self.1)
            }
        }
    )*};
}

ref_ops_responses!(Component, ModalSubmit);

impl IntoResponse<Command> for LegacyMessage {
    fn __into_response(self) -> ResponseValue {
        ResponseValue::of(Response::Message(self.into()))
    }
}

/// A message body that may edit or update the message of a handler of kind
/// `K`: a [`V2Message`] or [`Components`] for every kind; a command's
/// [`edit_reply`](Responder::edit_reply) also takes a [`LegacyMessage`] or
/// an [`OutgoingMessage`]. A component's message (and a modal opened from
/// one) is edited only into V2 (CV2-01, compile time); `reply`, `reply_as`
/// and `follow_up` still accept a `LegacyMessage`, since they send a new
/// message. Sealed.
#[diagnostic::on_unimplemented(
    message = "[CV2-01] a component's message can't be turned back into a legacy message: send a `V2Message`",
    label = "a `LegacyMessage` can't edit or update a component's message",
    note = "build the body with `V2Message`/`Components`; `reply`, `reply_as` and `follow_up` still accept a `LegacyMessage` (they send a new message)"
)]
pub trait IntoEdit<K: ResponderKind>: sealed::Edit + Send + 'static {
    /// The message body to send.
    #[doc(hidden)]
    fn into_outgoing(self) -> OutgoingMessage;
}

impl sealed::Edit for V2Message {}
impl sealed::Edit for Components {}
impl sealed::Edit for LegacyMessage {}
impl sealed::Edit for OutgoingMessage {}

impl IntoEdit<Command> for LegacyMessage {
    fn into_outgoing(self) -> OutgoingMessage {
        self.into()
    }
}

impl IntoEdit<Command> for OutgoingMessage {
    fn into_outgoing(self) -> OutgoingMessage {
        self
    }
}

/// How a handler answers its interaction; `K` is the handler kind, `S` the
/// state ([`Pending`] or [`Acked`]) and `D` the [`DeferMode`]
/// ([`AutoDefer`] or [`Manual`]).
///
/// Take it as a handler parameter through its alias, [`CommandResponder`],
/// [`ComponentResponder`] or [`ModalResponder`]; a handler that returns a
/// message takes none. `Debug` never prints the interaction token.
///
/// ```
/// use noctis_core::{CommandResponder, HandlerResult, Responded, Visibility};
/// use noctis_ui::{Components, TextDisplay, V2Message};
///
/// async fn warn(r: CommandResponder) -> HandlerResult<Responded> {
///     let text = |t: &str| V2Message::new(Components::new().push(TextDisplay::new(t.to_owned())));
///     let acked = r.reply(text("Warned.")).await?;
///     acked.follow_up(text("Case #12 logged."), Visibility::Public).await?;
///     Ok(acked.done())
/// }
/// ```
#[must_use = "a Responder sends nothing until it is used; return a message instead when you do not need it"]
pub struct Responder<K: ResponderKind, S: ResponderState = Pending, D: DeferMode = AutoDefer> {
    shared: Arc<Shared>,
    #[expect(
        clippy::type_complexity,
        reason = "the three-parameter `PhantomData<fn() -> (K, S, D)>` marker trips type_complexity"
    )]
    marker: PhantomData<fn() -> (K, S, D)>,
}

/// The [`Responder`] of a slash or context-menu Command handler;
/// `CommandResponder<Manual>` for a handler declared `auto_defer = false`.
pub type CommandResponder<D = AutoDefer> = Responder<Command, Pending, D>;

/// The [`Responder`] of a button or select handler;
/// `ComponentResponder<Manual>` for a handler declared `auto_defer = false`.
///
/// ```
/// use noctis_core::{ComponentResponder, HandlerResult, Responded};
/// use noctis_ui::{Components, TextDisplay};
///
/// async fn bought(r: ComponentResponder) -> HandlerResult<Responded> {
///     let panel = Components::new().push(TextDisplay::new("Bought."));
///     Ok(r.update(panel).await?.done())
/// }
/// ```
pub type ComponentResponder<D = AutoDefer> = Responder<Component, Pending, D>;

/// The [`Responder`] of a modal submit handler.
///
/// `update` of a submit whose modal a slash command opened is
/// [`ResponderError::UpdateUnavailable`]; `reply` works for every submit.
///
/// ```
/// use noctis_core::{HandlerResult, ModalResponder, Responded};
/// use noctis_ui::{Components, TextDisplay};
///
/// async fn saved(r: ModalResponder) -> HandlerResult<Responded> {
///     let text = Components::new().push(TextDisplay::new("Saved."));
///     Ok(r.reply(text).await?.done())
/// }
/// ```
pub type ModalResponder<D = AutoDefer> = Responder<ModalSubmit, Pending, D>;

impl<K: ResponderKind, S: ResponderState, D: DeferMode> fmt::Debug for Responder<K, S, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Responder")
            .field("kind", &std::any::type_name::<K>())
            .field("state", &std::any::type_name::<S>())
            .field("defer", &std::any::type_name::<D>())
            .field("shared", &self.shared)
            .finish()
    }
}

impl<K: ResponderKind, S: ResponderState, D: DeferMode> Responder<K, S, D> {
    fn acked(self) -> Responder<K, Acked, D> {
        Responder {
            shared: self.shared,
            marker: PhantomData,
        }
    }
}

impl<K: ResponderKind, D: DeferMode> Responder<K, Pending, D> {
    /// A Pending responder over `shared`.
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            marker: PhantomData,
        }
    }

    /// Answers with `msg` as a new message (callback type 4), with the
    /// handler's declared `response` (`public` under a component's
    /// `response = update`); a reply to the invoking message over a
    /// [`Dispatcher`](crate::Dispatcher). When the auto-defer already
    /// acknowledged the interaction, `msg` edits the deferred reply instead
    /// and keeps its ephemerality (INT-03, INT-10); after a type-6 defer it
    /// is a follow-up, never an edit of the clicked message.
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]: the message is invalid or Discord rejected it.
    /// The responder is consumed either way.
    pub async fn reply(
        self,
        msg: impl Into<OutgoingMessage>,
    ) -> Result<Responder<K, Acked, D>, ResponderError> {
        let ephemerality = self.shared.default_ephemerality();
        self.shared.reply(msg.into(), ephemerality, false).await?;
        Ok(self.acked())
    }

    /// As [`reply`](Self::reply), with `ephemerality` instead of the
    /// declared `response`. After the auto-defer it edits the reply, whose
    /// ephemerality stays the auto-defer's (INT-10).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::EphemeralUnsupported`] for
    /// an ephemeral reply over a [`Dispatcher`](crate::Dispatcher).
    pub async fn reply_as(
        self,
        msg: impl Into<OutgoingMessage>,
        ephemerality: Ephemerality,
    ) -> Result<Responder<K, Acked, D>, ResponderError> {
        self.shared.reply(msg.into(), ephemerality, false).await?;
        Ok(self.acked())
    }

    /// As [`reply`](Self::reply), sent with `?with_response=true`; also
    /// returns Discord's [`CallbackResource`], or `None` when no callback
    /// was sent (the interaction was already acknowledged, so the reply
    /// became an edit or a follow-up).
    ///
    /// # Errors
    ///
    /// As [`reply`](Self::reply); [`ResponderError::Transport`] when the
    /// callback response has no `resource`.
    pub async fn reply_with_response(
        self,
        msg: impl Into<OutgoingMessage>,
    ) -> Result<(Responder<K, Acked, D>, Option<CallbackResource>), ResponderError> {
        let ephemerality = self.shared.default_ephemerality();
        let resource = self.shared.reply(msg.into(), ephemerality, true).await?;
        Ok((self.acked(), resource))
    }

    /// Acknowledges now and answers later (callback type 5, Discord's
    /// "thinking…" message) with the declared `response`; a typing
    /// indicator over a [`Dispatcher`](crate::Dispatcher). Edit the reply
    /// afterwards with [`edit_reply`](Responder::edit_reply). After any
    /// acknowledgement it sends nothing.
    ///
    /// # Errors
    ///
    /// A [`ResponderError`].
    pub async fn defer_reply(self) -> Result<Responder<K, Acked, D>, ResponderError> {
        let ephemerality = self.shared.default_ephemerality();
        self.shared.defer_reply(ephemerality, false).await?;
        Ok(self.acked())
    }

    /// As [`defer_reply`](Self::defer_reply), with `ephemerality` instead
    /// of the declared `response` (the type-5 callback's only flag is
    /// `EPHEMERAL`, INT-08).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`].
    pub async fn defer_reply_as(
        self,
        ephemerality: Ephemerality,
    ) -> Result<Responder<K, Acked, D>, ResponderError> {
        self.shared.defer_reply(ephemerality, false).await?;
        Ok(self.acked())
    }

    /// As [`defer_reply`](Self::defer_reply), sent with
    /// `?with_response=true`; also returns Discord's [`CallbackResource`],
    /// or `None` when no callback was sent.
    ///
    /// # Errors
    ///
    /// As [`defer_reply`](Self::defer_reply); [`ResponderError::Transport`]
    /// when the callback response has no `resource`.
    pub async fn defer_reply_with_response(
        self,
    ) -> Result<(Responder<K, Acked, D>, Option<CallbackResource>), ResponderError> {
        let ephemerality = self.shared.default_ephemerality();
        let resource = self.shared.defer_reply(ephemerality, true).await?;
        Ok((self.acked(), resource))
    }
}

/// `update` and `defer_update` of the kinds that have a message to update.
macro_rules! update_methods {
    ($($kind:ident),*) => {$(
        impl<D: DeferMode> Responder<$kind, Pending, D> {
            /// Replaces the clicked component's message with `msg` in one
            /// request (callback type 7); the message keeps its
            /// ephemerality (no `EPHEMERAL` flag is ever sent, INT-10). A
            /// legacy source message is converted to V2 by clearing its
            /// legacy fields (CV2-02); `msg` is always V2 ([`IntoEdit`],
            /// CV2-01).
            /// After an acknowledgement it edits the original instead.
            /// [`edit_reply`](Responder::edit_reply) afterwards edits the
            /// same message.
            ///
            /// # Errors
            ///
            /// [`ResponderError::UpdateUnavailable`] for a modal submit whose
            /// modal was not opened from a component message (no request;
            /// the interaction stays unanswered); any other
            /// [`ResponderError`].
            pub async fn update(
                self,
                msg: impl IntoEdit<$kind>,
            ) -> Result<Responder<$kind, Acked, D>, ResponderError> {
                self.shared.update(msg.into_outgoing(), false).await?;
                Ok(self.acked())
            }

            /// Acknowledges without a visible change (callback type 6); the
            /// original is then the clicked message, so
            /// [`edit_reply`](Responder::edit_reply) edits it
            /// (`defer_update` + `edit_reply` ≡ `update`). After any
            /// acknowledgement it sends nothing.
            ///
            /// # Errors
            ///
            /// [`ResponderError::UpdateUnavailable`] as for `update`; any
            /// other [`ResponderError`].
            pub async fn defer_update(self) -> Result<Responder<$kind, Acked, D>, ResponderError> {
                self.shared.defer_update(false).await?;
                Ok(self.acked())
            }

            /// As `update`, sent with `?with_response=true`; also returns
            /// Discord's [`CallbackResource`], or `None` when no callback was
            /// sent (the update became an edit).
            ///
            /// # Errors
            ///
            /// As `update`; [`ResponderError::Transport`] when the callback
            /// response has no `resource`.
            pub async fn update_with_response(
                self,
                msg: impl IntoEdit<$kind>,
            ) -> Result<(Responder<$kind, Acked, D>, Option<CallbackResource>), ResponderError> {
                let resource = self.shared.update(msg.into_outgoing(), true).await?;
                Ok((self.acked(), resource))
            }

            /// As `defer_update`, sent with `?with_response=true`; also
            /// returns Discord's [`CallbackResource`], or `None` when no
            /// callback was sent.
            ///
            /// # Errors
            ///
            /// As `defer_update`; [`ResponderError::Transport`] when the
            /// callback response has no `resource`.
            pub async fn defer_update_with_response(
                self,
            ) -> Result<(Responder<$kind, Acked, D>, Option<CallbackResource>), ResponderError> {
                let resource = self.shared.defer_update(true).await?;
                Ok((self.acked(), resource))
            }
        }
    )*};
}

update_methods!(Component, ModalSubmit);

/// `show_modal` of the kinds that may open a modal.
macro_rules! show_modal {
    ($($kind:ident),*) => {$(
        impl<D: DeferMode> Responder<$kind, Pending, D> {
            /// Opens `modal` (callback type 9). Only a handler declared
            /// `auto_defer = false` has it: no auto-defer is armed for one,
            /// so the modal stays the first response (INT-05). A modal
            /// leaves no reply to edit: return [`done`](Responder::done)
            /// afterwards.
            ///
            /// `modal` is anything that converts into a [`Modal`]: a
            /// `#[derive(Modal)]` struct value, its `X { .. }.build()`
            /// builder (with `.title(..)`, `.value(..)`, `.overrides(..)`),
            /// `X::blank(..)` or a `Modal`. Locale keys in it are resolved in
            /// the User Locale (then the Guild Locale) before it is
            /// validated; a pre-fill or override that breaks a limit is
            /// [`ResponderError::Invalid`] with no request.
            ///
            /// ```
            /// use noctis_core::{CommandResponder, HandlerResult, Manual, Responded};
            /// use noctis_ui::Modal;
            ///
            /// async fn feedback(r: CommandResponder<Manual>, form: Modal) -> HandlerResult<Responded> {
            ///     Ok(r.show_modal(form).await?.done())
            /// }
            /// ```
            ///
            /// # Errors
            ///
            /// [`ResponderError::Invalid`] (Limits) or
            /// [`ResponderError::Encode`] (an unresolvable Route
            /// `custom_id`) before any request;
            /// [`ResponderError::ModalUnavailable`] over a
            /// [`Dispatcher`](crate::Dispatcher) or when the interaction is
            /// already answered; any other [`ResponderError`] from Discord.
            pub async fn show_modal(
                self,
                modal: impl Into<Modal>,
            ) -> Result<Responder<$kind, Acked, D>, ResponderError>
            where
                D: CanShowModal,
            {
                self.shared.show_modal(modal.into()).await?;
                Ok(self.acked())
            }
        }
    )*};
}

show_modal!(Command, Component);

impl<K: ResponderKind, D: DeferMode> Responder<K, Acked, D> {
    /// Replaces the reply (or, after `update`/`defer_update`, the clicked
    /// message) with `msg`; its visibility does not change (INT-10).
    /// Editing a legacy original into a V2 message clears its legacy fields
    /// (CV2-02). A component or modal handler edits only into V2
    /// ([`IntoEdit`], CV2-01, compile time); a command's legacy edit of a V2
    /// original is [`UiError::V2ToLegacy`] at run time. After 15 minutes a
    /// public original whose id is known is edited with the bot token
    /// (INT-02).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes for an ephemeral original or one whose id is unknown.
    pub async fn edit_reply(&self, msg: impl IntoEdit<K>) -> Result<(), ResponderError> {
        self.shared.edit_reply(msg.into_outgoing()).await
    }

    /// Fetches the reply message: over the interaction token while it is
    /// valid, with the bot token after 15 minutes for a public reply whose
    /// id is known (INT-02), and with the bot token for a
    /// [`Dispatcher`](crate::Dispatcher) reply.
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]: [`ResponderError::TokenExpired`] after 15
    /// minutes for an ephemeral reply or one whose id is unknown;
    /// [`ResponderError::NoReply`] when a
    /// [`Dispatcher`](crate::Dispatcher) handler has not replied yet;
    /// [`ResponderError::Rejected`] for an error status;
    /// [`ResponderError::Transport`] when the body is not a message.
    pub async fn fetch_reply(&self) -> Result<Message, ResponderError> {
        self.shared.fetch_reply().await
    }

    /// Sends one more message, visible to everyone or only to the invoking
    /// user.
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::EphemeralUnsupported`] for
    /// an ephemeral follow-up over a [`Dispatcher`](crate::Dispatcher);
    /// [`ResponderError::TokenExpired`] after 15 minutes;
    /// [`ResponderError::FollowupCap`] for the sixth follow-up of a
    /// user-installed app (INT-11).
    pub async fn follow_up(
        &self,
        msg: impl Into<OutgoingMessage>,
        visibility: Visibility,
    ) -> Result<FollowupHandle, ResponderError> {
        self.shared.follow_up(msg.into(), visibility).await
    }

    /// Deletes the reply (nothing to do when a
    /// [`Dispatcher`](crate::Dispatcher) handler has not replied yet).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes, as for [`edit_reply`](Self::edit_reply).
    pub async fn delete_reply(&self) -> Result<(), ResponderError> {
        self.shared.delete_reply().await
    }

    /// Ends the response; return the [`Responded`] from the handler.
    #[must_use = "return the `Responded` from the handler"]
    pub fn done(self) -> Responded {
        Responded { _private: () }
    }
}

/// One follow-up message sent with [`Responder::follow_up`].
#[must_use = "a FollowupHandle is the only way to edit or delete the follow-up"]
pub struct FollowupHandle {
    shared: Arc<Shared>,
    message_id: Id<MessageMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    visibility: Visibility,
    format: Mutex<MessageFormat>,
    /// The follow-up's attachments on Discord (its keep-list, MSG-08).
    files: Mutex<KnownFiles>,
}

impl FollowupHandle {
    /// The follow-up's message id.
    pub fn message_id(&self) -> Id<MessageMarker> {
        self.message_id
    }

    /// Replaces the follow-up with `msg`, keeping its attachments that
    /// `msg` still uses by id and uploading the new ones (MSG-08). After 15
    /// minutes a public follow-up is edited with the bot token (INT-02).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes for an ephemeral follow-up.
    pub async fn edit(&self, msg: impl Into<OutgoingMessage>) -> Result<(), ResponderError> {
        let mut msg = msg.into();
        self.shared.prepare(&mut msg, self.visibility).await?;
        let format = *self.format.lock().unwrap_or_else(PoisonError::into_inner);
        let known = self
            .files
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let body = self
            .shared
            .body(&msg, edit_body(&msg, format)?, Some(&known), None)?;
        let posted = match self.shared.expired_route(
            self.visibility,
            Some(self.message_id),
            self.channel_id,
        )? {
            Some((app, channel_id, id)) => {
                transport::channel_edit(app, channel_id, id, body.payload).await?
            }
            None => {
                self.shared
                    .transport
                    .edit_message(self.message_id, body.payload)
                    .await?
            }
        };
        *self.format.lock().unwrap_or_else(PoisonError::into_inner) = msg.format();
        *self.files.lock().unwrap_or_else(PoisonError::into_inner) =
            posted.files.unwrap_or(body.kept);
        Ok(())
    }

    /// Deletes the follow-up. After 15 minutes a public follow-up is
    /// deleted with the bot token (INT-02).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes for an ephemeral follow-up.
    pub async fn delete(&self) -> Result<(), ResponderError> {
        match self
            .shared
            .expired_route(self.visibility, Some(self.message_id), self.channel_id)?
        {
            Some((app, channel_id, id)) => transport::channel_delete(app, channel_id, id).await,
            None => self.shared.transport.delete_message(self.message_id).await,
        }
    }
}

impl fmt::Debug for FollowupHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FollowupHandle")
            .field("message_id", &self.message_id)
            .field("visibility", &self.visibility)
            .finish_non_exhaustive()
    }
}

/// The responder state (`Shared::state`): not answered yet.
pub(crate) const PENDING: u8 = 0;
/// Acknowledged with a type-5 defer (a loading reply).
pub(crate) const DEFERRED: u8 = 1;
/// Answered with a message: a type-4 reply or a type-7 update.
pub(crate) const REPLIED: u8 = 2;
/// Answered with a modal (type 9).
pub(crate) const MODAL: u8 = 3;
/// Acknowledged with a type-6 defer: the original is the clicked message.
pub(crate) const DEFERRED_UPDATE: u8 = 4;
/// Closed unanswered by a `Flow::Stop(None)`: nothing is sent, and a
/// pending auto-defer returns without a type-5 defer.
pub(crate) const STOPPED: u8 = 5;

/// What a handler declared it answers with: a new message of an
/// ephemerality, or an update of the clicked message (AD-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Declared {
    /// `response = public | ephemeral`: a new message.
    Reply(Ephemerality),
    /// `response = update` (components and modal submits).
    Update,
}

/// The message a component lives on (or a modal was opened from), read from
/// the raw interaction JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceMessage {
    pub(crate) id: Id<MessageMarker>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) format: MessageFormat,
    pub(crate) ephemerality: Ephemerality,
    /// The select nonces of every Noctis `custom_id` in the raw interaction
    /// (its `data` and this message's components), which a send must not
    /// reuse (FR-25).
    pub(crate) nonces: NonceSet,
    /// The message's attachments (`message.attachments`), which an update
    /// keeps by id (MSG-08).
    pub(crate) files: KnownFiles,
}

/// The `message` fields of a raw interaction that the Responder reads; the
/// rest of the message (a V2 component tree twilight may not parse) is
/// skipped.
#[derive(serde::Deserialize)]
struct RawSource<'a> {
    #[serde(borrow)]
    message: Option<RawMessage<'a>>,
}

#[derive(serde::Deserialize)]
struct RawMessage<'a> {
    id: &'a str,
    channel_id: &'a str,
    #[serde(default)]
    flags: Option<u64>,
    #[serde(default)]
    attachments: Option<Value>,
}

/// `IS_COMPONENTS_V2`.
const IS_COMPONENTS_V2: u64 = 1 << 15;
/// `EPHEMERAL`.
const EPHEMERAL: u64 = 1 << 6;

impl SourceMessage {
    /// The `message` of a raw interaction object, if it has one with a
    /// valid id and channel.
    pub(crate) fn from_interaction(raw: &[u8]) -> Option<Self> {
        let message = serde_json::from_slice::<RawSource<'_>>(raw).ok()?.message?;
        let id = message.id.parse().ok().and_then(Id::new_checked)?;
        let channel_id = message.channel_id.parse().ok().and_then(Id::new_checked)?;
        let flags = message.flags.unwrap_or(0);
        Some(Self {
            id,
            channel_id,
            format: if flags & IS_COMPONENTS_V2 == 0 {
                MessageFormat::Legacy
            } else {
                MessageFormat::V2
            },
            ephemerality: if flags & EPHEMERAL == 0 {
                Ephemerality::Public
            } else {
                Ephemerality::Ephemeral
            },
            nonces: nonce::nonces_in(raw),
            files: KnownFiles::from_list(message.attachments.as_ref()),
        })
    }
}

/// What the original response is.
#[derive(Clone, Copy, Debug, Default)]
enum Shape {
    /// No initial response yet.
    #[default]
    Unsent,
    /// Discord's loading message (a defer).
    Deferred,
    /// A message in this format.
    Sent(MessageFormat),
}

/// The original response.
#[derive(Clone, Debug, Default)]
struct Original {
    shape: Shape,
    /// The original message, when known: the message reply, the
    /// `with_response` callback's message, the updated component message or
    /// the last edit's response.
    message_id: Option<Id<MessageMarker>>,
    /// Who sees it.
    ephemerality: Ephemerality,
    /// Its channel, when known.
    channel_id: Option<Id<ChannelMarker>>,
    /// Its attachments on Discord (the keep-list of the next edit, MSG-08):
    /// from the reply's, update's or edit's response, the source message's
    /// after `update`/`defer_update`, else the entries the last send kept.
    files: KnownFiles,
}

/// The bot-token route of an expired interaction: the App, the channel and
/// the message.
type BotRoute<'a> = (&'a AppShared, Id<ChannelMarker>, Id<MessageMarker>);

/// The Guard phase of a run (AD-23 stage 8), stored on [`Shared`]: whether
/// a guarded Route's handler body ever started.
pub(crate) struct GuardPhase;

impl GuardPhase {
    /// The initial phase; it stays for an unguarded Route and when stage 7
    /// stopped the run.
    pub(crate) const UNGUARDED: u8 = 0;
    /// The Route's Guards are running; the handler body has not started.
    pub(crate) const GUARDING: u8 = 1;
    /// The Guards allowed the run and the handler body started.
    pub(crate) const BODY: u8 = 2;
}

/// What one interaction's responders and its pipeline share: the
/// transport, the state machine and its gate, the original response, the
/// declared response, the source message, the App's Limits, the
/// follow-up cap and whether a modal may be shown at all.
pub(crate) struct Shared {
    transport: Transport,
    /// `PENDING`, `DEFERRED`, `REPLIED`, `MODAL`, `DEFERRED_UPDATE` or
    /// `STOPPED`; written only under `gate`, read without it.
    state: AtomicU8,
    /// Serializes every initial response: the handler's and the
    /// auto-defer's (INT-03).
    gate: tokio::sync::Mutex<()>,
    /// Never locked across an `.await`.
    original: Mutex<Original>,
    declared: Declared,
    /// The component's message (or the message a modal was opened from).
    source: Option<SourceMessage>,
    limits: Limits,
    /// Follow-up slots taken (INT-11).
    followups: AtomicU8,
    /// `Some(5)` for a user-install-only interaction.
    followup_cap: Option<u8>,
    /// The run's [`GuardPhase`]; written only by the guarded handler future.
    guard_phase: AtomicU8,
    /// The select nonces of the interaction, which no send of this run may
    /// draw (FR-25). Draws never write here, so they cannot evict them.
    source_nonces: NonceSet,
    /// The run's last eight drawn nonces, which no later send may draw
    /// (FR-25). Never locked across an `.await`.
    drawn: Mutex<NonceSet>,
    /// A scripted `fill` for the nonce draw, replacing the OS CSPRNG.
    #[cfg(test)]
    fill: Mutex<Option<ScriptedFill>>,
    /// `true` for a modal submit: `show_modal` is refused with
    /// `ModalUnavailable` before any work (INT-04, INT-05).
    no_modal: bool,
    /// Whether the handler itself took the initial response (a context
    /// method or a typestate Pending method); never set by the auto-defer,
    /// `finish` or `notice`. Written only under `gate`.
    handler_answered: AtomicBool,
    /// The App's strings and the run's locales, which resolve the locale
    /// keys of every send ([`I18n::DETACHED`] until the pipeline sets it).
    i18n: OnceLock<I18n>,
}

/// What resolves the [`noctis_ui::Text::Key`]s of a send: the App's
/// strings and the run's locale cell (read at send time, so a Middleware
/// override reaches every send).
pub(crate) struct I18n {
    pub(crate) strings: Arc<dyn Translate>,
    pub(crate) locales: Arc<LocaleCell>,
}

impl I18n {
    /// The built-in strings in `en-US`.
    fn detached() -> Self {
        Self {
            strings: Arc::new(noctis_i18n::Builtin),
            locales: LocaleCell::new(Locales::both(noctis_rules::Locale::EnUs)),
        }
    }

    /// The text of `key` for a send of `visibility`: in the User Locale
    /// for an ephemeral send (and a modal), in the Guild Locale for a
    /// public one. A key with no translation is its own key string, with
    /// one `warn` on target `noctis::i18n`.
    fn text(&self, key: MessageKey, visibility: Ephemerality) -> String {
        let locales = self.locales.get();
        let locale = match visibility {
            Ephemerality::Ephemeral => locales.user,
            Ephemerality::Public => locales.guild,
        };
        if let Some(text) = self.strings.translate(locale, key, &[]) {
            return text;
        }
        tracing::warn!(target: "noctis::i18n", key = key.as_str(), "missing message key");
        key.as_str().to_owned()
    }
}

impl fmt::Debug for I18n {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("I18n")
            .field("locales", &self.locales)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared")
            .field("transport", &self.transport)
            .field("state", &self.state.load(Ordering::Acquire))
            .field("declared", &self.declared)
            .field("source", &self.source)
            .field("followup_cap", &self.followup_cap)
            .field("no_modal", &self.no_modal)
            .finish_non_exhaustive()
    }
}

impl Shared {
    pub(crate) fn new(
        transport: Transport,
        ephemerality: Ephemerality,
        limits: Limits,
    ) -> Arc<Self> {
        Self::with_followup_cap(transport, ephemerality, limits, None)
    }

    /// As [`new`](Self::new), with at most `cap` follow-ups (INT-11).
    pub(crate) fn with_followup_cap(
        transport: Transport,
        ephemerality: Ephemerality,
        limits: Limits,
        cap: Option<u8>,
    ) -> Arc<Self> {
        Self::declared(transport, Declared::Reply(ephemerality), None, limits, cap)
    }

    /// The state of a handler that declared `declared`, on an interaction
    /// whose source message is `source`.
    pub(crate) fn declared(
        transport: Transport,
        declared: Declared,
        source: Option<SourceMessage>,
        limits: Limits,
        cap: Option<u8>,
    ) -> Arc<Self> {
        Self::build(transport, declared, source, limits, cap, false)
    }

    /// As [`declared`](Self::declared), for a modal submit: its
    /// `show_modal` is `ModalUnavailable` without a request, since a modal
    /// never answers a modal (INT-04, INT-05).
    pub(crate) fn declared_submit(
        transport: Transport,
        declared: Declared,
        source: Option<SourceMessage>,
        limits: Limits,
        cap: Option<u8>,
    ) -> Arc<Self> {
        Self::build(transport, declared, source, limits, cap, true)
    }

    fn build(
        transport: Transport,
        declared: Declared,
        source: Option<SourceMessage>,
        limits: Limits,
        cap: Option<u8>,
        no_modal: bool,
    ) -> Arc<Self> {
        let declared_ephemerality = match declared {
            Declared::Reply(e) => e,
            Declared::Update => Ephemerality::Public,
        };
        let source_nonces = source.as_ref().map_or(NonceSet::EMPTY, |s| s.nonces);
        Arc::new(Self {
            transport,
            state: AtomicU8::new(PENDING),
            gate: tokio::sync::Mutex::new(()),
            original: Mutex::new(Original {
                ephemerality: declared_ephemerality,
                ..Original::default()
            }),
            declared,
            source,
            limits,
            followups: AtomicU8::new(0),
            followup_cap: cap,
            guard_phase: AtomicU8::new(GuardPhase::UNGUARDED),
            source_nonces,
            drawn: Mutex::new(NonceSet::EMPTY),
            #[cfg(test)]
            fill: Mutex::new(None),
            no_modal,
            handler_answered: AtomicBool::new(false),
            i18n: OnceLock::new(),
        })
    }

    /// The state of one webhook send of the App behind `app` (see
    /// `crate::webhook`): its signer, `RefStore` and Limits; its strings
    /// in the App's default locale; visibility public. Only [`prepare`] and
    /// [`body`] are used on it.
    ///
    /// [`prepare`]: Self::prepare
    /// [`body`]: Self::body
    pub(crate) fn webhook(app: Arc<AppShared>) -> Arc<Self> {
        let limits = app.limits.clone();
        let strings = Arc::clone(&app.strings);
        let locales = LocaleCell::new(Locales::both(app.locales.default_locale()));
        let shared = Self::build(
            Transport::Webhook(app),
            Declared::Reply(Ephemerality::Public),
            None,
            limits,
            None,
            true,
        );
        shared.set_i18n(strings, locales);
        shared
    }

    /// Sets the strings and the locale cell that resolve the locale keys of
    /// this run's sends; the first call wins.
    pub(crate) fn set_i18n(&self, strings: Arc<dyn Translate>, locales: Arc<LocaleCell>) {
        let _ = self.i18n.set(I18n { strings, locales });
    }

    fn i18n(&self) -> &I18n {
        self.i18n.get_or_init(I18n::detached)
    }

    /// Who sees a send while nothing is known about the original: the
    /// response's ephemerality while Pending, else the original's (an edit
    /// keeps it).
    fn send_visibility(&self) -> Ephemerality {
        if self.state() == PENDING {
            self.response_ephemerality()
        } else {
            self.original().ephemerality
        }
    }

    /// Records the run's [`GuardPhase`].
    pub(crate) fn set_guard_phase(&self, phase: u8) {
        self.guard_phase.store(phase, Ordering::Release);
    }

    /// The run's [`GuardPhase`].
    pub(crate) fn guard_phase(&self) -> u8 {
        self.guard_phase.load(Ordering::Acquire)
    }

    /// `PENDING`, `DEFERRED`, `REPLIED`, `MODAL`, `DEFERRED_UPDATE` or
    /// `STOPPED`.
    pub(crate) fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    /// Who sees the handler's response, for its response locale (FR-43):
    /// the declared ephemerality, or under `response = update` the source
    /// message's (`Public` without one).
    pub(crate) fn response_ephemerality(&self) -> Ephemerality {
        match (self.declared, &self.source) {
            (Declared::Reply(ephemerality), _) => ephemerality,
            (Declared::Update, Some(source)) => source.ephemerality,
            (Declared::Update, None) => Ephemerality::Public,
        }
    }

    /// Whether the handler itself took the initial response (INT-03).
    pub(crate) fn answered(&self) -> bool {
        self.handler_answered.load(Ordering::Acquire)
    }

    /// Records that the handler took the initial response; the caller
    /// holds the gate.
    fn mark_answered(&self) {
        self.handler_answered.store(true, Ordering::Release);
    }

    /// [`ResponderError::AlreadyResponded`] once the handler answered.
    fn refuse_answered(&self) -> Result<(), ResponderError> {
        if self.answered() {
            Err(ResponderError::AlreadyResponded)
        } else {
            Ok(())
        }
    }

    /// The ephemerality of `reply` and `defer_reply`: the declared one, or
    /// `Public` under `response = update`.
    fn default_ephemerality(&self) -> Ephemerality {
        match self.declared {
            Declared::Reply(e) => e,
            Declared::Update => Ephemerality::Public,
        }
    }

    fn original(&self) -> Original {
        self.original
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_original(&self, original: Original) {
        *self.original.lock().unwrap_or_else(PoisonError::into_inner) = original;
    }

    /// The original after an `update`/`defer_update` of `source`: the
    /// clicked message, now in `format` and holding `files`.
    fn set_source_original(
        &self,
        source: &SourceMessage,
        format: MessageFormat,
        files: KnownFiles,
    ) {
        self.set_original(Original {
            shape: Shape::Sent(format),
            message_id: Some(source.id),
            ephemerality: source.ephemerality,
            channel_id: Some(source.channel_id),
            files,
        });
    }

    /// The interaction's `attachment_size_limit` (ATT-01); `None` off the
    /// interaction transport.
    fn file_limit(&self) -> Option<usize> {
        match &self.transport {
            Transport::Interaction(target) => target.file_limit,
            Transport::Message(_) | Transport::Detached | Transport::Webhook(_) => None,
        }
    }

    /// Builds one send of the prepared `msg`: `json` is its message object
    /// (callback `data`, a follow-up, or a Create or Edit body), `known`
    /// the edit target's attachments (`None` for a new message) and
    /// `callback` the callback type that wraps it as `{"type", "data"}`.
    /// It places the `attachments` entries (FR-30, MSG-08, ATT-04), then
    /// checks the Upload Budget (ATT-01..03) once, before any request, in
    /// every mode (also when detached).
    ///
    /// # Errors
    ///
    /// [`ResponderError::Invalid`] over the Upload Budget,
    /// [`ResponderError::Encode`] when the JSON does not serialize.
    pub(crate) fn body(
        &self,
        msg: &OutgoingMessage,
        mut json: Value,
        known: Option<&KnownFiles>,
        callback: Option<u8>,
    ) -> Result<Body, ResponderError> {
        let files = Files::plan(msg, known);
        files.place(&mut json, known.is_some());
        let payload = match callback {
            Some(kind) => json!({ "type": kind, "data": json }),
            None => json,
        };
        files.finish(&payload, self.file_limit(), &self.limits)
    }

    /// The App's payload key ring, the signer of `signed` Routes; never
    /// handed anywhere else (AD-29).
    fn signer(&self) -> Option<&dyn Sign> {
        let app = match &self.transport {
            Transport::Interaction(target) => &target.app,
            Transport::Message(target) => &target.app,
            Transport::Webhook(app) => app,
            Transport::Detached => return None,
        };
        app.payload_keys.as_deref().map(|keys| keys as &dyn Sign)
    }

    /// The App's namespaced `RefStore` and default Ref TTL, where minted
    /// Refs are stored; `None` on the detached transport, which sends
    /// nothing and so stores nothing.
    fn ref_target(&self) -> Option<(&Arc<dyn RefStore>, Duration)> {
        let app = match &self.transport {
            Transport::Interaction(target) => &target.app,
            Transport::Message(target) => &target.app,
            Transport::Webhook(app) => app,
            Transport::Detached => return None,
        };
        Some((&app.refs, app.ref_ttl))
    }

    /// One select nonce for a send, outside the interaction's nonces and
    /// the run's last eight draws, which it then joins (FR-25).
    fn draw_nonce(&self) -> Result<SelectNonce, ResponderError> {
        let mut drawn = self.drawn.lock().unwrap_or_else(PoisonError::into_inner);
        let avoid = [&self.source_nonces, &*drawn];
        #[cfg(test)]
        let nonce = {
            let mut fill = self.fill.lock().unwrap_or_else(PoisonError::into_inner);
            match fill.as_mut() {
                Some(fill) => nonce::draw_nonce(&avoid, fill)?,
                None => nonce::draw_nonce(&avoid, nonce::fill_os)?,
            }
        };
        #[cfg(not(test))]
        let nonce = nonce::draw_nonce(&avoid, nonce::fill_os)?;
        drawn.insert(nonce);
        Ok(nonce)
    }

    /// Whether a draw must avoid `nonce`.
    #[cfg(test)]
    pub(crate) fn avoids(&self, nonce: SelectNonce) -> bool {
        self.source_nonces.contains(nonce)
            || self
                .drawn
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(nonce)
    }

    /// Replaces the OS CSPRNG of the nonce draw with `fill`.
    #[cfg(test)]
    pub(crate) fn script_fill(&self, fill: ScriptedFill) {
        *self.fill.lock().unwrap_or_else(PoisonError::into_inner) = Some(fill);
    }

    /// Resolves every unresolved Route `custom_id` for this send: one
    /// select nonce for the whole message, drawn only when a select needs
    /// it (never a nonce of the interaction nor one of the run's last eight
    /// sends), the signer only for `signed` Routes, and a minted Ref for every Ref slot (AD-18). Equal slots
    /// share one Ref; each distinct one is stored in the App's `RefStore`
    /// before anything is sent. An already-resolved id is kept.
    ///
    /// On an error nothing is sent; Refs already stored stay until their
    /// TTL reaps them.
    async fn resolve_ids<'a>(
        &self,
        ids: impl Iterator<Item = &'a mut CustomId>,
    ) -> Result<(), ResponderError> {
        let signer = self.signer();
        let mut nonce = None;
        let mut slots: Vec<(&'a mut CustomId, PendingRef)> = Vec::new();
        for id in ids {
            if id.get().is_some() {
                continue;
            }
            let nonce = match nonce {
                Some(nonce) => nonce,
                None if id.has_select() => *nonce.insert(self.draw_nonce()?),
                None => PLACEHOLDER_NONCE,
            };
            match id.resolve_for_send(nonce, signer) {
                Ok(None) => {}
                Ok(Some(slot)) => slots.push((id, slot)),
                Err(error) => {
                    return Err(ResponderError::Encode {
                        reason: format!("custom_id of route `{}`: {error}", id.route()),
                    });
                }
            }
        }
        if slots.is_empty() {
            return Ok(());
        }
        let target = self.ref_target();
        let mut minted: HashMap<PendingRef, Ref> = HashMap::new();
        for (id, slot) in slots {
            let r = if let Some(&r) = minted.get(&slot) {
                r
            } else {
                let r = mint_ref(&slot, target).await?;
                minted.insert(slot.clone(), r);
                r
            };
            id.finish(&slot, r)
                .map_err(|error| ResponderError::Encode {
                    reason: format!("custom_id of route `{}`: {error}", slot.route()),
                })?;
        }
        Ok(())
    }

    /// Resolves the locale keys of `msg` for a send seen as `visibility`
    /// (FR-43), mints its Route `custom_id`s (storing their Refs), then
    /// validates it against the Limits (Lenient mode truncates). The
    /// Upload Budget is checked later, when the body is built.
    pub(crate) async fn prepare(
        &self,
        msg: &mut OutgoingMessage,
        visibility: Ephemerality,
    ) -> Result<(), ResponderError> {
        let i18n = self.i18n();
        let mut text = |key: MessageKey| i18n.text(key, visibility);
        let report = match msg {
            OutgoingMessage::V2(m) => {
                m.resolve_keys(&mut text);
                self.resolve_ids(m.custom_ids_mut()).await?;
                noctis_ui::validate(m, &self.limits)?
            }
            OutgoingMessage::Legacy(m) => {
                m.resolve_keys(&mut text);
                self.resolve_ids(m.custom_ids_mut()).await?;
                noctis_ui::validate(m, &self.limits)?
            }
        };
        for warning in &report.warnings {
            tracing::debug!(target: "noctis::responder", ?warning, "message adjusted to fit");
        }
        Ok(())
    }

    /// The bot-token route for a message of `visibility` once the
    /// interaction token expired (INT-02): `Ok(None)` while the token is
    /// valid (or off the interaction transport), the route for a public
    /// message whose id and channel are known, else
    /// [`ResponderError::TokenExpired`].
    fn expired_route(
        &self,
        visibility: Visibility,
        message_id: Option<Id<MessageMarker>>,
        channel_id: Option<Id<ChannelMarker>>,
    ) -> Result<Option<BotRoute<'_>>, ResponderError> {
        let Transport::Interaction(target) = &self.transport else {
            return Ok(None);
        };
        if !target.expired() {
            return Ok(None);
        }
        match (visibility, message_id, channel_id.or(target.channel_id)) {
            (Ephemerality::Public, Some(id), Some(channel_id)) => {
                Ok(Some((&*target.app, channel_id, id)))
            }
            _ => Err(ResponderError::TokenExpired),
        }
    }

    /// Sends a returned message (INT-12): while Pending, an `update` of the
    /// source message under `response = update` (a type-4 public reply when
    /// there is none, a slash-opened modal), else a type-4 reply with the
    /// declared ephemerality; after an acknowledgement, an edit of the
    /// original. The gate decides.
    pub(crate) async fn finish(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
    ) -> Result<(), ResponderError> {
        self.prepare(&mut msg, self.send_visibility()).await?;
        let gate = self.gate.lock().await;
        if self.state() != PENDING {
            drop(gate);
            return self.edit_prepared(msg).await;
        }
        match (self.declared, self.source.as_ref()) {
            (Declared::Update, Some(source)) => {
                self.send_update(&msg, source, false).await.map(drop)
            }
            (Declared::Update, None) => self
                .send_reply(&msg, Ephemerality::Public, false)
                .await
                .map(drop),
            (Declared::Reply(ephemerality), _) => {
                self.send_reply(&msg, ephemerality, false).await.map(drop)
            }
        }
    }

    /// The initial reply with `ephemerality`; when the interaction is
    /// already acknowledged, an edit of the reply (INT-03, INT-10) or, after
    /// a type-6 defer, a follow-up of `ephemerality`. With `with_response`,
    /// the resource of the callback, `None` when none was sent.
    async fn reply(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
        ephemerality: Ephemerality,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        self.refuse_answered()?;
        // A reply (or, after a type-6 defer, a follow-up) has
        // `ephemerality`; an edit of a deferred reply keeps the defer's.
        let visibility = match self.state() {
            PENDING | DEFERRED_UPDATE => ephemerality,
            _ => self.original().ephemerality,
        };
        self.prepare(&mut msg, visibility).await?;
        let _gate = self.gate.lock().await;
        self.refuse_answered()?;
        let sent = match self.state() {
            PENDING => self.send_reply(&msg, ephemerality, with_response).await?,
            DEFERRED_UPDATE => {
                // The original is the clicked message: a reply is a new
                // message, never an edit of it.
                self.send_follow_up(msg, ephemerality).await.map(|_| None)?
            }
            _ => {
                // The auto-defer (or a defer) answered first (INT-03); the
                // edit decides by the acknowledgement's ephemerality.
                self.edit_prepared(msg).await.map(|()| None)?
            }
        };
        self.mark_answered();
        Ok(sent)
    }

    /// Sends the type-4 reply of the prepared `msg`; the caller holds the
    /// gate and saw `PENDING`.
    async fn send_reply(
        &self,
        msg: &OutgoingMessage,
        ephemerality: Ephemerality,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        if self.transport.is_message() && ephemerality == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        let mut ack_body = None;
        let (message_id, files) = if let Transport::Message(target) = &self.transport {
            let mut json = create(msg)?;
            target.reference(&mut json);
            let body = self.body(msg, json, None, None)?;
            let (id, files) = target.reply(body.payload).await?;
            (Some(id), files.unwrap_or(body.kept))
        } else {
            // Encoded (and budget-checked) even when detached, so the seams
            // catch encode and Limits errors.
            let data = callback_data(msg, ephemerality.callback_flags())?;
            let body = self.body(msg, data, None, Some(4))?;
            match &self.transport {
                Transport::Interaction(target) => {
                    let ack = target.callback(body.payload, true).await?;
                    let posted = transport::callback_posted(&ack);
                    ack_body = Some(ack);
                    (posted.message_id, posted.files.unwrap_or(body.kept))
                }
                Transport::Message(_) | Transport::Detached | Transport::Webhook(_) => {
                    (None, body.kept)
                }
            }
        };
        self.state.store(REPLIED, Ordering::Release);
        self.set_original(Original {
            shape: Shape::Sent(msg.format()),
            message_id,
            ephemerality,
            channel_id: None,
            files,
        });
        resource_of(ack_body.as_deref(), with_response)
    }

    async fn defer_reply(
        &self,
        ephemerality: Ephemerality,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        self.refuse_answered()?;
        if self.transport.is_message() && ephemerality == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        let _gate = self.gate.lock().await;
        self.refuse_answered()?;
        if self.state() != PENDING {
            tracing::debug!(target: "noctis::responder", "already acknowledged: defer_reply sends nothing");
            self.mark_answered();
            return Ok(None);
        }
        let sent = self.send_defer(ephemerality, with_response).await?;
        self.mark_answered();
        Ok(sent)
    }

    /// Sends the type-5 defer of `ephemerality`; the caller holds the gate
    /// and saw `PENDING`.
    async fn send_defer(
        &self,
        ephemerality: Ephemerality,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        let mut ack = None;
        match &self.transport {
            Transport::Interaction(target) => {
                let body = match ephemerality {
                    Ephemerality::Public => json!({ "type": 5 }),
                    Ephemerality::Ephemeral => {
                        json!({ "type": 5, "data": { "flags": CallbackFlags::EPHEMERAL.bits() } })
                    }
                };
                ack = Some(target.callback_json(&body, with_response).await?);
            }
            Transport::Message(target) => target.typing().await?,
            Transport::Detached | Transport::Webhook(_) => {}
        }
        self.state.store(DEFERRED, Ordering::Release);
        self.set_original(Original {
            shape: Shape::Deferred,
            message_id: None,
            ephemerality,
            channel_id: None,
            files: KnownFiles::EMPTY,
        });
        resource_of(ack.as_deref(), with_response)
    }

    /// `update`: a type-7 callback while Pending, else an edit of the
    /// original; [`ResponderError::UpdateUnavailable`] without a source
    /// message (no request, the state unchanged).
    async fn update(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        self.refuse_answered()?;
        let Some(source) = self.source.as_ref() else {
            return Err(ResponderError::UpdateUnavailable);
        };
        let visibility = if self.state() == PENDING {
            source.ephemerality
        } else {
            self.original().ephemerality
        };
        self.prepare(&mut msg, visibility).await?;
        let _gate = self.gate.lock().await;
        self.refuse_answered()?;
        let sent = if self.state() == PENDING {
            self.send_update(&msg, source, with_response).await?
        } else {
            self.edit_prepared(msg).await.map(|()| None)?
        };
        self.mark_answered();
        Ok(sent)
    }

    /// Sends the type-7 update of `source` with the prepared `msg`, keeping
    /// the source's attachments `msg` still uses by id (MSG-08); an update
    /// that uploads a file is sent `with_response`, so the new ids are
    /// learned. The caller holds the gate and saw `PENDING`.
    async fn send_update(
        &self,
        msg: &OutgoingMessage,
        source: &SourceMessage,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        let data = edit_body(msg, source.format)?;
        let body = self.body(msg, data, Some(&source.files), Some(7))?;
        let (ack, files) = match &self.transport {
            Transport::Interaction(target) => {
                let ask = with_response || body.uploads();
                let ack = target.callback(body.payload, ask).await?;
                let files = transport::callback_posted(&ack).files.unwrap_or(body.kept);
                (Some(ack), files)
            }
            Transport::Message(_) | Transport::Detached | Transport::Webhook(_) => {
                (None, body.kept)
            }
        };
        self.state.store(REPLIED, Ordering::Release);
        self.set_source_original(source, msg.format(), files);
        resource_of(ack.as_deref(), with_response)
    }

    /// `defer_update`: a type-6 callback while Pending, nothing after an
    /// acknowledgement; [`ResponderError::UpdateUnavailable`] without a
    /// source message.
    async fn defer_update(
        &self,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        self.refuse_answered()?;
        let Some(source) = self.source.as_ref() else {
            return Err(ResponderError::UpdateUnavailable);
        };
        let _gate = self.gate.lock().await;
        self.refuse_answered()?;
        if self.state() != PENDING {
            tracing::debug!(target: "noctis::responder", "already acknowledged: defer_update sends nothing");
            self.mark_answered();
            return Ok(None);
        }
        let sent = self.send_defer_update(source, with_response).await?;
        self.mark_answered();
        Ok(sent)
    }

    /// Sends the type-6 defer; the caller holds the gate and saw `PENDING`.
    async fn send_defer_update(
        &self,
        source: &SourceMessage,
        with_response: bool,
    ) -> Result<Option<CallbackResource>, ResponderError> {
        let ack = match &self.transport {
            Transport::Interaction(target) => Some(
                target
                    .callback_json(&json!({ "type": 6 }), with_response)
                    .await?,
            ),
            Transport::Message(_) | Transport::Detached | Transport::Webhook(_) => None,
        };
        self.state.store(DEFERRED_UPDATE, Ordering::Release);
        self.set_source_original(source, source.format, source.files.clone());
        resource_of(ack.as_deref(), with_response)
    }

    /// The auto-defer (INT-01): while Pending, a type-6 callback when the
    /// handler declared `response = update` and a source message exists,
    /// else a type-5 callback with the declared ephemerality (`Public`
    /// under `update`). `false` when that callback failed (the state stays
    /// Pending; the caller logs it).
    pub(crate) async fn auto_defer(&self) -> bool {
        let Transport::Interaction(target) = &self.transport else {
            return true;
        };
        let _gate = self.gate.lock().await;
        if self.state() != PENDING {
            return true;
        }
        let sent = match (self.declared, self.source.as_ref()) {
            (Declared::Update, Some(source)) => self.send_defer_update(source, false).await,
            _ => self.send_defer(self.default_ephemerality(), false).await,
        };
        if sent.is_err() {
            return false;
        }
        metrics::counter!("noctis_autodefer_total", "app" => Arc::clone(&target.app.app))
            .increment(1);
        tracing::debug!(target: "noctis::responder", "auto-deferred at the ack deadline");
        true
    }

    async fn show_modal(&self, mut modal: Modal) -> Result<(), ResponderError> {
        if self.no_modal || self.transport.is_message() {
            return Err(ResponderError::ModalUnavailable);
        }
        self.refuse_answered()?;
        // A modal is seen by the user alone: User Locale first.
        let i18n = self.i18n();
        modal.resolve_keys(&mut |key| i18n.text(key, Ephemerality::Ephemeral));
        self.resolve_ids(std::iter::once(modal.custom_id_mut()))
            .await?;
        let report = noctis_ui::validate(&mut modal, &self.limits)?;
        for warning in &report.warnings {
            tracing::debug!(target: "noctis::responder", ?warning, "modal adjusted to fit");
        }
        let data = serde_json::to_value(&modal).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        let _gate = self.gate.lock().await;
        self.refuse_answered()?;
        if self.state() != PENDING {
            return Err(ResponderError::ModalUnavailable);
        }
        if let Transport::Interaction(target) = &self.transport {
            target
                .callback_json(&json!({ "type": 9, "data": data }), false)
                .await?;
        }
        self.state.store(MODAL, Ordering::Release);
        self.mark_answered();
        Ok(())
    }

    async fn edit_reply(&self, mut msg: OutgoingMessage) -> Result<(), ResponderError> {
        self.prepare(&mut msg, self.original().ephemerality).await?;
        self.edit_prepared(msg).await
    }

    /// Edits the original into the prepared `msg`.
    async fn edit_prepared(&self, msg: OutgoingMessage) -> Result<(), ResponderError> {
        let original = self.original();
        // After a defer, Discord's loading message needs no legacy clears.
        let format = match original.shape {
            Shape::Sent(format) => format,
            Shape::Unsent | Shape::Deferred => msg.format(),
        };
        let mut channel_id = original.channel_id;
        let (message_id, files) = match (&self.transport, original.message_id) {
            (Transport::Message(target), None) => {
                // A deferred message reply has no message yet: post it,
                // unless the reply is ephemeral.
                if original.ephemerality == Ephemerality::Ephemeral {
                    return Err(ResponderError::EphemeralUnsupported);
                }
                let mut json = create(&msg)?;
                target.reference(&mut json);
                let body = self.body(&msg, json, None, None)?;
                let (id, files) = target.reply(body.payload).await?;
                (Some(id), files.unwrap_or(body.kept))
            }
            (transport, message_id) => {
                let json = edit_body(&msg, format)?;
                let body = self.body(&msg, json, Some(&original.files), None)?;
                let posted = match self.expired_route(
                    original.ephemerality,
                    message_id,
                    original.channel_id,
                )? {
                    Some((app, channel, id)) => {
                        transport::channel_edit(app, channel, id, body.payload).await?
                    }
                    None => transport.edit_reply(message_id, body.payload).await?,
                };
                channel_id = posted.channel_id.or(channel_id);
                (
                    posted.message_id.or(message_id),
                    posted.files.unwrap_or(body.kept),
                )
            }
        };
        self.set_original(Original {
            shape: Shape::Sent(msg.format()),
            message_id,
            ephemerality: original.ephemerality,
            channel_id,
            files,
        });
        Ok(())
    }

    /// Fetches the reply message (see [`Responder::fetch_reply`]); a
    /// public reply's id and channel are kept when they were unknown.
    async fn fetch_reply(&self) -> Result<Message, ResponderError> {
        if let Transport::Interaction(target) = &self.transport {
            target.token()?;
        }
        let original = self.original();
        let message = match self.expired_route(
            original.ephemerality,
            original.message_id,
            original.channel_id,
        )? {
            Some((app, channel, id)) => transport::channel_fetch(app, channel, id).await?,
            None => self.transport.fetch_reply(original.message_id).await?,
        };
        if original.ephemerality == Ephemerality::Public {
            let mut stored = self.original.lock().unwrap_or_else(PoisonError::into_inner);
            stored.message_id = stored.message_id.or(Some(message.id));
            stored.channel_id = stored.channel_id.or(Some(message.channel_id));
        }
        Ok(message)
    }

    async fn follow_up(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
        visibility: Visibility,
    ) -> Result<FollowupHandle, ResponderError> {
        if self.transport.is_message() && visibility == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        self.prepare(&mut msg, visibility).await?;
        self.send_follow_up(msg, visibility).await
    }

    /// Sends the prepared `msg` as a follow-up of `visibility`.
    async fn send_follow_up(
        self: &Arc<Self>,
        msg: OutgoingMessage,
        visibility: Visibility,
    ) -> Result<FollowupHandle, ResponderError> {
        if self.transport.is_message() && visibility == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        let json = match &self.transport {
            Transport::Message(_) => create(&msg)?,
            _ => callback_data(&msg, visibility.callback_flags())?,
        };
        let body = self.body(&msg, json, None, None)?;
        if let Transport::Interaction(target) = &self.transport
            && target.expired()
        {
            return Err(ResponderError::TokenExpired);
        }
        if let Some(cap) = self.followup_cap {
            // A slot is never refunded: a rejected request may still have
            // counted at Discord (INT-11).
            self.followups
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    (n < cap).then_some(n + 1)
                })
                .map_err(|_| ResponderError::FollowupCap)?;
        }
        let posted = self.transport.follow_up(body.payload).await?;
        Ok(FollowupHandle {
            shared: Arc::clone(self),
            message_id: posted.id,
            channel_id: posted.channel_id,
            visibility,
            format: Mutex::new(msg.format()),
            files: Mutex::new(posted.files.unwrap_or(body.kept)),
        })
    }

    /// Sends the localized notice of a failed handler run or a rejected
    /// interaction (FR-22, AD-23, CID-06): one V2 message holding a single
    /// `TextDisplay(text)`, validated like every other send.
    ///
    /// - Pending: the initial response, a type-4 callback flagged
    ///   `EPHEMERAL` whatever the handler declared; over a message
    ///   transport a public reply to the invoking message.
    /// - Deferred (type 5): the deferred original is deleted first (a
    ///   failure is a `warn` and the notice is still sent), then the notice
    ///   is an ephemeral follow-up; over a message transport a public
    ///   follow-up.
    /// - Deferred with type 6, or Replied: an ephemeral follow-up (public
    ///   over a message transport); the component's message is never
    ///   deleted.
    /// - Modal: nothing is sent (`debug` log).
    ///
    /// The follow-up cap and the token expiry apply (INT-11, INT-02).
    pub(crate) async fn notice(self: &Arc<Self>, text: String) -> Result<(), ResponderError> {
        let mut msg = OutgoingMessage::V2(V2Message::new(
            Components::new().push(TextDisplay::new(text)),
        ));
        self.prepare(&mut msg, Ephemerality::Ephemeral).await?;
        let visibility = if self.transport.is_message() {
            Ephemerality::Public
        } else {
            Ephemerality::Ephemeral
        };
        let gate = self.gate.lock().await;
        match self.state() {
            PENDING => {
                let message_id = match &self.transport {
                    Transport::Interaction(target) => {
                        let data = callback_data(&msg, Ephemerality::Ephemeral.callback_flags())?;
                        let body = self.body(&msg, data, None, Some(4))?;
                        let ack = target.callback(body.payload, false).await?;
                        transport::callback_posted(&ack).message_id
                    }
                    Transport::Message(target) => {
                        let mut json = create(&msg)?;
                        target.reference(&mut json);
                        let body = self.body(&msg, json, None, None)?;
                        Some(target.reply(body.payload).await?.0)
                    }
                    Transport::Detached | Transport::Webhook(_) => None,
                };
                self.state.store(REPLIED, Ordering::Release);
                self.set_original(Original {
                    shape: Shape::Sent(msg.format()),
                    message_id,
                    ephemerality: visibility,
                    channel_id: None,
                    files: KnownFiles::EMPTY,
                });
                drop(gate);
                Ok(())
            }
            DEFERRED => {
                drop(gate);
                if let Transport::Interaction(_) = &self.transport
                    && matches!(self.original().shape, Shape::Deferred)
                    && let Err(error) = self.delete_reply().await
                {
                    tracing::warn!(
                        target: "noctis::dispatch",
                        %error,
                        "could not delete the deferred original before the notice"
                    );
                }
                self.send_follow_up(msg, visibility).await.map(|_| ())
            }
            REPLIED | DEFERRED_UPDATE => {
                drop(gate);
                self.send_follow_up(msg, visibility).await.map(|_| ())
            }
            _ => {
                drop(gate);
                tracing::debug!(target: "noctis::dispatch", "no notice after a modal");
                Ok(())
            }
        }
    }

    /// Ends a run a before-Middleware stopped with `Flow::Stop(None)`: it
    /// sends nothing, except that a type-5 deferred original ("thinking…")
    /// over an interaction is deleted (a failure is a `warn`).
    pub(crate) async fn stop_silently(&self) {
        // The gate waits for an in-flight auto-defer; a still-pending
        // state is closed so a later auto-defer sends nothing.
        let state = {
            let _gate = self.gate.lock().await;
            let state = self.state();
            if state == PENDING {
                self.state.store(STOPPED, Ordering::Release);
            }
            state
        };
        if state == DEFERRED
            && let Transport::Interaction(_) = &self.transport
            && matches!(self.original().shape, Shape::Deferred)
            && let Err(error) = self.delete_reply().await
        {
            tracing::warn!(
                target: "noctis::dispatch",
                %error,
                "could not delete the deferred original after a Middleware stop"
            );
        }
    }

    async fn delete_reply(&self) -> Result<(), ResponderError> {
        let original = self.original();
        if let Some((app, channel, id)) = self.expired_route(
            original.ephemerality,
            original.message_id,
            original.channel_id,
        )? {
            return transport::channel_delete(app, channel, id).await;
        }
        match (&self.transport, original.message_id) {
            (Transport::Message(_), None) => Ok(()),
            (transport, message_id) => transport.delete_reply(message_id).await,
        }
    }
}

/// How many Refs a slot may mint before [`ResponderError::RefCollision`].
const MINT_ATTEMPTS: usize = 3;

/// Mints the Ref of `slot` and stores its record under it in `target`
/// (`put_if_absent`, with the Route's TTL or the App's), retrying on a
/// collision; with no target (detached) the Ref is only minted.
async fn mint_ref(
    slot: &PendingRef,
    target: Option<(&Arc<dyn RefStore>, Duration)>,
) -> Result<Ref, ResponderError> {
    let mint = || {
        Ref::mint().map_err(|error| ResponderError::Encode {
            reason: format!(
                "no Ref could be minted for route `{}`: {error}",
                slot.route()
            ),
        })
    };
    let Some((refs, default_ttl)) = target else {
        return mint();
    };
    let ttl = slot.ttl().unwrap_or(default_ttl);
    for _ in 0..MINT_ATTEMPTS {
        let r = mint()?;
        if refs
            .put_if_absent(r.as_str(), slot.record(), Some(ttl))
            .await
            .map_err(|error| ResponderError::RefStore(redact_ref(error)))?
        {
            return Ok(r);
        }
    }
    Err(ResponderError::RefCollision)
}

/// Replaces the namespaced key an [`PortError::InvalidValue`] carries
/// (`<app>:ref:<ref>`) with a fixed placeholder, so the error's Display and
/// source chain never print the Ref.
fn redact_ref(error: PortError) -> PortError {
    match error {
        PortError::InvalidValue { why, .. } => PortError::invalid_value("<ref>", why),
        other => other,
    }
}

/// The callback resource of an acknowledgement body when it was asked for
/// (`None` when no callback was sent).
fn resource_of(
    body: Option<&[u8]>,
    with_response: bool,
) -> Result<Option<CallbackResource>, ResponderError> {
    match (body, with_response) {
        (Some(body), true) => CallbackResource::parse(body).map(Some),
        _ => Ok(None),
    }
}

/// The JSON of a message body, or [`ResponderError::Encode`] (an
/// unresolved Route `custom_id`).
pub(crate) fn encode(body: MessageBody<'_>) -> Result<Value, ResponderError> {
    serde_json::to_value(body).map_err(|e| ResponderError::Encode {
        reason: e.to_string(),
    })
}

/// The callback `data` (or follow-up body) of `msg` with `flags`.
fn callback_data(msg: &OutgoingMessage, flags: CallbackFlags) -> Result<Value, ResponderError> {
    match msg {
        OutgoingMessage::V2(m) => encode(m.callback_body(flags)),
        OutgoingMessage::Legacy(m) => encode(m.callback_body(flags)),
    }
}

/// The body editing a message in `format` into the prepared `msg`, before
/// [`Shared::body`] adds its attachments keep-list; a V2 → legacy edit is
/// [`UiError::V2ToLegacy`].
fn edit_body(msg: &OutgoingMessage, format: MessageFormat) -> Result<Value, ResponderError> {
    match msg {
        OutgoingMessage::V2(m) => encode(m.edit_body(format, EditFlags::empty())),
        OutgoingMessage::Legacy(m) => encode(m.edit_body(format, EditFlags::empty())?),
    }
}

/// The Create Message body of `msg`.
fn create(msg: &OutgoingMessage) -> Result<Value, ResponderError> {
    match msg {
        OutgoingMessage::V2(m) => encode(m.create_body(CreateFlags::empty())),
        OutgoingMessage::Legacy(m) => encode(m.create_body(CreateFlags::empty())),
    }
}
