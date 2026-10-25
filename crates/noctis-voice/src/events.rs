//! The track events: typed [`ExtensionEvent`]s a Module's `#[derive(Hook)]`
//! handlers take, fired from the guild's queue on the App's hook queue.

use noctis_core::lifecycle::ExtensionEvent;
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

use crate::track::TrackInfo;

/// A track became current and the driver reported it playable
/// (`VOICE_TRACK_START`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct TrackStart {
    /// The guild.
    pub guild: Id<GuildMarker>,
    /// The track.
    pub track: TrackInfo,
}

/// The current track ended: it finished, was skipped or was stopped
/// (`VOICE_TRACK_END`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEnd {
    /// The guild.
    pub guild: Id<GuildMarker>,
    /// The track.
    pub track: TrackInfo,
    /// Why it ended.
    pub reason: EndReason,
}

/// The current track failed: its input could not be created (a missing
/// file, a refused or failed fetch, a command that did not spawn), parsed or
/// decoded (`VOICE_TRACK_ERROR`). No [`TrackEnd`] follows; the next track
/// starts.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct TrackError {
    /// The guild.
    pub guild: Id<GuildMarker>,
    /// The track.
    pub track: TrackInfo,
    /// The driver's error message (never a token).
    pub error: String,
}

/// The queue ran out of tracks: the last one ended, failed, was skipped or
/// the queue was stopped (`VOICE_QUEUE_EMPTY`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct QueueEmpty {
    /// The guild.
    pub guild: Id<GuildMarker>,
}

/// Why a track ended ([`TrackEnd::reason`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndReason {
    /// It played to its end.
    Finished,
    /// [`CallHandle::skip`](crate::CallHandle::skip).
    Skipped,
    /// [`CallHandle::stop`](crate::CallHandle::stop).
    Stopped,
}

impl EndReason {
    /// `"finished"`, `"skipped"` or `"stopped"` (the `result` label of
    /// `noctis_voice_tracks_total`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Skipped => "skipped",
            Self::Stopped => "stopped",
        }
    }
}

impl ExtensionEvent for TrackStart {
    const NAME: &'static str = "VOICE_TRACK_START";
}

impl ExtensionEvent for TrackEnd {
    const NAME: &'static str = "VOICE_TRACK_END";
}

impl ExtensionEvent for TrackError {
    const NAME: &'static str = "VOICE_TRACK_ERROR";
}

impl ExtensionEvent for QueueEmpty {
    const NAME: &'static str = "VOICE_QUEUE_EMPTY";
}
