//! The `Shard::Generic` adapter: Songbird's op 4 path through Noctis's
//! gateway (VC-01, GW-04).

use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use noctis_gateway::{GatewayError, GatewayHandle, VoiceStateRequest};
use songbird::error::{JoinError, JoinResult};
use songbird::id::{ChannelId, GuildId};
use songbird::shards::VoiceUpdate;
use twilight_model::id::Id;

/// Sends a `Call`'s Voice State Updates with
/// [`GatewayHandle::update_voice_state`]: the owning shard, the Shard Range
/// check and the GW-04 window, never a second send path.
///
/// Songbird can only return a [`JoinError`], so a refusal is stored here
/// and reported as [`JoinError::NoSender`]; the caller takes it back with
/// [`take_error`](Self::take_error) while it still holds the `Call`'s lock.
pub(crate) struct NoctisShard {
    /// The App's handle; every clone follows attach and detach.
    gateway: GatewayHandle,
    /// The configured `self_deaf`, sent instead of Songbird's (always
    /// `false` unless `deafen` sends an extra op 4).
    self_deaf: bool,
    /// The last refusal, until taken.
    last_error: Mutex<Option<GatewayError>>,
}

impl NoctisShard {
    pub(crate) fn new(gateway: GatewayHandle, self_deaf: bool) -> Self {
        Self {
            gateway,
            self_deaf,
            last_error: Mutex::new(None),
        }
    }

    /// The refusal behind the last [`JoinError::NoSender`], if any.
    pub(crate) fn take_error(&self) -> Option<GatewayError> {
        self.last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    fn send(&self, guild: GuildId, channel: Option<ChannelId>, self_mute: bool) -> JoinResult<()> {
        let guild = Id::from(guild.0);
        let request = match channel {
            Some(channel) => VoiceStateRequest::join(guild, Id::from(channel.0)),
            None => VoiceStateRequest::leave(guild),
        }
        .self_deaf(self.self_deaf)
        .self_mute(self_mute);
        self.gateway.update_voice_state(request).map_err(|error| {
            *self
                .last_error
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(error);
            JoinError::NoSender
        })
    }
}

/// Written out as `#[async_trait]` expands it (no `async-trait`
/// dependency); the send is synchronous, so the future is ready at once.
impl VoiceUpdate for NoctisShard {
    fn update_voice_state<'life0, 'async_trait>(
        &'life0 self,
        guild_id: GuildId,
        channel_id: Option<ChannelId>,
        _self_deaf: bool,
        self_mute: bool,
    ) -> Pin<Box<dyn Future<Output = JoinResult<()>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(ready(self.send(guild_id, channel_id, self_mute)))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;

    fn id(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap_or(NonZeroU64::MIN)
    }

    /// VC-01: a refusal is kept for the caller and reported as `NoSender`.
    #[tokio::test]
    async fn vc_01_refusal_is_kept() {
        let shard = NoctisShard::new(GatewayHandle::detached(), true);
        let result = shard
            .update_voice_state(GuildId(id(1)), Some(ChannelId(id(2))), false, false)
            .await;
        assert!(matches!(result, Err(JoinError::NoSender)));
        assert_eq!(shard.take_error(), Some(GatewayError::NotRunning));
        assert_eq!(shard.take_error(), None);
    }
}
