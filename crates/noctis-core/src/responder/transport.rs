//! Where a [`Responder`](super::Responder) sends: an interaction (the
//! initial callback over the hedged edge path, everything else over the
//! App's twilight client with the interaction token), a message reply for
//! the [`Dispatcher`](crate::Dispatcher), or nowhere (the `__testing`
//! seams).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
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
}

/// What an edit response names: the message and its channel, when the
/// body carries them.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Posted {
    pub(crate) message_id: Option<Id<MessageMarker>>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
}

/// A sent follow-up: its id and, when the response names it, its channel.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sent {
    pub(crate) id: Id<MessageMarker>,
    pub(crate) channel_id: Option<Id<ChannelMarker>>,
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
        body: Value,
    ) -> Result<Posted, ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::UpdateInteractionOriginal {
                    application_id: t.application_id.get(),
                    interaction_token: token,
                };
                Ok(posted(&send(&t.app, &route, Some(&body), false).await?))
            }
            Self::Message(t) => match message_id {
                Some(id) => t.edit(id, &body).await.map(|()| Posted::default()),
                None => Ok(Posted::default()),
            },
            Self::Detached => Ok(Posted::default()),
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
            Self::Detached => Err(ResponderError::NoReply),
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
            Self::Detached => Ok(()),
        }
    }

    /// Sends a follow-up (`body`: callback data, or a Create Message body
    /// over a message transport); its message id and channel.
    pub(crate) async fn follow_up(&self, body: Value) -> Result<Sent, ResponderError> {
        let id = match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::ExecuteWebhook {
                    thread_id: None,
                    token,
                    wait: Some(true),
                    with_components: Some(true),
                    webhook_id: t.application_id.get(),
                };
                let response = send(&t.app, &route, Some(&body), false).await?;
                return Ok(Sent {
                    id: message_id(&response)?,
                    channel_id: posted(&response).channel_id,
                });
            }
            Self::Message(t) => {
                let route = Route::CreateMessage {
                    channel_id: t.channel_id.get(),
                };
                message_id(&send(&t.app, &route, Some(&body), true).await?)?
            }
            Self::Detached => DETACHED_ID,
        };
        Ok(Sent {
            id,
            channel_id: None,
        })
    }

    /// Edits the follow-up `id`.
    pub(crate) async fn edit_message(
        &self,
        id: Id<MessageMarker>,
        body: Value,
    ) -> Result<(), ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::UpdateWebhookMessage {
                    message_id: id.get(),
                    thread_id: None,
                    token,
                    webhook_id: t.application_id.get(),
                };
                send(&t.app, &route, Some(&body), false).await.map(drop)
            }
            Self::Message(t) => t.edit(id, &body).await,
            Self::Detached => Ok(()),
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
            Self::Detached => Ok(()),
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

    /// Sends the initial callback `body` over the hedged edge path and
    /// records the ack latency; with `with_response`, the message id the
    /// response names (`resource.message.id`), if any.
    pub(crate) async fn callback(
        &self,
        body: &Value,
        with_response: bool,
    ) -> Result<Option<Id<MessageMarker>>, ResponderError> {
        let body = serde_json::to_vec(body).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        let request = CallbackRequest {
            interaction_id: self.interaction_id,
            token: self.token.clone(),
            body: CallbackBody::Json(Bytes::from(body)),
            with_response,
            received_at: self.received_at,
        };
        match self.app.edge.interaction_callback(request).await {
            Ok(ack) => {
                metrics::histogram!("noctis_ack_latency_seconds", "app" => Arc::clone(&self.app.app))
                    .record(self.received_at.elapsed().as_secs_f64());
                Ok(callback_message_id(&ack.body))
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
    /// Posts `body` as a reply to the message; the reply's id.
    pub(crate) async fn reply(&self, mut body: Value) -> Result<Id<MessageMarker>, ResponderError> {
        if let Value::Object(map) = &mut body {
            map.insert(
                "message_reference".to_owned(),
                serde_json::json!({
                    "message_id": self.message_id.to_string(),
                    "fail_if_not_exists": false,
                }),
            );
        }
        let route = Route::CreateMessage {
            channel_id: self.channel_id.get(),
        };
        message_id(&send(&self.app, &route, Some(&body), true).await?)
    }

    /// Starts the typing indicator.
    pub(crate) async fn typing(&self) -> Result<(), ResponderError> {
        let route = Route::CreateTypingTrigger {
            channel_id: self.channel_id.get(),
        };
        send(&self.app, &route, None, true).await.map(drop)
    }

    async fn edit(&self, id: Id<MessageMarker>, body: &Value) -> Result<(), ResponderError> {
        channel_edit(&self.app, self.channel_id, id, body)
            .await
            .map(drop)
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
    body: &Value,
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

/// Sends one request through the App's twilight client; the response
/// body. `bot` sends the bot token (never for interaction-token routes).
async fn send(
    app: &AppShared,
    route: &Route<'_>,
    body: Option<&Value>,
    bot: bool,
) -> Result<Vec<u8>, ResponderError> {
    let mut builder = Request::builder(route).use_authorization_token(bot);
    if let Some(body) = body {
        let bytes = serde_json::to_vec(body).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        builder = builder.body(bytes);
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
fn http_error(error: &twilight_http::Error) -> ResponderError {
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
fn message_id(body: &[u8]) -> Result<Id<MessageMarker>, ResponderError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| ResponderError::Transport)?;
    value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(Id::new_checked)
        .ok_or(ResponderError::Transport)
}

/// A snowflake string field of `value`.
fn snowflake<T>(value: Option<&Value>) -> Option<Id<T>> {
    value
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(Id::new_checked)
}

/// The `id` and `channel_id` of a message object, when present.
fn posted(body: &[u8]) -> Posted {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Posted::default();
    };
    Posted {
        message_id: snowflake(value.get("id")),
        channel_id: snowflake(value.get("channel_id")),
    }
}

/// The `resource.message.id` of a `with_response` callback body.
fn callback_message_id(body: &[u8]) -> Option<Id<MessageMarker>> {
    let value: Value = serde_json::from_slice(body).ok()?;
    snowflake(value.pointer("/resource/message/id"))
}
