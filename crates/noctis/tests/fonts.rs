//! Story 10.7: fonts (priority, fallback, Unicode ranges, the bundled
//! font, the process-scoped registry) and static images through the facade.
//! Glyph comparisons are exact bitmap equality with single-font renders.

#![cfg(feature = "image")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::image::unstable::RenderJob;
use noctis::image::{
    AppBuilderImageExt, Font, Frame, ImageConfig, Node, RenderError, StaticImage, Style, Template,
    TemplateView, theme, view,
};
use noctis::{App, BuildViolation, Module, Runtime};
use serde::{Deserialize, Serialize};

static INTER: &[u8] = include_bytes!("../../noctis-image/fonts/InterVariable.woff2");
static JP400: &[u8] =
    include_bytes!("../../noctis-image/tests/fixtures/fonts/noto-sans-jp-119-400-normal.woff2");
static JP700: &[u8] =
    include_bytes!("../../noctis-image/tests/fixtures/fonts/noto-sans-jp-119-700-normal.woff2");

const RED: StaticImage = StaticImage::new("red", include_bytes!("fixtures/images/red-2x2.png"));
const BLUE: StaticImage = StaticImage::new("blue", include_bytes!("fixtures/images/blue-2x2.png"));
/// `red` with other bytes.
const FAKE_RED: StaticImage =
    StaticImage::new("red", include_bytes!("fixtures/images/blue-2x2.png"));

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED_PX: [u8; 4] = [255, 0, 0, 255];

/// One text run, centred, black on white.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "glyph", version = 1, size = (48, 48), format = Png)]
struct Glyph {
    text: String,
}

impl TemplateView for Glyph {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex w-full h-full items-center justify-center bg-white text-black text-3xl">
                {&self.text}
            </div>
        }
    }
}

/// [`Glyph`] in the theme's `font-display` family (`"Inter", sans-serif`).
#[derive(Template, Serialize, Deserialize)]
#[template(id = "display", version = 1, size = (48, 48), format = Png)]
struct Display {
    text: String,
}

impl TemplateView for Display {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex w-full h-full items-center justify-center bg-white text-black text-3xl font-display">
                {&self.text}
            </div>
        }
    }
}

/// The red 2x2 image, unsized, on a white 4x4 root.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "picture", version = 1, size = (4, 4), format = Png)]
struct Picture;

impl TemplateView for Picture {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-start w-full h-full bg-white">
                <img src={RED} />
            </div>
        }
    }
}

/// The red image sized by classes (16x16) and by `Style` (4 wide).
#[derive(Template, Serialize, Deserialize)]
#[template(id = "sized", version = 1, size = (24, 24), format = Png)]
struct Sized;

impl TemplateView for Sized {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex flex-col items-start w-full h-full bg-white">
                <img src={RED} tw="w-4 h-4" />
                <img src={RED} style={Style::new().width_px(4)} />
            </div>
        }
    }
}

/// Draws [`BLUE`], which no App registers.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "unregistered", version = 1, size = (4, 4), format = Png)]
struct Unregistered;

impl TemplateView for Unregistered {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div tw="flex w-full h-full"><img src={BLUE} /></div> }
    }
}

/// Draws [`FAKE_RED`]: the registered id `red` with other bytes.
#[derive(Template, Serialize, Deserialize)]
#[template(id = "fake", version = 1, size = (4, 4), format = Png)]
struct Fake;

impl TemplateView for Fake {
    fn view(&self, _frame: Frame) -> Node {
        view! { <div tw="flex w-full h-full"><img src={FAKE_RED} /></div> }
    }
}

fn templates() -> Module {
    Module::new("glyphs")
        .template::<Glyph>()
        .template::<Display>()
        .template::<Picture>()
        .template::<Sized>()
        .template::<Unregistered>()
        .template::<Fake>()
}

/// An App on its own test Runtime with `config` and every Template.
fn app(config: ImageConfig) -> App {
    app_on(&Runtime::for_test(), config).unwrap()
}

fn app_on(runtime: &Runtime, config: ImageConfig) -> Result<App, noctis::BuildError> {
    let mut builder = App::builder();
    builder
        .runtime(runtime)
        .theme(theme!())
        .image(config)
        .module(templates());
    builder.build()
}

fn with(fonts: impl IntoIterator<Item = Font>) -> App {
    app(fonts
        .into_iter()
        .fold(ImageConfig::new().threads(1), ImageConfig::font))
}

async fn rgba<T: Template>(app: &App, template: &T) -> Vec<u8> {
    let out = try_render(app, template).await.unwrap();
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&out.bytes[..]));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    buf
}

async fn try_render<T: Template>(
    app: &App,
    template: &T,
) -> Result<noctis::image::unstable::RenderOutput, RenderError> {
    app.renderer()
        .expect("a Renderer")
        .render(RenderJob::of(template).unwrap())
        .await
}

fn glyph(text: &str) -> Glyph {
    Glyph {
        text: text.to_owned(),
    }
}

fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * width + x) * 4;
    rgba[at..at + 4].try_into().unwrap()
}

/// The drawn pixels cropped to their bounding box, row by row, so glyphs
/// compare exactly wherever the line box placed them.
fn glyph_ink(rgba: &[u8]) -> Vec<Vec<[u8; 4]>> {
    let Some((x0, y0, x1, y1)) = bbox(rgba) else {
        return Vec::new();
    };
    (y0..=y1)
        .map(|y| (x0..=x1).map(|x| pixel(rgba, 48, x, y)).collect())
        .collect()
}

/// The bounding box `(x0, y0, x1, y1)` of every non-white pixel of a 48x48
/// render.
fn bbox(rgba: &[u8]) -> Option<(usize, usize, usize, usize)> {
    let mut found: Option<(usize, usize, usize, usize)> = None;
    for (index, px) in rgba.chunks_exact(4).enumerate() {
        if px != WHITE {
            let (x, y) = (index % 48, index / 48);
            found = Some(match found {
                None => (x, y, x, y),
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
            });
        }
    }
    found
}

/// Whether anything but the white background was drawn.
fn has_ink(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|px| px != WHITE)
}

#[tokio::test(flavor = "multi_thread")]
async fn fallback_walks_the_chain_in_priority_order() {
    let a = "あ";
    let both = rgba(
        &with([Font::from_static(INTER), Font::from_static(JP400)]),
        &glyph(a),
    )
    .await;
    let kana = rgba(&with([Font::from_static(JP400)]), &glyph(a)).await;
    let latin = rgba(&with([Font::from_static(INTER)]), &glyph(a)).await;
    assert!(has_ink(&kana));
    // The line box takes the primary (first) font's metrics, so the glyph may
    // sit at another height: compare the drawn glyph itself.
    assert_eq!(glyph_ink(&both), glyph_ink(&kana));
    assert_ne!(glyph_ink(&both), glyph_ink(&latin));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_font_in_priority_order_wins() {
    let regular = || Font::from_static(JP400).family("Kana Regular");
    let bold = || Font::from_static(JP700).family("Kana Bold");
    let text = glyph("あ");
    let regular_first = rgba(&with([regular(), bold()]), &text).await;
    let bold_first = rgba(&with([bold(), regular()]), &text).await;
    assert_eq!(regular_first, rgba(&with([regular()]), &text).await);
    assert_eq!(bold_first, rgba(&with([bold()]), &text).await);
    assert_ne!(regular_first, bold_first);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_unicode_range_limits_what_a_font_serves() {
    let hiragana = Font::from_static(JP700)
        .family("Hira Bold")
        .unicode_range('\u{3041}'..='\u{3096}');
    let app = with([hiragana, Font::from_static(JP400)]);
    let bold = with([Font::from_static(JP700)]);
    let regular = with([Font::from_static(JP400)]);
    assert_eq!(
        rgba(&app, &glyph("あ")).await,
        rgba(&bold, &glyph("あ")).await
    );
    assert_eq!(
        rgba(&app, &glyph("ア")).await,
        rgba(&regular, &glyph("ア")).await
    );
    assert_ne!(
        rgba(&app, &glyph("ア")).await,
        rgba(&bold, &glyph("ア")).await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_template_family_is_tried_before_the_chain() {
    // `font-display` asks for "Inter", here the regular kana face.
    let app = with([
        Font::from_static(JP700).family("Kana Bold"),
        Font::from_static(JP400).family("Inter"),
    ]);
    let display = Display {
        text: "あ".to_owned(),
    };
    let regular = with([Font::from_static(JP400)]);
    let bold = with([Font::from_static(JP700)]);
    assert_eq!(
        rgba(&app, &display).await,
        rgba(&regular, &glyph("あ")).await
    );
    assert_eq!(
        rgba(&app, &glyph("あ")).await,
        rgba(&bold, &glyph("あ")).await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn img_draws_a_static_image_at_its_size() {
    let app = app(ImageConfig::new()
        .threads(1)
        .font(Font::from_static(INTER))
        .image(RED)
        .image(RED));
    let picture = rgba(&app, &Picture).await;
    assert_eq!(pixel(&picture, 4, 0, 0), RED_PX);
    assert_eq!(pixel(&picture, 4, 1, 1), RED_PX);
    assert_eq!(pixel(&picture, 4, 3, 3), WHITE);
    let sized = rgba(&app, &Sized).await;
    // `w-4 h-4`: 16x16 at the top.
    assert_eq!(pixel(&sized, 24, 15, 15), RED_PX);
    assert_eq!(pixel(&sized, 24, 17, 8), WHITE);
    // `width_px(4)`: 4x4 below it.
    assert_eq!(pixel(&sized, 24, 3, 19), RED_PX);
    assert_eq!(pixel(&sized, 24, 5, 19), WHITE);
}

#[tokio::test(flavor = "multi_thread")]
async fn img_02_an_unregistered_image_fails_the_render() {
    let app = app(ImageConfig::new()
        .threads(1)
        .font(Font::from_static(INTER))
        .image(RED));
    for (result, image) in [
        (try_render(&app, &Unregistered).await, "blue"),
        (try_render(&app, &Fake).await, "red"),
    ] {
        match result {
            Err(RenderError::UnknownImage { id, image: got }) => {
                assert!(matches!(id.as_str(), "unregistered" | "fake"), "{id}");
                assert_eq!(got, image);
            }
            other => panic!("expected UnknownImage, got {other:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn img_02_bad_static_images_fail_startup() {
    let gif = StaticImage::new("anim", b"GIF89a\x01\0\x01\0\0\0\0;");
    let err = app_on(
        &Runtime::for_test(),
        ImageConfig::new().font(Font::from_static(INTER)).image(gif),
    )
    .unwrap_err();
    let [BuildViolation::Plugin { message, .. }] = err.violations() else {
        panic!("{err}");
    };
    assert_eq!(
        message,
        "[IMG-02] static image `anim`: unsupported format: use PNG, JPEG or WebP"
    );
    let err = app_on(
        &Runtime::for_test(),
        ImageConfig::new()
            .font(Font::from_static(INTER))
            .image(RED)
            .image(FAKE_RED),
    )
    .unwrap_err();
    let [BuildViolation::Plugin { message, .. }] = err.violations() else {
        panic!("{err}");
    };
    assert_eq!(
        message,
        "[IMG-02] static image id `red` is registered twice with different bytes"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn img_01_bad_font_config_fails_startup() {
    let err = app_on(
        &Runtime::for_test(),
        ImageConfig::new()
            .font(Font::from_static(INTER).unicode_range('b'..='a'))
            .font(Font::from_static(INTER).family("")),
    )
    .unwrap_err();
    let messages: Vec<&str> = err
        .violations()
        .iter()
        .map(|v| match v {
            BuildViolation::Plugin { message, .. } => message.as_str(),
            other => panic!("{other}"),
        })
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(
        messages[0],
        "[IMG-01] font #0: unicode range U+0062..=U+0061 is inverted"
    );
    assert!(messages[1].starts_with("[IMG-01] font #1: family name"));
    let err = app_on(
        &Runtime::for_test(),
        ImageConfig::new().font(Font::from_bytes(b"wOFF\0\0\0\0".to_vec())),
    )
    .unwrap_err();
    let [BuildViolation::Plugin { message, .. }] = err.violations() else {
        panic!("{err}");
    };
    assert!(
        message.starts_with("[IMG-01] font #0 (8 bytes) could not be loaded: "),
        "{message}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn apps_on_one_runtime_share_equal_fonts_and_conflict_on_others() {
    let a = || Font::from_static(INTER);
    let b = || Font::from_static(JP400);
    let runtime = Runtime::for_test();
    app_on(&runtime, ImageConfig::new().threads(1).font(a())).unwrap();
    app_on(&runtime, ImageConfig::new().threads(1).font(a())).unwrap();
    for other in [
        ImageConfig::new().threads(1).font(b()),
        ImageConfig::new().threads(1).font(b()).font(a()),
        ImageConfig::new().threads(1).font(a()).font(b()),
        ImageConfig::new().threads(1),
    ] {
        let err = app_on(&runtime, other).unwrap_err();
        assert!(
            matches!(
                err.violations(),
                [BuildViolation::ResourceConflict(c)] if c.resource.ends_with("FontRegistry")
            ),
            "{err}"
        );
    }
    // Test Runtimes are isolated.
    app_on(
        &Runtime::for_test(),
        ImageConfig::new().threads(1).font(b()),
    )
    .unwrap();
}

#[cfg(not(feature = "default-font"))]
#[tokio::test(flavor = "multi_thread")]
async fn img_01_no_font_fails_startup() {
    let err = app_on(&Runtime::for_test(), ImageConfig::new().threads(1)).unwrap_err();
    let [BuildViolation::TemplateRejected { message }] = err.violations() else {
        panic!("{err}");
    };
    assert!(
        message.starts_with("[IMG-01] no font is registered"),
        "{message}"
    );
    assert!(message.contains("ImageConfig::font"), "{message}");
    assert!(message.contains("default-font"), "{message}");
    // Without Templates, no font is needed.
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .image(ImageConfig::new().threads(1));
    assert!(builder.build().unwrap().has_renderer());
}

#[cfg(feature = "default-font")]
#[tokio::test(flavor = "multi_thread")]
async fn default_font_draws_text() {
    let app = app(ImageConfig::new().threads(1));
    let latin = rgba(&app, &glyph("Ag")).await;
    assert!(has_ink(&latin));
    // The bundled font is the last resort: an author font comes first.
    let kana = with([Font::from_static(JP400)]);
    assert_eq!(
        rgba(&kana, &glyph("あ")).await,
        rgba(&with([Font::from_static(JP400)]), &glyph("あ")).await
    );
    // The bundled "Inter" serves `font-display`.
    let display = rgba(
        &app,
        &Display {
            text: "Ag".to_owned(),
        },
    )
    .await;
    assert!(has_ink(&display));
}
