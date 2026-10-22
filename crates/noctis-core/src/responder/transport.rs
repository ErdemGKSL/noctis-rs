//! Where a [`Responder`](super::Responder) sends: an interaction (the
//! initial callback over the hedged edge path, everything else over the
//! App's twilight client with the interaction token), a message reply for
//! the [`Dispatcher`](crate::Dispatcher), or nowhere (the `__testing`
//! seams).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use hyper::header::HeaderValue;
use serde_json::Value;
use twilight_http::error::ErrorType;
use twilight_http::request::Request;
use twilight_http::response::marker::EmptyBody;
use twilight_http::routing::Route;
use twilight_model::channel::Message;
use twilight_model::id::Id;
use twilight_model::id::marker::{
    ApplicationMarker, ChannelMarker, InteractionMarker, MessageMarker,
};

use super::ResponderError;
use super::files::{KnownFiles, Payload};
use crate::edge::callback::{CallbackBody, CallbackError, CallbackRequest, valid_token};
use crate::handler::AppShared;

/// Where a responder sends.
pub(crate) enum Transport {
    /// An interaction.
    Interaction(InteractionTarget),
    /// A reply to a message (the Dispatcher).
    Message(MessageTarget),
    /// Nowhere: no I/O (the `__testing` seams only).
    Detached,
    /// A webhook send of the App (`crate::webhook`): only the App's
    /// signer, `RefStore` and Limits are used; the webhook module sends
    /// its own requests, and the interaction methods behave as
    /// [`Detached`](Self::Detached) (they are never called).
    Webhook(Arc<AppShared>),
}

/// How long an interaction token stays valid after the interaction
/// arrived (INT-02).
pub(crate) const TOKEN_LIFETIME: Duration = Duration::from_mins(15);

/// One interaction to answer.
pub(crate) struct InteractionTarget {
    pub(crate) app: Arc<AppShared>,
    pub(crate) application_id: Id<ApplicationMarker>,
    pub(crate) interaction_id: Id<InteractionMarker>,
    /// Never printed.
    pub(crate) token: String,
    pub(crate) received_at: tokio::time::Instant,
    /// The channel the interaction was invoked in (the bot-token fallback
    /// after the token expired, INT-02).
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    /// The interaction's `attachment_size_limit` (ATT-01), when Discord sent
    /// a positive one.
    pub(crate) file_limit: Option<usize>,
}

/// What an edit response names: the message, its channel and its
/// attachments, when the body carries them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Posted {
    pub(crate) message_id: Option<Id<MessageMarker>>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    /// The message's attachments; `None` when the body is not a message.
    pub(crate) files: Option<KnownFiles>,
}

/// A sent follow-up: its id and, when the response names them, its channel
/// and attachments.
#[derive(Clone, Debug)]
pub(crate) struct Sent {
    pub(crate) id: Id<MessageMarker>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
    pub(crate) files: Option<KnownFiles>,
}

/// One message to reply to.
pub(crate) struct MessageTarget {
    pub(crate) app: Arc<AppShared>,
    pub(crate) channel_id: Id<ChannelMarker>,
    pub(crate) message_id: Id<MessageMarker>,
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interaction(target) => f
                .debug_struct("Interaction")
                .field("application_id", &target.application_id)
                .field("interaction_id", &target.interaction_id)
                .field("token", &"<redacted>")
                .finish_non_exhaustive(),
            Self::Message(target) => f
                .debug_struct("Message")
                .field("channel_id", &target.channel_id)
                .field("message_id", &target.message_id)
                .finish_non_exhaustive(),
            Self::Detached => f.write_str("Detached"),
            Self::Webhook(_) => f.write_str("Webhook"),
        }
    }
}

/// The placeholder id of a detached follow-up.
const DETACHED_ID: Id<MessageMarker> = Id::new(1);

impl Transport {
    pub(crate) fn is_message(&self) -> bool {
        matches!(self, Self::Message(_))
    }

    /// Edits the reply (`message_id`: the reply over a message transport);
    /// what the response names.
    pub(crate) async fn edit_reply(
        &self,
        message_id: Option<Id<MessageMarker>>,
        body: Payload,
    ) -> Result<Posted, ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::UpdateInteractionOriginal {
                    application_id: t.application_id.get(),
                    interaction_token: token,
                };
                Ok(posted(&send(&t.app, &route, Some(body), false).await?))
            }
            Self::Message(t) => match message_id {
                Some(id) => t.edit(id, body).await,
                None => Ok(Posted::default()),
            },
            Self::Detached | Self::Webhook(_) => Ok(Posted::default()),
        }
    }

    /// Fetches the reply (`message_id`: the reply over a message
    /// transport); [`ResponderError::NoReply`] when there is none to fetch.
    pub(crate) async fn fetch_reply(
        &self,
        message_id: Option<Id<MessageMarker>>,
    ) -> Result<Message, ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::GetInteractionOriginal {
                    application_id: t.application_id.get(),
                    interaction_token: token,
                };
                message(&send(&t.app, &route, None, false).await?)
            }
            Self::Message(t) => match message_id {
                Some(id) => channel_fetch(&t.app, t.channel_id, id).await,
                None => Err(ResponderError::NoReply),
            },
            Self::Detached | Self::Webhook(_) => Err(ResponderError::NoReply),
        }
    }

    /// Deletes the reply.
    pub(crate) async fn delete_reply(
        &self,
        message_id: Option<Id<MessageMarker>>,
    ) -> Result<(), ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::DeleteInteractionOriginal {
                    application_id: t.application_id.get(),
                    interaction_token: token,
                };
                send(&t.app, &route, None, false).await.map(drop)
            }
            Self::Message(t) => match message_id {
                Some(id) => t.delete(id).await,
                None => Ok(()),
            },
            Self::Detached | Self::Webhook(_) => Ok(()),
        }
    }

    /// Sends a follow-up (`body`: callback data, or a Create Message body
    /// over a message transport); its message id and channel.
    pub(crate) async fn follow_up(&self, body: Payload) -> Result<Sent, ResponderError> {
        let response = match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::ExecuteWebhook {
                    thread_id: None,
                    token,
                    wait: Some(true),
                    // Interaction webhooks never send `with_components` (INT-13).
                    with_components: None,
                    webhook_id: t.application_id.get(),
                };
                send(&t.app, &route, Some(body), false).await?
            }
            Self::Message(t) => {
                let route = Route::CreateMessage {
                    channel_id: t.channel_id.get(),
                };
                let response = send(&t.app, &route, Some(body), true).await?;
                let id = message_id(&response)?;
                return Ok(Sent {
                    id,
                    channel_id: None,
                    files: posted(&response).files,
                });
            }
            Self::Detached | Self::Webhook(_) => {
                return Ok(Sent {
                    id: DETACHED_ID,
                    channel_id: None,
                    files: None,
                });
            }
        };
        let id = message_id(&response)?;
        let posted = posted(&response);
        Ok(Sent {
            id,
            channel_id: posted.channel_id,
            files: posted.files,
        })
    }

    /// Edits the follow-up `id`; what the response names.
    pub(crate) async fn edit_message(
        &self,
        id: Id<MessageMarker>,
        body: Payload,
    ) -> Result<Posted, ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::UpdateWebhookMessage {
                    message_id: id.get(),
                    thread_id: None,
                    token,
                    webhook_id: t.application_id.get(),
                };
                Ok(posted(&send(&t.app, &route, Some(body), false).await?))
            }
            Self::Message(t) => t.edit(id, body).await,
            Self::Detached | Self::Webhook(_) => Ok(Posted::default()),
        }
    }

    /// Deletes the follow-up `id`.
    pub(crate) async fn delete_message(&self, id: Id<MessageMarker>) -> Result<(), ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::DeleteWebhookMessage {
                    message_id: id.get(),
                    thread_id: None,
                    token,
                    webhook_id: t.application_id.get(),
                };
                send(&t.app, &route, None, false).await.map(drop)
            }
            Self::Message(t) => t.delete(id).await,
            Self::Detached | Self::Webhook(_) => Ok(()),
        }
    }
}

impl InteractionTarget {
    /// The token, or [`ResponderError::InvalidToken`] before any request.
    pub(crate) fn token(&self) -> Result<&str, ResponderError> {
        if valid_token(&self.token) {
            Ok(&self.token)
        } else {
            Err(ResponderError::InvalidToken)
        }
    }

    /// Whether the token expired: 15 minutes after `received_at` (INT-02).
    pub(crate) fn expired(&self) -> bool {
        tokio::time::Instant::now() >= self.received_at + TOKEN_LIFETIME
    }

    /// [`callback`](Self::callback) of a JSON `body`.
    pub(crate) async fn callback_json(
        &self,
        body: &Value,
        with_response: bool,
    ) -> Result<Bytes, ResponderError> {
        let json = serde_json::to_vec(body).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        self.callback(Payload::Json(json), with_response).await
    }

    /// Sends the initial callback `body` (JSON, or multipart with uploads)
    /// over the hedged edge path (no bot token, INT-13) and records the ack
    /// latency; the response body (with `with_response`, the callback
    /// resource; empty otherwise).
    pub(crate) async fn callback(
        &self,
        body: Payload,
        with_response: bool,
    ) -> Result<Bytes, ResponderError> {
        let body = match body {
            Payload::Json(json) => CallbackBody::Json(Bytes::from(json)),
            Payload::Form(form) => CallbackBody::Raw {
                content_type: HeaderValue::from_bytes(&form.content_type()).map_err(|e| {
                    ResponderError::Encode {
                        reason: e.to_string(),
                    }
                })?,
                body: Bytes::from(form.build()),
            },
        };
        let request = CallbackRequest {
            interaction_id: self.interaction_id,
            token: self.token.clone(),
            body,
            with_response,
            received_at: self.received_at,
        };
        match self.app.edge.interaction_callback(request).await {
            Ok(ack) => {
                metrics::histogram!("noctis_ack_latency_seconds", "app" => Arc::clone(&self.app.app))
                    .record(self.received_at.elapsed().as_secs_f64());
                Ok(ack.body)
            }
            Err(CallbackError::InvalidToken) => Err(ResponderError::InvalidToken),
            Err(CallbackError::DeadlineMissed(missed)) => Err(missed.into()),
            Err(CallbackError::Rejected { status, code }) => {
                Err(ResponderError::Rejected { status, code })
            }
            Err(CallbackError::Transport) => Err(ResponderError::Transport),
        }
    }
}

impl MessageTarget {
    /// Makes the Create Message `body` a reply to the message.
    pub(crate) fn reference(&self, body: &mut Value) {
        if let Value::Object(map) = body {
            map.insert(
                "message_reference".to_owned(),
                serde_json::json!({
                    "message_id": self.message_id.to_string(),
                    "fail_if_not_exists": false,
                }),
            );
        }
    }

    /// Posts `body` (built with [`reference`](Self::reference)) as a reply
    /// to the message; the reply's id and attachments.
    pub(crate) async fn reply(
        &self,
        body: Payload,
    ) -> Result<(Id<MessageMarker>, Option<KnownFiles>), ResponderError> {
        let route = Route::CreateMessage {
            channel_id: self.channel_id.get(),
        };
        let response = send(&self.app, &route, Some(body), true).await?;
        Ok((message_id(&response)?, posted(&response).files))
    }

    /// Starts the typing indicator.
    pub(crate) async fn typing(&self) -> Result<(), ResponderError> {
        let route = Route::CreateTypingTrigger {
            channel_id: self.channel_id.get(),
        };
        send(&self.app, &route, None, true).await.map(drop)
    }

    async fn edit(&self, id: Id<MessageMarker>, body: Payload) -> Result<Posted, ResponderError> {
        channel_edit(&self.app, self.channel_id, id, body).await
    }

    async fn delete(&self, id: Id<MessageMarker>) -> Result<(), ResponderError> {
        channel_delete(&self.app, self.channel_id, id).await
    }
}

/// Edits the message `id` in `channel_id` with the bot token; what the
/// response names.
pub(crate) async fn channel_edit(
    app: &AppShared,
    channel_id: Id<ChannelMarker>,
    id: Id<MessageMarker>,
    body: Payload,
) -> Result<Posted, ResponderError> {
    let route = Route::UpdateMessage {
        channel_id: channel_id.get(),
        message_id: id.get(),
    };
    Ok(posted(&send(app, &route, Some(body), true).await?))
}

/// Fetches the message `id` in `channel_id` with the bot token.
pub(crate) async fn channel_fetch(
    app: &AppShared,
    channel_id: Id<ChannelMarker>,
    id: Id<MessageMarker>,
) -> Result<Message, ResponderError> {
    let route = Route::GetMessage {
        channel_id: channel_id.get(),
        message_id: id.get(),
    };
    message(&send(app, &route, None, true).await?)
}

/// Deletes the message `id` in `channel_id` with the bot token.
pub(crate) async fn channel_delete(
    app: &AppShared,
    channel_id: Id<ChannelMarker>,
    id: Id<MessageMarker>,
) -> Result<(), ResponderError> {
    let route = Route::DeleteMessage {
        channel_id: channel_id.get(),
        message_id: id.get(),
    };
    send(app, &route, None, true).await.map(drop)
}

/// Sends one request through the App's twilight client, with a JSON or
/// multipart body; the response body. `bot` sends the bot token (never for
/// interaction-token routes, INT-13).
pub(crate) async fn send(
    app: &AppShared,
    route: &Route<'_>,
    body: Option<Payload>,
    bot: bool,
) -> Result<Vec<u8>, ResponderError> {
    let builder = Request::builder(route).use_authorization_token(bot);
    send_built(app, builder, body).await
}

/// Sends the request of `builder` with `body` through the App's twilight
/// client; the response body. The builder carries its route (or raw path)
/// and whether it sends the bot token.
pub(crate) async fn send_built(
    app: &AppShared,
    mut builder: twilight_http::request::RequestBuilder,
    body: Option<Payload>,
) -> Result<Vec<u8>, ResponderError> {
    match body {
        Some(Payload::Json(bytes)) => builder = builder.body(bytes),
        Some(Payload::Form(form)) => builder = builder.form(form),
        None => {}
    }
    let request = builder.build().map_err(|error| {
        tracing::debug!(target: "noctis::responder", %error, "request not built");
        ResponderError::Transport
    })?;
    let response = app
        .http
        .request::<EmptyBody>(request)
        .await
        .map_err(|e| http_error(&e))?;
    response.bytes().await.map_err(|error| {
        tracing::debug!(target: "noctis::responder", %error, "response body not read");
        ResponderError::Transport
    })
}

/// Maps a twilight error: an error status to
/// [`ResponderError::Rejected`], anything else to
/// [`ResponderError::Transport`].
pub(crate) fn http_error(error: &twilight_http::Error) -> ResponderError {
    if let ErrorType::Response { body, status, .. } = error.kind() {
        return ResponderError::Rejected {
            status: status.get(),
            code: error_code(body),
        };
    }
    tracing::debug!(target: "noctis::responder", %error, "request failed");
    ResponderError::Transport
}

/// Discord's JSON error `code`, if the body has one.
fn error_code(body: &[u8]) -> Option<u32> {
    let value: Value = serde_json::from_slice(body).ok()?;
    u32::try_from(value.get("code")?.as_u64()?).ok()
}

/// A whole message object, or [`ResponderError::Transport`].
fn message(body: &[u8]) -> Result<Message, ResponderError> {
    serde_json::from_slice(body).map_err(|error| {
        tracing::debug!(target: "noctis::responder", %error, "response body is not a message");
        ResponderError::Transport
    })
}

/// The `id` of a message object.
pub(crate) fn message_id(body: &[u8]) -> Result<Id<MessageMarker>, ResponderError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| ResponderError::Transport)?;
    value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(Id::new_checked)
        .ok_or(ResponderError::Transport)
}

/// A snowflake string field of `value`.
pub(crate) fn snowflake<T>(value: Option<&Value>) -> Option<Id<T>> {
    value
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(Id::new_checked)
}

/// The `id`, `channel_id` and `attachments` of a message object, when
/// present; `files` only when the body is a message (an object with an
/// `id`).
pub(crate) fn posted(body: &[u8]) -> Posted {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Posted::default();
    };
    message_posted(&value)
}

/// [`posted`] of a parsed message object.
fn message_posted(value: &Value) -> Posted {
    Posted {
        message_id: snowflake(value.get("id")),
        channel_id: snowflake(value.get("channel_id")),
        files: value
            .get("id")
            .is_some()
            .then(|| KnownFiles::from_message(value)),
    }
}

/// The `resource.message` of a `with_response` callback body: its id and
/// attachments.
pub(crate) fn callback_posted(body: &[u8]) -> Posted {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Posted::default();
    };
    value
        .pointer("/resource/message")
        .map(message_posted)
        .unwrap_or_default()
}
