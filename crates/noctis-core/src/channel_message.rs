//! Views posted as the bot in a channel (Create, Edit and Delete Message with
//! the bot token; CV2-01, CV2-02, MSG-07).
//!
//! [`Messages`] (from `ctx.messages()` or `App::messages()`) posts a
//! [`V2Message`] to any channel the bot can write in, outside an
//! interaction: an event handler, a lifecycle hook, a scheduled task. Route
//! ids, Refs and select nonces are resolved exactly as in a reply, so the
//! posted View's buttons and selects reach their handlers. Discord accepts
//! `IS_COMPONENTS_V2` on Create Message and on Edit Message (set only), so a
//! normal bot message is a full V2 message; no webhook is needed.
//!
//! Every send and edit goes through the Responder's own preparation (locale
//! keys in the App's default locale, Route ids and Refs, the Limits, the
//! Upload Budget and the attachments); flags are typed per endpoint
//! ([`CreateFlags`] on send, [`EditFlags`] on edit, MSG-07). An edit by id
//! clears the legacy fields, so a legacy bot message becomes V2 (CV2-02).
//! The bot needs `VIEW_CHANNEL` and `SEND_MESSAGES` (in a thread
//! `SEND_MESSAGES_IN_THREADS`); Discord's refusal is
//! [`MessageError::Rejected`].
//!
//! Route buttons on a posted message mint Refs with the App's `ref_ttl`,
//! so a `by_ref` button on a long-lived message expires into `on_expired`:
//! prefer inline (non-`by_ref`) Routes there, or a longer `ttl`.

#[cfg(test)]
mod tests;

use std::fmt;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use noctis_ports::PortError;
use noctis_ui::{CreateFlags, EditFlags, MessageFormat, UiError, V2Message};
use serde_json::{Value, json};
use twilight_http::routing::Route;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};

use crate::handler::AppShared;
use crate::responder::files::KnownFiles;
use crate::responder::transport::{self, Posted};
use crate::responder::{Ephemerality, OutgoingMessage, ResponderError, Shared, encode};

/// Why a channel message call failed.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    /// The message breaks a Discord rule (Limits, the Upload Budget);
    /// nothing was sent.
    #[error(transparent)]
    Invalid(#[from] UiError),
    /// The message could not be serialized (a Route `custom_id` could not
    /// be resolved), named in `reason`; nothing was sent.
    #[error("the message could not be encoded: {reason}")]
    Encode {
        /// What failed, naming the Route.
        reason: String,
    },
    /// A Ref minted for the message could not be stored in the App's
    /// `RefStore`; nothing was sent.
    #[error("the Ref could not be stored: {0}")]
    RefStore(#[source] PortError),
    /// Three freshly minted Refs in a row were already taken; nothing was
    /// sent.
    #[error("could not mint a unique Ref after 3 attempts")]
    RefCollision,
    /// Discord rate limited the request and Noctis did not retry it
    /// (RL-04).
    #[error(transparent)]
    RateLimited(crate::RateLimited),
    /// Discord answered with an error status (403 code 50001 or 50013
    /// without access or permission, 404 code 10003 or 10008 for an unknown
    /// channel or message).
    #[error("Discord rejected the message request with HTTP {status}{}", code.map(|c| format!(" (code {c})")).unwrap_or_default())]
    Rejected {
        /// The HTTP status.
        status: u16,
        /// Discord's JSON error code, when the body has one.
        code: Option<u32>,
    },
    /// No usable answer: the request failed in transport, timed out, or
    /// the response is not a message.
    #[error("the message request did not get a usable response")]
    Transport,
    /// A detached test context: nothing was sent.
    #[error("this context has no App: nothing was sent")]
    Detached,
}

impl From<ResponderError> for MessageError {
    fn from(error: ResponderError) -> Self {
        match error {
            ResponderError::Invalid(e) => Self::Invalid(e),
            ResponderError::Encode { reason } => Self::Encode { reason },
            ResponderError::RefStore(e) => Self::RefStore(e),
            ResponderError::RefCollision => Self::RefCollision,
            ResponderError::Rejected { status, code } => Self::Rejected { status, code },
            ResponderError::RateLimited(limited) => Self::RateLimited(limited),
            ResponderError::Transport => Self::Transport,
            other => {
                tracing::debug!(target: "noctis::message", error = %other, "unexpected responder error");
                Self::Transport
            }
        }
    }
}

/// The App's channel message client (`ctx.messages()` on every context,
/// `App::messages()`; a cheap clone): posts, edits and deletes bot messages
/// that carry a Components V2 View.
///
/// ```no_run
/// # async fn run(app: &noctis_core::App) -> Result<(), noctis_core::MessageError> {
/// use noctis_ui::{Components, TextDisplay, V2Message};
/// use twilight_model::id::Id;
///
/// let view = V2Message::new(Components::new().push(TextDisplay::new("Server restarted")));
/// let posted = app.messages().send(Id::new(1), view).await?;
/// posted.delete().await?;
/// # Ok(()) }
/// ```
#[derive(Clone, Debug)]
pub struct Messages {
    app: Target,
}

/// The App a request goes to; `None` in a detached test context.
#[derive(Clone)]
struct Target(Option<Arc<AppShared>>);

impl Target {
    fn app(self) -> Result<Arc<AppShared>, MessageError> {
        self.0.ok_or(MessageError::Detached)
    }
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("App")
            .field("detached", &self.0.is_none())
            .finish()
    }
}

impl Messages {
    pub(crate) fn of(app: Option<&Arc<AppShared>>) -> Self {
        Self {
            app: Target(app.map(Arc::clone)),
        }
    }

    /// Posts `msg` in `channel` as the bot (`POST /channels/{channel}/messages`,
    /// flags `32768 | CreateFlags`); resolves to the [`ChannelMessage`].
    pub fn send(&self, channel: Id<ChannelMarker>, msg: impl Into<V2Message>) -> SendMessage {
        SendMessage {
            app: self.app.clone(),
            channel,
            msg: msg.into(),
            flags: CreateFlags::empty(),
            reply_to: None,
        }
    }

    /// Edits the bot message `id` in `channel` into `msg`. The target's
    /// format is unknown, so the legacy fields are cleared (`content: null`,
    /// `embeds: []`, `poll: null`, `sticker_ids: []`): a legacy message
    /// becomes V2 (CV2-02). Attachments the message holds are dropped unless
    /// `msg` uploads them again; [`ChannelMessage::edit`] keeps them.
    pub fn edit(
        &self,
        channel: Id<ChannelMarker>,
        id: Id<MessageMarker>,
        msg: impl Into<V2Message>,
    ) -> EditMessage<'static> {
        EditMessage {
            app: self.app.clone(),
            channel,
            id,
            msg: msg.into(),
            flags: EditFlags::empty(),
            format: MessageFormat::Legacy,
            known: None,
        }
    }

    /// Deletes the message `id` in `channel` (the bot's own, or any with
    /// `MANAGE_MESSAGES`).
    pub fn delete(&self, channel: Id<ChannelMarker>, id: Id<MessageMarker>) -> DeleteMessage {
        DeleteMessage {
            app: self.app.clone(),
            channel,
            id,
        }
    }
}

/// Prepares `msg` for a public send of the App behind `app`.
async fn prepare(
    app: &Arc<AppShared>,
    msg: V2Message,
) -> Result<(Arc<Shared>, OutgoingMessage), MessageError> {
    let shared = Shared::webhook(Arc::clone(app));
    let mut msg = OutgoingMessage::V2(msg);
    shared.prepare(&mut msg, Ephemerality::Public).await?;
    Ok((shared, msg))
}

/// A pending Create Message (from [`Messages::send`]); does nothing until
/// awaited.
#[derive(Debug)]
#[must_use = "a message request does nothing until it is awaited"]
pub struct SendMessage {
    app: Target,
    channel: Id<ChannelMarker>,
    msg: V2Message,
    flags: CreateFlags,
    reply_to: Option<Id<MessageMarker>>,
}

impl SendMessage {
    /// The Create Message flags (`SUPPRESS_EMBEDS`,
    /// `SUPPRESS_NOTIFICATIONS`); `IS_COMPONENTS_V2` is always added.
    pub fn flags(mut self, flags: CreateFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Posts the message as a reply to `message` in the same channel
    /// (`message_reference`, `fail_if_not_exists: false`: a deleted
    /// target still posts).
    pub fn reply_to(mut self, message: Id<MessageMarker>) -> Self {
        self.reply_to = Some(message);
        self
    }

    async fn run(self) -> Result<ChannelMessage, MessageError> {
        let app = self.app.app()?;
        let (shared, msg) = prepare(&app, self.msg).await?;
        let OutgoingMessage::V2(prepared) = &msg else {
            return Err(MessageError::Transport);
        };
        let mut json = encode(prepared.create_body(self.flags))?;
        if let (Some(target), Value::Object(map)) = (self.reply_to, &mut json) {
            map.insert(
                "message_reference".to_owned(),
                json!({ "message_id": target.to_string(), "fail_if_not_exists": false }),
            );
        }
        let body = shared.body(&msg, json, None, None)?;
        let route = Route::CreateMessage {
            channel_id: self.channel.get(),
        };
        let response = transport::send(&app, &route, Some(body.payload), true).await?;
        let id = transport::message_id(&response)?;
        let Posted {
            channel_id, files, ..
        } = transport::posted(&response);
        Ok(ChannelMessage {
            app: Target(Some(app)),
            id,
            channel_id: channel_id.unwrap_or(self.channel),
            files: Mutex::new(files.unwrap_or(body.kept)),
        })
    }
}

impl IntoFuture for SendMessage {
    type Output = Result<ChannelMessage, MessageError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// A pending Edit Message (from [`Messages::edit`] or
/// [`ChannelMessage::edit`]); does nothing until awaited.
#[derive(Debug)]
#[must_use = "a message request does nothing until it is awaited"]
pub struct EditMessage<'a> {
    app: Target,
    channel: Id<ChannelMarker>,
    id: Id<MessageMarker>,
    msg: V2Message,
    flags: EditFlags,
    format: MessageFormat,
    known: Option<&'a Mutex<KnownFiles>>,
}

impl EditMessage<'_> {
    /// The Edit Message flags (`SUPPRESS_EMBEDS`, always sent);
    /// `IS_COMPONENTS_V2` is always added.
    pub fn flags(mut self, flags: EditFlags) -> Self {
        self.flags = flags;
        self
    }

    async fn run(self) -> Result<(), MessageError> {
        let app = self.app.app()?;
        let (shared, msg) = prepare(&app, self.msg).await?;
        let OutgoingMessage::V2(prepared) = &msg else {
            return Err(MessageError::Transport);
        };
        let json = encode(prepared.edit_body(self.format, self.flags))?;
        let known = self.known.map_or(KnownFiles::EMPTY, |k| {
            k.lock().unwrap_or_else(PoisonError::into_inner).clone()
        });
        let body = shared.body(&msg, json, Some(&known), None)?;
        let posted = transport::channel_edit(&app, self.channel, self.id, body.payload).await?;
        if let Some(known) = self.known {
            *known.lock().unwrap_or_else(PoisonError::into_inner) =
                posted.files.unwrap_or(body.kept);
        }
        Ok(())
    }
}

impl<'a> IntoFuture for EditMessage<'a> {
    type Output = Result<(), MessageError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// A pending Delete Message (from [`Messages::delete`] or
/// [`ChannelMessage::delete`]); does nothing until awaited.
#[derive(Debug)]
#[must_use = "a message request does nothing until it is awaited"]
pub struct DeleteMessage {
    app: Target,
    channel: Id<ChannelMarker>,
    id: Id<MessageMarker>,
}

impl DeleteMessage {
    async fn run(self) -> Result<(), MessageError> {
        let app = self.app.app()?;
        transport::channel_delete(&app, self.channel, self.id).await?;
        Ok(())
    }
}

impl IntoFuture for DeleteMessage {
    type Output = Result<(), MessageError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// A message the bot posted with [`Messages::send`]: its ids, and edits
/// that keep the attachments Discord already holds.
#[derive(Debug)]
pub struct ChannelMessage {
    app: Target,
    id: Id<MessageMarker>,
    channel_id: Id<ChannelMarker>,
    files: Mutex<KnownFiles>,
}

impl ChannelMessage {
    /// The message id.
    pub fn id(&self) -> Id<MessageMarker> {
        self.id
    }

    /// The channel (or thread) the message is in.
    pub fn channel_id(&self) -> Id<ChannelMarker> {
        self.channel_id
    }

    /// Edits the message into `msg`, keeping the attachments the message
    /// holds that `msg` still uses (no re-upload).
    pub fn edit(&self, msg: impl Into<V2Message>) -> EditMessage<'_> {
        EditMessage {
            app: self.app.clone(),
            channel: self.channel_id,
            id: self.id,
            msg: msg.into(),
            flags: EditFlags::empty(),
            format: MessageFormat::V2,
            known: Some(&self.files),
        }
    }

    /// Deletes the message.
    pub fn delete(&self) -> DeleteMessage {
        DeleteMessage {
            app: self.app.clone(),
            channel: self.channel_id,
            id: self.id,
        }
    }
}
