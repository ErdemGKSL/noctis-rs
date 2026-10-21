// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Commands and handlers. Each struct is the interaction (id + typed data); its
//! `#[handler]` method loads the data, renders the card, and answers with
//! `ctx.respond(render_profile(..))` (new message) or by returning the message from a
//! component or modal handler (it updates the clicked message: `response = update`).

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::image::RemoteImage;
use noctis::prelude::*;
use noctis::ui::{Components, ResolvedUser, TextDisplay, V2Message, escape_markdown};

use super::view::{ProfileCard, ProfileData, render_profile};
use super::{BioForm, EditBio, RefreshProfile, StoredProfile, profile_key, xp_key};
use crate::strings::{Strings, keys};

const XP_PER_LEVEL: u64 = 100;

/// `/profile [user]`: the struct's fields are the options (UD-3: fine for a quick
/// lookup target). `Option<T>` makes this one optional.
#[derive(SlashCommand, Debug)]
#[command(
    "profile",
    description = "Show a member's profile card",
    contexts = [Guild],
    rate_limit(user, 3, per = "10s"),      // [planned]: rejected until its release
)]
pub struct Profile {
    /// The literal equals `cmd.profile.opt.user.description` in `locales/en-US.yaml`.
    #[option(description = "Whose profile to show (default you)")]
    pub user: Option<ResolvedUser>,
}

impl Profile {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let target = self.user.map_or_else(|| ctx.user_id(), |u| u.user.id);
        let data = load(&ctx, target).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_profile(&data, &s, ctx.user_id() == target)).await // [planned, Story 8.1]
    }
}

/// `/bio` opens the bio modal directly. `auto_defer = false` turns off the auto-defer (a
/// modal must be the first response, INT-05), so the handler takes the `Manual`
/// responder, the only one with `show_modal()`, and opens it before any slow work.
#[derive(SlashCommand, Debug)]
#[command("bio", description = "Edit your profile bio", auto_defer = false, contexts = [Guild])]
pub struct Bio;

impl Bio {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> {
        // A fresh form: the Payload says whose bio, the inputs start empty. After a modal
        // there is no original message to edit: end with `done()`.
        let form = BioForm { user: ctx.user_id(), bio: None, pronouns: None };
        Ok(r.show_modal(form.build()).await?.done())
    }
}

/// The badges `/badge` knows.
const BADGES: &[&str] = &["Early supporter", "Helper", "Night owl", "Artist", "Bug hunter"];

/// `/badge <name>`: `name` is suggested while typing by the paired
/// `#[autocomplete(name)]` method. The command re-checks the value, because Discord
/// sends whatever the user typed (OPT-12).
#[derive(SlashCommand, Debug)]
#[command("badge", description = "Show a badge", contexts = [Guild])]
pub struct Badge {
    /// The literal equals `cmd.badge.opt.name.description` in `locales/en-US.yaml`.
    #[option(description = "Badge name", autocomplete)]
    pub name: String,
}

impl Badge {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Components> {
        let Some(name) = BADGES.iter().find(|b| b.eq_ignore_ascii_case(&self.name)) else {
            return Err(HandlerError::user(keys::PROFILE_BADGE_UNKNOWN));
        };
        let s = Strings::of(ctx.response_locale());
        Ok(Components::new().push(TextDisplay::new(s.profile.badge(escape_markdown(name)))))
    }

    /// Suggests badges matching the raw (possibly partial) `typed` text. Noctis sends the
    /// result itself: no `respond`, no defer (INT-06), at most 25 suggestions (INT-07),
    /// and an empty list if this fails or misses its budget.
    #[autocomplete(name)]
    async fn name(
        _ctx: AutocompleteCtx,
        typed: &str,
        _filled: Partial<Self>,
    ) -> HandlerResult<Suggestions<String>> {
        let typed = typed.to_lowercase();
        Ok(BADGES
            .iter()
            .filter(|b| b.to_lowercase().contains(&typed))
            .map(|b| ((*b).to_owned(), *b))
            .collect())
    }
}

/// Right-click a user → Apps → "Show profile". The name is localized as a whole
/// (`ctxcmd.show_profile.name`); CMD-01/03 do not apply to context-menu names (CTX-01).
#[derive(UserCommand, Debug)]
#[user_command(
    "Show profile",
    key = "show_profile", // [planned]: rejected until its release
    contexts = [Guild],
)]
pub struct ShowProfile;

impl ShowProfile {
    #[handler]
    async fn handle(self, ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Responded> {
        let data = load(&ctx, target.user.id).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_profile(&data, &s, ctx.user_id() == target.user.id)).await
    }
}

impl RefreshProfile {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<V2Message> {
        let data = load(&ctx, self.user).await?;
        let s = Strings::of(ctx.response_locale());
        Ok(render_profile(&data, &s, self.editable))
    }
}

impl EditBio {
    /// Button → modal: builds the modal from a `BioForm` value whose input fields hold
    /// the stored values, so the inputs open pre-filled (FR-63, [planned, Story 9.4]);
    /// its Payload (`user`) tells the submit whose profile message to redraw. A button
    /// that opens a modal is `auto_defer = false` (on its `#[button]`) and takes
    /// `ComponentResponder<Manual>` (INT-05).
    #[handler]
    async fn handle(self, ctx: ComponentCtx, responder: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        let guild = guild_of(&ctx)?;
        let stored: StoredProfile =
            ctx.kv().get(&profile_key(guild, self.user)).await?.unwrap_or_default();
        let form = BioForm { user: self.user, bio: stored.bio, pronouns: stored.pronouns };
        Ok(responder.show_modal(form.build()).await?.done())
    }
}

impl BioForm {
    /// Modal → re-render: `self` is the decoded Payload plus the submitted values. Save,
    /// then redraw the message the modal was opened from (`response = update`, the
    /// default for a component-opened modal).
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<V2Message> {
        let guild = guild_of(&ctx)?;
        let stored = StoredProfile {
            bio: self.bio.filter(|b| !b.trim().is_empty()),
            pronouns: self.pronouns,
        };
        ctx.kv().put(&profile_key(guild, self.user), &stored, None).await?;
        let data = load(&ctx, self.user).await?;
        let s = Strings::of(ctx.response_locale());
        Ok(render_profile(&data, &s, true))
    }
}

/// Loads the profile and renders the card. Rendering is awaited here, inside the
/// handler, so the auto-defer covers a slow render (AD-22, FR-34).
async fn load(ctx: &impl Ctx, user_id: Id<UserMarker>) -> HandlerResult<ProfileData> {
    let guild = guild_of(ctx)?;
    let user = ctx.cache().user(user_id).await?;
    let stored: StoredProfile =
        ctx.kv().get(&profile_key(guild, user_id)).await?.unwrap_or_default();
    let xp = ctx.kv().counter(&xp_key(guild, user_id)).await?;

    // The card's text follows the response's locale (FR-43).
    let locale = ctx.response_locale();
    let s = Strings::of(locale);
    #[expect(clippy::cast_precision_loss, reason = "remainder is < 100")]
    let progress = (xp % XP_PER_LEVEL) as f32 / XP_PER_LEVEL as f32;
    let name = user.global_name.clone().unwrap_or_else(|| user.name.clone());

    let card = ctx
        .render(ProfileCard {
            name: name.clone(),
            subtitle: stored.pronouns.map(|p| p.label(locale)).unwrap_or_default(),
            level_label: s.profile.card.level(xp / XP_PER_LEVEL),
            xp_label: s.profile.card.xp(xp % XP_PER_LEVEL, XP_PER_LEVEL),
            progress,
            avatar: RemoteImage::avatar(&user, 256),
        })
        .await?;
    Ok(ProfileData { user: user_id, name, bio: stored.bio, card })
}

fn guild_of(ctx: &impl Ctx) -> HandlerResult<Id<GuildMarker>> {
    ctx.guild_id().ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))
}
