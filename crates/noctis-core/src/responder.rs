//! The typestate [`Responder`]: how a handler answers its interaction
//! (FR-21, FR-61, AD-11, AD-24; INT-01, INT-02, INT-03, INT-04, INT-05,
//! INT-08, INT-09, INT-10, INT-11, INT-12, CV2-01, CV2-02).
//!
//! A [`CommandResponder`] starts [`Pending`]: [`reply`](Responder::reply)
//! or [`defer`](Responder::defer) (and [`modal`](Responder::modal) for an
//! `opens_modal` Command) consume it and give a
//! [`Responder<K, Acked>`](Responder), which only edits the original,
//! sends follow-ups or deletes the original, and ends with
//! [`done`](Responder::done). A second initial response, an `update`, a
//! `modal` without `opens_modal` or a flag set on `defer` do not compile.
//!
//! At runtime one gate serializes every initial response, so at most one
//! interaction callback is ever sent (INT-03): when the auto-defer
//! answered first, a later `reply` becomes an edit of the original. After
//! 15 minutes the interaction token is expired (INT-02): edits and deletes
//! of public messages fall back to the bot token, everything else is
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
//! ```compile_fail,E0599
//! use noctis_core::{CommandResponder, HandlerResult, Responded};
//! use noctis_ui::Modal;
//!
//! async fn modal(r: CommandResponder, m: Modal) -> HandlerResult<Responded> {
//!     Ok(r.modal(m).await?.done()) // [INT-05]: only `CommandResponder<OpensModal>`
//! }
//! ```
//!
//! ```compile_fail,E0308
//! use noctis_core::{CommandResponder, HandlerResult, Responded};
//! use noctis_ui::CallbackFlags;
//!
//! async fn defer(r: CommandResponder) -> HandlerResult<Responded> {
//!     Ok(r.defer(CallbackFlags::EPHEMERAL).await?.done()) // [INT-08]
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
//!     acked.edit_original(m(), Ephemerality::Ephemeral).await?; // [INT-10]
//!     Ok(acked.done())
//! }
//! ```

mod transport;

#[cfg(test)]
pub(crate) mod tests;

use std::fmt;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use noctis_rules::Limits;
use noctis_ui::{
    CallbackFlags, Components, CreateFlags, EditFlags, LegacyMessage, MessageBody, MessageFormat,
    Modal, TextDisplay, UiError, V2Message,
};
use serde_json::{Value, json};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

pub(crate) use self::transport::{InteractionTarget, MessageTarget, Transport};
use crate::AckDeadlineMissed;
use crate::handler::AppShared;

pub(crate) mod sealed {
    /// Seals [`ResponderKind`](super::ResponderKind).
    pub trait Kind {}
    /// Seals [`ResponderState`](super::ResponderState).
    pub trait State {}
    /// Seals the modal marker of [`Command`](super::Command).
    pub trait Modal: Send + Sync + 'static {}
    /// Seals [`IntoResponse`](super::IntoResponse).
    pub trait Response {}
}

/// The handler kind of a [`Responder`]: which Pending methods exist and
/// which values a handler may return (AD-11, INT-04). Sealed.
pub trait ResponderKind: sealed::Kind + Send + Sync + 'static {}

/// The state of a [`Responder`]: [`Pending`] or [`Acked`] (INT-03). Sealed.
pub trait ResponderState: sealed::State + Send + Sync + 'static {}

/// The modal marker of a [`Command`] or responder: [`NoModal`] or
/// [`OpensModal`]. Sealed.
pub trait ModalMarker: sealed::Modal {
    /// Whether the marker opens a modal.
    #[doc(hidden)]
    const OPENS_MODAL: bool;
}

/// The modal marker of a [`Command`] that does not open a modal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NoModal;

impl sealed::Modal for NoModal {}
impl ModalMarker for NoModal {
    const OPENS_MODAL: bool = false;
}

/// The modal marker of a Command declared `#[command(.., opens_modal)]`:
/// its handler takes `CommandResponder<OpensModal>`, the only responder
/// with [`modal`](Responder::modal), and no auto-defer is armed for it
/// (INT-05).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct OpensModal;

impl sealed::Modal for OpensModal {}
impl ModalMarker for OpensModal {
    const OPENS_MODAL: bool = true;
}

/// The [`ResponderKind`] of slash and context-menu Commands; `M` is the
/// modal marker ([`NoModal`] or [`OpensModal`]).
pub struct Command<M: ModalMarker = NoModal>(PhantomData<M>);

impl<M: ModalMarker> fmt::Debug for Command<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Command")
    }
}

impl<M: ModalMarker> sealed::Kind for Command<M> {}
impl<M: ModalMarker> ResponderKind for Command<M> {}

/// Implemented only by `CommandResponder<OpensModal>`: what `#[command(..,
/// opens_modal)]` asserts about its responder parameter (INT-05).
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-05] `opens_modal` is declared, so the handler must take `CommandResponder<OpensModal>`",
    label = "take `CommandResponder<OpensModal>` here"
)]
pub trait __DeclaresOpensModal {}

impl __DeclaresOpensModal for Responder<Command<OpensModal>, Pending> {}

/// Implemented only by `CommandResponder<NoModal>`: what `#[command]`
/// without `opens_modal` asserts about its responder parameter (INT-05).
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-05] this handler takes `CommandResponder<OpensModal>`, so its `#[command]` must declare `opens_modal`",
    label = "declare `opens_modal` or take `CommandResponder`"
)]
pub trait __DeclaresNoModal {}

impl __DeclaresNoModal for Responder<Command<NoModal>, Pending> {}

/// The `opens_modal` assertion `#[command]` emits (INT-05).
#[doc(hidden)]
pub fn __assert_opens_modal<T: __DeclaresOpensModal>() {}

/// The assertion `#[command]` emits without `opens_modal` (INT-05).
#[doc(hidden)]
pub fn __assert_no_modal<T: __DeclaresNoModal>() {}

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
/// A Command declares it once (`#[command(.., ephemeral)]`) and it never
/// changes afterwards (INT-10): `reply` and a returned message use the
/// declared value, [`defer`](Responder::defer) takes one explicitly and
/// [`followup`](Responder::followup) takes a [`Visibility`]. No edit takes
/// one.
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
    /// The message breaks a Discord Limit (Strict mode), or an edit would
    /// turn a V2 message back into a legacy one; nothing was sent.
    #[error(transparent)]
    Invalid(#[from] UiError),
    /// The message could not be serialized (for example an unresolved Route
    /// `custom_id`); nothing was sent.
    #[error("the message could not be encoded: {reason}")]
    Encode {
        /// The serializer's message.
        reason: String,
    },
    /// The message carries attachments, which arrive with Story 9.1;
    /// nothing was sent.
    #[error("attachments are not supported by the Responder yet")]
    AttachmentsUnsupported,
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
    /// The request did not get a usable response.
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
    /// A modal was not the first response to an interaction, or the
    /// transport has none (a [`Dispatcher`](crate::Dispatcher) message
    /// reply); nothing was sent.
    #[error("a modal can only be the first response to an interaction")]
    ModalUnavailable,
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
/// [`Components`], a [`LegacyMessage`] or the [`Responded`] from
/// [`done`](Responder::done). Sealed.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a Command response",
    label = "return a `V2Message`, `Components`, a `LegacyMessage`, or the `Responded` from `done()`"
)]
pub trait IntoResponse<K: ResponderKind>: sealed::Response + Send + 'static {
    /// The response the pipeline sends.
    #[doc(hidden)]
    fn __into_response(self) -> ResponseValue;
}

/// What a handler returned, as the pipeline sends it.
#[doc(hidden)]
#[derive(Debug)]
pub struct ResponseValue(pub(crate) Response);

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
        ResponseValue(Response::Done)
    }
}

impl<M: ModalMarker> IntoResponse<Command<M>> for V2Message {
    fn __into_response(self) -> ResponseValue {
        ResponseValue(Response::Message(self.into()))
    }
}

impl<M: ModalMarker> IntoResponse<Command<M>> for Components {
    fn __into_response(self) -> ResponseValue {
        ResponseValue(Response::Message(self.into()))
    }
}

impl<M: ModalMarker> IntoResponse<Command<M>> for LegacyMessage {
    fn __into_response(self) -> ResponseValue {
        ResponseValue(Response::Message(self.into()))
    }
}

/// How a handler answers its interaction; `K` is the handler kind and `S`
/// the state ([`Pending`] or [`Acked`]).
///
/// Take it as a handler parameter through its alias, [`CommandResponder`];
/// a handler that returns a message takes none. `Debug` never prints the
/// interaction token.
///
/// ```
/// use noctis_core::{CommandResponder, HandlerResult, Responded, Visibility};
/// use noctis_ui::{Components, TextDisplay, V2Message};
///
/// async fn warn(r: CommandResponder) -> HandlerResult<Responded> {
///     let text = |t: &str| V2Message::new(Components::new().push(TextDisplay::new(t.to_owned())));
///     let acked = r.reply(text("Warned.")).await?;
///     acked.followup(text("Case #12 logged."), Visibility::Public).await?;
///     Ok(acked.done())
/// }
/// ```
#[must_use = "a Responder sends nothing until it is used; return a message instead when you do not need it"]
pub struct Responder<K: ResponderKind, S: ResponderState = Pending> {
    shared: Arc<Shared>,
    marker: PhantomData<fn() -> (K, S)>,
}

/// The [`Responder`] of a slash or context-menu Command handler.
pub type CommandResponder<M = NoModal> = Responder<Command<M>, Pending>;

impl<K: ResponderKind, S: ResponderState> fmt::Debug for Responder<K, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Responder")
            .field("kind", &std::any::type_name::<K>())
            .field("state", &std::any::type_name::<S>())
            .field("shared", &self.shared)
            .finish()
    }
}

impl<K: ResponderKind> Responder<K, Pending> {
    /// A Pending responder over `shared`.
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            marker: PhantomData,
        }
    }
}

impl<K: ResponderKind, S: ResponderState> Responder<K, S> {
    fn acked(self) -> Responder<K, Acked> {
        Responder {
            shared: self.shared,
            marker: PhantomData,
        }
    }
}

impl<M: ModalMarker> Responder<Command<M>, Pending> {
    /// Answers with `msg` (callback type 4), with the Command's declared
    /// ephemerality; a reply to the invoking message over a
    /// [`Dispatcher`](crate::Dispatcher). When the auto-defer already
    /// acknowledged the interaction, `msg` edits the original instead
    /// (INT-03).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]: the message is invalid or Discord rejected it.
    /// The responder is consumed either way.
    pub async fn reply(
        self,
        msg: impl Into<OutgoingMessage>,
    ) -> Result<Responder<Command<M>, Acked>, ResponderError> {
        self.shared.reply(msg.into()).await?;
        Ok(self.acked())
    }

    /// Acknowledges now and answers later (callback type 5, whose only flag
    /// is `EPHEMERAL`, INT-08); a typing indicator over a
    /// [`Dispatcher`](crate::Dispatcher). Edit the original afterwards.
    /// After the auto-defer it sends nothing.
    ///
    /// # Errors
    ///
    /// A [`ResponderError`].
    pub async fn defer(
        self,
        ephemerality: Ephemerality,
    ) -> Result<Responder<Command<M>, Acked>, ResponderError> {
        self.shared.defer(ephemerality).await?;
        Ok(self.acked())
    }
}

impl Responder<Command<OpensModal>, Pending> {
    /// Opens `modal` (callback type 9); only an `opens_modal` Command has
    /// it, and no auto-defer is armed for one, so the modal stays the first
    /// response (INT-05). A modal leaves no original message to edit:
    /// return [`done`](Responder::done) afterwards.
    ///
    /// ```
    /// use noctis_core::{CommandResponder, HandlerResult, OpensModal, Responded};
    /// use noctis_ui::Modal;
    ///
    /// async fn feedback(r: CommandResponder<OpensModal>, form: Modal) -> HandlerResult<Responded> {
    ///     Ok(r.modal(form).await?.done())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ResponderError::Invalid`] (Limits) or [`ResponderError::Encode`]
    /// (an unresolved Route `custom_id`) before any request;
    /// [`ResponderError::ModalUnavailable`] over a
    /// [`Dispatcher`](crate::Dispatcher) or when the interaction is already
    /// answered; any other [`ResponderError`] from Discord.
    pub async fn modal(
        self,
        modal: Modal,
    ) -> Result<Responder<Command<OpensModal>, Acked>, ResponderError> {
        self.shared.modal(modal).await?;
        Ok(self.acked())
    }
}

impl<K: ResponderKind> Responder<K, Acked> {
    /// Replaces the original response with `msg`; its visibility does not
    /// change (INT-10). Editing a legacy original into a V2 message clears
    /// its legacy fields (CV2-02); the reverse is
    /// [`UiError::V2ToLegacy`]. After 15 minutes a public original whose
    /// id is known is edited with the bot token (INT-02).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes for an ephemeral original or one whose id is unknown.
    pub async fn edit_original(
        &self,
        msg: impl Into<OutgoingMessage>,
    ) -> Result<(), ResponderError> {
        self.shared.edit_original(msg.into()).await
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
    pub async fn followup(
        &self,
        msg: impl Into<OutgoingMessage>,
        visibility: Visibility,
    ) -> Result<FollowupHandle, ResponderError> {
        self.shared.followup(msg.into(), visibility).await
    }

    /// Deletes the original response (nothing to do when a
    /// [`Dispatcher`](crate::Dispatcher) handler has not replied yet).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes, as for [`edit_original`](Self::edit_original).
    pub async fn delete_original(&self) -> Result<(), ResponderError> {
        self.shared.delete_original().await
    }

    /// Ends the response; return the [`Responded`] from the handler.
    #[must_use = "return the `Responded` from the handler"]
    pub fn done(self) -> Responded {
        Responded { _private: () }
    }
}

/// One follow-up message sent with [`Responder::followup`].
#[must_use = "a FollowupHandle is the only way to edit or delete the follow-up"]
pub struct FollowupHandle {
    shared: Arc<Shared>,
    message_id: Id<MessageMarker>,
    channel_id: Option<Id<ChannelMarker>>,
    visibility: Visibility,
    format: Mutex<MessageFormat>,
}

impl FollowupHandle {
    /// The follow-up's message id.
    pub fn message_id(&self) -> Id<MessageMarker> {
        self.message_id
    }

    /// Replaces the follow-up with `msg`. After 15 minutes a public
    /// follow-up is edited with the bot token (INT-02).
    ///
    /// # Errors
    ///
    /// A [`ResponderError`]; [`ResponderError::TokenExpired`] after 15
    /// minutes for an ephemeral follow-up.
    pub async fn edit(&self, msg: impl Into<OutgoingMessage>) -> Result<(), ResponderError> {
        let mut msg = msg.into();
        let format = *self.format.lock().unwrap_or_else(PoisonError::into_inner);
        let body = self.shared.edit_body(&mut msg, format)?;
        match self
            .shared
            .expired_route(self.visibility, Some(self.message_id), self.channel_id)?
        {
            Some((app, channel_id, id)) => transport::channel_edit(app, channel_id, id, &body)
                .await
                .map(drop)?,
            None => {
                self.shared
                    .transport
                    .edit_message(self.message_id, body)
                    .await?;
            }
        }
        *self.format.lock().unwrap_or_else(PoisonError::into_inner) = msg.format();
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

/// The responder state (`Shared::state`).
pub(crate) const PENDING: u8 = 0;
pub(crate) const DEFERRED: u8 = 1;
pub(crate) const REPLIED: u8 = 2;
pub(crate) const MODAL: u8 = 3;

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
#[derive(Clone, Copy, Debug, Default)]
struct Original {
    shape: Shape,
    /// The original message, when known: the message reply, the
    /// `with_response` callback's message or the last edit's response.
    message_id: Option<Id<MessageMarker>>,
    /// Who sees it.
    ephemerality: Ephemerality,
    /// Its channel, when an edit response named it.
    channel_id: Option<Id<ChannelMarker>>,
}

/// The bot-token route of an expired interaction: the App, the channel and
/// the message.
type BotRoute<'a> = (&'a AppShared, Id<ChannelMarker>, Id<MessageMarker>);

/// What one interaction's responders and its pipeline share: the
/// transport, the state machine and its gate, the original response, the
/// declared ephemerality, the App's Limits and the follow-up cap.
pub(crate) struct Shared {
    transport: Transport,
    /// `PENDING`, `DEFERRED`, `REPLIED` or `MODAL`; written only under
    /// `gate`, read without it.
    state: AtomicU8,
    /// Serializes every initial response: the handler's and the
    /// auto-defer's (INT-03).
    gate: tokio::sync::Mutex<()>,
    /// Never locked across an `.await`.
    original: Mutex<Original>,
    ephemerality: Ephemerality,
    limits: Limits,
    /// Follow-up slots taken (INT-11).
    followups: AtomicU8,
    /// `Some(5)` for a user-install-only interaction.
    followup_cap: Option<u8>,
}

impl fmt::Debug for Shared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shared")
            .field("transport", &self.transport)
            .field("state", &self.state.load(Ordering::Acquire))
            .field("ephemerality", &self.ephemerality)
            .field("followup_cap", &self.followup_cap)
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
        Arc::new(Self {
            transport,
            state: AtomicU8::new(PENDING),
            gate: tokio::sync::Mutex::new(()),
            original: Mutex::new(Original {
                ephemerality,
                ..Original::default()
            }),
            ephemerality,
            limits,
            followups: AtomicU8::new(0),
            followup_cap: cap,
        })
    }

    /// `PENDING`, `DEFERRED`, `REPLIED` or `MODAL`.
    pub(crate) fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    fn original(&self) -> Original {
        *self.original.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_original(&self, original: Original) {
        *self.original.lock().unwrap_or_else(PoisonError::into_inner) = original;
    }

    /// Sends a returned message: the initial reply while Pending, else an
    /// edit of the original (INT-12); the gate decides.
    pub(crate) async fn finish(&self, msg: OutgoingMessage) -> Result<(), ResponderError> {
        self.reply(msg).await
    }

    /// Validates `msg` against the Limits (Lenient mode truncates), then
    /// rejects attachments.
    fn prepare(&self, msg: &mut OutgoingMessage) -> Result<(), ResponderError> {
        let report = match msg {
            OutgoingMessage::V2(m) => {
                let report = noctis_ui::validate(m, &self.limits)?;
                if !m.attachment_set().is_empty() {
                    return Err(ResponderError::AttachmentsUnsupported);
                }
                report
            }
            OutgoingMessage::Legacy(m) => noctis_ui::validate(m, &self.limits)?,
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

    async fn reply(&self, mut msg: OutgoingMessage) -> Result<(), ResponderError> {
        if self.transport.is_message() && self.ephemerality == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        self.prepare(&mut msg)?;
        let gate = self.gate.lock().await;
        if self.state() != PENDING {
            // The auto-defer (or a defer) answered first (INT-03).
            drop(gate);
            return self.edit_original(msg).await;
        }
        let message_id = if let Transport::Message(target) = &self.transport {
            Some(target.reply(create(&msg)?).await?)
        } else {
            // Encoded even when detached, so the seams catch encode errors.
            let flags = self.ephemerality.callback_flags();
            let data = match &msg {
                OutgoingMessage::V2(m) => encode(m.callback_body(flags))?,
                OutgoingMessage::Legacy(m) => encode(m.callback_body(flags))?,
            };
            match &self.transport {
                Transport::Interaction(target) => {
                    target
                        .callback(&json!({ "type": 4, "data": data }), true)
                        .await?
                }
                Transport::Message(_) | Transport::Detached => None,
            }
        };
        self.state.store(REPLIED, Ordering::Release);
        self.set_original(Original {
            shape: Shape::Sent(msg.format()),
            message_id,
            ephemerality: self.ephemerality,
            channel_id: None,
        });
        Ok(())
    }

    async fn defer(&self, ephemerality: Ephemerality) -> Result<(), ResponderError> {
        if self.transport.is_message() && ephemerality == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        let _gate = self.gate.lock().await;
        if self.state() != PENDING {
            tracing::debug!(target: "noctis::responder", "already acknowledged: defer sends nothing");
            return Ok(());
        }
        self.send_defer(ephemerality).await
    }

    /// Sends the defer of `ephemerality`; the caller holds the gate and saw
    /// `PENDING`.
    async fn send_defer(&self, ephemerality: Ephemerality) -> Result<(), ResponderError> {
        match &self.transport {
            Transport::Interaction(target) => {
                let body = match ephemerality {
                    Ephemerality::Public => json!({ "type": 5 }),
                    Ephemerality::Ephemeral => {
                        json!({ "type": 5, "data": { "flags": CallbackFlags::EPHEMERAL.bits() } })
                    }
                };
                target.callback(&body, false).await?;
            }
            Transport::Message(target) => target.typing().await?,
            Transport::Detached => {}
        }
        self.state.store(DEFERRED, Ordering::Release);
        self.set_original(Original {
            shape: Shape::Deferred,
            message_id: None,
            ephemerality,
            channel_id: None,
        });
        Ok(())
    }

    /// The auto-defer (INT-01): while Pending, a type-5 callback with the
    /// declared ephemerality. `false` when that callback failed (the state
    /// stays Pending; the caller logs it).
    pub(crate) async fn auto_defer(&self) -> bool {
        let Transport::Interaction(target) = &self.transport else {
            return true;
        };
        let _gate = self.gate.lock().await;
        if self.state() != PENDING {
            return true;
        }
        if self.send_defer(self.ephemerality).await.is_err() {
            return false;
        }
        metrics::counter!("noctis_autodefer_total", "app" => Arc::clone(&target.app.app))
            .increment(1);
        true
    }

    async fn modal(&self, mut modal: Modal) -> Result<(), ResponderError> {
        if self.transport.is_message() {
            return Err(ResponderError::ModalUnavailable);
        }
        let report = noctis_ui::validate(&mut modal, &self.limits)?;
        for warning in &report.warnings {
            tracing::debug!(target: "noctis::responder", ?warning, "modal adjusted to fit");
        }
        let data = serde_json::to_value(&modal).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        let _gate = self.gate.lock().await;
        if self.state() != PENDING {
            return Err(ResponderError::ModalUnavailable);
        }
        if let Transport::Interaction(target) = &self.transport {
            target
                .callback(&json!({ "type": 9, "data": data }), false)
                .await?;
        }
        self.state.store(MODAL, Ordering::Release);
        Ok(())
    }

    /// The body editing a message in `format` into `msg`, after
    /// [`prepare`](Self::prepare).
    fn edit_body(
        &self,
        msg: &mut OutgoingMessage,
        format: MessageFormat,
    ) -> Result<Value, ResponderError> {
        self.prepare(msg)?;
        match msg {
            OutgoingMessage::V2(m) => encode(m.edit_body(format, EditFlags::empty())),
            OutgoingMessage::Legacy(m) => encode(m.edit_body(format, EditFlags::empty())?),
        }
    }

    async fn edit_original(&self, mut msg: OutgoingMessage) -> Result<(), ResponderError> {
        let original = self.original();
        // After a defer, Discord's loading message needs no legacy clears.
        let format = match original.shape {
            Shape::Sent(format) => format,
            Shape::Unsent | Shape::Deferred => msg.format(),
        };
        let mut channel_id = original.channel_id;
        let message_id = match (&self.transport, original.message_id) {
            (Transport::Message(target), None) => {
                // A deferred message reply has no message yet: post it,
                // unless the Command is declared ephemeral.
                if self.ephemerality == Ephemerality::Ephemeral {
                    return Err(ResponderError::EphemeralUnsupported);
                }
                self.prepare(&mut msg)?;
                Some(target.reply(create(&msg)?).await?)
            }
            (transport, message_id) => {
                let body = self.edit_body(&mut msg, format)?;
                let posted = match self.expired_route(
                    original.ephemerality,
                    message_id,
                    original.channel_id,
                )? {
                    Some((app, channel, id)) => {
                        transport::channel_edit(app, channel, id, &body).await?
                    }
                    None => transport.edit_original(message_id, body).await?,
                };
                channel_id = posted.channel_id.or(channel_id);
                posted.message_id.or(message_id)
            }
        };
        self.set_original(Original {
            shape: Shape::Sent(msg.format()),
            message_id,
            ephemerality: original.ephemerality,
            channel_id,
        });
        Ok(())
    }

    async fn followup(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
        visibility: Visibility,
    ) -> Result<FollowupHandle, ResponderError> {
        if self.transport.is_message() && visibility == Ephemerality::Ephemeral {
            return Err(ResponderError::EphemeralUnsupported);
        }
        self.prepare(&mut msg)?;
        let body = match (&self.transport, &msg) {
            (Transport::Message(_), m) => create(m)?,
            (_, OutgoingMessage::V2(m)) => encode(m.callback_body(visibility.callback_flags()))?,
            (_, OutgoingMessage::Legacy(m)) => {
                encode(m.callback_body(visibility.callback_flags()))?
            }
        };
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
        let posted = self.transport.followup(body).await?;
        Ok(FollowupHandle {
            shared: Arc::clone(self),
            message_id: posted.id,
            channel_id: posted.channel_id,
            visibility,
            format: Mutex::new(msg.format()),
        })
    }

    /// Sends the localized notice of a failed handler run (FR-22, AD-23):
    /// one V2 message holding a single `TextDisplay(text)`, validated like
    /// every other send.
    ///
    /// - Pending: the initial response, a type-4 callback flagged
    ///   `EPHEMERAL` whatever the Command declared; over a message
    ///   transport a public reply to the invoking message.
    /// - Deferred: the deferred original is deleted first (a failure is a
    ///   `warn` and the notice is still sent), then the notice is an
    ///   ephemeral follow-up; over a message transport a public follow-up.
    /// - Replied: an ephemeral follow-up (public over a message transport).
    /// - Modal: nothing is sent (`debug` log).
    ///
    /// The follow-up cap and the token expiry apply (INT-11, INT-02).
    pub(crate) async fn notice(self: &Arc<Self>, text: String) -> Result<(), ResponderError> {
        let mut msg = OutgoingMessage::V2(V2Message::new(
            Components::new().push(TextDisplay::new(text)),
        ));
        self.prepare(&mut msg)?;
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
                        let flags = Ephemerality::Ephemeral.callback_flags();
                        let data = match &msg {
                            OutgoingMessage::V2(m) => encode(m.callback_body(flags))?,
                            OutgoingMessage::Legacy(m) => encode(m.callback_body(flags))?,
                        };
                        target
                            .callback(&json!({ "type": 4, "data": data }), false)
                            .await?
                    }
                    Transport::Message(target) => Some(target.reply(create(&msg)?).await?),
                    Transport::Detached => None,
                };
                self.state.store(REPLIED, Ordering::Release);
                self.set_original(Original {
                    shape: Shape::Sent(msg.format()),
                    message_id,
                    ephemerality: visibility,
                    channel_id: None,
                });
                drop(gate);
                Ok(())
            }
            DEFERRED => {
                drop(gate);
                if let Transport::Interaction(_) = &self.transport
                    && matches!(self.original().shape, Shape::Deferred)
                    && let Err(error) = self.delete_original().await
                {
                    tracing::warn!(
                        target: "noctis::dispatch",
                        %error,
                        "could not delete the deferred original before the notice"
                    );
                }
                self.followup(msg, visibility).await.map(|_| ())
            }
            REPLIED => {
                drop(gate);
                self.followup(msg, visibility).await.map(|_| ())
            }
            _ => {
                drop(gate);
                tracing::debug!(target: "noctis::dispatch", "no notice after a modal");
                Ok(())
            }
        }
    }

    async fn delete_original(&self) -> Result<(), ResponderError> {
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
            (transport, message_id) => transport.delete_original(message_id).await,
        }
    }
}

/// The JSON of a message body, or [`ResponderError::Encode`] (an
/// unresolved Route `custom_id`).
fn encode(body: MessageBody<'_>) -> Result<Value, ResponderError> {
    serde_json::to_value(body).map_err(|e| ResponderError::Encode {
        reason: e.to_string(),
    })
}

/// The Create Message body of `msg`.
fn create(msg: &OutgoingMessage) -> Result<Value, ResponderError> {
    match msg {
        OutgoingMessage::V2(m) => encode(m.create_body(CreateFlags::empty())),
        OutgoingMessage::Legacy(m) => encode(m.create_body(CreateFlags::empty())),
    }
}
