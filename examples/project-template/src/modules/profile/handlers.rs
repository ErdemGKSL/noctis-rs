// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Commands and handlers. Each loads the data, renders the card, and answers with
//! `ctx.respond(render_profile(..))` (new message) or `ctx.update(..)` (same message).

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::image::RemoteImage;
use noctis::prelude::*;
use noctis::ui::ResolvedUser;

use super::view::{ProfileCard, ProfileData, render_profile};
use super::{BioForm, EditBio, RefreshProfile, SaveBio, StoredProfile, profile_key, xp_key};
use crate::strings::{Strings, keys};

const XP_PER_LEVEL: u64 = 100;

/// Options are fine for a quick lookup target (UD-3). `Option<T>` makes this one optional.
#[derive(Options, Debug)]
pub struct ProfileOptions {
    /// The literal equals `cmd.profile.opt.user.description` in `locales/en-US.yaml`.
    #[option(description = "Whose profile to show (default you)")]
    pub user: Option<ResolvedUser>,
}

#[command(
    "profile",
    description = "Show a member's profile card",
    contexts = [Guild],
    rate_limit(user, 3, per = "10s"),      // [planned]: rejected until its release
)]
pub(super) async fn profile(ctx: CommandCtx, opts: ProfileOptions) -> HandlerResult<Responded> {
    let target = opts.user.map_or_else(|| ctx.user_id(), |u| u.user.id);
    let data = load(&ctx, target).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.respond(render_profile(&data, &s, ctx.user_id() == target)).await // [planned, Story 8.1]
}

/// `/bio` opens the bio modal directly. `opens_modal` turns off the auto-defer (a modal
/// must be the first response, INT-05), so the handler takes the `OpensModal` responder,
/// the only one with `modal()`, and opens it before any slow work.
#[command("bio", description = "Edit your profile bio", opens_modal, contexts = [Guild])]
pub(super) async fn bio(ctx: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded> {
    // After a modal there is no original message to edit: end with `done()`.
    Ok(r.modal(BioForm::modal(SaveBio { user: ctx.user_id() })).await?.done())
}

/// Right-click a user → Apps → "Show profile". The name is localized as a whole
/// (`ctxcmd.show_profile.name`); CMD-01/03 do not apply to context-menu names (CTX-01).
#[user_command(
    "Show profile",
    key = "show_profile", // [planned]: rejected until its release
    contexts = [Guild],
)]
pub(super) async fn show_profile(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<Responded> {
    let data = load(&ctx, target.user.id).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.respond(render_profile(&data, &s, ctx.user_id() == target.user.id)).await
}

#[handler]
pub(super) async fn refresh_profile(ctx: ComponentCtx, r: RefreshProfile) -> HandlerResult<Responded> {
    let data = load(&ctx, r.user).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.update(render_profile(&data, &s, r.editable)).await
}

/// Button → modal: opens the modal pre-filled with the stored values (FR-63; the
/// pre-fill setters are [planned, Story 9.4]) and hands the same Payload (`user`) to the
/// modal's Route, so the submit can redraw the profile message.
#[handler]
pub(super) async fn edit_bio(
    ctx: ComponentCtx,
    edit: EditBio,
    responder: ComponentResponder<OpensModal>,
) -> HandlerResult<Responded> {
    let guild = guild_of(&ctx)?;
    let stored: StoredProfile =
        ctx.kv().get(&profile_key(guild, edit.user)).await?.unwrap_or_default();
    let modal = BioForm::modal(SaveBio { user: edit.user })
        .bio(stored.bio)
        .pronouns(stored.pronouns);
    Ok(responder.modal(modal).await?.done())
}

/// Modal → re-render: save, then redraw the message the modal was opened from.
#[handler]
pub(super) async fn save_bio(ctx: ModalSubmitCtx, save: SaveBio, form: BioForm) -> HandlerResult<Responded> {
    let guild = guild_of(&ctx)?;
    let stored = StoredProfile {
        bio: form.bio.filter(|b| !b.trim().is_empty()),
        pronouns: form.pronouns,
    };
    ctx.kv().put(&profile_key(guild, save.user), &stored, None).await?;
    let data = load(&ctx, save.user).await?;
    let s = Strings::of(ctx.response_locale());
    ctx.update(render_profile(&data, &s, true)).await
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
