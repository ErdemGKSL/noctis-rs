//! The context methods `ctx.respond`, `ctx.update` and `ctx.reply_text`
//! (Story 8.1; INT-03, INT-04, INT-10, INT-12, MSG-05): one call that picks
//! the callback type, the deferral, the flags and the ephemerality from the
//! declared `response` and the run's state machine, under the same gate as
//! the typestate [`Responder`](super::Responder) and the auto-defer.

use std::borrow::Cow;
use std::fmt;
use std::future::IntoFuture;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use noctis_i18n::Builtin;
use noctis_rules::{MessageKey, Translate};
use noctis_ui::{Components, TextDisplay, V2Message};

use super::{
    DEFERRED, DEFERRED_UPDATE, Ephemerality, MODAL, OutgoingMessage, PENDING, REPLIED, Responded,
    ResponderError, STOPPED, Shape, Shared, Transport,
};
use crate::command::CtxInner;
use crate::handler::{HandlerError, HandlerResult};
use crate::notice;

/// The text of a [`ReplyText`] notice: what `ctx.reply_text(text)`
/// accepts through `impl Into<NoticeText>`.
///
/// Three inputs convert into it:
///
/// - a `String`, sent as is;
/// - a `&'static str`, sent as is;
/// - a [`MessageKey`], resolved in the response locale of the reply
///   (the User Locale first for an ephemeral reply, the Guild Locale first
///   for a public one, `en-US` when neither is known). A key no catalog
///   has renders the generic error text, with one `warn`.
///
/// ```
/// use noctis_core::NoticeText;
/// use noctis_rules::{MessageKey, Translate};
///
/// let owned: NoticeText = String::from("Saved.").into();
/// let literal: NoticeText = "Saved.".into();
/// let key: NoticeText = MessageKey::new("shop.saved").into();
/// assert!(matches!(owned, NoticeText::Text(_)));
/// assert!(matches!(literal, NoticeText::Text(_)));
/// assert!(matches!(key, NoticeText::Key(_)));
/// ```
///
/// Any other type is not a notice text:
///
/// ```compile_fail,E0277
/// use noctis_core::{CommandCtx, HandlerResult, Responded};
///
/// async fn count(ctx: CommandCtx) -> HandlerResult<Responded> {
///     ctx.reply_text(7_u32).await
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum NoticeText {
    /// A literal text, sent as is.
    Text(Cow<'static, str>),
    /// A locale key, resolved in the reply's response locale.
    Key(MessageKey),
}

impl From<String> for NoticeText {
    fn from(text: String) -> Self {
        Self::Text(Cow::Owned(text))
    }
}

impl From<&'static str> for NoticeText {
    fn from(text: &'static str) -> Self {
        Self::Text(Cow::Borrowed(text))
    }
}

impl From<MessageKey> for NoticeText {
    fn from(key: MessageKey) -> Self {
        Self::Key(key)
    }
}

/// A one-Text-Display notice from `ctx.reply_text(text)`: ephemeral unless
/// [`public`](Self::public), pinging nobody (MSG-05). `.await` it to send
/// it; it resolves to `HandlerResult<Responded>`.
///
/// `Debug` leaves the text out.
///
/// ```
/// use noctis_core::{ComponentCtx, HandlerResult, Responded};
///
/// async fn saved(ctx: ComponentCtx) -> HandlerResult<Responded> {
///     ctx.reply_text("Saved.").public().await
/// }
/// ```
#[must_use = "a ReplyText sends nothing until it is awaited"]
pub struct ReplyText {
    ctx: Arc<CtxInner>,
    text: NoticeText,
    ephemerality: Ephemerality,
}

impl ReplyText {
    /// The ephemeral notice of `text` for the context `ctx`.
    pub(crate) fn new(ctx: Arc<CtxInner>, text: NoticeText) -> Self {
        Self {
            ctx,
            text,
            ephemerality: Ephemerality::Ephemeral,
        }
    }

    /// Makes the notice visible to everyone in the channel. Over a
    /// [`Dispatcher`](crate::Dispatcher) message transport a notice is
    /// always public.
    pub fn public(mut self) -> Self {
        self.ephemerality = Ephemerality::Public;
        self
    }
}

impl fmt::Debug for ReplyText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplyText")
            .field("ephemerality", &self.ephemerality)
            .finish_non_exhaustive()
    }
}

impl IntoFuture for ReplyText {
    type Output = HandlerResult<Responded>;
    type IntoFuture = BoxFuture<'static, HandlerResult<Responded>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let shared = bound(&self.ctx)?;
            let visibility = shared.notice_visibility(self.ephemerality);
            let text = resolve(&self.ctx, self.text, visibility);
            shared
                .ctx_reply_text(text, visibility)
                .await
                .map(|()| Responded::new())
                .map_err(HandlerError::new)
        })
    }
}

/// The run's responder state of `ctx`, or [`ResponderError::NoReply`] for
/// an unbound context.
fn bound(ctx: &CtxInner) -> Result<&Arc<Shared>, HandlerError> {
    ctx.shared
        .as_ref()
        .ok_or_else(|| HandlerError::new(ResponderError::NoReply))
}

/// The text of `text` for a notice of `visibility` in `ctx`'s locales.
fn resolve(ctx: &CtxInner, text: NoticeText, visibility: Ephemerality) -> String {
    let strings: &dyn Translate = match &ctx.app {
        Some(app) => &*app.strings,
        None => &Builtin,
    };
    let locales = (ctx.user_locale.as_deref(), ctx.guild_locale.as_deref());
    text_in(strings, locales, text, visibility)
}

/// The text of `text` from `strings`: a key in the User Locale first for an
/// ephemeral notice, in the Guild Locale first for a public one, `en-US`
/// when neither is known or the first known code does not parse (LOC-04).
pub(super) fn text_in(
    strings: &dyn Translate,
    (user, guild): (Option<&str>, Option<&str>),
    text: NoticeText,
    visibility: Ephemerality,
) -> String {
    match text {
        NoticeText::Text(text) => text.into_owned(),
        NoticeText::Key(key) => {
            let locale = match visibility {
                Ephemerality::Ephemeral => notice::locale(user, guild),
                Ephemerality::Public => notice::locale(guild, user),
            };
            notice::text(strings, locale, key)
        }
    }
}

/// `ctx.respond(msg)` of the context `ctx`.
pub(crate) async fn respond(ctx: &CtxInner, msg: OutgoingMessage) -> HandlerResult<Responded> {
    bound(ctx)?
        .ctx_respond(msg)
        .await
        .map(|()| Responded::new())
        .map_err(HandlerError::new)
}

/// `ctx.update(msg)` of the context `ctx`.
pub(crate) async fn update(ctx: &CtxInner, msg: OutgoingMessage) -> HandlerResult<Responded> {
    bound(ctx)?
        .ctx_update(msg)
        .await
        .map(|()| Responded::new())
        .map_err(HandlerError::new)
}

impl Shared {
    /// [`ResponderError::AlreadyResponded`] after a modal or a Middleware
    /// stop: nothing more may be sent.
    fn refuse_closed(&self) -> Result<(), ResponderError> {
        if matches!(self.state(), MODAL | STOPPED) {
            Err(ResponderError::AlreadyResponded)
        } else {
            Ok(())
        }
    }

    /// The visibility a notice of `requested` gets: always public over a
    /// message transport (an ephemeral message reply does not exist).
    fn notice_visibility(&self, requested: Ephemerality) -> Ephemerality {
        if self.transport.is_message() {
            Ephemerality::Public
        } else {
            requested
        }
    }

    /// Whether the original is still Discord's unedited type-5 loading
    /// message.
    fn loading(&self) -> bool {
        self.state() == DEFERRED && matches!(self.original().shape, Shape::Deferred)
    }

    /// `ctx.respond(msg)`: while unanswered the initial response (type 4)
    /// with the declared ephemerality; after a type-5 defer the edit of the
    /// loading message (INT-12, keeping its ephemerality, INT-10); after a
    /// type-6 defer, or once the original was answered, a follow-up with
    /// the declared visibility. The initial response and the edit of the
    /// loading message mark the handler answered.
    ///
    /// # Errors
    ///
    /// [`ResponderError::AlreadyResponded`] after a modal or a stop (no
    /// request); any other [`ResponderError`] of the send.
    pub(crate) async fn ctx_respond(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
    ) -> Result<(), ResponderError> {
        self.refuse_closed()?;
        self.prepare(&mut msg).await?;
        let visibility = self.default_ephemerality();
        let _gate = self.gate.lock().await;
        match self.state() {
            PENDING => {
                self.send_reply(&msg, visibility, false).await?;
                self.mark_answered();
            }
            DEFERRED if self.loading() => {
                self.edit_prepared(msg).await?;
                self.mark_answered();
            }
            DEFERRED | DEFERRED_UPDATE | REPLIED => {
                self.send_follow_up(msg, visibility).await.map(drop)?;
            }
            _ => return Err(ResponderError::AlreadyResponded),
        }
        Ok(())
    }

    /// `ctx.update(msg)`: while unanswered a type-7 update of the source
    /// message; after the auto-defer an edit of the original (the clicked
    /// message after type 6, the deferred reply after type 5). It never
    /// sends a new message.
    ///
    /// # Errors
    ///
    /// [`ResponderError::AlreadyResponded`] once the handler answered, or
    /// after a modal or a stop; [`ResponderError::UpdateUnavailable`]
    /// without a source message. Neither sends a request, and neither
    /// marks the handler answered. Any other [`ResponderError`] of the send.
    pub(crate) async fn ctx_update(
        self: &Arc<Self>,
        mut msg: OutgoingMessage,
    ) -> Result<(), ResponderError> {
        self.refuse_answered()?;
        self.refuse_closed()?;
        let Some(source) = self.source.as_ref() else {
            return Err(ResponderError::UpdateUnavailable);
        };
        self.prepare(&mut msg).await?;
        let _gate = self.gate.lock().await;
        self.refuse_answered()?;
        match self.state() {
            PENDING => {
                self.send_update(&msg, source, false).await?;
            }
            DEFERRED | DEFERRED_UPDATE => self.edit_prepared(msg).await?,
            _ => return Err(ResponderError::AlreadyResponded),
        }
        self.mark_answered();
        Ok(())
    }

    /// `ctx.reply_text(text)`: one Text Display of `visibility`. While
    /// unanswered the initial response (type 4); after a type-5 defer the
    /// edit of the loading message when the visibilities match, else the
    /// loading message is deleted (a failure is one `warn`) and the notice
    /// is a follow-up; after a type-6 defer or once answered a follow-up.
    ///
    /// # Errors
    ///
    /// [`ResponderError::AlreadyResponded`] after a modal or a stop (no
    /// request); any other [`ResponderError`] of the send.
    pub(crate) async fn ctx_reply_text(
        self: &Arc<Self>,
        text: String,
        visibility: Ephemerality,
    ) -> Result<(), ResponderError> {
        self.refuse_closed()?;
        let mut msg = OutgoingMessage::V2(V2Message::new(
            Components::new().push(TextDisplay::new(text)),
        ));
        self.prepare(&mut msg).await?;
        let _gate = self.gate.lock().await;
        match self.state() {
            PENDING => {
                self.send_reply(&msg, visibility, false).await?;
                self.mark_answered();
            }
            DEFERRED if self.loading() => {
                let original = self.original();
                if original.ephemerality == visibility {
                    self.edit_prepared(msg).await?;
                } else {
                    if let Transport::Interaction(_) = &self.transport
                        && let Err(error) = self.delete_reply().await
                    {
                        tracing::warn!(
                            target: "noctis::dispatch",
                            %error,
                            "could not delete the deferred original before reply_text"
                        );
                    }
                    // The loading message is gone: later sends follow up.
                    self.set_original(super::Original {
                        shape: Shape::Sent(msg.format()),
                        ..original
                    });
                    self.send_follow_up(msg, visibility).await.map(drop)?;
                }
                self.mark_answered();
            }
            DEFERRED | DEFERRED_UPDATE | REPLIED => {
                self.send_follow_up(msg, visibility).await.map(drop)?;
            }
            _ => return Err(ResponderError::AlreadyResponded),
        }
        Ok(())
    }
}

/// The context methods of a handler context: `respond` and `reply_text`
/// for every interactive context, plus `update` for the component family
/// (`update = Kind`, the [`IntoEdit`](crate::IntoEdit) kind).
macro_rules! ctx_methods {
    ($ty:ident) => {
        impl $ty {
            /// Answers with `msg` as a new message, picking the flow from
            /// the declared `response` and the interaction's state: the
            /// initial response (callback type 4, the declared ephemerality)
            /// while unanswered; the edit of the deferred original after a
            /// type-5 defer, which keeps its ephemerality (INT-10, INT-12);
            /// a follow-up with the declared visibility after a type-6
            /// defer or once the original was answered. Over a
            /// [`Dispatcher`](crate::Dispatcher) it replies to the invoking
            /// message.
            ///
            /// Every send is validated against the App's Limits, carries
            /// `IS_COMPONENTS_V2` for a V2 message and pings nobody
            /// (MSG-05). Return its result from the handler.
            ///
            /// ```
            #[doc = concat!("use noctis_core::{", stringify!($ty), ", HandlerResult, Responded};")]
            /// use noctis_ui::{Components, TextDisplay, V2Message};
            ///
            #[doc = concat!("async fn answer(ctx: ", stringify!($ty), ") -> HandlerResult<Responded> {")]
            ///     let msg = V2Message::new(Components::new().push(TextDisplay::new("Saved.")));
            ///     ctx.respond(msg).await
            /// }
            /// ```
            ///
            /// # Errors
            ///
            /// A [`HandlerError`](crate::HandlerError) wrapping the
            /// [`ResponderError`](crate::ResponderError) (find it with
            /// `err.as_error().downcast_ref::<ResponderError>()`):
            /// `Invalid` for a message over a Limit (Strict mode, no
            /// request), `AlreadyResponded` after a modal or a stop,
            /// `EphemeralUnsupported` for an ephemeral reply over a
            /// Dispatcher, `FollowupCap` or `TokenExpired` for a follow-up,
            /// or Discord's rejection.
            pub fn respond(
                &self,
                msg: impl Into<$crate::OutgoingMessage>,
            ) -> impl ::std::future::Future<Output = $crate::HandlerResult<$crate::Responded>>
                   + Send
                   + '_ {
                let msg = msg.into();
                async move { $crate::responder::ergonomics::respond(&self.0, msg).await }
            }

            /// Sends `text` as one Text Display, **ephemeral** unless
            /// [`public`](crate::ReplyText::public), pinging nobody
            /// (MSG-05): the initial response while unanswered; after a
            /// type-5 defer the edit of the loading message when it has the
            /// same visibility, else the loading message is deleted and
            /// the text follows up; a follow-up after a type-6 defer or
            /// once answered. `text` is a `String`, a `&'static str` or a
            /// [`MessageKey`](noctis_rules::MessageKey), resolved in the
            /// User Locale (ephemeral) or the Guild Locale (public) first.
            /// Over a [`Dispatcher`](crate::Dispatcher) the text is always
            /// public.
            ///
            /// ```
            #[doc = concat!("use noctis_core::{", stringify!($ty), ", HandlerResult, Responded};")]
            ///
            #[doc = concat!("async fn saved(ctx: ", stringify!($ty), ") -> HandlerResult<Responded> {")]
            ///     ctx.reply_text("Saved.").await
            /// }
            /// ```
            ///
            /// # Errors
            ///
            /// Awaiting it gives a [`HandlerError`](crate::HandlerError)
            /// wrapping the [`ResponderError`](crate::ResponderError):
            /// `Invalid` for text over 4000 characters (Strict mode, no
            /// request), `AlreadyResponded` after a modal or a stop,
            /// `FollowupCap` or `TokenExpired` for a follow-up, or
            /// Discord's rejection.
            pub fn reply_text(
                &self,
                text: impl Into<$crate::NoticeText>,
            ) -> $crate::ReplyText {
                $crate::ReplyText::new(::std::sync::Arc::clone(&self.0), text.into())
            }
        }
    };
    ($ty:ident, update = $kind:ident) => {
        $crate::responder::ergonomics::ctx_methods!($ty);

        impl $ty {
            /// Updates the message with `msg`. While unanswered it is a
            /// type-7 update of the message the component lives on (or
            /// the modal was opened from); after a type-6 auto-defer
            /// (`response = update`) it edits that clicked message. After
            /// a type-5 auto-defer (`response = ephemeral|public`) it edits
            /// the deferred reply and leaves the clicked message
            /// untouched, so a handler that calls `ctx.update` keeps
            /// `response = update`. The message keeps its
            /// ephemerality, gets `IS_COMPONENTS_V2` and never `EPHEMERAL`
            /// (INT-10); `msg` is always V2 ([`IntoEdit`](crate::IntoEdit),
            /// CV2-01). It never sends a new message: answer a modal
            /// submit opened from a slash command with
            /// [`respond`](Self::respond).
            ///
            /// ```
            #[doc = concat!("use noctis_core::{", stringify!($ty), ", HandlerResult, Responded};")]
            /// use noctis_ui::{Components, TextDisplay};
            ///
            #[doc = concat!("async fn bought(ctx: ", stringify!($ty), ") -> HandlerResult<Responded> {")]
            ///     ctx.update(Components::new().push(TextDisplay::new("Bought."))).await
            /// }
            /// ```
            ///
            /// # Errors
            ///
            /// A [`HandlerError`](crate::HandlerError) wrapping the
            /// [`ResponderError`](crate::ResponderError):
            /// `UpdateUnavailable` when there is no message to update (a
            /// modal opened from a slash command; no request),
            /// `AlreadyResponded` once the handler answered through a
            /// context method or a Responder, or after a modal (no
            /// request), `Invalid` for a message over a Limit, or
            /// Discord's rejection.
            pub fn update(
                &self,
                msg: impl $crate::IntoEdit<$crate::$kind>,
            ) -> impl ::std::future::Future<Output = $crate::HandlerResult<$crate::Responded>>
                   + Send
                   + '_ {
                let msg = $crate::IntoEdit::<$crate::$kind>::into_outgoing(msg);
                async move { $crate::responder::ergonomics::update(&self.0, msg).await }
            }
        }
    };
}

pub(crate) use ctx_methods;
