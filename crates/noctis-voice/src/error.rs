//! [`VoiceError`]: why a voice operation failed.

use std::time::Duration;

use noctis_core::gateway::GatewayError;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

/// Why [`Voice::join`](crate::Voice::join), [`Voice::leave`](crate::Voice::leave)
/// or [`CallHandle::leave`](crate::CallHandle::leave) failed.
///
/// ```
/// use noctis_gateway::GatewayError;
/// use noctis_voice::VoiceError;
/// use twilight_model::id::Id;
///
/// let refused = VoiceError::Gateway(GatewayError::NotRunning);
/// assert!(refused.to_string().starts_with("[VC-01]"));
/// let replaced = VoiceError::Interrupted { guild: Id::new(1) };
/// assert!(replaced.to_string().starts_with("[VC-02]"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceError {
    /// The App has no voice: call `.voice(VoiceConfig::default())` on the
    /// `AppBuilder`.
    #[error(
        "voice is not installed: call `.voice(VoiceConfig::default())` on the AppBuilder \
         (feature `voice`)"
    )]
    NotInstalled,
    /// The gateway runs but no shard has received READY yet, so the bot's
    /// user id is unknown.
    #[error("no shard has received READY yet: the bot user id is unknown; retry later")]
    NotReady,
    /// The Voice State Update (op 4) was refused before it was sent (VC-01):
    /// the App is not running, the guild is outside this process's Shard
    /// Range, its shard ended, or the shard is over its GW-04 budget.
    #[error("[VC-01] the voice state update was not sent: {0}")]
    Gateway(GatewayError),
    /// Discord did not answer the join within `VoiceConfig::join_timeout`;
    /// the call was left and removed.
    #[error("the voice connection to guild {guild} timed out after {after:?}")]
    Timeout {
        /// The guild.
        guild: Id<GuildMarker>,
        /// The join timeout that elapsed.
        after: Duration,
    },
    /// The voice driver could not connect; the call was left and removed.
    #[error("the voice driver of guild {guild} failed: {reason}")]
    Driver {
        /// The guild.
        guild: Id<GuildMarker>,
        /// The driver's error message (never a token).
        reason: String,
    },
    /// The guild's call was replaced by a newer join, left, disconnected or
    /// removed (VC-02: one voice connection per guild).
    #[error(
        "[VC-02] the voice connection of guild {guild} was replaced, left or closed before \
         this operation finished"
    )]
    Interrupted {
        /// The guild.
        guild: Id<GuildMarker>,
    },
}
