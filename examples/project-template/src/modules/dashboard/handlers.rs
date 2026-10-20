// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Every handler has the same shape, `impl X { #[handler] async fn handle(self, ctx, ..) }`
//! ([planned: Story 6.10]): `self` is the decoded Payload,
//! validate and authorize (Route Guards run before the handler), write to storage, then
//! `ctx.update(render_dashboard(..))` with freshly loaded data.

use std::time::UNIX_EPOCH;

use noctis::id::marker::GuildMarker;
use noctis::prelude::*;

use super::view::{DashData, render_dashboard};
use super::{DashTab, ModuleKey, OpenTab, ToggleModule, TurnPage};
use crate::BotMeta;
use crate::strings::{Strings, keys};

/// Entry point (UD-3): no options, so a unit struct; opens on the Overview tab. Public
/// by default (`response = public`), so the shared dashboard renders in the Guild Locale
/// (`ctx.response_locale()`).
#[derive(SlashCommand, Debug)]
#[command(
    "dashboard",
    description = "Open the server dashboard",
    permissions = MANAGE_GUILD,
    contexts = [Guild],
)]
pub struct Dashboard;

impl Dashboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx, meta: State<BotMeta>) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(keys::COMMON_GUILD_ONLY).await; // [planned, Story 8.1]
        };
        let data = load(&ctx, guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_dashboard(&data, &s, DashTab::Overview, 0, ctx.limits())).await // [planned, Story 8.1]
    }
}

impl OpenTab {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, meta: State<BotMeta>) -> HandlerResult<Responded> {
        let data = load(&ctx, self.guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_dashboard(&data, &s, self.tab, 0, ctx.limits())).await // [planned, Story 8.1]
    }
}

impl TurnPage {
    /// The page is untrusted client input: `render_dashboard` clamps it to the last page.
    #[handler]
    async fn handle(self, ctx: ComponentCtx, meta: State<BotMeta>) -> HandlerResult<Responded> {
        let data = load(&ctx, self.guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_dashboard(&data, &s, DashTab::Roles, self.page, ctx.limits())).await
    }
}

impl ToggleModule {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, meta: State<BotMeta>) -> HandlerResult<Responded> {
        ctx.modules()
            .set_enabled(self.module.module_id(), ModuleScope::Guild(self.guild), self.enable)
            .await?;
        // Re-render the same tab with the new state, in place.
        let data = load(&ctx, self.guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_dashboard(&data, &s, DashTab::Modules, 0, ctx.limits())).await
    }
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
