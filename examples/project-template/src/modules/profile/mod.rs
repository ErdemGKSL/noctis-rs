// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Profiles: a rendered card in a Media Gallery (UD-2), a button → modal → re-render
//! chain to edit the bio (FR-29, FR-63), a user context-menu command (FR-12) and an XP
//! event handler (FR-6).
//!
//! Layout: `mod.rs` (Module, Routes, the modal form, storage keys), `view.rs` (the card
//! Template and `render_profile`), `handlers.rs` (Commands, component and modal
//! handlers), `events.rs` (gateway events; tescord's "trigger" file).

mod events;
mod handlers;
pub mod view;

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::prelude::*;
use serde::{Deserialize, Serialize};

pub use view::ProfileCard;

pub fn module() -> Module {
    Module::new("profile")
        .add(handlers::profile())
        .add(handlers::bio())
        .add(handlers::show_profile())
        .add(handlers::refresh_profile())
        .add(handlers::edit_bio())
        .add(handlers::save_bio())
        .add(events::award_xp())
        // Registering the Template lets App build check fonts, theme tokens and keyframes.
        .template::<ProfileCard>()
}

// ---------------------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------------------

/// The refresh button. `editable` remembers whether the viewer looks at their own
/// profile, so the redraw shows the same buttons.
///
/// Width (`noctis_route_width`): `profile: F 1 + route 7 + sep 1 + S 0 + N 0 + body 13 + T 0 = 22 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("profile")]
pub struct RefreshProfile {
    pub user: Id<UserMarker>,
    pub editable: bool,
}

/// "Edit bio" button. `opens_modal` turns off the auto-defer for its handler, because a
/// modal must be the first response (INT-05). `owner = user`: only that user may click.
///
/// Width (`noctis_route_width`): `profile.bio: F 1 + route 11 + sep 1 + S 0 + N 0 + body 11 + T 0 = 24 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("profile.bio", opens_modal, owner = user)] // [planned]: runtime arguments are rejected until their release
pub struct EditBio {
    pub user: Id<UserMarker>,
}

/// The modal's own Route. It carries the same Payload as `EditBio`, so the submit
/// handler knows whose profile message to redraw.
///
/// Width (`noctis_route_width`): `profile.save: F 1 + route 12 + sep 1 + S 0 + N 0 + body 11 + T 0 = 25 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("profile.save", owner = user)] // [planned]: runtime arguments are rejected until their release
pub struct SaveBio {
    pub user: Id<UserMarker>,
}

/// The pronoun choices: a Radio Group option set (MOD-07: 2-10 options, each text
/// 1-100). The variant name is the submitted value unless `value = "…"` is given.
// localized texts: [planned, Epic 11]
#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pronouns {
    #[choice("he/him", value = "he")]
    He,
    #[choice("she/her", value = "she")]
    She,
    #[choice("they/them", value = "they")]
    They,
    #[choice("any pronouns", value = "any")]
    Any,
}

/// The bio modal. One field = one top-level component (MOD-02: at most 5); the field
/// name is the input key. Every literal is checked at `cargo check`.
// localized texts: [planned, Epic 11]
#[derive(Modal, Debug)]
#[modal(route = SaveBio, title = "Edit your profile")]
pub struct BioForm {
    #[text_input("Bio", style = paragraph, max_length = 300, placeholder = "A few words about you")]
    pub bio: Option<String>,

    #[radio_group("Pronouns")]
    pub pronouns: Option<Pronouns>,
}

// ---------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredProfile {
    pub bio: Option<String>,
    pub pronouns: Option<Pronouns>,
}

pub(crate) fn profile_key(guild: Id<GuildMarker>, user: Id<UserMarker>) -> String {
    format!("profile:{guild}:{user}")
}

pub(crate) fn xp_key(guild: Id<GuildMarker>, user: Id<UserMarker>) -> String {
    format!("xp:{guild}:{user}")
}
