//! Profiles: a rendered card in a Media Gallery (UD-2), a button → modal → re-render
//! chain to edit the bio (FR-29, FR-63), a user context-menu command (FR-12), a badge
//! lookup with autocomplete (FR-10) and an XP event handler (FR-6).
//!
//! Layout: `mod.rs` (Module, component structs, the modal form, storage keys),
//! `view.rs` (the card Template and `render_profile`), `handlers.rs` (Command structs
//! and every `#[handler]` impl), `events.rs` (gateway events; tescord's "trigger" file).

mod events;
mod handlers;
pub mod view;

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::prelude::*;
use serde::{Deserialize, Serialize};

use crate::strings::keys;

/// The `/profile` and `/badge` Commands; `configure` also opts them into prefix
/// commands with the `prefix` feature.
pub use handlers::{Badge, Profile};
pub use view::ProfileCard;

pub fn module() -> Module {
    Module::new("profile")
        .add::<Profile>()
        .add::<handlers::Bio>()
        .add::<handlers::ShowProfile>()
        .add::<RefreshProfile>()
        .add::<EditBio>()
        .add::<BioForm>()
        .add::<events::AwardXp>()
        // Registers the card for the Renderer; App build rejects a Template id used twice.
        .template::<ProfileCard>()
        // A child's id is its parent's id plus one segment (checked at App build).
        .module(
            // Boot Flag `badges`: the Module exists only when the selection is `all` (the
            // default) or holds `badges` (`--features badges`, `NOCTIS_FEATURES=badges`).
            // Registers `/badge` and its `#[autocomplete(name)]` method together.
            Module::new("profile/badges")
                .flags(["badges"])
                .add::<Badge>(),
        )
}

// ---------------------------------------------------------------------------------------
// Components (their `#[handler]` impls are in `handlers.rs`)
// ---------------------------------------------------------------------------------------

/// The refresh button. `editable` remembers whether the viewer looks at their own
/// profile, so the redraw shows the same buttons. A `keys::` path in `defaults` is a
/// locale key, resolved in the response locale; a string literal is fixed text.
///
/// Width (`noctis_route_width`): `profile: F 1 + route 7 + sep 1 + S 0 + N 0 + body 13 + T 0 = 22 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button("profile", defaults(style = Secondary, label = keys::COMMON_REFRESH))]
pub struct RefreshProfile {
    pub user: Id<UserMarker>,
    pub editable: bool,
}

/// "Edit bio" button. It opens a modal, so it is `auto_defer = false` (a modal must be
/// the first response, INT-05) and its handler takes `ComponentResponder<Manual>`.
/// `owner = user`: only that user may click.
///
/// Width (`noctis_route_width`): `profile.bio: F 1 + route 11 + sep 1 + S 0 + N 0 + body 11 + T 0 = 24 (limit 100)`.
#[derive(Button, Debug, Clone, Copy)]
#[button(
    "profile.bio",
    auto_defer = false,
    owner = user,
    defaults(style = Primary, label = keys::PROFILE_EDIT_BIO),
)]
pub struct EditBio {
    pub user: Id<UserMarker>,
}

/// The pronoun choices: a Radio Group option set (MOD-07: 2-10 options, each text
/// 1-100). The variant name is the submitted value unless `value = "…"` is given.
// Choices labels are literal text: `label()` returns the `#[choice(..)]` text as is.
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

/// The bio modal: its own id, Payload and handler. Fields without an input attribute
/// (`user`) are the Payload, so the submit knows whose profile message to redraw; each
/// input field is one top-level component (MOD-02: at most 5), the field name is the
/// input key, and a built value pre-fills the inputs. Every literal is checked at
/// `cargo check`. It keeps the default `response = update` (and `auto_defer = true`):
/// opened from the "Edit bio" button, `ctx.update(..)` redraws that message in place
/// (type 7); opened from `/bio` there is no message to update (`UpdateUnavailable`,
/// nothing sent), so the handler answers with `ctx.respond(..)`, a new public reply
/// (type 4).
///
/// Width (`noctis_route_width`): `profile.save: F 1 + route 12 + sep 1 + S 0 + N 0 + body 11 + T 0 = 25 (limit 100)`.
// Input attribute texts are literals; `handlers::bio_modal` localizes them per render
// through `BioFormOverrides`. The title is a locale key.
#[derive(Modal, Debug)]
#[modal(
    "profile.save",
    owner = user,
    // `manual`: never counted before the handler; it counts a save with
    // `ctx.rate_limits().hit(RateScope::User)`, and the next submit within the minute
    // is refused before the handler runs.
    rate_limit(user, 1, per = "1m", manual),
    defaults(title = keys::PROFILE_BIO_MODAL_TITLE),
)]
pub struct BioForm {
    pub user: Id<UserMarker>,

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
