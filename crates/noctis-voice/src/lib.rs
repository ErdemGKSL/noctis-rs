//! Voice for Noctis (FR-80, AD-30). Internal; use `noctis` (feature `voice`).
//!
//! Joins and leaves voice channels through Noctis's own gateway: every
//! Voice State Update (op 4) goes through
//! [`GatewayHandle::update_voice_state`](noctis_gateway::GatewayHandle::update_voice_state)
//! from the guild's owning shard, inside the GW-04 budget (VC-01). Songbird
//! 0.6 is the voice driver (UDP, RTP, DAVE); its own gateway glue and
//! `Songbird` manager are never used, and no Songbird type is public.
//!
//! - [`AppBuilderVoiceExt::voice`] installs [`VoicePlugin`] with a
//!   [`VoiceConfig`] and adds the `GUILD_VOICE_STATES` intent.
//! - [`VoiceExt::voice`] (`ctx.voice()` on every handler context,
//!   `app.voice()`) is the App's [`Voice`]: [`join`](Voice::join),
//!   [`leave`](Voice::leave), [`call`](Voice::call) and
//!   [`active_calls`](Voice::active_calls).
//! - One call per guild (VC-02): a [`CallHandle`] names it; joining another
//!   channel of the guild moves the call.
//! - At shutdown, before the shards close, every call sends op 4 leave
//!   (an `AppBuilder::shutdown_task`).
//! - Each call has a bounded queue of [`Track`]s (files, `http`/`https`
//!   media URLs, child-process stdout) on its [`CallHandle`]: `enqueue`,
//!   `skip`, `pause`, `resume`, `stop`, `clear`, `set_volume`,
//!   `now_playing`, `list`. The driver only ever holds the current track.
//! - [`TrackStart`], [`TrackEnd`], [`TrackError`] and [`QueueEmpty`] are
//!   hook events (`noctis_core::lifecycle::ExtensionEvent`): a Module's
//!   `#[derive(Hook)]` handler takes them.
//! - URL tracks are fetched with the App's own media client: a connect
//!   timeout, a redirect cap, no proxy, no bot token or Noctis headers, and
//!   never Discord's API.
//! - `VoiceConfig::idle_leave` leaves calls that stayed idle.
//! - Demux formats are the features `ogg`, `mkv`, `mp3`, `aac`, `isomp4`,
//!   `flac` and `wav` (all on by default).
//! - The gauge `noctis_voice_calls{app}` counts the live and pending calls;
//!   the counter `noctis_voice_tracks_total{app, result}` counts the tracks
//!   that became current by outcome (`finished`, `skipped`, `stopped`,
//!   `error`).

mod call;
mod config;
mod error;
mod events;
mod media;
mod player;
mod plugin;
mod queue;
mod shard;
mod track;
mod voice;

pub use config::VoiceConfig;
pub use error::VoiceError;
pub use events::{EndReason, QueueEmpty, TrackEnd, TrackError, TrackStart};
pub use plugin::{AppBuilderVoiceExt, VoicePlugin};
pub use track::{Track, TrackId, TrackInfo, TrackSource};
pub use voice::{CallHandle, Voice, VoiceExt};

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
