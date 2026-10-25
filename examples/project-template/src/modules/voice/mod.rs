//! Voice (template feature `voice`, off by default): `/join channel` and `/leave`.
//!
//! `.voice(VoiceConfig::default())` in `configure` installs voice (it adds the
//! `GUILD_VOICE_STATES` intent); `ctx.voice()` is the App's `Voice`. Every op 4 goes
//! through the guild's owning shard inside the GW-04 budget ([VC-01]), and a guild has
//! one call: `/join` in another channel moves it ([VC-02]). The Noctis `voice` feature
//! builds libopus from source, so it needs a C compiler and cmake.

use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::ChannelMarker;

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
                ctx.reply_text(s.voice.joined(format!("<#{channel}>"))).await
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

/// The `voice` Module: `/join` and `/leave`.
pub fn module() -> Module {
    Module::new("voice").add::<Join>().add::<Leave>()
}
