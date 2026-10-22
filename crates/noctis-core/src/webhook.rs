//! Views over webhooks (FR-65; AD-11, AD-12; WH-01..05, MSG-07).
//!
//! [`Webhooks`] (from `ctx.webhooks()` or `App::webhooks()`) creates a
//! webhook the App owns, or wraps stored credentials as an owned or a
//! foreign one. A [`Webhook<Owned>`] sends any [`V2Message`]: Route ids,
//! Refs and select nonces are resolved exactly as in a reply. A
//! [`Webhook<Foreign>`] sends only a [`StaticMessage`](noctis_ui::StaticMessage) (WH-02, a compile
//! error otherwise).
//!
//! Every send and edit goes through the Responder's own preparation (locale
//! keys, Route ids and Refs, the Limits, the Upload Budget and the
//! attachments) and always carries `?with_components=true` (WH-01, no
//! setter); flags are typed per endpoint ([`CreateFlags`] on execute,
//! [`EditFlags`] on edit, MSG-07); every edit clears the legacy fields
//! (WH-03); a forum or media channel needs a thread (WH-05). Webhook-token
//! requests never carry the bot `Authorization` header, and Noctis never
//! logs, displays or puts the token in an error. twilight-http's `req` span
//! carries the request URL, which holds the token: keep the
//! `twilight_http=warn` directive in the log filter.
//!
//! Route buttons on an owned webhook message mint Refs with the App's
//! `ref_ttl`, so a `by_ref` button on a long-lived log or persona message
//! expires into `on_expired`: prefer inline (non-`by_ref`) Routes there, or
//! a longer `ttl`.
//!
//! A foreign webhook takes no [`V2Message`]:
//!
//! ```compile_fail,E0277
//! use noctis_core::ForeignWebhook;
//! use noctis_ui::{Components, TextDisplay, V2Message};
//! use twilight_model::id::Id;
//!
//! fn mirror(hook: &ForeignWebhook) {
//!     let msg = V2Message::new(Components::new().push(TextDisplay::new("x")));
//!     let _ = hook.edit(Id::new(1), msg); // [WH-02]
//! }
//! ```

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::fmt;
use std::future::{Future, IntoFuture};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use noctis_ports::PortError;
use noctis_rules::message::{WebhookThreadDecl, validate_webhook_thread};
use noctis_rules::{Actual, Bound, RuleId, Violation};
use noctis_ui::{CreateFlags, EditFlags, IntoStaticMessage, UiError, V2Message, WebhookName};
use serde_json::{Value, json};
use twilight_http::request::{Method, RequestBuilder};
use twilight_http::routing::Route;
use twilight_model::application::interaction::InteractionChannel;
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, MessageMarker, WebhookMarker};

use crate::edge::callback::valid_token;
use crate::handler::AppShared;
use crate::responder::files::{KnownFiles, Payload};
use crate::responder::transport::{self, Posted};
use crate::responder::{Ephemerality, OutgoingMessage, ResponderError, Shared, encode};

/// The Discord code of "Webhooks posted to forum channels must have a
/// `thread_name` or `thread_id`".
const FORUM_NEEDS_THREAD: u32 = 220_001;
/// The Discord code of "Webhooks can only create threads in forum
/// channels".
const THREAD_NAME_OFF_FORUM: u32 = 220_003;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Owned {}
    impl Sealed for super::Foreign {}
}

/// Who owns a [`Webhook`]: [`Owned`] or [`Foreign`]. Sealed.
pub trait Ownership: sealed::Sealed + fmt::Debug + Send + Sync + 'static {}

/// A webhook the App created (`Webhooks::create`), or whose stored
/// credentials it created: it may send interactive components.
#[derive(Debug)]
pub enum Owned {}

/// A webhook the App did not create: it sends only non-interactive
/// components, a [`StaticMessage`](noctis_ui::StaticMessage) (WH-02).
#[derive(Debug)]
pub enum Foreign {}

impl Ownership for Owned {}
impl Ownership for Foreign {}

/// A webhook the App owns.
pub type OwnedWebhook = Webhook<Owned>;
/// A webhook the App does not own.
pub type ForeignWebhook = Webhook<Foreign>;

/// Why a webhook call failed. Nothing in it names the webhook token.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    /// The message, the name or the thread breaks a Discord rule
    /// (Limits, the Upload Budget, WH-02, WH-04, WH-05, including Discord's
    /// own forum answers 220001 and 220003); nothing was sent, except for
    /// those two Discord answers.
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
    /// The webhook token is not 1-512 characters of `[A-Za-z0-9._-]`;
    /// nothing was sent.
    #[error("the webhook token is not 1-512 characters of [A-Za-z0-9._-]")]
    InvalidToken,
    /// Discord answered with an error status.
    #[error("Discord rejected the webhook request with HTTP {status}{}", code.map(|c| format!(" (code {c})")).unwrap_or_default())]
    Rejected {
        /// The HTTP status.
        status: u16,
        /// Discord's JSON error code, when the body has one.
        code: Option<u32>,
    },
    /// The request did not get a usable response (a missing message or
    /// webhook field included).
    #[error("the webhook request did not get a usable response")]
    Transport,
    /// The context has no App (a detached test seam); nothing was sent.
    #[error("this context has no App: nothing was sent")]
    Detached,
}

impl From<ResponderError> for WebhookError {
    fn from(error: ResponderError) -> Self {
        match error {
            ResponderError::Invalid(e) => Self::Invalid(e),
            ResponderError::Encode { reason } => Self::Encode { reason },
            ResponderError::RefStore(e) => Self::RefStore(e),
            ResponderError::RefCollision => Self::RefCollision,
            ResponderError::InvalidToken => Self::InvalidToken,
            ResponderError::Rejected { status, code } => Self::Rejected { status, code },
            ResponderError::Transport => Self::Transport,
            other => {
                tracing::debug!(target: "noctis::webhook", error = %other, "unexpected responder error");
                Self::Transport
            }
        }
    }
}

/// The WH-05 violation of a Discord forum answer (400 with code 220001 or
/// 220003), or the error itself.
fn forum_error(error: ResponderError) -> WebhookError {
    let (path, describe, code) = match error {
        ResponderError::Rejected {
            status: 400,
            code: Some(FORUM_NEEDS_THREAD),
        } => (
            "thread_id",
            "a thread_id or a thread_name in a forum or media channel",
            FORUM_NEEDS_THREAD,
        ),
        ResponderError::Rejected {
            status: 400,
            code: Some(THREAD_NAME_OFF_FORUM),
        } => (
            "thread_name",
            "a thread_name only in a forum or media channel",
            THREAD_NAME_OFF_FORUM,
        ),
        other => return other.into(),
    };
    WebhookError::Invalid(UiError::Limits {
        violations: vec![Violation::new(
            RuleId::WH_05,
            path,
            Bound::Describe(describe),
            Actual::Text(format!("Discord code {code}")),
        )],
    })
}

/// Whether a webhook in a channel of `kind` must name a thread (forum and
/// media channels, WH-05).
fn thread_only(kind: ChannelType) -> bool {
    matches!(kind, ChannelType::GuildForum | ChannelType::GuildMedia)
}

/// WH-05 for a request declaring `thread`: in full when the channel kind is
/// known; else only a `thread_name`'s length.
fn check_thread(
    kind: Option<ChannelType>,
    thread: WebhookThreadDecl<'_>,
    app: &AppShared,
) -> Result<(), WebhookError> {
    let violations = match (kind, thread) {
        (Some(kind), thread) => validate_webhook_thread(thread_only(kind), thread, &app.limits),
        (None, name @ WebhookThreadDecl::Name(_)) => {
            validate_webhook_thread(true, name, &app.limits)
        }
        (None, _) => Vec::new(),
    };
    if violations.is_empty() {
        Ok(())
    } else {
        Err(UiError::Limits { violations }.into())
    }
}

/// The channel a webhook is created in: its id and kind (the kind decides
/// WH-05 before sending).
///
/// ```
/// use noctis_core::WebhookChannel;
/// use twilight_model::channel::ChannelType;
/// use twilight_model::id::Id;
///
/// let channel = WebhookChannel::new(Id::new(5), ChannelType::GuildForum);
/// assert_eq!((channel.id().get(), channel.kind()), (5, ChannelType::GuildForum));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebhookChannel {
    id: Id<ChannelMarker>,
    kind: ChannelType,
}

impl WebhookChannel {
    /// The channel `id`, of kind `kind`.
    pub const fn new(id: Id<ChannelMarker>, kind: ChannelType) -> Self {
        Self { id, kind }
    }

    /// The channel id.
    pub const fn id(&self) -> Id<ChannelMarker> {
        self.id
    }

    /// The channel kind.
    pub const fn kind(&self) -> ChannelType {
        self.kind
    }
}

impl From<&InteractionChannel> for WebhookChannel {
    /// A channel option's or a channel select's resolved channel.
    fn from(channel: &InteractionChannel) -> Self {
        Self::new(channel.id, channel.kind)
    }
}

/// The thread of an execute: an existing one (`thread_id`) or a new forum
/// or media post (`thread_name`, 1-100 characters, WH-05). An enum, so a
/// send never carries both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookThread {
    /// An existing thread of the webhook's channel.
    Existing(Id<ChannelMarker>),
    /// A new post of a forum or media channel, with this name.
    Create(Cow<'static, str>),
}

impl From<Id<ChannelMarker>> for WebhookThread {
    fn from(id: Id<ChannelMarker>) -> Self {
        Self::Existing(id)
    }
}

impl WebhookThread {
    fn decl(&self) -> WebhookThreadDecl<'_> {
        match self {
            Self::Existing(_) => WebhookThreadDecl::Id,
            Self::Create(name) => WebhookThreadDecl::Name(name),
        }
    }
}

/// The webhook factory of a context or an App; a cheap clone.
///
/// A detached context (the `__testing` seams) builds handles whose every
/// request is [`WebhookError::Detached`], with nothing sent.
#[derive(Clone)]
pub struct Webhooks {
    app: Option<Arc<AppShared>>,
}

impl fmt::Debug for Webhooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Webhooks")
            .field("detached", &self.app.is_none())
            .finish()
    }
}

impl Webhooks {
    /// The factory of the App behind `app`, or a detached one.
    pub(crate) fn of(app: Option<&Arc<AppShared>>) -> Self {
        Self {
            app: app.map(Arc::clone),
        }
    }

    /// Creates a webhook named `name` in `channel` (Create Webhook, with the
    /// bot token; the bot needs `MANAGE_WEBHOOKS`). The handle knows the
    /// channel's kind; store its [`id`](Webhook::id) and
    /// [`token`](Webhook::token) to reuse it with [`owned`](Self::owned): a
    /// channel holds at most 15 webhooks (`Rejected { status: 400, code:
    /// Some(30007) }` beyond), and lost credentials leak a webhook Noctis
    /// cannot delete.
    ///
    /// # Errors
    ///
    /// [`WebhookError::Rejected`] when Discord refuses (403 code 50013
    /// without the permission), [`WebhookError::Transport`] when the answer
    /// lacks a valid `id`, `token` or `channel_id`,
    /// [`WebhookError::Detached`] on a detached context.
    pub async fn create(
        &self,
        channel: WebhookChannel,
        name: WebhookName,
    ) -> Result<Webhook<Owned>, WebhookError> {
        let app = self.app.as_ref().ok_or(WebhookError::Detached)?;
        let route = Route::CreateWebhook {
            channel_id: channel.id.get(),
        };
        let body = serde_json::to_vec(&json!({ "name": name.as_str() })).map_err(|e| {
            WebhookError::Encode {
                reason: e.to_string(),
            }
        })?;
        let response = transport::send(app, &route, Some(Payload::Json(body)), true).await?;
        let value: Value =
            serde_json::from_slice(&response).map_err(|_| WebhookError::Transport)?;
        let id = transport::snowflake::<WebhookMarker>(value.get("id"));
        let channel_id = transport::snowflake::<ChannelMarker>(value.get("channel_id"));
        let token = value.get("token").and_then(Value::as_str);
        match (id, channel_id, token) {
            (Some(id), Some(_), Some(token)) if valid_token(token) => Ok(Webhook::new(
                Some(Arc::clone(app)),
                id,
                token.to_owned(),
                Some(channel.kind),
            )),
            _ => {
                tracing::debug!(target: "noctis::webhook", "create webhook answer without id, token or channel_id");
                Err(WebhookError::Transport)
            }
        }
    }

    /// The webhook the App owns with these stored credentials. Its channel
    /// kind is unknown until [`in_channel`](Webhook::in_channel) declares
    /// it.
    ///
    /// Ownership is the caller's declaration: Noctis does not verify it.
    /// Credentials of a webhook the App did not create belong in
    /// [`foreign`](Self::foreign); Discord refuses interactive components
    /// through a non-owned webhook ([`WebhookError::Rejected`]).
    ///
    /// # Errors
    ///
    /// [`WebhookError::InvalidToken`] when `token` is not 1-512 characters
    /// of `[A-Za-z0-9._-]`.
    pub fn owned(
        &self,
        id: Id<WebhookMarker>,
        token: impl Into<String>,
    ) -> Result<Webhook<Owned>, WebhookError> {
        self.wrap(id, token.into())
    }

    /// A webhook the App does not own (one a user created, say): it sends
    /// only a [`StaticMessage`](noctis_ui::StaticMessage) (WH-02).
    ///
    /// # Errors
    ///
    /// [`WebhookError::InvalidToken`] as [`owned`](Self::owned).
    pub fn foreign(
        &self,
        id: Id<WebhookMarker>,
        token: impl Into<String>,
    ) -> Result<Webhook<Foreign>, WebhookError> {
        self.wrap(id, token.into())
    }

    fn wrap<O: Ownership>(
        &self,
        id: Id<WebhookMarker>,
        token: String,
    ) -> Result<Webhook<O>, WebhookError> {
        if !valid_token(&token) {
            return Err(WebhookError::InvalidToken);
        }
        Ok(Webhook::new(self.app.clone(), id, token, None))
    }
}

/// A webhook handle: its id, its token and, when known, its channel's
/// kind. `O` is [`Owned`] or [`Foreign`].
///
/// `Debug` never prints the token.
pub struct Webhook<O: Ownership> {
    app: Option<Arc<AppShared>>,
    id: Id<WebhookMarker>,
    /// A secret: never printed.
    token: String,
    kind: Option<ChannelType>,
    owner: PhantomData<fn() -> O>,
}

impl<O: Ownership> Clone for Webhook<O> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            id: self.id,
            token: self.token.clone(),
            kind: self.kind,
            owner: PhantomData,
        }
    }
}

impl<O: Ownership> fmt::Debug for Webhook<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Webhook")
            .field("id", &self.id)
            .field("token", &"<redacted>")
            .field("channel_kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl<O: Ownership> Webhook<O> {
    fn new(
        app: Option<Arc<AppShared>>,
        id: Id<WebhookMarker>,
        token: String,
        kind: Option<ChannelType>,
    ) -> Self {
        Self {
            app,
            id,
            token,
            kind,
            owner: PhantomData,
        }
    }

    /// The webhook id.
    pub fn id(&self) -> Id<WebhookMarker> {
        self.id
    }

    /// The webhook token: a secret that lets anyone post as this webhook.
    /// Store it like a password (with the id) to reuse the webhook; never
    /// log it.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The kind of the webhook's channel, when known (`create`, or
    /// [`in_channel`](Self::in_channel)).
    pub fn channel_kind(&self) -> Option<ChannelType> {
        self.kind
    }

    /// Declares the kind of the webhook's channel for stored credentials,
    /// so WH-05 is checked before sending.
    #[must_use]
    pub fn in_channel(mut self, kind: ChannelType) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Deletes the webhook's message `id` (Delete Webhook Message); in a
    /// forum or media channel, pass its thread with
    /// [`thread`](DeleteWebhookMessage::thread) (WH-05).
    pub fn delete(&self, id: Id<MessageMarker>) -> DeleteWebhookMessage<'_, O> {
        DeleteWebhookMessage {
            hook: self,
            id,
            thread: None,
        }
    }

    fn execute(&self, msg: V2Message) -> ExecuteWebhook<'_, O> {
        ExecuteWebhook {
            hook: self,
            msg,
            flags: CreateFlags::empty(),
            username: None,
            avatar_url: None,
            thread: None,
        }
    }

    fn edit_by_id(&self, id: Id<MessageMarker>, msg: V2Message) -> EditWebhookMessage<'_, O> {
        EditWebhookMessage {
            hook: self,
            id,
            msg,
            flags: EditFlags::empty(),
            thread: None,
            known: None,
        }
    }

    /// The App and the valid token, checked before any request (token
    /// first).
    fn ready(&self) -> Result<(&Arc<AppShared>, &str), WebhookError> {
        if !valid_token(&self.token) {
            return Err(WebhookError::InvalidToken);
        }
        let app = self.app.as_ref().ok_or(WebhookError::Detached)?;
        Ok((app, &self.token))
    }
}

impl Webhook<Owned> {
    /// Sends `msg` (Execute Webhook, `?wait=true&with_components=true`):
    /// any V2 message, interactive components included; a
    /// [`StaticMessage`](noctis_ui::StaticMessage) converts too.
    pub fn send(&self, msg: impl Into<V2Message>) -> ExecuteWebhook<'_, Owned> {
        self.execute(msg.into())
    }

    /// Edits the webhook's message `id` into `msg` (Edit Webhook Message).
    /// An edit by id knows no attachment of the target: every attachment
    /// `msg` uses is uploaded again and the others are dropped; edit
    /// through the [`WebhookMessage`] of a send to keep them.
    pub fn edit(
        &self,
        id: Id<MessageMarker>,
        msg: impl Into<V2Message>,
    ) -> EditWebhookMessage<'_, Owned> {
        self.edit_by_id(id, msg.into())
    }
}

impl Webhook<Foreign> {
    /// Sends the static `msg` (Execute Webhook,
    /// `?wait=true&with_components=true`); a [`V2Message`] is a `[WH-02]`
    /// compile error: convert one built at run time with
    /// [`StaticMessage::try_from`](noctis_ui::StaticMessage).
    pub fn send(&self, msg: impl IntoStaticMessage) -> ExecuteWebhook<'_, Foreign> {
        self.execute(noctis_ui::__into_static(msg).into())
    }

    /// Edits the webhook's message `id` into the static `msg`, as
    /// [`Webhook::<Owned>::edit`](Webhook::edit).
    pub fn edit(
        &self,
        id: Id<MessageMarker>,
        msg: impl IntoStaticMessage,
    ) -> EditWebhookMessage<'_, Foreign> {
        self.edit_by_id(id, noctis_ui::__into_static(msg).into())
    }
}

/// A pending Execute Webhook; await it. It always sends `wait=true` and
/// `with_components=true` (WH-01): there is no setter for either.
///
/// ```compile_fail,E0599
/// use noctis_core::OwnedWebhook;
/// use noctis_ui::{Components, TextDisplay, V2Message};
///
/// fn send(webhook: &OwnedWebhook) {
///     let msg = V2Message::new(Components::new().push(TextDisplay::new("x")));
///     let _ = webhook.send(msg).with_components(false); // [WH-01]
/// }
/// ```
#[derive(Debug)]
#[must_use = "a webhook request does nothing until it is awaited"]
pub struct ExecuteWebhook<'a, O: Ownership> {
    hook: &'a Webhook<O>,
    msg: V2Message,
    flags: CreateFlags,
    username: Option<WebhookName>,
    avatar_url: Option<String>,
    thread: Option<WebhookThread>,
}

impl<O: Ownership> ExecuteWebhook<'_, O> {
    /// The message flags (Execute Webhook takes `SUPPRESS_EMBEDS` and
    /// `SUPPRESS_NOTIFICATIONS`, MSG-07); `IS_COMPONENTS_V2` is always set.
    pub fn flags(mut self, flags: CreateFlags) -> Self {
        self.flags = flags;
        self
    }

    /// The name shown for this message instead of the webhook's (WH-04).
    pub fn username(mut self, name: WebhookName) -> Self {
        self.username = Some(name);
        self
    }

    /// The avatar URL shown for this message instead of the webhook's.
    pub fn avatar_url(mut self, url: impl Into<String>) -> Self {
        self.avatar_url = Some(url.into());
        self
    }

    /// The thread: an existing one, or a new forum or media post (WH-05).
    pub fn thread(mut self, thread: impl Into<WebhookThread>) -> Self {
        self.thread = Some(thread.into());
        self
    }

    async fn run(self) -> Result<WebhookMessage<O>, WebhookError> {
        let hook = self.hook;
        let (app, token) = hook.ready()?;
        let decl = self
            .thread
            .as_ref()
            .map_or(WebhookThreadDecl::None, WebhookThread::decl);
        check_thread(hook.kind, decl, app)?;
        let shared = Shared::webhook(Arc::clone(app));
        let mut msg = OutgoingMessage::V2(self.msg);
        shared.prepare(&mut msg, Ephemerality::Public).await?;
        let OutgoingMessage::V2(prepared) = &msg else {
            return Err(WebhookError::Transport);
        };
        let mut json = encode(prepared.create_body(self.flags))?;
        if let Value::Object(map) = &mut json {
            if let Some(name) = &self.username {
                map.insert("username".to_owned(), json!(name.as_str()));
            }
            if let Some(url) = &self.avatar_url {
                map.insert("avatar_url".to_owned(), json!(url));
            }
            if let Some(WebhookThread::Create(name)) = &self.thread {
                map.insert("thread_name".to_owned(), json!(name));
            }
        }
        let body = shared.body(&msg, json, None, None)?;
        let thread_id = match &self.thread {
            Some(WebhookThread::Existing(id)) => Some(*id),
            _ => None,
        };
        let route = Route::ExecuteWebhook {
            thread_id: thread_id.map(Id::get),
            token,
            wait: Some(true),
            with_components: Some(true),
            webhook_id: hook.id.get(),
        };
        let response = transport::send(app, &route, Some(body.payload), false)
            .await
            .map_err(forum_error)?;
        let id = transport::message_id(&response)?;
        let Posted {
            channel_id, files, ..
        } = transport::posted(&response);
        let channel_id = channel_id.ok_or(WebhookError::Transport)?;
        let thread_id = match &self.thread {
            Some(WebhookThread::Existing(id)) => Some(*id),
            Some(WebhookThread::Create(_)) => Some(channel_id),
            None => None,
        };
        Ok(WebhookMessage {
            hook: hook.clone(),
            id,
            channel_id,
            thread_id,
            files: Mutex::new(files.unwrap_or(body.kept)),
        })
    }
}

impl<'a, O: Ownership> IntoFuture for ExecuteWebhook<'a, O> {
    type Output = Result<WebhookMessage<O>, WebhookError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// A pending Edit Webhook Message; await it. It always sends
/// `with_components=true` (WH-01) and clears `content`, `embeds` and `poll`
/// (WH-03).
#[derive(Debug)]
#[must_use = "a webhook request does nothing until it is awaited"]
pub struct EditWebhookMessage<'a, O: Ownership> {
    hook: &'a Webhook<O>,
    id: Id<MessageMarker>,
    msg: V2Message,
    flags: EditFlags,
    thread: Option<Id<ChannelMarker>>,
    /// The handle's known attachments (`None` for an edit by id).
    known: Option<&'a Mutex<KnownFiles>>,
}

impl<O: Ownership> EditWebhookMessage<'_, O> {
    /// The edit flags (`SUPPRESS_EMBEDS`, MSG-07); `IS_COMPONENTS_V2` is
    /// always set.
    pub fn flags(mut self, flags: EditFlags) -> Self {
        self.flags = flags;
        self
    }

    /// The thread the message is in (needed in a forum or media channel,
    /// WH-05).
    pub fn thread(mut self, thread: Id<ChannelMarker>) -> Self {
        self.thread = Some(thread);
        self
    }

    async fn run(self) -> Result<(), WebhookError> {
        let hook = self.hook;
        let (app, token) = hook.ready()?;
        let decl = match self.thread {
            Some(_) => WebhookThreadDecl::Id,
            None => WebhookThreadDecl::None,
        };
        check_thread(hook.kind, decl, app)?;
        let shared = Shared::webhook(Arc::clone(app));
        let mut msg = OutgoingMessage::V2(self.msg);
        shared.prepare(&mut msg, Ephemerality::Public).await?;
        let OutgoingMessage::V2(prepared) = &msg else {
            return Err(WebhookError::Transport);
        };
        let json = encode(prepared.webhook_edit_body(self.flags))?;
        let known = self.known.map_or(KnownFiles::EMPTY, |k| {
            k.lock().unwrap_or_else(PoisonError::into_inner).clone()
        });
        let body = shared.body(&msg, json, Some(&known), None)?;
        let thread = self
            .thread
            .map(|thread| format!("&thread_id={thread}"))
            .unwrap_or_default();
        let path = format!(
            "webhooks/{}/{token}/messages/{}?with_components=true{thread}",
            hook.id, self.id
        );
        let builder = RequestBuilder::raw(Method::Patch, path).use_authorization_token(false);
        let response = transport::send_built(app, builder, Some(body.payload))
            .await
            .map_err(forum_error)?;
        if let Some(known) = self.known {
            let files = transport::posted(&response).files.unwrap_or(body.kept);
            *known.lock().unwrap_or_else(PoisonError::into_inner) = files;
        }
        Ok(())
    }
}

impl<'a, O: Ownership> IntoFuture for EditWebhookMessage<'a, O> {
    type Output = Result<(), WebhookError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// A pending Delete Webhook Message; await it.
#[derive(Debug)]
#[must_use = "a webhook request does nothing until it is awaited"]
pub struct DeleteWebhookMessage<'a, O: Ownership> {
    hook: &'a Webhook<O>,
    id: Id<MessageMarker>,
    thread: Option<Id<ChannelMarker>>,
}

impl<O: Ownership> DeleteWebhookMessage<'_, O> {
    /// The thread the message is in (needed in a forum or media channel,
    /// WH-05).
    pub fn thread(mut self, thread: Id<ChannelMarker>) -> Self {
        self.thread = Some(thread);
        self
    }

    async fn run(self) -> Result<(), WebhookError> {
        let hook = self.hook;
        let (app, token) = hook.ready()?;
        let decl = match self.thread {
            Some(_) => WebhookThreadDecl::Id,
            None => WebhookThreadDecl::None,
        };
        check_thread(hook.kind, decl, app)?;
        let route = Route::DeleteWebhookMessage {
            message_id: self.id.get(),
            thread_id: self.thread.map(Id::get),
            token,
            webhook_id: hook.id.get(),
        };
        transport::send(app, &route, None, false)
            .await
            .map_err(forum_error)?;
        Ok(())
    }
}

impl<'a, O: Ownership> IntoFuture for DeleteWebhookMessage<'a, O> {
    type Output = Result<(), WebhookError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

/// A message a webhook sent: its id, channel and thread, and the
/// attachments Discord holds for it, so [`edit`](WebhookMessage::edit)
/// keeps them without uploading them again.
#[derive(Debug)]
pub struct WebhookMessage<O: Ownership> {
    hook: Webhook<O>,
    id: Id<MessageMarker>,
    channel_id: Id<ChannelMarker>,
    thread_id: Option<Id<ChannelMarker>>,
    files: Mutex<KnownFiles>,
}

impl<O: Ownership> WebhookMessage<O> {
    /// The message id.
    pub fn id(&self) -> Id<MessageMarker> {
        self.id
    }

    /// The channel Discord posted it in (the thread for a thread send).
    pub fn channel_id(&self) -> Id<ChannelMarker> {
        self.channel_id
    }

    /// The thread it is in: the [`WebhookThread::Existing`] id, or the new
    /// post's channel for [`WebhookThread::Create`]; `None` outside a
    /// thread.
    pub fn thread_id(&self) -> Option<Id<ChannelMarker>> {
        self.thread_id
    }

    /// Deletes the message (in its thread).
    pub fn delete(&self) -> DeleteWebhookMessage<'_, O> {
        DeleteWebhookMessage {
            hook: &self.hook,
            id: self.id,
            thread: self.thread_id,
        }
    }

    fn edit_known(&self, msg: V2Message) -> EditWebhookMessage<'_, O> {
        EditWebhookMessage {
            hook: &self.hook,
            id: self.id,
            msg,
            flags: EditFlags::empty(),
            thread: self.thread_id,
            known: Some(&self.files),
        }
    }
}

impl WebhookMessage<Owned> {
    /// Edits the message into `msg` (in its thread): attachments it still
    /// holds are kept by id, new ones are uploaded, unused ones dropped.
    pub fn edit(&self, msg: impl Into<V2Message>) -> EditWebhookMessage<'_, Owned> {
        self.edit_known(msg.into())
    }
}

impl WebhookMessage<Foreign> {
    /// Edits the message into the static `msg`, as
    /// [`WebhookMessage::<Owned>::edit`](WebhookMessage::edit).
    pub fn edit(&self, msg: impl IntoStaticMessage) -> EditWebhookMessage<'_, Foreign> {
        self.edit_known(noctis_ui::__into_static(msg).into())
    }
}
