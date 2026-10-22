//! The Usage Skill's UI recipes as a compiled, tested bot: tabs, pagination, a step
//! flow and a settings screen, each a plain render function plus one `#[handler]` per
//! Route. `views-and-recipes.md` §3 shows these files verbatim.

// recipe: app
pub mod pagination;
pub mod settings;
pub mod steps;
pub mod store;
pub mod tabs;

use noctis::prelude::*;

use crate::store::Data;

/// Registers the recipes' state and Modules on `builder`; `main` and the tests share it.
pub fn configure(builder: &mut AppBuilder, data: Data) -> &mut AppBuilder {
    builder
        .state(data.catalog)
        .state(data.setup)
        .state(data.tickets)
        .module(
            Module::new("tabs")
                .add::<tabs::Dashboard>()
                .add::<tabs::OpenTab>(),
        )
        .module(
            Module::new("pagination")
                .add::<pagination::Items>()
                .add::<pagination::TurnPage>(),
        )
        .module(
            Module::new("steps")
                .add::<steps::Setup>()
                .add::<steps::SetupState>()
                .add::<steps::PickChannel>()
                .add::<steps::PickRole>()
                .add::<steps::CancelSetup>()
                .add::<steps::FinishSetup>(),
        )
        .module(
            Module::new("settings")
                .add::<settings::Tickets>()
                .add::<settings::PickSystem>()
                .add::<settings::RenameSystem>()
                .add::<settings::RenameForm>()
                .add::<settings::SetRoles>()
                .add::<settings::ToggleSystem>()
                .add::<settings::AddSystem>()
                .add::<settings::AskDelete>()
                .add::<settings::ConfirmDelete>()
                .add::<settings::BackToSettings>(),
        )
}
// recipe-end: app
