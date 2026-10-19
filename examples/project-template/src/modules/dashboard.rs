// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! `/dashboard`: a Navbar View (UD-5) with three tabs. The Roles tab paginates on its own
//! when a guild has more roles than one page holds (UD-6, FR-62): no page math here.

use std::time::UNIX_EPOCH;

use noctis::id::marker::GuildMarker;
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, ButtonStyle, ContainerComponent, Emoji, Section, TextDisplay, escape_markdown,
};

use crate::BotMeta;
use crate::strings::{Strings, keys};

const ACCENT: Rgb = rgb!(0x5865_F2);

pub fn module() -> Module {
    Module::new("dashboard")
        .add(dashboard())
        .add(open_dashboard())
        .add(toggle_module())
}

// ---------------------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------------------

/// The dashboard's View Route. Tab and page ride in the view envelope (AD-20), so the
/// Payload is only the guild. `permissions` is a Route Guard checked on every click.
///
/// Width (`noctis_route_width`): `dash: F 1 + route 4 + sep 1 + S 0 + E 5 + N 0 + body 11 + T 0 = 22 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("dash", view, permissions = MANAGE_GUILD)] // [planned]: runtime arguments are rejected until their release
pub struct DashRoute {
    pub guild: Id<GuildMarker>,
}

/// Modules this dashboard can switch on and off. `PayloadEnum` encodes as one character.
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKey {
    Setup,
    Profile,
}

impl ModuleKey {
    pub const ALL: [Self; 2] = [Self::Setup, Self::Profile];

    /// The id the Module was mounted with (`Module::new(..)`).
    pub fn module_id(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Profile => "profile",
        }
    }
}

/// Enable/disable button on the Modules tab. Width: 11 + 1 + 1 + 2 separators = 15.
///
/// Width (`noctis_route_width`): `dash.mod: F 1 + route 8 + sep 1 + S 0 + E 0 + N 0 + body 15 + T 0 = 25 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("dash.mod", permissions = MANAGE_GUILD)] // [planned]: runtime arguments are rejected until their release
pub struct ToggleModule {
    pub guild: Id<GuildMarker>,
    pub module: ModuleKey,
    pub enable: bool,
}

// ---------------------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------------------

/// Public by default: the dashboard is shared content, so it renders in the Guild Locale.
#[command("dashboard", permissions = MANAGE_GUILD, contexts = [Guild])]
async fn dashboard(ctx: CommandCtx, meta: State<BotMeta>) -> HandlerResult<DashboardView> {
    let guild = ctx
        .guild_id()
        .ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))?;
    DashboardView::load(&ctx, DashRoute { guild }, &meta).await
}

/// Loader for tab switches, page turns, jumps and the reload button (AD-20).
#[handler]
async fn open_dashboard(
    ctx: ComponentCtx,
    route: DashRoute,
    meta: State<BotMeta>,
) -> HandlerResult<DashboardView> {
    DashboardView::load(&ctx, route, &meta).await
}

#[handler]
async fn toggle_module(
    ctx: ComponentCtx,
    toggle: ToggleModule,
    meta: State<BotMeta>,
) -> HandlerResult<impl IntoResponse> {
    ctx.modules()
        .set_enabled(
            toggle.module.module_id(),
            ModuleScope::Guild(toggle.guild),
            toggle.enable,
        )
        .await?;
    let view = DashboardView::load(&ctx, DashRoute { guild: toggle.guild }, &meta).await?;
    // Re-render on the Modules tab so the user sees the new state in place.
    Ok(view.at(Nav::tab(2)))
}

// ---------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------

pub struct RoleRow {
    pub name: String,
    pub mention: String,
    pub position: i64,
}

pub struct DashboardView {
    route: DashRoute,
    guild_name: String,
    roles: Vec<RoleRow>,
    modules: Vec<(ModuleKey, bool)>,
    started_unix: u64,
    version: &'static str,
}

impl DashboardView {
    /// All I/O happens here, before rendering; `render` stays pure.
    pub async fn load(
        ctx: &impl Ctx,
        route: DashRoute,
        meta: &BotMeta,
    ) -> HandlerResult<Self> {
        let guild = ctx.cache().guild(route.guild).await?;
        let mut roles: Vec<RoleRow> = ctx
            .cache()
            .roles(route.guild)
            .await?
            .into_iter()
            .map(|r| RoleRow {
                name: r.name,
                mention: format!("<@&{}>", r.id),
                position: r.position,
            })
            .collect();
        roles.sort_unstable_by(|a, b| b.position.cmp(&a.position));

        let modules = ModuleKey::ALL
            .into_iter()
            .map(|m| (m, ctx.modules().is_enabled(m.module_id(), Some(route.guild))))
            .collect();

        let started_unix = meta
            .started_at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        Ok(Self {
            route,
            guild_name: guild.name,
            roles,
            modules,
            started_unix,
            version: meta.version,
        })
    }
}

impl View for DashboardView {
    type Route = DashRoute;

    fn state(&self) -> &DashRoute {
        &self.route
    }

    fn render(&self, cx: &mut ViewCx<'_, DashRoute>) -> UiResult<V2Message> {
        let s = Strings::of(cx.locale());

        // Top Action Row of tab buttons; the active tab is disabled (UD-5).
        let navbar = Navbar::new(
            cx,
            [
                s.dashboard.tab.overview(),
                s.dashboard.tab.roles(),
                s.dashboard.tab.modules(),
            ],
        )?;

        let header = TextDisplay::new(format!(
            "## {}\n-# {}",
            s.dashboard.title(escape_markdown(&self.guild_name)),
            s.dashboard.subtitle(self.version),
        ));

        let body = match navbar.active() {
            0 => Body::from(vec![
                TextDisplay::new(s.dashboard.overview(
                    self.roles.len(),
                    self.modules.iter().filter(|(_, on)| *on).count(),
                    format!("<t:{}:R>", self.started_unix),
                ))
                .into(),
            ]),
            // Homogeneous list: hand it to the Paginator. It reserves the Chrome's cost
            // (header, Navbar, footer), packs as many rows as the Limits allow, and adds
            // the pager row and, past 5 pages, a jump select (FR-28).
            1 => Paginator::new(&self.roles, |role| {
                TextDisplay::new(format!("{} · `{}`", role.mention, escape_markdown(&role.name)))
            })
            .empty(TextDisplay::new(s.dashboard.no_roles()))
            .into(),
            _ => Body::from(
                self.modules
                    .iter()
                    .map(|&(module, enabled)| self.module_row(&s, module, enabled))
                    .collect::<Vec<_>>(),
            ),
        };

        Layout::new()
            .accent(ACCENT)
            .header(header)
            .navbar(navbar)
            .body(body)
            .footer(ActionRow::buttons([cx
                .reload_button()
                .label(s.common.refresh())
                .emoji(Emoji::unicode("🔄"))]))
            .build(cx)
    }
}

impl DashboardView {
    /// One Section per Module: state as text, the action as the accessory. The style
    /// encodes the action (Success to enable, Danger to disable).
    fn module_row(&self, s: &Strings, module: ModuleKey, enabled: bool) -> ContainerComponent {
        let name = match module {
            ModuleKey::Setup => s.dashboard.module.setup(),
            ModuleKey::Profile => s.dashboard.module.profile(),
        };
        let state = if enabled { s.common.enabled() } else { s.common.disabled() };
        let toggle = ToggleModule {
            guild: self.route.guild,
            module,
            enable: !enabled,
        }
        .button()
        .label(if enabled { s.common.disable() } else { s.common.enable() })
        .style(if enabled { ButtonStyle::Danger } else { ButtonStyle::Success });

        Section::new([TextDisplay::new(format!("**{name}** · {state}"))], toggle).into()
    }
}
