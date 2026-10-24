//! The reference card (NFR-3): a hand-written copy of the Project
//! Template's profile card, `examples/project-template/src/modules/profile/view.rs`,
//! node for node. Keep the two in step: when that view changes, change this
//! file (and `noctis-theme.css`, a verbatim copy of the template's theme) in
//! the same commit.
//!
//! It mirrors what `#[derive(Template)]` and `view!` expand to, without the
//! `noctis` facade those macros name, so both `noctis-image`'s
//! `reference_card` bench and the facade's `tests/reference_card.rs` compile
//! it. Only `noctis_core`, `noctis_image`, `noctis_ports` and `serde` paths
//! are used. The class strings skip the compile-time check (TW-01) as macro
//! literals do; the facade's `reference_card_classes_are_valid` test checks
//! [`CLASS_STRINGS`] instead.

#![allow(
    dead_code,
    reason = "the bench and the facade test each use part of this file"
)]

use noctis_core::{AppBuilder, Module};
use noctis_image::tw::{Theme, ThemeError};
use noctis_image::{
    AppBuilderImageExt, AssetField, Classes, DeclaredImages, Font, ImageConfig, Node, RemoteImage,
    Style, Tag, TemplateAssets, TemplateView, div, erased_view, img, span,
};
use noctis_ports::{AnimationSpec, ErasedView, Format, Frame, Size, Template};
use serde::{Deserialize, Serialize};

/// The card's root: a dark rounded surface.
pub const ROOT: &str = "flex items-center gap-8 w-full h-full p-10 bg-surface rounded-3xl";
/// The avatar.
pub const AVATAR: &str = "w-40 h-40 rounded-full border-4 border-brand";
/// The text column.
pub const COLUMN: &str = "flex flex-col gap-3 flex-1";
/// The name.
pub const NAME: &str = "text-5xl font-bold text-white";
/// The subtitle.
pub const SUBTITLE: &str = "text-xl text-muted";
/// The level and XP row.
pub const LEVEL_ROW: &str = "flex justify-between text-2xl text-white";
/// The XP label.
pub const XP: &str = "text-muted";
/// The progress track.
pub const TRACK: &str = "flex w-full h-6 rounded-full bg-slate-700";
/// The progress fill.
pub const FILL: &str = "h-full rounded-full bg-linear-to-r from-indigo-500 to-violet-400";

/// Every class string of the view.
pub const CLASS_STRINGS: [&str; 9] = [
    ROOT, AVATAR, COLUMN, NAME, SUBTITLE, LEVEL_ROW, XP, TRACK, FILL,
];

/// The bundled Inter, registered explicitly so renders are deterministic.
pub const INTER: &[u8] = include_bytes!("../../fonts/InterVariable.woff2");

/// The reference card: the Project Template's `ProfileCard`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceCard {
    pub name: String,
    pub subtitle: String,
    pub level_label: String,
    pub xp_label: String,
    /// Progress to the next level, 0.0..=1.0.
    pub progress: f32,
    /// What `#[asset] avatar: RemoteImage` declares.
    pub avatar: RemoteImage,
}

// `#[derive(Template)]` with
// `#[template(id = "reference.card", version = 1, size = (934, 282), format = Png)]`.
impl Template for ReferenceCard {
    const ID: &'static str = "reference.card";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(934, 282);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

// The derive's `TemplateAssets` impl for one `#[asset]` field.
impl TemplateAssets for ReferenceCard {
    const DECLARES: bool = true;

    fn declare(&self, out: &mut DeclaredImages) {
        <RemoteImage as AssetField>::declare(&self.avatar, "avatar", false, out);
    }
}

impl TemplateView for ReferenceCard {
    // What the template's `view!` lowers to.
    fn view(&self, _frame: Frame) -> Node {
        div(Classes::__literal(ROOT))
            .child(img(&self.avatar, Classes::__literal(AVATAR)))
            .child(
                div(Classes::__literal(COLUMN))
                    .child(span(Classes::__literal(NAME)).child(&self.name))
                    .child(span(Classes::__literal(SUBTITLE)).child(&self.subtitle))
                    .child(
                        div(Classes::__literal(LEVEL_ROW))
                            .child(Node::element(Tag::Span).child(&self.level_label))
                            .child(span(Classes::__literal(XP)).child(&self.xp_label)),
                    )
                    .child(
                        div(Classes::__literal(TRACK)).child(
                            div(Classes::__literal(FILL))
                                .style(Style::new().width_pct(self.progress * 100.0)),
                        ),
                    ),
            )
    }
}

/// The card's Theme: `noctis-theme.css`, the template's theme.
///
/// # Errors
///
/// A [`ThemeError`] when the file does not parse.
pub fn theme() -> Result<Theme, ThemeError> {
    Theme::parse(include_str!("noctis-theme.css"))
}

/// Adds the card's Theme, `config` with [`INTER`], and the `reference`
/// Module that registers [`ReferenceCard`].
///
/// # Errors
///
/// A [`ThemeError`] when the Theme does not parse.
pub fn configure_with(
    builder: &mut AppBuilder,
    config: ImageConfig,
) -> Result<&mut AppBuilder, ThemeError> {
    Ok(builder
        .theme(theme()?)
        .image(config.font(Font::from_static(INTER)))
        .module(Module::new("reference").template::<ReferenceCard>()))
}

/// [`configure_with`] the default [`ImageConfig`].
///
/// # Errors
///
/// A [`ThemeError`] when the Theme does not parse.
pub fn configure(builder: &mut AppBuilder) -> Result<&mut AppBuilder, ThemeError> {
    configure_with(builder, ImageConfig::new())
}

/// The sample card; distinct `xp` values give distinct renders.
pub fn sample(xp: u32) -> ReferenceCard {
    ReferenceCard {
        name: "Deniz".to_owned(),
        subtitle: "they/them".to_owned(),
        level_label: "Level 7".to_owned(),
        xp_label: format!("{xp} / 100 XP"),
        progress: 0.42,
        avatar: RemoteImage::placeholder(),
    }
}
