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
//! - The gauge `noctis_voice_calls{app}` counts the live and pending calls.

mod call;
mod config;
mod error;
mod plugin;
mod shard;
mod voice;

pub use config::VoiceConfig;
pub use error::VoiceError;
pub use plugin::{AppBuilderVoiceExt, VoicePlugin};
pub use voice::{CallHandle, Voice, VoiceExt};
