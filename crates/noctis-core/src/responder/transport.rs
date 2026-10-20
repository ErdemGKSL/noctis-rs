//! Where a [`Responder`](super::Responder) sends: an interaction (the
//! initial callback over the hedged edge path, everything else over the
//! App's twilight client with the interaction token), a message reply for
//! the [`Dispatcher`](crate::Dispatcher), or nowhere (the `__testing`
//! seams).

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use serde_json::Value;
use twilight_http::error::ErrorType;
use twilight_http::request::Request;
use twilight_http::response::marker::EmptyBody;
use twilight_http::routing::Route;
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

/// One interaction to answer.
pub(crate) struct InteractionTarget {
    pub(crate) app: Arc<AppShared>,
    pub(crate) application_id: Id<ApplicationMarker>,
    pub(crate) interaction_id: Id<InteractionMarker>,
    /// Never printed.
    pub(crate) token: String,
    pub(crate) received_at: tokio::time::Instant,
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

    /// Edits the original response (`message_id`: the reply over a message
    /// transport).
    pub(crate) async fn edit_original(
        &self,
        message_id: Option<Id<MessageMarker>>,
        body: Value,
    ) -> Result<(), ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::UpdateInteractionOriginal {
                    application_id: t.application_id.get(),
                    interaction_token: token,
                };
                send(&t.app, &route, Some(&body), false).await.map(drop)
            }
            Self::Message(t) => match message_id {
                Some(id) => t.edit(id, &body).await,
                None => Ok(()),
            },
            Self::Detached => Ok(()),
        }
    }

    /// Deletes the original response.
    pub(crate) async fn delete_original(
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
    /// over a message transport); its message id.
    pub(crate) async fn followup(&self, body: Value) -> Result<Id<MessageMarker>, ResponderError> {
        match self {
            Self::Interaction(t) => {
                let token = t.token()?;
                let route = Route::ExecuteWebhook {
                    thread_id: None,
                    token,
                    wait: Some(true),
                    with_components: Some(true),
                    webhook_id: t.application_id.get(),
                };
                message_id(&send(&t.app, &route, Some(&body), false).await?)
            }
            Self::Message(t) => {
                let route = Route::CreateMessage {
                    channel_id: t.channel_id.get(),
                };
                message_id(&send(&t.app, &route, Some(&body), true).await?)
            }
            Self::Detached => Ok(DETACHED_ID),
        }
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
    fn token(&self) -> Result<&str, ResponderError> {
        if valid_token(&self.token) {
            Ok(&self.token)
        } else {
            Err(ResponderError::InvalidToken)
        }
    }

    /// Sends the initial callback `body` over the hedged edge path.
    pub(crate) async fn callback(&self, body: &Value) -> Result<(), ResponderError> {
        let body = serde_json::to_vec(body).map_err(|e| ResponderError::Encode {
            reason: e.to_string(),
        })?;
        let request = CallbackRequest {
            interaction_id: self.interaction_id,
            token: self.token.clone(),
            body: CallbackBody::Json(Bytes::from(body)),
            with_response: false,
            received_at: self.received_at,
        };
        match self.app.edge.interaction_callback(request).await {
            Ok(_) => Ok(()),
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
        let route = Route::UpdateMessage {
            channel_id: self.channel_id.get(),
            message_id: id.get(),
        };
        send(&self.app, &route, Some(body), true).await.map(drop)
    }

    async fn delete(&self, id: Id<MessageMarker>) -> Result<(), ResponderError> {
        let route = Route::DeleteMessage {
            channel_id: self.channel_id.get(),
            message_id: id.get(),
        };
        send(&self.app, &route, None, true).await.map(drop)
    }
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
