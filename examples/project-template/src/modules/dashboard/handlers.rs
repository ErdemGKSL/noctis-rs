// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Every handler has the same shape: decode the Payload (done by `#[handler]`),
//! validate and authorize (Route Guards run before the handler), write to storage, then
//! `ctx.update(render_dashboard(..))` with freshly loaded data.

use std::time::UNIX_EPOCH;

use noctis::id::marker::GuildMarker;
use noctis::prelude::*;

use super::view::{DashData, render_dashboard};
use super::{DashTab, ModuleKey, OpenTab, ToggleModule, TurnPage};
use crate::BotMeta;
use crate::strings::{Strings, keys};

/// Entry point (UD-3): no options; opens on the Overview tab. Public by default, so the
/// shared dashboard renders in the Guild Locale (`ctx.response_locale()`).
#[command(
    "dashboard",
    description = "Open the server dashboard",
    permissions = MANAGE_GUILD,
    contexts = [Guild],
)]
pub(super) async fn dashboard(ctx: CommandCtx, meta: State<BotMeta>) -> HandlerResult<Responded> {
    let Some(guild) = ctx.guild_id() else {
        return ctx.reply_text(keys::COMMON_GUILD_ONLY).await; // [planned, Story 8.1]
    };
    let data = load(&ctx, guild, &meta).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.respond(render_dashboard(&data, &s, DashTab::Overview, 0, ctx.limits())).await // [planned, Story 8.1]
}

#[handler]
pub(super) async fn open_tab(ctx: ComponentCtx, to: OpenTab, meta: State<BotMeta>) -> HandlerResult<Responded> {
    let data = load(&ctx, to.guild, &meta).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.update(render_dashboard(&data, &s, to.tab, 0, ctx.limits())).await // [planned, Story 8.1]
}

/// The page is untrusted client input: `render_dashboard` clamps it to the last page.
#[handler]
pub(super) async fn turn_page(ctx: ComponentCtx, to: TurnPage, meta: State<BotMeta>) -> HandlerResult<Responded> {
    let data = load(&ctx, to.guild, &meta).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.update(render_dashboard(&data, &s, DashTab::Roles, to.page, ctx.limits())).await
}

#[handler]
pub(super) async fn toggle_module(
    ctx: ComponentCtx,
    toggle: ToggleModule,
    meta: State<BotMeta>,
) -> HandlerResult<Responded> {
    ctx.modules()
        .set_enabled(toggle.module.module_id(), ModuleScope::Guild(toggle.guild), toggle.enable)
        .await?;
    // Re-render the same tab with the new state, in place.
    let data = load(&ctx, toggle.guild, &meta).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.update(render_dashboard(&data, &s, DashTab::Modules, 0, ctx.limits())).await
}

/// All I/O happens here, before rendering.
async fn load(ctx: &impl Ctx, guild: Id<GuildMarker>, meta: &BotMeta) -> HandlerResult<DashData> {
    let guild_name = ctx.cache().guild(guild).await?.name;
    let mut roles = ctx.cache().roles(guild).await?;
    roles.sort_unstable_by(|a, b| b.position.cmp(&a.position));
    let modules = ModuleKey::ALL
        .into_iter()
        .map(|m| (m, ctx.modules().is_enabled(m.module_id(), Some(guild))))
        .collect();
    Ok(DashData {
        guild,
        guild_name,
        roles: roles.into_iter().map(|r| (format!("<@&{}>", r.id), r.name)).collect(),
        modules,
        started_unix: meta.started_at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        version: meta.version,
    })
}
