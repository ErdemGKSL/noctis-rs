// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Commands and handlers. Each struct is the interaction (id + typed data); its
//! `#[handler]` method loads the data, renders the card, and answers with
//! `ctx.respond(render_profile(..))` (a new message) from a Command, or with
//! `ctx.update(render_profile(..))` from a component or modal handler (it updates the
//! clicked message in place: `response = update`).

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::image::RemoteImage;
use noctis::prelude::*;
use noctis::ResponderError;
use noctis::ui::{Components, ResolvedUser, TextDisplay, V2Message, escape_markdown};

use super::view::{ProfileCard, ProfileData, render_profile};
use super::{BioForm, BioFormOverrides, EditBio, RefreshProfile, StoredProfile, profile_key, xp_key};
use crate::strings::{Strings, keys};

const XP_PER_LEVEL: u64 = 100;

/// `/profile [user]`: the struct's fields are the options (UD-3: fine for a quick
/// lookup target). `Option<T>` makes this one optional. `rate_limit` lets each user run
/// it 3 times per 10 s; the 4th gets the localized `noctis.rate_limited` notice and the
/// handler never runs.
#[derive(SlashCommand, Debug)]
#[command(
    "profile",
    contexts = [Guild],
    rate_limit(user, 3, per = "10s"),
)]
pub struct Profile {
    /// The description is `cmd.profile.opt.user.description` in `locales/en-US.yaml`.
    pub user: Option<ResolvedUser>,
}

impl Profile {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let target = self.user.map_or_else(|| ctx.user_id(), |u| u.user.id);
        let data = load(&ctx, target).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_profile(&data, &s, ctx.user_id() == target)).await
    }
}

/// `/bio` opens the bio modal directly. `auto_defer = false` turns off the auto-defer (a
/// modal must be the first response, INT-05), so the handler takes the `Manual`
/// responder, the only one with `show_modal()`, and opens it before any slow work.
#[derive(SlashCommand, Debug)]
#[command("bio", auto_defer = false, contexts = [Guild])]
pub struct Bio;

impl Bio {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> {
        // A fresh form: the Payload says whose bio, the inputs start empty (`blank` takes
        // the Payload fields). After a modal there is no original message to edit: end
        // with `done()`.
        Ok(r.show_modal(bio_modal(BioForm::blank(ctx.user_id()))).await?.done())
    }
}

/// The bio modal of `form`, its input texts localized for this render: input attribute
/// texts are literals, so the Labels and the placeholder come from the locale files through
/// `BioFormOverrides` (each `InputOverrides` field takes a literal or a `keys::` path).
fn bio_modal(form: BioForm) -> noctis::ui::ModalBuilder<BioForm> {
    form.build().overrides(BioFormOverrides {
        bio: InputOverrides {
            label: Some(keys::PROFILE_BIO_MODAL_BIO.into()),
            placeholder: Some(keys::PROFILE_BIO_MODAL_BIO_HINT.into()),
            ..Default::default()
        },
        pronouns: InputOverrides { label: Some(keys::PROFILE_BIO_MODAL_PRONOUNS.into()), ..Default::default() },
        ..Default::default()
    })
}

/// The badges `/badge` knows.
const BADGES: &[&str] = &["Early supporter", "Helper", "Night owl", "Artist", "Bug hunter"];

/// `/badge <name>`: `name` is suggested while typing by the paired
/// `#[autocomplete(name)]` method. The command re-checks the value, because Discord
/// sends whatever the user typed (OPT-12).
#[derive(SlashCommand, Debug)]
#[command("badge", contexts = [Guild])]
pub struct Badge {
    /// The description is `cmd.badge.opt.name.description` in `locales/en-US.yaml`.
    #[option(autocomplete)]
    pub name: String,
}

impl Badge {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(name) = BADGES.iter().find(|b| b.eq_ignore_ascii_case(&self.name)) else {
            return Err(HandlerError::user(keys::PROFILE_BADGE_UNKNOWN));
        };
        let s = Strings::of(ctx.response_locale());
        let text = TextDisplay::new(s.profile.badge(escape_markdown(name)));
        ctx.respond(V2Message::new(Components::new().push(text))).await
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
    key = "show_profile",
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
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let data = load(&ctx, self.user).await?;
        let s = Strings::of(ctx.response_locale());
        ctx.update(render_profile(&data, &s, self.editable)).await
    }
}

impl EditBio {
    /// Button → modal: builds the modal from a `BioForm` value whose input fields hold
    /// the stored values, so the inputs open pre-filled (FR-63; `None` sends nothing);
    /// its Payload (`user`) tells the submit whose profile message to redraw. A button
    /// that opens a modal is `auto_defer = false` (on its `#[button]`) and takes
    /// `ComponentResponder<Manual>` (INT-05).
    #[handler]
    async fn handle(self, ctx: ComponentCtx, responder: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        let guild = guild_of(&ctx)?;
        let stored: StoredProfile =
            ctx.kv().get(&profile_key(guild, self.user)).await?.unwrap_or_default();
        // Every BioForm input has an empty value, so `blank` returns a `BioForm` and the
        // struct update fills in the rest. `.build()` is a `ModalBuilder` that also takes
        // `.title(..)`, `.value(BioForm::BIO, ..)` and `.overrides(BioFormOverrides { .. })`;
        // `bio_modal` localizes the input texts with the latter.
        let form = BioForm { bio: stored.bio, pronouns: stored.pronouns, ..BioForm::blank(self.user) };
        Ok(responder.show_modal(bio_modal(form)).await?.done())
    }
}

impl BioForm {
    /// Modal → re-render: `self` is the decoded Payload plus the submitted values. Save,
    /// then redraw the message the modal was opened from with `ctx.update` (`response =
    /// update`, the default). A `/bio`-opened submit has no such message: `ctx.update`
    /// sends nothing and fails with `UpdateUnavailable` (it never falls back to a new
    /// message), so that submit answers with the card as a new message (`ctx.respond`).
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        let guild = guild_of(&ctx)?;
        let stored = StoredProfile {
            bio: self.bio.filter(|b| !b.trim().is_empty()),
            pronouns: self.pronouns,
        };
        ctx.kv().put(&profile_key(guild, self.user), &stored, None).await?;
        // The `manual` limit counts only saves that happened: one per user per minute.
        ctx.rate_limits().hit(RateScope::User).await?;
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
    matches!(err.as_error().downcast_ref(), Some(ResponderError::UpdateUnavailable))
}

/// Loads the profile and renders the card. Rendering is awaited here, inside the
/// handler, so the auto-defer covers a slow render (AD-22, FR-34).
async fn load(ctx: &impl Ctx, user_id: Id<UserMarker>) -> HandlerResult<ProfileData> {
    let guild = guild_of(ctx)?;
    // `Option<User>`: a user the cache has not seen yet (USER is filled from interactions).
    let user = ctx.cache().user(user_id).await?;
    let stored: StoredProfile =
        ctx.kv().get(&profile_key(guild, user_id)).await?.unwrap_or_default();
    let xp = ctx.kv().counter(&xp_key(guild, user_id)).await?;

    // The card's text follows the response's locale (FR-43).
    let locale = ctx.response_locale();
    let s = Strings::of(locale);
    #[expect(clippy::cast_precision_loss, reason = "remainder is < 100")]
    let progress = (xp % XP_PER_LEVEL) as f32 / XP_PER_LEVEL as f32;
    let name = user.as_ref().map_or_else(
        || user_id.to_string(),
        |u| u.global_name.clone().unwrap_or_else(|| u.name.clone()),
    );

    let card = ctx
        .render(ProfileCard {
            name: name.clone(),
            subtitle: stored.pronouns.map(|p| p.label(locale)).unwrap_or_default(),
            level_label: s.profile.card.level(xp / XP_PER_LEVEL),
            xp_label: s.profile.card.xp(xp % XP_PER_LEVEL, XP_PER_LEVEL),
            progress,
            avatar: user
                .as_ref()
                .map_or_else(RemoteImage::placeholder, |u| RemoteImage::avatar(u, 256)),
        })
        .await?;
    Ok(ProfileData { user: user_id, name, bio: stored.bio, card })
}

fn guild_of(ctx: &impl Ctx) -> HandlerResult<Id<GuildMarker>> {
    ctx.guild_id().ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))
}
