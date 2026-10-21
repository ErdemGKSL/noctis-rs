// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! The dashboard's render function: the single source of truth for the message. The
//! Command and every handler load [`DashData`] and call [`render_dashboard`]; nothing
//! else builds this message. It is pure (no I/O), so tests snapshot it directly.

use noctis::id::marker::GuildMarker;
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    ActionRow, Budget, ButtonStyle, Components, Container, ContainerComponent, Section,
    Separator, TextDisplay, V2Message, escape_markdown,
};

use super::{DashTab, ModuleKey, OpenTab, ToggleModule, TurnPage};
use crate::strings::Strings;

const ACCENT: Rgb = rgb!(0x5865_F2);
/// A role row is at most a mention (22) + " · " + a 100-character name escaped to 200.
const ROLE_ROW_MAX_CHARS: usize = 230;

/// Everything the dashboard shows, loaded by the handler before rendering.
pub struct DashData {
    pub guild: Id<GuildMarker>,
    pub guild_name: String,
    /// `(mention, name)`, highest role first.
    pub roles: Vec<(String, String)>,
    pub modules: Vec<(ModuleKey, bool)>,
    pub started_unix: u64,
    pub version: &'static str,
}

/// Renders the dashboard on `tab`; `page` matters only on the Roles tab and is clamped.
pub fn render_dashboard(
    data: &DashData,
    s: &Strings,
    tab: DashTab,
    page: u16,
    limits: &Limits,
) -> V2Message {
    let header = TextDisplay::new(format!(
        "## {}\n-# {}",
        s.dashboard.title(escape_markdown(&data.guild_name)),
        s.dashboard.subtitle(data.version),
    ));

    // The tab row: one Route button per tab, the active one disabled (UD-5).
    let tabs = ActionRow::buttons(DashTab::ALL.map(|t| {
        OpenTab { guild: data.guild, tab: t }
            .build()
            .label(match t {
                DashTab::Overview => s.dashboard.tab.overview(),
                DashTab::Roles => s.dashboard.tab.roles(),
                DashTab::Modules => s.dashboard.tab.modules(),
            })
            .style(if t == tab { ButtonStyle::Primary } else { ButtonStyle::Secondary })
            .disabled(t == tab)
    }));

    // The section is chosen by a plain `match` on the tab from the Payload.
    let body: Vec<ContainerComponent> = match tab {
        DashTab::Overview => vec![
            TextDisplay::new(s.dashboard.overview(
                data.roles.len(),
                data.modules.iter().filter(|(_, on)| *on).count(),
                format!("<t:{}:R>", data.started_unix),
            ))
            .into(),
        ],
        DashTab::Roles => roles_page(data, s, page, limits, &header, &tabs),
        DashTab::Modules => data
            .modules
            .iter()
            .map(|&(module, enabled)| module_row(data, s, module, enabled))
            .collect(),
    };

    frame(header, tabs, body)
}

/// Header, tab row and body in one Container.
fn frame(header: TextDisplay, tabs: ActionRow, body: Vec<ContainerComponent>) -> V2Message {
    let container = body.into_iter().fold(
        Container::new()
            .accent(ACCENT)
            .push(header)
            .push(tabs)
            .push(Separator::small()),
        Container::push,
    );
    V2Message::new(Components::new().push(container))
}

/// One page of roles: as many rows as fit beside the header, the tabs and the pager.
fn roles_page(
    data: &DashData,
    s: &Strings,
    page: u16,
    limits: &Limits,
    header: &TextDisplay,
    tabs: &ActionRow,
) -> Vec<ContainerComponent> {
    if data.roles.is_empty() {
        // Designed empty state instead of a missing block.
        return vec![TextDisplay::new(s.dashboard.no_roles()).into()];
    }

    // Size the page with the budget helper [planned, Story 8.3]: measure the fixed parts
    // (with a pager row in place) and ask how many worst-case rows still fit. A
    // worst-case sample keeps page boundaries stable however long the names are.
    let fixed = frame(
        header.clone(),
        tabs.clone(),
        vec![page_line(s, 0, 1).into(), pager(data, s, 0, 1).into()],
    );
    let per_page = Budget::of(&fixed, limits)
        .fits(&TextDisplay::new("x".repeat(ROLE_ROW_MAX_CHARS)))
        .max(1);

    let pages = data.roles.len().div_ceil(per_page);
    let page = usize::from(page).min(pages - 1);
    let rows = data.roles[page * per_page..]
        .iter()
        .take(per_page)
        .map(|(mention, name)| {
            TextDisplay::new(format!("{mention} · {}", escape_markdown(name))).into()
        });

    let page = u16::try_from(page).unwrap_or(u16::MAX);
    let pages = u16::try_from(pages).unwrap_or(u16::MAX);
    rows.chain([page_line(s, page, pages).into(), pager(data, s, page, pages).into()])
        .collect()
}

/// "-# Page n of m" under the list.
fn page_line(s: &Strings, page: u16, pages: u16) -> TextDisplay {
    TextDisplay::new(format!("-# {}", s.dashboard.page(page + 1, pages)))
}

/// Prev and next. Prev is disabled on the first page and next on the last;
/// their target pages differ, so their ids are unique in the message (CID-02).
fn pager(data: &DashData, s: &Strings, page: u16, pages: u16) -> ActionRow {
    let prev = TurnPage { guild: data.guild, page: page.saturating_sub(1) }
        .build()
        .label(s.common.previous())
        .disabled(page == 0);
    let next = TurnPage { guild: data.guild, page: page + 1 }.build().disabled(page + 1 >= pages);
    ActionRow::buttons([prev, next])
}

/// A settings row: the state as text, the action as the accessory. The style encodes the
/// action (Success to enable, Danger to disable).
fn module_row(data: &DashData, s: &Strings, module: ModuleKey, enabled: bool) -> ContainerComponent {
    let name = module_name(s, module);
    let state = if enabled { s.common.enabled() } else { s.common.disabled() };
    let toggle = ToggleModule { guild: data.guild, module, enable: !enabled }
        .build()
        .label(if enabled { s.common.disable() } else { s.common.enable() })
        .style(if enabled { ButtonStyle::Danger } else { ButtonStyle::Success });
    Section::new([TextDisplay::new(format!("**{name}** · {state}"))], toggle).into()
}

/// A module's display name in the given locale (the Modules tab and `/dashboard`'s
/// autocomplete use the same text).
pub(super) fn module_name(s: &Strings, module: ModuleKey) -> String {
    match module {
        ModuleKey::Setup => s.dashboard.module.setup(),
        ModuleKey::Profile => s.dashboard.module.profile(),
    }
    .to_string()
}
