//! Story 7.8: the project template's bio flow compiles against the real
//! facade (the template's tracking-mode check, decision in the spec).
//!
//! `EditBio`, `Pronouns`, `BioForm` and `Bio` and their handlers are copied
//! verbatim from `examples/project-template/src/modules/profile/{mod.rs,
//! handlers.rs}`. Only the template-local helpers are stubs: the storage is
//! `ctx.kv()`, the View is a one-line `render_profile`, `Strings`/`keys` are
//! minimal, and the planned `Ctx` trait is a local one forwarding to the real
//! context methods (`response_locale` included, Story 8.1). The handlers
//! answer through the real `ctx.update`/`ctx.respond` (Story 8.1). Keep the
//! copies in sync with the template.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]
#![allow(
    missing_docs,
    reason = "the template's declarations are copied verbatim, with their `pub`"
)]

use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::{GuildMarker, UserMarker};
use noctis::ui::{Components, Modal, TextDisplay, V2Message};
use noctis::{Kv, ResponderError, Runtime};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Template-local stubs
// ---------------------------------------------------------------------------

/// The template's `strings::keys`.
mod keys {
    use noctis::prelude::MessageKey;

    pub const COMMON_GUILD_ONLY: MessageKey = MessageKey::new("common.guild_only");
}

/// The template's generated `Strings` (here: nothing to look up).
struct Strings;

impl Strings {
    fn of(_locale: noctis::rules::Locale) -> Self {
        Self
    }
}

/// The planned `Ctx` trait the template's helpers take.
trait Ctx {
    fn guild_id(&self) -> Option<Id<GuildMarker>>;
    fn kv(&self) -> Kv;
    fn response_locale(&self) -> noctis::rules::Locale;
}

macro_rules! ctx {
    ($($t:ty),*) => {$(
        impl Ctx for $t {
            fn guild_id(&self) -> Option<Id<GuildMarker>> {
                <$t>::guild_id(self)
            }
            fn kv(&self) -> Kv {
                <$t>::kv(self)
            }
            fn response_locale(&self) -> noctis::rules::Locale {
                <$t>::response_locale(self)
            }
        }
    )*};
}

ctx!(ComponentCtx, ModalSubmitCtx);

/// The template's `ProfileData`, without the rendered card.
struct ProfileData {
    bio: Option<String>,
}

/// The template's View, in one line.
fn render_profile(data: &ProfileData, _s: &Strings, _editable: bool) -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new(data.bio.clone().unwrap_or_default())))
}

/// The template's `load`, reading the stored profile only.
async fn load(ctx: &impl Ctx, user_id: Id<UserMarker>) -> HandlerResult<ProfileData> {
    let guild = guild_of(ctx)?;
    let stored: StoredProfile = ctx
        .kv()
        .get(&profile_key(guild, user_id))
        .await?
        .unwrap_or_default();
    // The card's text follows the response's locale (FR-43).
    let _s = Strings::of(ctx.response_locale());
    Ok(ProfileData { bio: stored.bio })
}

fn guild_of(ctx: &impl Ctx) -> HandlerResult<Id<GuildMarker>> {
    ctx.guild_id()
        .ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredProfile {
    pub bio: Option<String>,
    pub pronouns: Option<Pronouns>,
}

fn profile_key(guild: Id<GuildMarker>, user: Id<UserMarker>) -> String {
    format!("profile:{guild}:{user}")
}

// ---------------------------------------------------------------------------
// Verbatim from the template (`profile/mod.rs`)
// ---------------------------------------------------------------------------

#[derive(Button, Debug, Clone, Copy)]
#[button(
    "profile.bio",
    auto_defer = false,
    owner = user,
    defaults(style = Primary, label = "profile.edit_bio"),
)]
pub struct EditBio {
    pub user: Id<UserMarker>,
}

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

#[derive(Modal, Debug)]
#[modal(
    "profile.save",
    owner = user,
    defaults(title = "profile.bio_modal.title"),
)]
pub struct BioForm {
    pub user: Id<UserMarker>,

    #[text_input("Bio", style = paragraph, max_length = 300, placeholder = "A few words about you")]
    pub bio: Option<String>,

    #[radio_group("Pronouns")]
    pub pronouns: Option<Pronouns>,
}

// ---------------------------------------------------------------------------
// Verbatim from the template (`profile/handlers.rs`)
// ---------------------------------------------------------------------------

#[derive(SlashCommand, Debug)]
#[command("bio", description = "Edit your profile bio", auto_defer = false, contexts = [Guild])]
pub struct Bio;

impl Bio {
    #[handler]
    async fn handle(
        self,
        ctx: CommandCtx,
        r: CommandResponder<Manual>,
    ) -> HandlerResult<Responded> {
        // A fresh form: the Payload says whose bio, the inputs start empty. After a modal
        // there is no original message to edit: end with `done()`.
        let form = BioForm {
            user: ctx.user_id(),
            bio: None,
            pronouns: None,
        };
        Ok(r.show_modal(form.build()).await?.done())
    }
}

impl EditBio {
    #[handler]
    async fn handle(
        self,
        ctx: ComponentCtx,
        responder: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        let guild = guild_of(&ctx)?;
        let stored: StoredProfile = ctx
            .kv()
            .get(&profile_key(guild, self.user))
            .await?
            .unwrap_or_default();
        let form = BioForm {
            bio: stored.bio,
            pronouns: stored.pronouns,
            ..BioForm::blank(self.user)
        };
        Ok(responder.show_modal(form.build()).await?.done())
    }
}

impl BioForm {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        let guild = guild_of(&ctx)?;
        let stored = StoredProfile {
            bio: self.bio.filter(|b| !b.trim().is_empty()),
            pronouns: self.pronouns,
        };
        ctx.kv()
            .put(&profile_key(guild, self.user), &stored, None)
            .await?;
        let data = load(&ctx, self.user).await?;
        let s = Strings::of(ctx.response_locale());
        let card = render_profile(&data, &s, true);
        match ctx.update(card.clone()).await {
            Err(err) if is_update_unavailable(&err) => ctx.respond(card).await,
            answered => answered,
        }
    }
}

/// Whether `ctx.update` found no message to update (a modal opened from a slash command).
fn is_update_unavailable(err: &HandlerError) -> bool {
    matches!(
        err.as_error().downcast_ref(),
        Some(ResponderError::UpdateUnavailable)
    )
}

// ---------------------------------------------------------------------------
// Row 13, positive half: an `auto_defer = false` modal's hooks inherit
// `Manual`.
// ---------------------------------------------------------------------------

#[derive(Modal, Debug)]
#[modal("profile.raw", auto_defer = false, response = ephemeral, defaults(title = "Raw"))]
pub struct RawForm {
    pub user: Id<UserMarker>,
    #[text_input("Note")]
    pub note: Option<String>,
}

impl RawForm {
    #[handler]
    async fn handle(
        self,
        _ctx: ModalSubmitCtx,
        r: ModalResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }

    #[on_stale]
    async fn stale(_ctx: ModalSubmitCtx, r: ModalResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }

    #[on_expired]
    async fn gone(
        _ctx: ModalSubmitCtx,
        _why: Expired,
        r: ModalResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }
}

#[tokio::test]
async fn template_bio_flow_builds_an_app() {
    let app = App::builder()
        .runtime(&Runtime::for_test())
        .module(
            Module::new("profile")
                .add::<Bio>()
                .add::<EditBio>()
                .add::<BioForm>()
                .add::<RawForm>(),
        )
        .build();
    assert!(app.is_ok(), "{:?}", app.err());
    // The built modal carries the declared Route id and both inputs.
    let modal = serde_json::to_value(Modal::from(BioForm::blank(Id::new(3)))).unwrap();
    assert!(
        modal["custom_id"]
            .as_str()
            .unwrap()
            .starts_with("0profile.save:"),
        "{modal}"
    );
    assert_eq!(modal["components"].as_array().unwrap().len(), 2, "{modal}");
}
