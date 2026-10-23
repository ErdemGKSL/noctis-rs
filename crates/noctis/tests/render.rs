//! Story 10.3: `ImagePlugin`, `ImageConfig`, the render pool and the takumi
//! Renderer through the facade, with derived Templates. The macros validate
//! against this crate's `noctis-theme.css` (`--color-brand: #5865f2`).

#![cfg(feature = "image")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use bytes::Bytes;
use noctis::image::{
    AppBuilderImageExt, Classes, Format, Frame, ImageConfig, ImagePlugin, Node, RenderError, Style,
    Template, TemplateView, Theme, div, theme, view,
};
use noctis::ports::{EncodeSettings, ErasedView, OutputFormat, RenderJob, Size, ValueError};
use noctis::{App, AppBuilder, BuildError, BuildViolation, Module, Runtime};
use serde::{Deserialize, Serialize};

const BRAND: [u8; 4] = [0x58, 0x65, 0xF2, 0xFF];

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "card", version = 1, size = (64, 32), format = Png)]
struct Card {
    name: String,
}

impl TemplateView for Card {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="flex w-full h-full bg-brand">{&self.name}</div> }
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "dot", version = 1, size = (1, 1), format = Png)]
struct Dot;

impl TemplateView for Dot {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="w-full h-full bg-brand" /> }
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "surface", version = 1, size = (1, 1), format = Png)]
struct Surface;

impl TemplateView for Surface {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="w-full h-full bg-surface" /> }
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "huge", version = 1, size = (10000, 10000), format = Png)]
struct Huge;

impl TemplateView for Huge {
    fn view(&self, _: Frame) -> Node {
        view! { <div /> }
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "styled", version = 1, size = (8, 8), format = Png)]
struct Styled;

impl TemplateView for Styled {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="w-full h-full bg-red-500" style={Style::new().bg_rgb(0x00_FF00)} /> }
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "boom", version = 1, size = (4, 4), format = Png)]
struct Boom {
    string: bool,
}

impl TemplateView for Boom {
    fn view(&self, _: Frame) -> Node {
        assert!(!self.string, "boom");
        std::panic::panic_any(5_u8)
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "dynamic", version = 1, size = (4, 4), format = Png)]
struct Dynamic {
    classes: String,
}

impl TemplateView for Dynamic {
    fn view(&self, _: Frame) -> Node {
        div(Classes::dynamic(self.classes.clone()))
            .style(Style::new().width_pct(100.0).height_pct(100.0))
    }
}

/// Where the `Where` view ran: (thread name, outside tokio).
static WHERE: Mutex<Option<(String, bool)>> = Mutex::new(None);

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "where", version = 1, size = (2, 2), format = Png)]
struct Where;

impl TemplateView for Where {
    fn view(&self, _: Frame) -> Node {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        let outside = tokio::runtime::Handle::try_current().is_err();
        *WHERE.lock().unwrap() = Some((name, outside));
        view! { <div /> }
    }
}

/// The gate `Gated` views block on while it is set.
static GATE: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "gated", version = 1, size = (2, 2), format = Png)]
struct Gated;

impl TemplateView for Gated {
    fn view(&self, _: Frame) -> Node {
        if let Some(gate) = GATE.lock().unwrap().as_ref() {
            gate.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        view! { <div /> }
    }
}

#[derive(Template, Serialize, Deserialize, Debug, Clone)]
#[template(id = "spinner", version = 1, size = (2, 2), format = Png,
           animation(duration_ms = 1000, fps = 10, format = Gif))]
struct Spinner;

impl TemplateView for Spinner {
    fn view(&self, _: Frame) -> Node {
        view! { <div /> }
    }
}

/// A hand-written Template whose view the derive did not make.
#[derive(Serialize, Deserialize)]
struct Foreign;

impl Template for Foreign {
    const ID: &'static str = "x";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(2, 2);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<noctis::image::AnimationSpec> = None;

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

fn cards() -> Module {
    Module::new("cards")
        .template::<Card>()
        .template::<Dot>()
        .template::<Surface>()
        .template::<Huge>()
        .template::<Styled>()
        .template::<Boom>()
        .template::<Dynamic>()
        .template::<Where>()
        .template::<Gated>()
}

/// A builder on `runtime` with the test theme, `config` and [`cards`].
fn builder(runtime: &Runtime, config: ImageConfig) -> AppBuilder {
    let mut builder = App::builder();
    builder
        .runtime(runtime)
        .theme(theme!())
        .image(config)
        .module(cards());
    builder
}

/// A test-Runtime App with `config`.
fn app_with(config: ImageConfig) -> App {
    builder(&Runtime::for_test(), config).build().unwrap()
}

fn app() -> App {
    app_with(ImageConfig::new().threads(2))
}

fn build_err(config: ImageConfig) -> BuildError {
    builder(&Runtime::for_test(), config).build().unwrap_err()
}

fn plugin_messages(err: &BuildError) -> Vec<String> {
    err.violations()
        .iter()
        .map(|v| match v {
            BuildViolation::Plugin { plugin, message } => {
                assert!(plugin.ends_with("ImagePlugin"), "{plugin}");
                message.clone()
            }
            other => panic!("{other}"),
        })
        .collect()
}

async fn render<T: Template>(
    app: &App,
    template: &T,
) -> Result<noctis::ports::RenderOutput, RenderError> {
    app.renderer()
        .expect("a Renderer")
        .render(RenderJob::of(template).unwrap())
        .await
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(
        reader.output_color_type(),
        (png::ColorType::Rgba, png::BitDepth::Eight)
    );
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    rgba[at..at + 4].try_into().unwrap()
}

fn ada() -> Card {
    Card {
        name: "Ada".to_owned(),
    }
}

// ---- config and plugin ----

#[tokio::test(flavor = "multi_thread")]
async fn threads_bounds() {
    for threads in [1, 256] {
        app_with(ImageConfig::new().threads(threads));
    }
    for threads in [0, 257] {
        let messages = plugin_messages(&build_err(ImageConfig::new().threads(threads)));
        assert_eq!(
            messages,
            [format!(
                "render pool threads {threads} is out of range: use 1..=256"
            )]
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_bounds() {
    for capacity in [1, 4096] {
        app_with(ImageConfig::new().threads(1).queue_capacity(capacity));
    }
    for capacity in [0, 4097] {
        let messages = plugin_messages(&build_err(
            ImageConfig::new().threads(1).queue_capacity(capacity),
        ));
        assert_eq!(
            messages,
            [format!(
                "render pool queue capacity {capacity} is out of range: use 1..=4096"
            )]
        );
    }
    let both = plugin_messages(&build_err(ImageConfig::new().threads(0).queue_capacity(0)));
    assert_eq!(both.len(), 2, "{both:?}");
}

#[test]
fn config_defaults() {
    assert_eq!(ImageConfig::new(), ImageConfig::default());
    let debug = format!("{:?}", ImageConfig::new());
    assert!(debug.contains("queue_capacity: 64"), "{debug}");
    assert!(!debug.contains("threads: 0,"), "{debug}");
}

#[tokio::test(flavor = "multi_thread")]
async fn ext_only_theme_or_only_image_installs_a_renderer() {
    let mut only_theme = App::builder();
    only_theme.runtime(&Runtime::for_test()).theme(theme!());
    assert!(only_theme.plugin_mut::<ImagePlugin>().is_some());
    assert!(only_theme.build().unwrap().has_renderer());

    let mut only_image = App::builder();
    only_image
        .runtime(&Runtime::for_test())
        .image(ImageConfig::new().threads(1));
    let debug = format!("{:?}", only_image.plugin_mut::<ImagePlugin>().unwrap());
    assert!(debug.contains(&format!("{:?}", Theme::empty())), "{debug}");
    assert!(only_image.build().unwrap().has_renderer());
}

#[tokio::test(flavor = "multi_thread")]
async fn ext_last_call_of_each_kind_wins() {
    let a = Theme::parse("@theme { --color-a: #000001; }").unwrap();
    let b = Theme::parse("@theme { --color-b: #000002; }").unwrap();
    let c1 = ImageConfig::new().threads(3);
    let c2 = ImageConfig::new().threads(4).queue_capacity(5);
    let mut builder = App::builder();
    builder
        .runtime(&Runtime::for_test())
        .theme(a)
        .image(c1)
        .theme(b.clone())
        .image(c2.clone());
    let debug = format!("{:?}", builder.plugin_mut::<ImagePlugin>().unwrap());
    assert!(debug.contains(&format!("{c2:?}")), "{debug}");
    assert!(debug.contains(&format!("{b:?}")), "{debug}");
    assert!(!debug.contains("color-a"), "{debug}");
    let app = builder.build().unwrap();
    assert!(app.has_renderer());
}

#[tokio::test(flavor = "multi_thread")]
async fn apps_on_one_runtime_share_the_pool_and_conflict_on_another_config() {
    let runtime = Runtime::for_test();
    let config = ImageConfig::new().threads(2).queue_capacity(8);
    builder(&runtime, config.clone()).build().unwrap();
    builder(&runtime, config.clone()).build().unwrap();
    let err = builder(&runtime, config.threads(3)).build().unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [BuildViolation::ResourceConflict(c)] if c.resource.ends_with("RenderPool")
        ),
        "{err}"
    );
    // Test Runtimes are isolated.
    app_with(ImageConfig::new().threads(1));
    app_with(ImageConfig::new().threads(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn register_rejects_foreign_and_animated_templates() {
    let mut foreign = builder(&Runtime::for_test(), ImageConfig::new().threads(1));
    foreign.module(Module::new("foreign").template::<Foreign>());
    let err = foreign.build().unwrap_err();
    let [BuildViolation::TemplateRejected { message }] = err.violations() else {
        panic!("{err}");
    };
    assert!(message.contains("`x`"), "{message}");
    assert!(message.contains("#[derive(Template)]"), "{message}");

    let mut animated = builder(&Runtime::for_test(), ImageConfig::new().threads(1));
    animated.module(Module::new("anim").template::<Spinner>());
    let err = animated.build().unwrap_err();
    let [BuildViolation::TemplateRejected { message }] = err.violations() else {
        panic!("{err}");
    };
    assert!(
        message.contains("animated Templates are not rendered yet"),
        "{message}"
    );
}

// ---- rendering ----

#[tokio::test(flavor = "multi_thread")]
async fn renders_a_themed_png_deterministically() {
    let app = app();
    let out = render(&app, &ada()).await.unwrap();
    assert_eq!(&out.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(out.format, OutputFormat::Png);
    assert_eq!(out.digest, *blake3::hash(&out.bytes).as_bytes());
    assert_eq!(
        out.settings_used,
        EncodeSettings {
            size: Size::new(64, 32),
            quality: None,
            fps: None,
            frames: 1,
        }
    );
    let (w, h, rgba) = decode_png(&out.bytes);
    assert_eq!((w, h), (64, 32));
    assert_eq!(pixel(&rgba, w, 0, 0), BRAND);
    assert_eq!(pixel(&rgba, w, 63, 31), BRAND);
    assert_eq!(render(&app, &ada()).await.unwrap().bytes, out.bytes);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_later_theme_token_reaches_the_stylesheet() {
    let out = render(&app(), &Surface).await.unwrap();
    let (w, _, rgba) = decode_png(&out.bytes);
    assert_eq!(pixel(&rgba, w, 0, 0), [0x11, 0x13, 0x1A, 0xFF]);
}

#[tokio::test(flavor = "multi_thread")]
async fn encodes_every_still_format() {
    let app = app();
    let renderer = app.renderer().unwrap();
    for (format, ext, quality) in [
        (Format::Jpeg { quality: 1 }, "jpg", Some(1)),
        (Format::Jpeg { quality: 100 }, "jpg", Some(100)),
        (Format::Webp { quality: 1 }, "webp", Some(1)),
        (Format::Webp { quality: 100 }, "webp", Some(100)),
        (Format::WebpLossless, "webp", None),
    ] {
        let job = RenderJob {
            format,
            ..RenderJob::of(&ada()).unwrap()
        };
        let out = renderer.render(job).await.unwrap();
        if ext == "jpg" {
            assert_eq!(&out.bytes[..2], b"\xFF\xD8", "{format:?}");
        } else {
            assert_eq!(&out.bytes[..4], b"RIFF", "{format:?}");
            assert_eq!(&out.bytes[8..12], b"WEBP", "{format:?}");
        }
        assert_eq!(out.format.extension(), ext);
        assert_eq!(out.settings_used.quality, quality, "{format:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn min_size_canvas_limit_style_and_unicode() {
    let app = app();
    let dot = render(&app, &Dot).await.unwrap();
    let (w, h, rgba) = decode_png(&dot.bytes);
    assert_eq!((w, h), (1, 1));
    assert_eq!(pixel(&rgba, w, 0, 0), BRAND);

    let err = render(&app, &Huge).await.unwrap_err();
    let RenderError::Failed(source) = &err else {
        panic!("{err}");
    };
    assert!(source.to_string().contains("megapixels"), "{source}");
    assert!(render(&app, &ada()).await.is_ok(), "the pool still renders");

    let styled = render(&app, &Styled).await.unwrap();
    let (w, _, rgba) = decode_png(&styled.bytes);
    assert_eq!(pixel(&rgba, w, 0, 0), [0, 0xFF, 0, 0xFF]);

    let unicode = Card {
        name: "Ada 😀 ğüş".to_owned(),
    };
    assert!(render(&app, &unicode).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_skewed_animated_and_bad_jobs_fail() {
    let app = app();
    let renderer = app.renderer().unwrap();
    let job = || RenderJob::of(&ada()).unwrap();
    let unknown = renderer
        .render(RenderJob {
            template_id: "nope",
            ..job()
        })
        .await;
    assert!(matches!(unknown, Err(RenderError::UnknownTemplate { id, .. }) if id == "nope"));
    let skew = renderer
        .render(RenderJob {
            template_version: Card::VERSION + 1,
            ..job()
        })
        .await;
    assert!(matches!(
        skew,
        Err(RenderError::UnknownTemplate { version: 2, .. })
    ));
    let animated = renderer
        .render(RenderJob {
            animation: Spinner::ANIMATION,
            ..job()
        })
        .await;
    assert!(matches!(animated, Err(RenderError::Unsupported { .. })));

    let mut trailing = job().data.to_vec();
    trailing.push(0);
    for (data, expected) in [
        (Bytes::new(), ValueError::Malformed),
        (Bytes::from(trailing), ValueError::Trailing { extra: 1 }),
    ] {
        let err = renderer
            .render(RenderJob { data, ..job() })
            .await
            .unwrap_err();
        let RenderError::Failed(source) = &err else {
            panic!("{err}");
        };
        assert_eq!(source.downcast_ref::<ValueError>(), Some(&expected));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn panics_are_errors_and_the_pool_survives() {
    let app = app_with(ImageConfig::new().threads(1));
    let boom = render(&app, &Boom { string: true }).await;
    assert!(matches!(boom, Err(RenderError::Panicked { message }) if message == "boom"));
    let other = render(&app, &Boom { string: false }).await;
    assert!(
        matches!(other, Err(RenderError::Panicked { message }) if message == "<non-string panic>")
    );
    assert!(render(&app, &ada()).await.is_ok());

    let single = app_with(ImageConfig::new().threads(1).queue_capacity(1));
    assert!(render(&single, &Boom { string: true }).await.is_err());
    assert!(
        render(&single, &ada()).await.is_ok(),
        "the panic freed its slot"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_queue_fails_at_once_and_admitted_renders_finish() {
    let (open, gate) = mpsc::channel();
    *GATE.lock().unwrap() = Some(gate);
    let app = app_with(ImageConfig::new().threads(1).queue_capacity(2));
    let renderer = Arc::clone(app.renderer().unwrap());
    let a = renderer.render(RenderJob::of(&Gated).unwrap());
    let b = renderer.render(RenderJob::of(&Gated).unwrap());
    let c = renderer.render(RenderJob::of(&Gated).unwrap()).await;
    assert!(matches!(c, Err(RenderError::QueueFull)));
    open.send(()).unwrap();
    open.send(()).unwrap();
    assert!(a.await.is_ok());
    assert!(b.await.is_ok());
    *GATE.lock().unwrap() = None;
    assert!(
        renderer
            .render(RenderJob::of(&Gated).unwrap())
            .await
            .is_ok()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn views_run_on_the_render_pool() {
    let app = app();
    render(&app, &Where).await.unwrap();
    let (name, outside) = WHERE.lock().unwrap().clone().unwrap();
    assert!(name.starts_with("noctis-render-"), "{name}");
    assert!(outside, "no tokio runtime on a pool thread");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_runtimes_deny_unknown_dynamic_classes() {
    let app = app();
    let flx = Dynamic {
        classes: "flx".to_owned(),
    };
    let err = render(&app, &flx).await.unwrap_err();
    let RenderError::Failed(source) = &err else {
        panic!("{err}");
    };
    let invalid = source
        .downcast_ref::<noctis::image::InvalidClasses>()
        .expect("InvalidClasses");
    assert!(invalid.to_string().contains("flx"), "{invalid}");

    let brand = Dynamic {
        classes: "bg-brand".to_owned(),
    };
    let out = render(&app, &brand).await.unwrap();
    let (w, _, rgba) = decode_png(&out.bytes);
    assert_eq!(pixel(&rgba, w, 0, 0), BRAND);
}

/// The only test on `Runtime::global()` in this binary, so its pool config
/// is the default one.
#[tokio::test(flavor = "multi_thread")]
async fn the_global_runtime_warns_on_unknown_dynamic_classes() {
    let mut builder = App::builder();
    builder.theme(theme!()).module(cards());
    let app = builder.build().unwrap();
    let flx = Dynamic {
        classes: "flx".to_owned(),
    };
    assert!(render(&app, &flx).await.is_ok());
}
