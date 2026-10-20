// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Profiles: a rendered card in a Media Gallery (UD-2), a modal to edit the bio
//! (FR-29, FR-63), a user context-menu command (FR-12) and an XP event handler (FR-6).
//!
//! The card is read-only: every action lives in buttons next to it, never in the image.

use std::time::Duration;

use noctis::events::MessageCreate;
use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::image::{Frame, Node, RemoteImage, Style, Template, TemplateView, view};
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Button, ButtonStyle, MediaGallery, MediaItem, ResolvedUser, Separator, TextDisplay,
    escape_markdown,
};
use serde::{Deserialize, Serialize};

use crate::strings::{Strings, keys};

const ACCENT: Rgb = rgb!(0xEB_459E);
const XP_PER_LEVEL: u64 = 100;

pub fn module() -> Module {
    Module::new("profile")
        .add(profile())
        .add(show_profile())
        .add(open_profile())
        .add(edit_bio())
        .add(save_bio())
        .add(award_xp())
        // Registering the Template lets App build check fonts, theme tokens and keyframes.
        .template::<ProfileCard>()
}

// ---------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------

/// Options are fine for a quick lookup target (UD-3). Required fields come first
/// (OPT-01); `Option<T>` makes this one optional.
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
async fn profile(ctx: CommandCtx, opts: ProfileOptions) -> HandlerResult<ProfileView> {
    let target = opts.user.map_or_else(|| ctx.user_id(), |u| u.user.id);
    ProfileView::load(&ctx, target).await
}

/// Right-click a user → Apps → "Show profile". The name is localized as a whole
/// (`ctxcmd.show_profile.name`); CMD-01/03 do not apply to context-menu names (CTX-01).
/// The target is the right-clicked user; `member` is `None` when the target is
/// not a member of the guild, including outside a guild.
#[user_command(
    "Show profile",
    key = "show_profile", // [planned]: rejected until its release
    contexts = [Guild],
)]
async fn show_profile(ctx: CommandCtx, target: ResolvedUser) -> HandlerResult<ProfileView> {
    ProfileView::load(&ctx, target.user.id).await
}

// ---------------------------------------------------------------------------------------
// Routes and handlers
// ---------------------------------------------------------------------------------------

/// The profile's View Route: the reload button re-runs the loader.
/// `editable` remembers whether the invoker viewed their own profile.
///
/// Width (`noctis_route_width`): `profile: F 1 + route 7 + sep 1 + S 0 + E 5 + N 0 + body 13 + T 0 = 27 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("profile", view)]
pub struct ProfileRoute {
    pub user: Id<UserMarker>,
    pub editable: bool,
}

/// "Edit bio" button. `opens_modal` turns off the auto-defer for its handler, because a
/// modal must be the first response (INT-05). `owner = user`: only that user may click.
///
/// Width (`noctis_route_width`): `profile.bio: F 1 + route 11 + sep 1 + S 0 + E 0 + N 0 + body 11 + T 0 = 24 (limit 100)`.
#[derive(Route, Debug, Clone, Copy)]
#[route("profile.bio", opens_modal, owner = user)] // [planned]: runtime arguments are rejected until their release
pub struct EditBio {
    pub user: Id<UserMarker>,
}

/// The modal's own Route: its submit lands in `save_bio`.
///
/// Width (`noctis_route_width`): `profile.save: F 1 + route 12 + sep 1 + S 0 + E 0 + N 0 + body 11 + T 0 = 25 (limit 100)`.
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

#[handler]
async fn open_profile(ctx: ComponentCtx, route: ProfileRoute) -> HandlerResult<ProfileView> {
    ProfileView::load(&ctx, route.user).await.map(|v| v.editable(route.editable))
}

/// Opens the modal pre-filled with the stored values (FR-63; the pre-fill setters are
/// [planned, Story 9.4]). A modal is a response
/// type of its own, so this handler takes the Responder instead of returning a View.
/// `EditBio` is declared `opens_modal`, so the handler must take the `OpensModal`
/// responder, the only one with `modal()` (INT-05).
#[handler]
async fn edit_bio(
    ctx: ComponentCtx,
    edit: EditBio,
    responder: ComponentResponder<OpensModal>,
) -> HandlerResult<Responded> {
    let guild = guild_of(&ctx)?;
    let stored: StoredProfile = ctx
        .kv()
        .get(&profile_key(guild, edit.user))
        .await?
        .unwrap_or_default();

    let modal = BioForm::modal(SaveBio { user: edit.user })
        .bio(stored.bio) // pre-fill setters: [planned, Story 9.4]
        .pronouns(stored.pronouns); // [planned, Story 9.4]
    Ok(responder.modal(modal).await?.done())
}

/// The modal was opened from the profile message, so returning the View updates that
/// message in place.
#[handler]
async fn save_bio(ctx: ModalSubmitCtx, save: SaveBio, form: BioForm) -> HandlerResult<ProfileView> {
    let guild = guild_of(&ctx)?;
    let stored = StoredProfile {
        bio: form.bio.filter(|b| !b.trim().is_empty()),
        pronouns: form.pronouns,
    };
    ctx.kv().put(&profile_key(guild, save.user), &stored, None).await?;
    ProfileView::load(&ctx, save.user).await.map(|v| v.editable(true))
}

/// +1 XP per message, at most once a minute per member. A `MessageCreate` handler
/// derives the GUILD_MESSAGES intent: `build()` adds it to the App (not privileged, so
/// no `.intents(..)` call; message content and DMs are not requested) and the gateway
/// starts parsing MESSAGE_CREATE. Concurrent (not `ordered`), never droppable.
#[event]
async fn award_xp(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
    let Some(guild) = ctx.guild_id() else {
        return Ok(());
    };
    if msg.author.bot {
        return Ok(());
    }
    let kv = ctx.kv(); // [planned, Epic 7]
    let cooldown = format!("xpcd:{guild}:{}", msg.author.id);
    if kv
        .put_if_absent(&cooldown, &(), Some(Duration::from_secs(60)))
        .await?
    {
        kv.incr(&xp_key(guild, msg.author.id), 1, None).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Data
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredProfile {
    pub bio: Option<String>,
    pub pronouns: Option<Pronouns>,
}

fn profile_key(guild: Id<GuildMarker>, user: Id<UserMarker>) -> String {
    format!("profile:{guild}:{user}")
}

fn xp_key(guild: Id<GuildMarker>, user: Id<UserMarker>) -> String {
    format!("xp:{guild}:{user}")
}

fn guild_of(ctx: &impl Ctx) -> HandlerResult<Id<GuildMarker>> {
    ctx.guild_id()
        .ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))
}

// ---------------------------------------------------------------------------------------
// Template (image)
// ---------------------------------------------------------------------------------------

/// The profile card. Data must serialize deterministically (no `HashMap`), because it is
/// part of the render cache key; bump `version` when `view` changes. `Deserialize` is
/// required too: the Renderer decodes the value it was sent (AD-22).
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "profile.card", version = 1, size = (934, 282), format = Png)]
pub struct ProfileCard {
    pub name: String,
    pub subtitle: String,
    pub level_label: String,
    pub xp_label: String,
    /// Progress to the next level, 0.0..=1.0. Applied with a typed style setter, never by
    /// formatting a class string (FR-66).
    pub progress: f32,
    /// Declared remote image: fetched from the Discord CDN and cached before rendering.
    /// On failure the built-in placeholder is used (add `required` to fail instead).
    #[asset]
    pub avatar: RemoteImage,
}

impl TemplateView for ProfileCard {
    // A still card ignores `frame` (it is `Frame::STILL`); an animated Template can compute
    // its state per frame from `frame.index` and `frame.count` (FR-69).
    fn view(&self, _frame: Frame) -> Node {
        // Literal classes are checked at compile time against the Tailwind v4 grammar and
        // the tokens in noctis-theme.css (bg-surface, text-muted, from-brand, ...).
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl">
                <img src={&self.avatar} tw="w-40 h-40 rounded-full border-4 border-brand" />
                <div tw="flex flex-col gap-3 flex-1">
                    <span tw="text-5xl font-bold text-white">{&self.name}</span>
                    <span tw="text-xl text-muted">{&self.subtitle}</span>
                    <div tw="flex justify-between text-2xl text-white">
                        <span>{&self.level_label}</span>
                        <span tw="text-muted">{&self.xp_label}</span>
                    </div>
                    <div tw="flex w-full h-6 rounded-full bg-slate-700">
                        <div
                            tw="h-full rounded-full bg-linear-to-r from-brand to-brand-soft"
                            style={Style::new().width_pct(self.progress * 100.0)}
                        />
                    </div>
                </div>
            </div>
        }
    }
}

// ---------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------

pub struct ProfileView {
    route: ProfileRoute,
    name: String,
    bio: Option<String>,
    card: Attachment,
}

impl ProfileView {
    /// Loads data and renders the card. Rendering is awaited here, inside the handler, so
    /// the auto-defer covers a slow render (AD-22, FR-34).
    pub async fn load(ctx: &impl Ctx, user_id: Id<UserMarker>) -> HandlerResult<Self> {
        let guild = guild_of(ctx)?;
        let user = ctx.cache().user(user_id).await?;
        let stored: StoredProfile = ctx
            .kv()
            .get(&profile_key(guild, user_id))
            .await?
            .unwrap_or_default();
        let xp = ctx.kv().counter(&xp_key(guild, user_id)).await?;
        let level = xp / XP_PER_LEVEL;

        // The card's text follows the response's locale: the User Locale for ephemeral
        // replies, the Guild Locale for public ones (FR-43).
        let locale = ctx.response_locale();
        let s = Strings::of(locale);
        #[expect(clippy::cast_precision_loss, reason = "remainder is < 100")]
        let progress = (xp % XP_PER_LEVEL) as f32 / XP_PER_LEVEL as f32;
        let pronouns = stored.pronouns.map(|p| p.label(locale));

        let card = ctx
            .render(ProfileCard {
                name: user.global_name.clone().unwrap_or_else(|| user.name.clone()),
                subtitle: pronouns.unwrap_or_default(),
                level_label: s.profile.card.level(level),
                xp_label: s.profile.card.xp(xp % XP_PER_LEVEL, XP_PER_LEVEL),
                progress,
                avatar: RemoteImage::avatar(&user, 256),
            })
            .await?;

        Ok(Self {
            route: ProfileRoute {
                user: user_id,
                editable: ctx.user_id() == user_id,
            },
            name: user.global_name.unwrap_or(user.name),
            bio: stored.bio,
            card,
        })
    }

    #[must_use]
    pub fn editable(mut self, editable: bool) -> Self {
        self.route.editable = editable;
        self
    }
}

impl View for ProfileView {
    type Route = ProfileRoute;

    fn state(&self) -> &ProfileRoute {
        &self.route
    }

    fn render(&self, cx: &mut ViewCx<'_, ProfileRoute>) -> UiResult<V2Message> {
        let s = Strings::of(cx.locale());
        let name = escape_markdown(&self.name);

        // `cx.attach` is the only way to reference an attachment, so a typo in a file
        // name cannot happen; alt text is required in strict mode (FR-30).
        let card = MediaGallery::new([
            MediaItem::new(cx.attach(&self.card)).alt(s.profile.card_alt(&name)),
        ]);

        // Designed empty state instead of a missing block.
        let bio = self.bio.as_deref().map_or_else(
            || format!("-# {}", s.profile.no_bio()),
            escape_markdown,
        );

        // A row takes a fixed-size array: one branch per shape. Mixed button kinds
        // convert to `Button`.
        let refresh = cx.reload_button().label(s.common.refresh());
        let footer = if self.route.editable {
            let edit = EditBio { user: self.route.user }
                .button()
                .label(s.profile.edit_bio())
                .style(ButtonStyle::Primary);
            ActionRow::buttons([Button::from(edit), Button::from(refresh)])
        } else {
            ActionRow::buttons([refresh])
        };

        Layout::new()
            .accent(ACCENT)
            .header(TextDisplay::new(format!("## {}", s.profile.title(&name))))
            .body(vec![
                card.into(),
                TextDisplay::new(bio).into(),
                Separator::small().into(),
            ])
            .footer(footer)
            .build(cx)
    }
}
