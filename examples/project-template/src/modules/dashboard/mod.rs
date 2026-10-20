// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! `/dashboard`: a tabbed server dashboard (UD-5) whose Roles tab is paginated by hand
//! (UD-6), and whose Modules tab is a settings screen of enable/disable toggles.
//!
//! The feature follows the Usage Skill's layout: `mod.rs` holds the Module and the
//! Routes, `view.rs` the render function (the single source of truth for the message),
//! `handlers.rs` the Command and component handlers. All UI state (tab, page) is a
//! typed Payload field of this module's own components; nothing is stored per message.
//! [planned: Story 6.11] kind derives, `defaults(..)`, `.build()` and `add::<T>()`.

mod handlers;
pub mod view;

use noctis::id::marker::GuildMarker;
use noctis::prelude::*;

pub fn module() -> Module {
    Module::new("dashboard")
        .add::<handlers::Dashboard>()
        .add::<OpenTab>()
        .add::<TurnPage>()
        .add::<ToggleModule>()
}

// ---------------------------------------------------------------------------------------
// Routes: every id the dashboard needs to redraw itself travels here.
// ---------------------------------------------------------------------------------------

/// The dashboard's tabs. `PayloadEnum` encodes as one character; append variants, never
/// reorder them (or bump the Route's `schema`).
#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashTab {
    Overview,
    Roles,
    Modules,
}

impl DashTab {
    pub const ALL: [Self; 3] = [Self::Overview, Self::Roles, Self::Modules];
}

/// A tab button. The active tab's button is rendered disabled, so the tab row shows the
/// current state. `permissions` is a Route Guard checked on every click. The render
/// overrides the label (one per tab), the style and `disabled`.
///
/// Width (`noctis_route_width`): `dash.tab: F 1 + route 8 + sep 1 + S 0 + N 0 + body 13 + T 0 = 23 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "dash.tab",
    permissions = MANAGE_GUILD, // [planned]: runtime arguments are rejected until their release
    defaults(style = Secondary, label = "dashboard.tab.overview"),
)]
pub struct OpenTab {
    pub guild: Id<GuildMarker>,
    pub tab: DashTab,
}

/// Prev/next on the Roles tab: the target page is the Payload. A Route of its own keeps
/// the prev and next ids distinct from the tab buttons (CID-02).
///
/// Width (`noctis_route_width`): `dash.page: F 1 + route 9 + sep 1 + S 0 + N 0 + body 15 + T 0 = 26 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "dash.page",
    permissions = MANAGE_GUILD, // [planned]: runtime arguments are rejected until their release
    defaults(style = Secondary, label = "common.next"),
)]
pub struct TurnPage {
    pub guild: Id<GuildMarker>,
    pub page: u16,
}

/// Modules this dashboard can switch on and off.
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

    /// The inverse of [`Self::module_id`]; `None` for anything else (option values are
    /// client input).
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.module_id() == id)
    }
}

/// Enable/disable toggle on the Modules tab. `enable` is the action the button performs,
/// so a stale click can't flip the state the wrong way.
///
/// Width (`noctis_route_width`): `dash.mod: F 1 + route 8 + sep 1 + S 0 + N 0 + body 15 + T 0 = 25 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "dash.mod",
    permissions = MANAGE_GUILD, // [planned]: runtime arguments are rejected until their release
    defaults(style = Success, label = "common.enable"),
)]
pub struct ToggleModule {
    pub guild: Id<GuildMarker>,
    pub module: ModuleKey,
    pub enable: bool,
}
