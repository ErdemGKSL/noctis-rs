//! A complete music bot on Noctis's `voice` feature (FR-80):
//!
//! - `/play query` joins the author's voice channel (found with Discord's "Get User
//!   Voice State", `ctx.http().user_voice_state`) and queues a `yt-dlp` track: a link
//!   or a search (`ytsearch1:`, [`input`]);
//! - a public Components V2 now-playing View with Routed Pause/Resume, Skip and Stop
//!   buttons and a paginated queue ([`view`]), updated in place with `ctx.update`;
//! - `/queue`, `/pause`, `/resume`, `/skip`, `/stop`, `/volume percent`, `/leave`
//!   ([`music`]); a call idle for 5 minutes is left.
//!
//! **Runtime requirement.** Tracks are `yt-dlp` child processes, so `yt-dlp` must be on
//! `PATH`. It asks for `bestaudio[acodec=opus]`: 48 kHz Opus plays by passthrough,
//! without decode or re-encode.
//!
//! **SSRF.** `yt-dlp` fetches any public or private host it is given (loopback,
//! `169.254.169.254`, ...). Only queries `Track::url` accepts become links; anything
//! else is a `ytsearch1:` search, so `file://` or a credentialed URL is never fetched
//! as a link. A production bot should still allow-list the sites it plays from.
//!
//! The tests (`tests/music.rs`) run every command and button through `noctis::testkit`
//! with `VoiceConfig::recording()`: no UDP, no `yt-dlp`, no network.

pub mod input;
pub mod music;
pub mod view;

use std::time::Duration;

use noctis::prelude::*;
use noctis::voice::VoiceConfig;

/// Installs voice (leaving a call idle for 5 minutes) and the `music` Module on
/// `builder`; `main` and the tests share it.
pub fn configure(builder: &mut AppBuilder) -> &mut AppBuilder {
    builder
        .voice(VoiceConfig::default().idle_leave(Some(Duration::from_mins(5))))
        .module(music::module())
}
