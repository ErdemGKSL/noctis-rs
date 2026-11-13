//! Declared application emoji sets (FR-82, AD-33): the [`AppEmojis`] trait
//! `#[derive(AppEmojis)]` implements, the per-variant [`EmojiDecl`], the
//! Template-drawn [`EmojiArt`], and the App's folded [`EmojiDeclarations`].
//!
//! A set is data: the derive checks names and files at `cargo check`
//! (EMO-02..04) and embeds the files; [`AppBuilder::emojis`] registers a
//! set and `App::build` folds every set once (EMO-01, EMO-03 across sets).
//! Nothing here does I/O.
//!
//! [`AppBuilder::emojis`]: crate::AppBuilder::emojis

use std::any::TypeId;
use std::fmt;

use noctis_ports::{RenderJob, Template, TemplateDescriptor, ValueError};
use noctis_rules::Limits;
pub use noctis_rules::emoji::EmojiFormat;

use crate::BuildViolation;

/// A declared set of application emojis: a fieldless enum with
/// `#[derive(AppEmojis)]`, one variant per icon.
///
/// Implement it with the derive, which checks every name and file at
/// `cargo check`; a hand-written impl skips those checks (the App still
/// checks names, uniqueness and the count at build).
pub trait AppEmojis: Copy + Send + Sync + 'static {
    /// One declaration per variant, in declaration order.
    const DECLARATIONS: &'static [EmojiDecl];

    /// This variant's declaration.
    fn declaration(self) -> &'static EmojiDecl;
}

/// Which variant of an icon to draw or look up: the coloured one or the
/// `<name>_white` one (`#[emoji(white)]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmojiTone {
    /// The coloured variant, `<name>`.
    Colour,
    /// The white variant, `<name>_white`, for `Primary`, `Success` and
    /// `Danger` buttons.
    White,
}

/// Where the image of one declared emoji comes from.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum EmojiImage {
    /// A file embedded with `include_bytes!`, already checked (EMO-02,
    /// EMO-04).
    File {
        /// The file's bytes.
        bytes: &'static [u8],
        /// The sniffed format; GIF is animated.
        format: EmojiFormat,
    },
    /// Drawn at Publish by the set's `art` function (`#[emoji(render)]`).
    Render(fn(EmojiTone) -> EmojiArt),
}

impl EmojiImage {
    /// Whether the emoji is animated, when it is known before rendering:
    /// `Some(true)` for a GIF file, `Some(false)` for a PNG or JPEG file,
    /// `None` for a rendered one.
    pub const fn animated(&self) -> Option<bool> {
        match self {
            Self::File { format, .. } => Some(format.is_animated()),
            Self::Render(_) => None,
        }
    }
}

impl fmt::Debug for EmojiImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { bytes, format } => f
                .debug_struct("File")
                .field("format", format)
                .field("len", &bytes.len())
                .finish(),
            Self::Render(_) => f.write_str("Render"),
        }
    }
}

/// One variant of an [`AppEmojis`] set: its name, its image and, with
/// `#[emoji(white)]`, its white variant.
#[derive(Clone, Copy, Debug)]
pub struct EmojiDecl {
    name: &'static str,
    image: EmojiImage,
    white: Option<(&'static str, EmojiImage)>,
}

impl EmojiDecl {
    /// A declaration without a white variant; what the derive emits.
    #[doc(hidden)]
    pub const fn __new(name: &'static str, image: EmojiImage) -> Self {
        Self {
            name,
            image,
            white: None,
        }
    }

    /// Adds the white variant `white_name`; what the derive emits.
    #[doc(hidden)]
    #[must_use]
    pub const fn __white(mut self, white_name: &'static str, image: EmojiImage) -> Self {
        self.white = Some((white_name, image));
        self
    }

    /// The coloured variant's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The coloured variant's image.
    pub const fn image(&self) -> &EmojiImage {
        &self.image
    }

    /// The white variant's name (`<name>_white`), if declared.
    pub const fn white_name(&self) -> Option<&'static str> {
        match &self.white {
            Some((name, _)) => Some(name),
            None => None,
        }
    }

    /// The white variant's image, if declared.
    pub const fn white_image(&self) -> Option<&EmojiImage> {
        match &self.white {
            Some((_, image)) => Some(image),
            None => None,
        }
    }

    /// The name of the variant in `tone`; `None` for [`EmojiTone::White`]
    /// without a white variant.
    pub const fn name_in(&self, tone: EmojiTone) -> Option<&'static str> {
        match tone {
            EmojiTone::Colour => Some(self.name),
            EmojiTone::White => self.white_name(),
        }
    }
}

/// The art of a `#[emoji(render)]` declaration: a Template drawn through
/// the App's Renderer at Publish.
///
/// The set's `art` function (`#[emojis(art = path::to::fn)]`, a
/// `fn(Self, EmojiTone) -> EmojiArt`) returns it for every rendered
/// variant and tone.
///
/// ```
/// use noctis_core::emoji::{EmojiArt, EmojiTone};
/// # use noctis_ports::{AnimationSpec, ErasedView, Format, Size, Template};
/// # #[derive(serde::Serialize, serde::Deserialize)]
/// # struct Glyph { white: bool }
/// # impl Template for Glyph {
/// #     const ID: &'static str = "emoji.glyph";
/// #     const VERSION: u32 = 1;
/// #     const SIZE: Size = Size::new(128, 128);
/// #     const FORMAT: Format = Format::Png;
/// #     const ANIMATION: Option<AnimationSpec> = None;
/// #     fn view() -> ErasedView { ErasedView::new(()) }
/// # }
///
/// let art = EmojiArt::template(Glyph { white: true });
/// assert_eq!(art.template_id(), "emoji.glyph");
/// ```
pub struct EmojiArt {
    pub(crate) descriptor: TemplateDescriptor,
    #[expect(dead_code, reason = "rendered by the Publish emoji sync (Story 20.5)")]
    pub(crate) job: Result<RenderJob, ValueError>,
}

impl EmojiArt {
    /// The art drawn by `template`.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "an art function builds the value inline: EmojiArt::template(Glyph { .. })"
    )]
    pub fn template<T: Template>(template: T) -> Self {
        Self {
            descriptor: TemplateDescriptor::of::<T>(),
            job: RenderJob::of(&template),
        }
    }

    /// The Template's id.
    pub fn template_id(&self) -> &'static str {
        self.descriptor.id
    }
}

impl fmt::Debug for EmojiArt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmojiArt")
            .field("template", &self.template_id())
            .finish_non_exhaustive()
    }
}

/// One declared emoji of an App: a name, its image and its tone, from one
/// registered set.
#[derive(Clone, Copy, Debug)]
pub struct DeclaredEmoji {
    name: &'static str,
    image: EmojiImage,
    tone: EmojiTone,
    set: &'static str,
}

impl DeclaredEmoji {
    /// The emoji's name on Discord.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Its image.
    pub const fn image(&self) -> &EmojiImage {
        &self.image
    }

    /// Whether it is a coloured or a `_white` variant.
    pub const fn tone(&self) -> EmojiTone {
        self.tone
    }

    /// The type name of the set that declares it.
    pub const fn set(&self) -> &'static str {
        self.set
    }
}

/// Every emoji an App declares, folded once at `App::build` from the sets
/// of [`AppBuilder::emojis`](crate::AppBuilder::emojis), in registration
/// order (each variant, then its white variant). Empty without a set, and
/// then the App makes no emoji request.
#[derive(Clone, Debug, Default)]
pub struct EmojiDeclarations {
    emojis: Vec<DeclaredEmoji>,
}

impl EmojiDeclarations {
    /// The number of declared names, `_white` variants included.
    pub fn len(&self) -> usize {
        self.emojis.len()
    }

    /// Whether no set is registered (or every set is empty).
    pub fn is_empty(&self) -> bool {
        self.emojis.is_empty()
    }

    /// Every declared emoji, in fold order.
    pub fn all(&self) -> &[DeclaredEmoji] {
        &self.emojis
    }

    /// The declared emoji named `name`.
    pub fn get(&self, name: &str) -> Option<&DeclaredEmoji> {
        self.emojis.iter().find(|e| e.name == name)
    }
}

/// One set registered with `AppBuilder::emojis`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EmojiSet {
    pub(crate) id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) declarations: &'static [EmojiDecl],
}

impl EmojiSet {
    pub(crate) fn of<T: AppEmojis>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            declarations: T::DECLARATIONS,
        }
    }
}

/// Folds `sets` (one per type, in registration order) into the App's
/// declarations: a bad name or a name two sets declare is an EMO-03
/// violation, more than `limits.application_emojis` names an EMO-01 one.
pub(crate) fn fold(sets: &[EmojiSet], limits: &Limits) -> (EmojiDeclarations, Vec<BuildViolation>) {
    let mut emojis: Vec<DeclaredEmoji> = Vec::new();
    let mut violations = Vec::new();
    for set in sets {
        for decl in set.declarations {
            let mut push = |name: &'static str, image: EmojiImage, tone: EmojiTone| {
                if let Err(violation) = noctis_rules::emoji::validate_emoji_name(name) {
                    violations.push(BuildViolation::InvalidEmoji {
                        set: set.type_name,
                        violation: *violation,
                    });
                } else if let Some(first) = emojis.iter().find(|e| e.name == name) {
                    violations.push(BuildViolation::DuplicateEmoji {
                        name,
                        first_set: first.set,
                        second_set: set.type_name,
                    });
                } else {
                    emojis.push(DeclaredEmoji {
                        name,
                        image,
                        tone,
                        set: set.type_name,
                    });
                }
            };
            push(decl.name, decl.image, EmojiTone::Colour);
            if let Some((white, image)) = decl.white {
                push(white, image, EmojiTone::White);
            }
        }
    }
    if emojis.len() > limits.application_emojis {
        violations.push(BuildViolation::TooManyEmojis {
            count: emojis.len(),
            limit: limits.application_emojis,
        });
    }
    (EmojiDeclarations { emojis }, violations)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const FILE: EmojiImage = EmojiImage::File {
        bytes: PNG,
        format: EmojiFormat::Png,
    };

    #[derive(Clone, Copy)]
    struct A;
    impl AppEmojis for A {
        const DECLARATIONS: &'static [EmojiDecl] = &[
            EmojiDecl::__new("check", FILE).__white("check_white", FILE),
            EmojiDecl::__new("cross", FILE),
        ];
        fn declaration(self) -> &'static EmojiDecl {
            &Self::DECLARATIONS[0]
        }
    }

    #[derive(Clone, Copy)]
    struct B;
    impl AppEmojis for B {
        const DECLARATIONS: &'static [EmojiDecl] =
            &[EmojiDecl::__new("cross", FILE), EmojiDecl::__new("x", FILE)];
        fn declaration(self) -> &'static EmojiDecl {
            &Self::DECLARATIONS[0]
        }
    }

    #[test]
    fn fold_keeps_order_and_white_pairs() {
        let (decls, violations) = fold(&[EmojiSet::of::<A>()], &Limits::default());
        assert!(violations.is_empty());
        let names: Vec<_> = decls.all().iter().map(DeclaredEmoji::name).collect();
        assert_eq!(names, ["check", "check_white", "cross"]);
        assert_eq!(decls.get("check_white").unwrap().tone(), EmojiTone::White);
        assert_eq!(decls.get("check").unwrap().image().animated(), Some(false));
        assert!(decls.get("nope").is_none());
        assert_eq!(decls.len(), 3);
        let decl = A.declaration();
        assert_eq!(decl.name_in(EmojiTone::White), Some("check_white"));
        assert_eq!(A::DECLARATIONS[1].name_in(EmojiTone::White), None);
    }

    #[test]
    fn emo_03_duplicate_and_bad_names_across_sets() {
        let (decls, violations) = fold(
            &[EmojiSet::of::<A>(), EmojiSet::of::<B>()],
            &Limits::default(),
        );
        assert_eq!(decls.len(), 3);
        assert_eq!(violations.len(), 2);
        assert!(matches!(
            violations[0],
            BuildViolation::DuplicateEmoji { name: "cross", .. }
        ));
        assert!(
            violations[0]
                .to_string()
                .starts_with("[EMO-03] emoji `cross` is declared by two sets")
        );
        assert!(matches!(violations[1], BuildViolation::InvalidEmoji { .. }));
        assert!(
            violations[1].to_string().starts_with("[EMO-03]"),
            "{}",
            violations[1]
        );
    }

    #[test]
    fn emo_01_too_many_emojis() {
        let mut limits = Limits::default();
        limits.application_emojis = 2;
        let (_, violations) = fold(&[EmojiSet::of::<A>()], &limits);
        assert_eq!(
            violations,
            [BuildViolation::TooManyEmojis { count: 3, limit: 2 }]
        );
        assert!(violations[0].to_string().starts_with("[EMO-01]"));
    }

    #[test]
    fn empty_fold_declares_nothing() {
        let (decls, violations) = fold(&[], &Limits::default());
        assert!(decls.is_empty() && violations.is_empty());
    }

    #[test]
    fn debug_hides_bytes() {
        assert_eq!(format!("{FILE:?}"), "File { format: Png, len: 8 }");
    }
}
