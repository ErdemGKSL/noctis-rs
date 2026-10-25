//! The profile card Template (an image) and `render_profile` (the message around it).
//! The card is read-only: every action lives in buttons next to it, never in the image.

use noctis::id::marker::UserMarker;
use noctis::image::RemoteImage;
use noctis::image::{Frame, Node, Style, Template, TemplateView, view};
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Attachment, AttachmentSet, Button, Components, Container, MediaGallery, MediaItem,
    Separator, TextDisplay, V2Message, escape_markdown, escape_mentions,
};
use serde::{Deserialize, Serialize};

use super::{EditBio, RefreshProfile};
use crate::strings::Strings;

const ACCENT: Rgb = rgb!(0xEB_459E);

/// Everything the profile message shows, loaded (and the card rendered) by the handler.
pub struct ProfileData {
    pub user: Id<UserMarker>,
    pub name: String,
    pub bio: Option<String>,
    pub card: Attachment,
}

/// Renders the profile message. `editable` adds the "Edit bio" button for the owner.
pub fn render_profile(p: &ProfileData, s: &Strings, editable: bool) -> V2Message {
    let name = escape_markdown(&p.name);

    // The attachment set is the only way to reference a file, so a typo in a file name
    // can't happen; alt text is required in strict mode (FR-30).
    let mut files = AttachmentSet::new();
    let card = files.add(p.card.clone());

    // Designed empty state instead of a missing block. A bio is free text: neutralize
    // its Markdown and its mentions (escape once each, in either order).
    let bio = p.bio.as_deref().map_or_else(
        || format!("-# {}", s.profile.no_bio()),
        |b| escape_mentions(&escape_markdown(b)),
    );

    // A row takes a fixed-size array: one branch per shape; mixed kinds convert to `Button`.
    // `.build()` uses each Route's declared defaults (label, style); no overrides needed.
    let refresh = RefreshProfile {
        user: p.user,
        editable,
    }
    .build();
    let footer = if editable {
        let edit = EditBio { user: p.user }.build();
        ActionRow::buttons([Button::from(edit), Button::from(refresh)])
    } else {
        ActionRow::buttons([refresh])
    };

    let container = Container::new()
        .accent(ACCENT)
        .push(TextDisplay::new(format!("## {}", s.profile.title(&name))))
        .push(MediaGallery::new([
            MediaItem::new(card).alt(s.profile.card_alt(&name))
        ]))
        .push(TextDisplay::new(bio))
        .push(Separator::small())
        .push(footer);
    V2Message::new(Components::new().push(container)).attachments(files)
}

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
    /// Declared remote image: fetched from the Discord CDN and cached in the `AssetStore`
    /// before rendering ([IMG-03]). On failure the fallback is drawn with a warning (add
    /// `required` to fail instead).
    #[asset]
    pub avatar: RemoteImage,
}

impl TemplateView for ProfileCard {
    // A still card ignores `frame` (it is `Frame::STILL`); an animated Template can compute
    // its state per frame from `frame.index` and `frame.count` (FR-69).
    fn view(&self, _frame: Frame) -> Node {
        // Literal classes are checked at compile time ([TW-01]) against the Tailwind v4
        // grammar and the tokens in noctis-theme.css (bg-surface, text-muted, border-brand).
        // Gradient stops take palette colours only, never theme tokens.
        view! {
            <div tw="flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl">
                // A remote `<img>` draws only a declared `#[asset]` field.
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
                            tw="h-full rounded-full bg-linear-to-r from-indigo-500 to-violet-400"
                            style={Style::new().width_pct(self.progress * 100.0)}
                        />
                    </div>
                </div>
            </div>
        }
    }
}
