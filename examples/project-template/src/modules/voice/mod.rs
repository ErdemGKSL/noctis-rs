//! Voice (template feature `voice`, off by default): `/join channel`, `/leave`,
//! `/play url` (joins the author's voice channel when the bot is in none), `/skip` and
//! `/stop`, plus a hook that logs failed tracks. `tests/voice.rs` tests them without UDP:
//! `VoiceConfig::recording()` plus `TestApp::answer_voice(true)`.
//!
//! `.voice(VoiceConfig::default())` in `configure` installs voice (it adds the
//! `GUILD_VOICE_STATES` intent); `ctx.voice()` is the App's `Voice`. Every op 4 goes
//! through the guild's owning shard inside the GW-04 budget ([VC-01]), and a guild has
//! one call: `/join` in another channel moves it ([VC-02]). The Noctis `voice` feature
//! builds libopus from source, so it needs a C compiler and cmake.
//!
//! Each call has a queue on its `CallHandle` (`enqueue`, `skip`, `pause`, `resume`,
//! `stop`, `clear`, `set_volume`, `now_playing`, `list`), at most
//! `VoiceConfig::max_queue` tracks (default 1000). Track events (`TrackStart`,
//! `TrackEnd`, `TrackError`, `QueueEmpty`) are hook events a `#[derive(Hook)]` takes.
//!
//! Pre-encoded Opus (Ogg or `WebM` Opus at 48 kHz with 20 ms frames) plays by
//! passthrough: the packets go to Discord without decode or re-encode while the track
//! plays alone at volume 1.0. Other inputs are decoded and encoded at the channel's
//! bitrate (capped by `VoiceConfig::max_bitrate`).

use noctis::lifecycle::HookCtx;
use noctis::prelude::*;
use noctis::twilight::http::error::ErrorType;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker};
use noctis::voice::{Track, TrackError};

use crate::strings::{Strings, keys};

/// `/join channel`: joins (or moves to) a voice or stage channel. The join waits for
/// Discord and the voice driver (up to `VoiceConfig::join_timeout`, 10 s), so the
/// auto-defer acknowledges it first; `response = ephemeral` keeps that private.
#[derive(SlashCommand, Debug)]
#[command("join", contexts = [Guild], response = ephemeral)]
pub struct Join {
    /// The description is `cmd.join.opt.channel.description` in `locales/en-US.yaml`.
    #[option(channel_types = [GuildVoice, GuildStageVoice])]
    pub channel: Id<ChannelMarker>,
}

impl Join {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(keys::COMMON_GUILD_ONLY).await;
        };
        let s = Strings::of(ctx.response_locale());
        match ctx.voice().join(guild, self.channel).await {
            Ok(call) => {
                let channel = call.channel().unwrap_or(self.channel);
                ctx.reply_text(s.voice.joined(format!("<#{channel}>")))
                    .await
            }
            // A typed `VoiceError`: Gateway ([VC-01]), Timeout, Driver, Interrupted ([VC-02]).
            Err(error) => ctx.reply_text(s.voice.failed(error)).await,
        }
    }
}

/// `/leave`: leaves this server's voice channel (op 4 with no channel).
#[derive(SlashCommand, Debug)]
#[command("leave", contexts = [Guild], response = ephemeral)]
pub struct Leave;

impl Leave {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(keys::COMMON_GUILD_ONLY).await;
        };
        let s = Strings::of(ctx.response_locale());
        match ctx.voice().leave(guild).await {
            Ok(()) => ctx.reply_text(s.voice.left()).await,
            Err(error) => ctx.reply_text(s.voice.failed(error)).await,
        }
    }
}

/// `/play url`: queues a media URL in this server's call. Without a call it first joins
/// the author's voice channel, read with Discord's "Get User Voice State"
/// (`ctx.http().user_voice_state`, one request; a 404 means they are in none).
/// `Track::url` refuses non-http(s) URLs, credentials and Discord's API; the queue
/// refuses more than `max_queue` tracks (`VoiceError::QueueFull`).
/// Warning: the media client fetches any public or private host (loopback,
/// `169.254.169.254`, ...), so a real bot should allow-list hosts (for example only
/// `cdn.discordapp.com` attachments) before `Track::url`.
#[derive(SlashCommand, Debug)]
#[command("play", contexts = [Guild], response = ephemeral)]
pub struct Play {
    /// The description is `cmd.play.opt.url.description` in `locales/en-US.yaml`;
    /// `Track::url` accepts at most 4096 bytes.
    #[option(max_length = 4096)]
    pub url: String,
}

impl Play {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(keys::COMMON_GUILD_ONLY).await;
        };
        let s = Strings::of(ctx.response_locale());
        // Build the track first: an invalid URL must not leave the bot idle in a channel.
        let track = match Track::url(&self.url) {
            Ok(track) => track.title(&self.url).requested_by(ctx.user_id()),
            Err(error) => return ctx.reply_text(s.voice.failed(error)).await,
        };
        let call = if let Some(call) = ctx.voice().call(guild) {
            call
        } else {
            let Some(channel) = author_channel(&ctx, guild).await? else {
                return ctx.reply_text(s.voice.join_a_channel()).await;
            };
            match ctx.voice().join(guild, channel).await {
                Ok(call) => call,
                Err(error) => return ctx.reply_text(s.voice.failed(error)).await,
            }
        };
        match call.enqueue(track).await {
            Ok(info) => {
                let title = info.title().unwrap_or_default().to_owned();
                ctx.reply_text(s.voice.queued(title, call.queue_len()))
                    .await
            }
            Err(error) => ctx.reply_text(s.voice.failed(error)).await,
        }
    }
}

/// The voice channel the command's author is in, from Discord's "Get User Voice State"
/// (`GET /guilds/{guild}/voice-states/{user}`); `None` when Discord answers 404 (they
/// are in none). Noctis keeps no voice-state cache, so this is one request per call.
async fn author_channel(
    ctx: &CommandCtx,
    guild: Id<GuildMarker>,
) -> HandlerResult<Option<Id<ChannelMarker>>> {
    match ctx.http().user_voice_state(guild, ctx.user_id()).await {
        Ok(response) => Ok(response.model().await?.channel_id),
        Err(error) if matches!(error.kind(), ErrorType::Response { status, .. } if status.get() == 404) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

/// `/skip`: skips the current track; the next one starts.
#[derive(SlashCommand, Debug)]
#[command("skip", contexts = [Guild], response = ephemeral)]
pub struct Skip;

impl Skip {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let Some(call) = ctx.guild_id().and_then(|guild| ctx.voice().call(guild)) else {
            return ctx.reply_text(s.voice.not_in_call()).await;
        };
        match call.skip() {
            Ok(Some(track)) => {
                let title = track.title().unwrap_or_default().to_owned();
                ctx.reply_text(s.voice.skipped(title)).await
            }
            Ok(None) => ctx.reply_text(s.voice.nothing_playing()).await,
            Err(error) => ctx.reply_text(s.voice.failed(error)).await,
        }
    }
}

/// `/stop`: stops the current track and clears the queue; the bot stays in the channel.
#[derive(SlashCommand, Debug)]
#[command("stop", contexts = [Guild], response = ephemeral)]
pub struct Stop;

impl Stop {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let Some(call) = ctx.guild_id().and_then(|guild| ctx.voice().call(guild)) else {
            return ctx.reply_text(s.voice.not_in_call()).await;
        };
        match call.stop() {
            Ok(count) => ctx.reply_text(s.voice.stopped(count)).await,
            Err(error) => ctx.reply_text(s.voice.failed(error)).await,
        }
    }
}

/// Logs every failed track (`VOICE_TRACK_ERROR`); the queue has already moved on.
#[derive(Hook)]
pub struct TrackErrorLog;

impl TrackErrorLog {
    #[handler]
    async fn handle(self, _ctx: HookCtx, event: TrackError) -> HandlerResult<()> {
        tracing::warn!(guild = %event.guild, track = %event.track.id(), error = %event.error, "track failed");
        Ok(())
    }
}

/// The `voice` Module: `/join`, `/leave`, `/play`, `/skip`, `/stop` and the
/// `TrackErrorLog` hook.
pub fn module() -> Module {
    Module::new("voice")
        .add::<Join>()
        .add::<Leave>()
        .add::<Play>()
        .add::<Skip>()
        .add::<Stop>()
        .add::<TrackErrorLog>()
}
