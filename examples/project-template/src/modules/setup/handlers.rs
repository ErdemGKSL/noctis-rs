// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Handlers: decode (done by `#[handler]`), validate, write, `ctx.update(render_setup(..))`.

use noctis::prelude::*;

use super::view::{render_setup, render_setup_cancelled, render_setup_done};
use super::{
    CancelSetup, FinishSetup, GuildConfig, PickLogChannel, PickModRole, SetupState, SetupStep,
    config_key,
};
use crate::strings::{Strings, keys};

/// Entry point (UD-3): no options. `ephemeral`: a single-admin flow, so it is private
/// and renders in the User Locale.
#[command(
    "setup start",
    description = "Configure the bot for this server, step by step",
    ephemeral,
    permissions = MANAGE_GUILD,
    contexts = [Guild],
)]
pub(super) async fn setup_start(ctx: CommandCtx) -> HandlerResult<Responded> {
    let state = SetupState {
        owner: ctx.user_id(),
        step: SetupStep::Channel,
        log_channel: None,
        mod_role: None,
    };
    ctx.respond(render_setup(state, &Strings::of(ctx.response_locale()))).await // [planned, Story 8.1]
}

/// Back and Next. Next is rendered disabled until the step is valid, but a Payload is
/// client input (an old message, a forged id): check again and answer with a one-line
/// notice instead of moving on.
#[handler]
pub(super) async fn go_to_step(ctx: ComponentCtx, st: SetupState) -> HandlerResult<Responded> {
    let missing = match st.step {
        SetupStep::Channel => None,
        SetupStep::Role => st.log_channel.is_none().then_some(keys::SETUP_NEED_CHANNEL),
        SetupStep::Confirm => (st.log_channel.is_none() || st.mod_role.is_none())
            .then_some(keys::SETUP_NEED_ROLE),
    };
    if let Some(key) = missing {
        return ctx.reply_text(key).await; // ephemeral by default [planned, Story 8.1]
    }
    ctx.update(render_setup(st, &Strings::of(ctx.response_locale()))).await
}

/// Stays on the step so the user sees the choice, then presses Next.
#[handler]
pub(super) async fn pick_log_channel(ctx: ComponentCtx, picked: PickLogChannel) -> HandlerResult<Responded> {
    let st = SetupState {
        owner: picked.owner,
        step: SetupStep::Channel,
        log_channel: ctx.values().channels().first().copied(),
        mod_role: picked.mod_role,
    };
    ctx.update(render_setup(st, &Strings::of(ctx.response_locale()))).await
}

#[handler]
pub(super) async fn pick_mod_role(ctx: ComponentCtx, picked: PickModRole) -> HandlerResult<Responded> {
    let st = SetupState {
        owner: picked.owner,
        step: SetupStep::Role,
        log_channel: picked.log_channel,
        mod_role: ctx.values().roles().first().copied(),
    };
    ctx.update(render_setup(st, &Strings::of(ctx.response_locale()))).await
}

#[handler]
pub(super) async fn cancel_setup(ctx: ComponentCtx, _cancel: CancelSetup) -> HandlerResult<Responded> {
    ctx.update(render_setup_cancelled(&Strings::of(ctx.response_locale()))).await
}

#[handler]
pub(super) async fn finish_setup(ctx: ComponentCtx, done: FinishSetup) -> HandlerResult<Responded> {
    let s = Strings::of(ctx.response_locale());
    let Some(guild) = ctx.guild_id() else {
        return ctx.reply_text(keys::COMMON_GUILD_ONLY).await;
    };
    let config = GuildConfig { log_channel: done.log_channel, mod_role: done.mod_role };
    ctx.kv().put(&config_key(guild), &config, None).await?;
    ctx.update(render_setup_done(config, &s)).await
}
