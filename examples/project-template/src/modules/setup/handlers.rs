// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! The command struct and every `#[handler]` impl: `self` is the decoded Payload (done
//! by the derive), then validate, write, `ctx.update(render_setup(..))`.

use noctis::prelude::*;

use super::view::{render_setup, render_setup_cancelled, render_setup_done};
use super::{
    CancelSetup, FinishSetup, GuildConfig, PickLogChannel, PickModRole, SetupState, SetupStep,
    config_key,
};
use crate::strings::{Strings, keys};

/// Entry point (UD-3): no options, so a unit struct. `response = ephemeral`: a
/// single-admin flow, so it is private (the auto-defer too) and renders in the User
/// Locale.
#[derive(SlashCommand, Debug)]
#[command(
    "setup start",
    description = "Configure the bot for this server, step by step",
    response = ephemeral,
    permissions = MANAGE_GUILD,
    contexts = [Guild],
)]
pub struct SetupStart;

impl SetupStart {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let state = SetupState {
            owner: ctx.user_id(),
            step: SetupStep::Channel,
            log_channel: None,
            mod_role: None,
        };
        ctx.respond(render_setup(state, &Strings::of(ctx.response_locale()))).await // [planned, Story 8.1]
    }
}

impl SetupState {
    /// Back and Next. Next is rendered disabled until the step is valid, but a Payload is
    /// client input (an old message, a forged id): check again and answer with a one-line
    /// notice instead of moving on.
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let missing = match self.step {
            SetupStep::Channel => None,
            SetupStep::Role => self.log_channel.is_none().then_some(keys::SETUP_NEED_CHANNEL),
            SetupStep::Confirm => (self.log_channel.is_none() || self.mod_role.is_none())
                .then_some(keys::SETUP_NEED_ROLE),
        };
        if let Some(key) = missing {
            return ctx.reply_text(key).await; // ephemeral by default [planned, Story 8.1]
        }
        ctx.update(render_setup(self, &Strings::of(ctx.response_locale()))).await
    }
}

impl PickLogChannel {
    /// Stays on the step so the user sees the choice, then presses Next.
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let st = SetupState {
            owner: self.owner,
            step: SetupStep::Channel,
            log_channel: ctx.values().channels().first().copied(),
            mod_role: self.mod_role,
        };
        ctx.update(render_setup(st, &Strings::of(ctx.response_locale()))).await
    }
}

impl PickModRole {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let st = SetupState {
            owner: self.owner,
            step: SetupStep::Role,
            log_channel: self.log_channel,
            mod_role: ctx.values().roles().first().copied(),
        };
        ctx.update(render_setup(st, &Strings::of(ctx.response_locale()))).await
    }
}

impl CancelSetup {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(render_setup_cancelled(&Strings::of(ctx.response_locale()))).await
    }
}

impl FinishSetup {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(keys::COMMON_GUILD_ONLY).await;
        };
        let config = GuildConfig { log_channel: self.log_channel, mod_role: self.mod_role };
        ctx.kv().put(&config_key(guild), &config, None).await?;
        ctx.update(render_setup_done(config, &s)).await
    }
}
