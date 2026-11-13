//! Story 20.3 (FR-82; EMO-01, EMO-03, EMO-04): `#[derive(AppEmojis)]`
//! naming, files, the white pair and rendered art, and the fold of
//! `AppBuilder::emojis` at `App::build`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::emoji::{
    AppEmojis, DeclaredEmoji, EmojiArt, EmojiDecl, EmojiFormat, EmojiImage, EmojiTone,
};
use noctis::{App, BuildViolation};
use noctis_ports::{AnimationSpec, ErasedView, Format, Size, Template};
use serde::{Deserialize, Serialize};

static CHECK_PNG: &[u8] = include_bytes!("fixtures/emoji/check.png");
static SPIN_GIF: &[u8] = include_bytes!("fixtures/emoji/spin.gif");

#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "tests/fixtures/emoji")]
enum Icons {
    #[emoji(white)]
    Check,
    StOnline,
    #[emoji(name = "ok", file = "check.png")]
    Tick,
    Spin,
}

#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "tests/fixtures/emoji")]
#[expect(dead_code, reason = "only the declarations are read")]
enum MoreIcons {
    #[emoji(name = "check")]
    Again,
    #[emoji(name = "spin")]
    Spinner,
}

/// A stand-in Template: only its id and value matter here.
#[derive(Serialize, Deserialize)]
struct Glyph {
    icon: u8,
    white: bool,
}

impl Template for Glyph {
    const ID: &'static str = "emoji.glyph";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(128, 128);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

fn glyph(icon: Drawn, tone: EmojiTone) -> EmojiArt {
    EmojiArt::template(Glyph {
        icon: icon as u8,
        white: tone == EmojiTone::White,
    })
}

#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "tests/fixtures/emoji", art = glyph)]
enum Drawn {
    #[emoji(render, white)]
    Logo,
    #[emoji(name = "plain", file = "check.png")]
    Plain,
}

#[test]
fn variants_are_snake_case_names_with_embedded_files() {
    let names: Vec<_> = Icons::DECLARATIONS.iter().map(EmojiDecl::name).collect();
    assert_eq!(names, ["check", "st_online", "ok", "spin"]);
    let check = Icons::Check.declaration();
    assert_eq!(check.white_name(), Some("check_white"));
    assert_eq!(check.name_in(EmojiTone::White), Some("check_white"));
    let EmojiImage::File { bytes, format } = *check.image() else {
        panic!("a file image");
    };
    assert_eq!((bytes, format), (CHECK_PNG, EmojiFormat::Png));
    assert!(matches!(
        check.white_image(),
        Some(EmojiImage::File { bytes, .. }) if *bytes != CHECK_PNG
    ));
    let EmojiImage::File { bytes, .. } = *Icons::Tick.declaration().image() else {
        panic!("a file image");
    };
    assert_eq!(bytes, CHECK_PNG, "#[emoji(file)] picks the file");
    assert_eq!(Icons::StOnline.declaration().white_name(), None);
}

#[test]
fn a_gif_is_an_animated_emoji() {
    let spin = Icons::Spin.declaration().image();
    assert_eq!(spin.animated(), Some(true));
    assert!(
        matches!(spin, EmojiImage::File { bytes, format: EmojiFormat::Gif } if *bytes == SPIN_GIF)
    );
    assert_eq!(Icons::Check.declaration().image().animated(), Some(false));
}

#[test]
fn rendered_variants_call_the_art_function_per_tone() {
    let logo = Drawn::Logo.declaration();
    assert_eq!(logo.name(), "logo");
    let EmojiImage::Render(colour) = *logo.image() else {
        panic!("a rendered image");
    };
    assert_eq!(logo.image().animated(), None);
    assert_eq!(colour(EmojiTone::Colour).template_id(), "emoji.glyph");
    let Some(EmojiImage::Render(white)) = logo.white_image().copied() else {
        panic!("a rendered white image");
    };
    assert!(format!("{:?}", white(EmojiTone::White)).contains("emoji.glyph"));
    assert!(matches!(
        Drawn::Plain.declaration().image(),
        EmojiImage::File { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn build_folds_every_set_once() {
    let app = App::builder()
        .emojis::<Icons>()
        .emojis::<Drawn>()
        .emojis::<Icons>()
        .build()
        .unwrap();
    let names: Vec<_> = app
        .emoji_declarations()
        .all()
        .iter()
        .map(DeclaredEmoji::name)
        .collect();
    assert_eq!(
        names,
        [
            "check",
            "check_white",
            "st_online",
            "ok",
            "spin",
            "logo",
            "logo_white",
            "plain"
        ],
        "registration order, each white variant after its colour; a repeated set is ignored"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn emo_03_a_name_two_sets_declare_fails_the_build() {
    let error = App::builder()
        .emojis::<Icons>()
        .emojis::<MoreIcons>()
        .build()
        .unwrap_err();
    let violations = error.into_violations();
    assert_eq!(violations.len(), 2, "{violations:?}");
    for (v, name) in violations.iter().zip(["check", "spin"]) {
        assert!(
            matches!(v, BuildViolation::DuplicateEmoji { name: n, .. } if *n == name),
            "{v:?}"
        );
        assert!(v.to_string().starts_with("[EMO-03]"), "{v}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn emo_01_more_names_than_the_limit_fail_the_build() {
    let mut limits = noctis::rules::Limits::default();
    limits.application_emojis = 4;
    let error = App::builder()
        .limits(limits)
        .emojis::<Icons>()
        .build()
        .unwrap_err();
    let violations = error.into_violations();
    assert_eq!(
        violations,
        [BuildViolation::TooManyEmojis { count: 5, limit: 4 }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_without_a_set_declares_nothing() {
    let app = App::builder().build().unwrap();
    assert!(app.emoji_declarations().is_empty());
}
