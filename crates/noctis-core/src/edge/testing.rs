//! The `__testing` seam: the crate-internal callback path for the testkit
//! end-to-end tests (Story 6.5 calls `Edge::interaction_callback`).

use bytes::Bytes;
use tokio::time::Instant;
use twilight_model::id::Id;

pub use super::callback::{CallbackAck, CallbackBody, CallbackError, CallbackRequest};
use crate::App;

/// Sends an interaction callback with a JSON `body` through `app`'s edge,
/// as if the interaction had been received at `received_at`. An `id` of 0
/// is [`CallbackError::InvalidToken`] (nothing is sent).
///
/// # Errors
///
/// The callback path's [`CallbackError`].
pub async fn interaction_callback(
    app: &App,
    id: u64,
    token: &str,
    body: Bytes,
    with_response: bool,
    received_at: Instant,
) -> Result<CallbackAck, CallbackError> {
    let interaction_id = Id::new_checked(id).ok_or(CallbackError::InvalidToken)?;
    app.edge()
        .interaction_callback(CallbackRequest {
            interaction_id,
            token: token.to_owned(),
            body: CallbackBody::Json(body),
            with_response,
            received_at,
        })
        .await
}

/// Keeps `app`'s upstream connection warm (the keeper `App::run` starts)
/// until the returned guard is dropped.
pub fn keep_warm(app: &App) -> KeepWarm {
    let (guard, _first) = app.edge().start_keeper();
    KeepWarm { _guard: guard }
}

/// Stops the keeper started by [`keep_warm`] when dropped.
pub struct KeepWarm {
    _guard: super::KeeperGuard,
}

impl std::fmt::Debug for KeepWarm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeepWarm")
    }
}
