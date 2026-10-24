// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Every handler has the same shape, `impl X { #[handler] async fn handle(self, ctx, ..) }`:
//! `self` is the decoded Payload,
//! validate and authorize (the Module's `guild_only` Middleware and the Route Guards run
//! before the handler), write to storage, then answer with
//! `ctx.update(render_dashboard(..))` on freshly loaded data: it updates the clicked
//! dashboard in place (`response = update`, the default); the Command answers with
//! `ctx.respond(..)`.

use std::time::UNIX_EPOCH;

use noctis::id::marker::GuildMarker;
use noctis::prelude::*;

use super::view::{DashData, module_name, render_dashboard};
use super::{DashGuild, DashTab, ModuleKey, OpenTab, ToggleModule, TurnPage};
use crate::BotMeta;
use crate::strings::Strings;

/// Entry point (UD-3): one optional lookup option; opens on the Overview tab, or on the
/// Modules tab when a module is picked. Public by default (`response = public`), so the
/// shared dashboard renders in the Guild Locale (`ctx.response_locale()`).
#[derive(SlashCommand, Debug)]
#[command(
    "dashboard",
    permissions = MANAGE_GUILD,
    contexts = [Guild],
)]
pub struct Dashboard {
    /// The description is `cmd.dashboard.opt.module.description` in `locales/en-US.yaml`.
    /// `autocomplete` pairs with the `#[autocomplete(module)]` method below (one without
    /// the other is an `[OPT-11]` error).
    #[option(autocomplete)]
    pub module: Option<String>,
}

impl Dashboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx, guild: Ext<DashGuild>) -> HandlerResult<Responded> {
        let meta = bot_meta(ctx.state::<BotMeta>())?;
        let guild = guild.0; // the `guild_only` Middleware stopped DMs already
        // Autocomplete does not constrain the value: only a known module id switches tabs.
        let tab = match self.module.as_deref().and_then(ModuleKey::from_id) {
            Some(_) => DashTab::Modules,
            None => DashTab::Overview,
        };
        let data = load(&ctx, guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_dashboard(&data, &s, tab, 0, ctx.limits())).await
    }

    /// Suggests modules from the Module registry's stored per-guild state, matched against
    /// the raw (possibly partial) `typed` text. Noctis sends the result itself: no
    /// `respond`, no defer, at most 25 suggestions, within 2.5 s (`commands.md` §4).
    #[autocomplete(module)]
    async fn module(
        ctx: AutocompleteCtx,
        typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        let guild = ctx.guild_id()?; // outside a guild: an empty list
        let s = Strings::of(ctx.user_locale());
        let typed = typed.trim().to_lowercase();
        Ok(ModuleKey::ALL
            .into_iter()
            .map(|m| (m, module_name(&s, m)))
            .filter(|(m, name)| m.module_id().contains(&typed) || name.to_lowercase().contains(&typed))
            .map(|(m, name)| {
                let on = ctx.modules().is_enabled(m.module_id(), Some(guild));
                let state = if on { s.common.enabled() } else { s.common.disabled() };
                (m.module_id().to_owned(), format!("{name} · {state}"))
            })
            .collect())
    }
}

impl OpenTab {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let meta = bot_meta(ctx.state::<BotMeta>())?;
        let data = load(&ctx, self.guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_dashboard(&data, &s, self.tab, 0, ctx.limits())).await
    }
}

impl TurnPage {
    /// The page is untrusted client input: `render_dashboard` clamps it to the last page.
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let meta = bot_meta(ctx.state::<BotMeta>())?;
        let data = load(&ctx, self.guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_dashboard(&data, &s, DashTab::Roles, self.page, ctx.limits())).await
    }
}

impl ToggleModule {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let meta = bot_meta(ctx.state::<BotMeta>())?;
        ctx.modules()
            .set_enabled(self.module.module_id(), ModuleScope::Guild(self.guild), self.enable)
            .await?;
        // Re-render the same tab with the new state, in place.
        let data = load(&ctx, self.guild, &meta).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_dashboard(&data, &s, DashTab::Modules, 0, ctx.limits())).await
    }
}

/// `BotMeta` is registered with `.state(..)` in `main`; a missing registration is a bug
/// (reported to the Error Sink), never a user condition.
fn bot_meta(meta: Option<State<BotMeta>>) -> HandlerResult<State<BotMeta>> {
    meta.ok_or_else(|| HandlerError::new(std::io::Error::other("BotMeta state is not registered")))
}

/// All I/O happens here, before rendering.
async fn load(ctx: &impl Ctx, guild: Id<GuildMarker>, meta: &BotMeta) -> HandlerResult<DashData> {
    // `None` means not cached (yet): fall back instead of failing.
    let guild_name = ctx.cache().guild(guild).await?.map_or_else(|| guild.to_string(), |g| g.name);
    let mut roles = ctx.cache().roles(guild).await?.unwrap_or_default();
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
