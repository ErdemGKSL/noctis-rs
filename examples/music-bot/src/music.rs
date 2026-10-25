//! The music commands. `/play` and `/queue` answer with the public player
//! ([`render_player`]); the controls answer privately with one line.

use noctis::lifecycle::HookCtx;
use noctis::prelude::*;
use noctis::twilight::http::error::ErrorType;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use noctis::ui::escape_markdown;
use noctis::voice::{CallHandle, TrackError};

use crate::input::track_for;
use crate::view::{PlayerState, QueuePage, SkipTrack, StopPlayback, TogglePause, render_player};

/// The reply without a call.
const NOT_IN_VOICE: &str = "I'm not in a voice channel.";

/// The channel `user` is in within `guild`, from Discord's "Get User Voice State"
/// (`GET /guilds/{guild}/voice-states/{user}`); `None` when they are in none (Discord
/// answers 404). One request per `/play`, with no voice-state cache.
async fn author_channel(
    ctx: &CommandCtx,
    guild: Id<GuildMarker>,
    user: Id<UserMarker>,
) -> HandlerResult<Option<Id<ChannelMarker>>> {
    match ctx.http().user_voice_state(guild, user).await {
        Ok(response) => Ok(response.model().await?.channel_id),
        Err(error) if matches!(error.kind(), ErrorType::Response { status, .. } if status.get() == 404) => {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

/// The guild's call, if the bot is in a voice channel there.
fn call_of(ctx: &CommandCtx) -> Option<CallHandle> {
    ctx.guild_id().and_then(|guild| ctx.voice().call(guild))
}

/// `/play query`: joins the author's voice channel when the bot is in none, queues
/// the track and shows the player.
#[derive(SlashCommand, Debug)]
#[command("play", description = "Play a song: a link or a search", contexts = [Guild])]
pub struct Play {
    /// The song name or link.
    #[option(
        description = "A song name or a link",
        min_length = 1,
        max_length = 512
    )]
    pub query: String,
}

impl Play {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text("Use this in a server.").await;
        };
        let Some(track) = track_for(&self.query, ctx.user_id()) else {
            return ctx.reply_text("Type a song name or a link.").await;
        };
        let Some(channel) = author_channel(&ctx, guild, ctx.user_id()).await? else {
            return ctx.reply_text("Join a voice channel first.").await;
        };
        let voice = ctx.voice();
        let call = match voice.call(guild) {
            // Never move a call someone else is listening to.
            Some(call) if call.channel().is_some_and(|c| c != channel) => {
                let playing_in = call.channel().unwrap_or(channel);
                return ctx
                    .reply_text(format!("I'm already playing in <#{playing_in}>."))
                    .await;
            }
            Some(call) => call,
            None => match voice.join(guild, channel).await {
                Ok(call) => call,
                Err(error) => return ctx.reply_text(format!("Could not join: {error}")).await,
            },
        };
        if let Err(error) = call.enqueue(track).await {
            return ctx.reply_text(format!("Could not queue it: {error}")).await;
        }
        ctx.respond(render_player(
            &PlayerState::of(Some(&call)),
            0,
            ctx.limits(),
        ))
        .await
    }
}

/// `/queue`: the player with the first page of the queue.
#[derive(SlashCommand, Debug)]
#[command("queue", description = "Show the player and the queue", contexts = [Guild])]
pub struct Queue;

impl Queue {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let call = call_of(&ctx);
        ctx.respond(render_player(
            &PlayerState::of(call.as_ref()),
            0,
            ctx.limits(),
        ))
        .await
    }
}

/// `/pause`.
#[derive(SlashCommand, Debug)]
#[command("pause", description = "Pause the current song", contexts = [Guild], response = ephemeral)]
pub struct Pause;

impl Pause {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(call) = call_of(&ctx) else {
            return ctx.reply_text(NOT_IN_VOICE).await;
        };
        let text = match call.pause() {
            Ok(true) => "Paused.",
            Ok(false) if call.now_playing().is_some() => "Already paused.",
            Ok(false) => "Nothing is playing.",
            Err(_) => NOT_IN_VOICE,
        };
        ctx.reply_text(text).await
    }
}

/// `/resume`.
#[derive(SlashCommand, Debug)]
#[command("resume", description = "Resume the paused song", contexts = [Guild], response = ephemeral)]
pub struct Resume;

impl Resume {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(call) = call_of(&ctx) else {
            return ctx.reply_text(NOT_IN_VOICE).await;
        };
        let text = match call.resume() {
            Ok(true) => "Resumed.",
            Ok(false) if call.now_playing().is_some() => "Already playing.",
            Ok(false) => "Nothing is playing.",
            Err(_) => NOT_IN_VOICE,
        };
        ctx.reply_text(text).await
    }
}

/// `/skip`: the next song starts.
#[derive(SlashCommand, Debug)]
#[command("skip", description = "Skip the current song", contexts = [Guild], response = ephemeral)]
pub struct Skip;

impl Skip {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(call) = call_of(&ctx) else {
            return ctx.reply_text(NOT_IN_VOICE).await;
        };
        match call.skip() {
            Ok(Some(track)) => {
                let title = escape_markdown(track.title().unwrap_or("Untitled"));
                ctx.reply_text(format!("Skipped {title}.")).await
            }
            Ok(None) => ctx.reply_text("Nothing is playing.").await,
            Err(_) => ctx.reply_text(NOT_IN_VOICE).await,
        }
    }
}

/// `/stop`: stops the song and clears the queue; the bot stays in the channel.
#[derive(SlashCommand, Debug)]
#[command("stop", description = "Stop and clear the queue", contexts = [Guild], response = ephemeral)]
pub struct Stop;

impl Stop {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(call) = call_of(&ctx) else {
            return ctx.reply_text(NOT_IN_VOICE).await;
        };
        match call.stop() {
            Ok(1) => ctx.reply_text("Stopped; 1 track removed.").await,
            Ok(n) => {
                ctx.reply_text(format!("Stopped; {n} tracks removed."))
                    .await
            }
            Err(_) => ctx.reply_text(NOT_IN_VOICE).await,
        }
    }
}

/// `/volume percent`: 0..=200 % (the queue's 0.0..=2.0).
#[derive(SlashCommand, Debug)]
#[command("volume", description = "Set the volume", contexts = [Guild], response = ephemeral)]
pub struct Volume {
    /// The volume in percent.
    #[option(
        description = "The volume in percent, 0-200",
        min_value = 0,
        max_value = 200
    )]
    pub percent: i64,
}

impl Volume {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(call) = call_of(&ctx) else {
            return ctx.reply_text(NOT_IN_VOICE).await;
        };
        // Discord enforces the declared bounds; clamp anyway, it is client input.
        let percent = u8::try_from(self.percent.clamp(0, 200)).unwrap_or(100);
        match call.set_volume(f32::from(percent) / 100.0) {
            Ok(()) => ctx.reply_text(format!("Volume set to {percent}%.")).await,
            Err(error) => {
                ctx.reply_text(format!("Could not set the volume: {error}"))
                    .await
            }
        }
    }
}

/// `/leave`: leaves the voice channel (the queue is dropped).
#[derive(SlashCommand, Debug)]
#[command("leave", description = "Leave the voice channel", contexts = [Guild], response = ephemeral)]
pub struct Leave;

impl Leave {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(call) = call_of(&ctx) else {
            return ctx.reply_text(NOT_IN_VOICE).await;
        };
        match call.leave().await {
            Ok(()) => ctx.reply_text("Left the voice channel.").await,
            Err(error) => ctx.reply_text(format!("Could not leave: {error}")).await,
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

/// The `music` Module: the commands, the player's Routes and the error log.
pub fn module() -> Module {
    Module::new("music")
        .add::<Play>()
        .add::<Queue>()
        .add::<Pause>()
        .add::<Resume>()
        .add::<Skip>()
        .add::<Stop>()
        .add::<Volume>()
        .add::<Leave>()
        .add::<TogglePause>()
        .add::<SkipTrack>()
        .add::<StopPlayback>()
        .add::<QueuePage>()
        .add::<TrackErrorLog>()
}
