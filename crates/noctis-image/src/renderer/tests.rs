//! The Renderer rows that need no App: register, render, encode, settings,
//! errors, panics, the queue and the class check.

use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use noctis_core::Runtime;
use noctis_ports::{AnimatedFormat, AnimationSpec, ErasedView, Size, Template};
use serde::{Deserialize, Serialize};

use super::*;
use crate::Font;
use crate::budget::Attempt;
use crate::fonts::FontsConfig;
use crate::pool::PoolConfig;
use crate::{Classes, Node, Style, TemplateView, div, erased_view};

const BRAND: [u8; 4] = [0x58, 0x65, 0xF2, 0xFF];

/// Defines a hand-written still Template `$name` of `$size` and `$format`
/// whose view is `$view` (a `fn(&Self) -> Node`).
macro_rules! template {
    ($name:ident { $($field:ident: $ty:ty),* }, $id:literal, $size:expr, $format:expr, $view:expr) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct $name { $($field: $ty),* }

        impl Template for $name {
            const ID: &'static str = $id;
            const VERSION: u32 = 1;
            const SIZE: Size = $size;
            const FORMAT: Format = $format;
            const ANIMATION: Option<AnimationSpec> = None;

            fn view() -> ErasedView {
                erased_view::<Self>()
            }
        }

        impl crate::TemplateAssets for $name {}


        impl TemplateView for $name {
            fn view(&self, _: Frame) -> Node {
                let view: fn(&Self) -> Node = $view;
                view(self)
            }
        }
    };
}

template!(
    Card { name: String },
    "card",
    Size::new(64, 32),
    Format::Png,
    |card| { div(Classes::__literal("flex w-full h-full bg-brand")).child(card.name.clone()) }
);
template!(Dot {}, "dot", Size::new(1, 1), Format::Png, |_| {
    div(Classes::__literal("w-full h-full bg-brand"))
});
template!(
    Huge {},
    "huge",
    Size::new(10_000, 10_000),
    Format::Png,
    |_| { div(Classes::default()) }
);
template!(StyleWins {}, "style", Size::new(8, 8), Format::Png, |_| {
    div(Classes::__literal("w-full h-full bg-red-500")).style(Style::new().bg_rgb(0x00_FF00))
});
template!(
    Boom { kind: u8 },
    "boom",
    Size::new(4, 4),
    Format::Png,
    |boom| {
        assert!(boom.kind != 0, "boom");
        std::panic::panic_any(5_u8)
    }
);
template!(
    Dynamic { classes: String },
    "dynamic",
    Size::new(4, 4),
    Format::Png,
    |d| { div(Classes::dynamic(d.classes.clone()).and(Classes::__literal("w-full h-full"))) }
);
template!(Where {}, "where", Size::new(2, 2), Format::Png, |_| {
    let name = std::thread::current().name().unwrap_or("").to_owned();
    let no_tokio = tokio::runtime::Handle::try_current().is_err();
    *WHERE.lock().unwrap() = Some((name, no_tokio));
    div(Classes::default())
});

template!(Edge {}, "edge", Size::new(4, 1), Format::Png, |_| {
    div(Classes::__literal("flex w-full h-full")).children([
        div(Classes::default()).style(Style::new().width_px(2).height_px(1)),
        div(Classes::default()).style(
            Style::new()
                .width_px(2)
                .height_px(1)
                .bg_rgb(0xFF_0000)
                .opacity(0.5),
        ),
    ])
});

/// Where the `Where` view last ran: (thread name, outside tokio).
static WHERE: Mutex<Option<(String, bool)>> = Mutex::new(None);

/// A Template whose view blocks until the test opens [`GATE`].
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Gated;

static GATE: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);

impl Template for Gated {
    const ID: &'static str = "gated";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(2, 2);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl crate::TemplateAssets for Gated {}

impl TemplateView for Gated {
    fn view(&self, _: Frame) -> Node {
        if let Some(gate) = GATE.lock().unwrap().as_ref() {
            gate.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        div(Classes::default())
    }
}

/// A derived-style animated Template.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Spinner;

impl Template for Spinner {
    const ID: &'static str = "spinner";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(2, 2);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = Some(AnimationSpec::new(
        Duration::from_secs(1),
        10,
        AnimatedFormat::Gif,
    ));

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl crate::TemplateAssets for Spinner {}

impl TemplateView for Spinner {
    fn view(&self, _: Frame) -> Node {
        div(Classes::default())
    }
}

/// A hand-written Template whose view was not made by the derive.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Foreign;

impl Template for Foreign {
    const ID: &'static str = "x";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(2, 2);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

/// The renderer tests' theme: a color, one animation and its keyframes.
const THEME: &str = "@theme { --color-brand: #5865f2; --animate-fade: fade 1s linear infinite; } \
     @keyframes fade { from { opacity: 0 } to { opacity: 1 } }";

static INTER: &[u8] = include_bytes!("../../fonts/InterVariable.woff2");

/// A font registry of `fonts`, on its own test Runtime.
fn test_fonts(fonts: Vec<Font>) -> Arc<FontRegistry> {
    Runtime::for_test()
        .resource_or_init::<FontRegistry>(FontsConfig {
            fonts,
            bundled: false,
        })
        .unwrap()
}

fn theme() -> Theme {
    Theme::parse(THEME).unwrap()
}

fn renderer_with(threads: usize, queue_capacity: usize, mode: ClassCheckMode) -> TakumiRenderer {
    renderer_budget(threads, queue_capacity, mode, true)
}

fn renderer_budget(
    threads: usize,
    queue_capacity: usize,
    mode: ClassCheckMode,
    still_fallback: bool,
) -> TakumiRenderer {
    let pool = Runtime::for_test()
        .resource_or_init::<RenderPool>(PoolConfig {
            threads,
            queue_capacity,
        })
        .unwrap();
    let theme = theme();
    let stylesheet = theme_stylesheet(&theme).unwrap();
    let renderer = TakumiRenderer::new(RendererConfig {
        pool,
        fonts: test_fonts(vec![Font::from_static(INTER)]),
        images: StaticImages::default(),
        theme,
        stylesheet,
        mode,
        size_target: crate::plugin::DEFAULT_SIZE_TARGET,
        still_fallback,
        caps: RenderCaps::new(),
        remote: AssetFetcher::new(&crate::RemoteConfig::new()).unwrap(),
    });
    renderer
        .register(&[
            TemplateDescriptor::of::<Card>(),
            TemplateDescriptor::of::<Dot>(),
            TemplateDescriptor::of::<Huge>(),
            TemplateDescriptor::of::<StyleWins>(),
            TemplateDescriptor::of::<Boom>(),
            TemplateDescriptor::of::<Dynamic>(),
            TemplateDescriptor::of::<Where>(),
            TemplateDescriptor::of::<Gated>(),
            TemplateDescriptor::of::<Noise>(),
            TemplateDescriptor::of::<Recorder>(),
            TemplateDescriptor::of::<Spinner>(),
            TemplateDescriptor::of::<Edge>(),
            TemplateDescriptor::of::<FirstProbe>(),
            TemplateDescriptor::of::<StopProbe>(),
            TemplateDescriptor::of::<DropProbe>(),
            TemplateDescriptor::of::<ShareProbe>(),
            TemplateDescriptor::of::<Fade>(),
            TemplateDescriptor::of::<FadeArbitrary>(),
            TemplateDescriptor::of::<Pulse>(),
            TemplateDescriptor::of::<RootSized>(),
            TemplateDescriptor::of::<NarrowStill>(),
        ])
        .unwrap();
    renderer
}

fn renderer() -> TakumiRenderer {
    renderer_with(2, 64, ClassCheckMode::Deny)
}

fn card(name: &str) -> RenderJob {
    RenderJob::of(&Card {
        name: name.to_owned(),
    })
    .unwrap()
}

/// The decoded RGBA pixels and size of a PNG.
fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    // takumi may write a palette PNG; expand it to 8-bit RGBA.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(
        reader.output_color_type(),
        (png::ColorType::Rgba, png::BitDepth::Eight),
        "{info:?}"
    );
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    rgba[at..at + 4].try_into().unwrap()
}

#[test]
fn register_accepts_derived_still_templates() {
    let r = renderer();
    assert!(r.register(&[]).is_ok());
    assert!(r.register(&[TemplateDescriptor::of::<Card>()]).is_ok());
    assert!(
        r.register(&[
            TemplateDescriptor::of::<Card>(),
            TemplateDescriptor::of::<Dot>()
        ])
        .is_ok()
    );
}

#[test]
fn register_rejects_foreign_views_and_accepts_animated_ones() {
    let r = renderer();
    let foreign = r
        .register(&[
            TemplateDescriptor::of::<Card>(),
            TemplateDescriptor::of::<Foreign>(),
        ])
        .unwrap_err();
    assert!(
        matches!(&foreign, RenderError::Unsupported { id, why } if id == "x" && why.contains("#[derive(Template)]")),
        "{foreign}"
    );
    assert!(r.register(&[TemplateDescriptor::of::<Spinner>()]).is_ok());
}

#[tokio::test]
async fn renders_a_png_with_the_theme_color() {
    let r = renderer();
    let out = r.render(card("Ada")).await.unwrap();
    assert_eq!(&out.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(out.format, noctis_ports::OutputFormat::Png);
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
    // Deterministic: the same job renders the same bytes.
    assert_eq!(r.render(card("Ada")).await.unwrap().bytes, out.bytes);
}

#[tokio::test]
async fn encodes_every_still_format() {
    let r = renderer();
    let cases = [
        (Format::Jpeg { quality: 1 }, "jpg", Some(1)),
        (Format::Jpeg { quality: 100 }, "jpg", Some(100)),
        (Format::Webp { quality: 1 }, "webp", Some(1)),
        (Format::Webp { quality: 100 }, "webp", Some(100)),
        (Format::WebpLossless, "webp", None),
    ];
    for (format, ext, quality) in cases {
        let job = RenderJob {
            format,
            ..card("Ada")
        };
        let out = r.render(job).await.unwrap();
        if ext == "jpg" {
            assert_eq!(&out.bytes[..2], b"\xFF\xD8", "{format:?}");
        } else {
            assert_eq!(&out.bytes[..4], b"RIFF", "{format:?}");
            assert_eq!(&out.bytes[8..12], b"WEBP", "{format:?}");
        }
        assert_eq!(out.format.extension(), ext, "{format:?}");
        assert_eq!(out.settings_used.quality, quality, "{format:?}");
        assert_eq!(out.digest, *blake3::hash(&out.bytes).as_bytes());
    }
}

#[tokio::test]
async fn smallest_size_and_style_over_classes() {
    let r = renderer();
    let dot = r.render(RenderJob::of(&Dot {}).unwrap()).await.unwrap();
    let (w, h, rgba) = decode_png(&dot.bytes);
    assert_eq!((w, h), (1, 1));
    assert_eq!(pixel(&rgba, w, 0, 0), BRAND);

    let styled = r
        .render(RenderJob::of(&StyleWins {}).unwrap())
        .await
        .unwrap();
    let (w, _, rgba) = decode_png(&styled.bytes);
    assert_eq!(pixel(&rgba, w, 0, 0), [0, 0xFF, 0, 0xFF]);
}

#[tokio::test]
async fn an_oversized_canvas_is_over_cap_and_the_pool_survives() {
    let r = renderer();
    let before = r.pool.admitted();
    let future = r.render(RenderJob::of(&Huge {}).unwrap());
    assert_eq!(r.pool.admitted(), before, "rejected before admission");
    let err = future.await.unwrap_err();
    assert!(
        matches!(
            &err,
            RenderError::OverCap {
                id,
                cap: noctis_ports::RenderCap::JobPixels,
                limit: 4_000_000,
                actual: 100_000_000,
            } if id == "huge"
        ),
        "{err}"
    );
    assert!(r.render(card("Ada")).await.is_ok());
}

#[tokio::test]
async fn unicode_text_renders() {
    assert!(renderer().render(card("Ada 😀 ğüş")).await.is_ok());
}

#[tokio::test]
async fn unknown_ids_and_versions_fail() {
    let r = renderer();
    let unknown = RenderJob {
        template_id: "nope",
        ..card("Ada")
    };
    assert!(matches!(
        r.render(unknown).await,
        Err(RenderError::UnknownTemplate { id, version: 1 }) if id == "nope"
    ));
    let skew = RenderJob {
        template_version: 2,
        ..card("Ada")
    };
    assert!(matches!(
        r.render(skew).await,
        Err(RenderError::UnknownTemplate { id, version: 2 }) if id == "card"
    ));
}

#[tokio::test]
async fn bad_data_fails_with_the_value_error() {
    let r = renderer();
    let empty = RenderJob {
        data: Bytes::new(),
        ..card("Ada")
    };
    let trailing = {
        let job = card("Ada");
        let mut data = job.data.to_vec();
        data.push(0);
        RenderJob {
            data: Bytes::from(data),
            ..job
        }
    };
    for (job, expected) in [
        (empty, noctis_ports::ValueError::Malformed),
        (trailing, noctis_ports::ValueError::Trailing { extra: 1 }),
    ] {
        let err = r.render(job).await.unwrap_err();
        let RenderError::Failed(source) = &err else {
            panic!("{err}");
        };
        assert_eq!(
            source.downcast_ref::<noctis_ports::ValueError>(),
            Some(&expected)
        );
    }
}

#[tokio::test]
async fn a_panicking_view_is_panicked_and_frees_its_slot() {
    let r = renderer_with(1, 1, ClassCheckMode::Deny);
    let boom = r.render(RenderJob::of(&Boom { kind: 0 }).unwrap()).await;
    assert!(matches!(boom, Err(RenderError::Panicked { message }) if message == "boom"));
    let other = r.render(RenderJob::of(&Boom { kind: 1 }).unwrap()).await;
    assert!(
        matches!(other, Err(RenderError::Panicked { message }) if message == "<non-string panic>")
    );
    assert!(r.render(card("Ada")).await.is_ok());
}

#[tokio::test]
async fn a_full_queue_fails_at_once() {
    let (open, gate) = mpsc::channel();
    *GATE.lock().unwrap() = Some(gate);
    let r = renderer_with(1, 2, ClassCheckMode::Deny);
    let gated = || RenderJob::of(&Gated).unwrap();
    // Admission happens at the call, before the futures are polled.
    let a = r.render(gated());
    let b = r.render(gated());
    assert!(matches!(
        r.render(gated()).await,
        Err(RenderError::QueueFull)
    ));
    open.send(()).unwrap();
    open.send(()).unwrap();
    assert!(a.await.is_ok());
    assert!(b.await.is_ok());
    *GATE.lock().unwrap() = None;
    assert!(r.render(gated()).await.is_ok());
}

#[tokio::test]
async fn views_run_on_pool_threads_without_tokio() {
    let r = renderer();
    r.render(RenderJob::of(&Where {}).unwrap()).await.unwrap();
    let (name, no_tokio) = WHERE.lock().unwrap().clone().unwrap();
    assert!(name.starts_with("noctis-render-"), "{name}");
    assert!(no_tokio);
}

#[tokio::test]
async fn deny_fails_unknown_dynamic_classes_and_warn_renders() {
    let flx = || {
        RenderJob::of(&Dynamic {
            classes: "flx".to_owned(),
        })
        .unwrap()
    };
    let err = renderer().render(flx()).await.unwrap_err();
    let RenderError::Failed(source) = &err else {
        panic!("{err}");
    };
    let invalid = source
        .downcast_ref::<crate::tw::InvalidClasses>()
        .expect("InvalidClasses");
    assert!(invalid.to_string().contains("flx"), "{invalid}");

    let warn = renderer_with(1, 4, ClassCheckMode::Warn);
    assert!(warn.render(flx()).await.is_ok());

    let brand = RenderJob::of(&Dynamic {
        classes: "bg-brand".to_owned(),
    })
    .unwrap();
    let out = renderer().render(brand).await.unwrap();
    let (w, _, rgba) = decode_png(&out.bytes);
    assert_eq!(pixel(&rgba, w, 0, 0), BRAND);
}

#[test]
fn the_theme_stylesheet_declares_every_token() {
    assert!(theme_stylesheet(&Theme::empty()).is_ok());
    let theme = Theme::parse(
        "@theme { --color-brand: #5865f2; --color-surface: #11131a; \
         --font-display: \"Inter\", sans-serif; --radius-card: 12px; }",
    )
    .unwrap();
    let props: Vec<(String, &str)> = theme.custom_properties().collect();
    assert_eq!(props.len(), 4, "{props:?}");
    let css = theme_css(&theme);
    for (name, value) in props {
        let declaration = format!("{name}: {value};");
        assert!(
            css.contains(&declaration),
            "{declaration} missing from {css}"
        );
    }
    assert!(theme_stylesheet(&theme).is_ok());
}

/// A 64×64 Template of 1 px cells in pseudo-random colors: incompressible
/// output. The colors also depend on the frame index, so every frame of an
/// animated job differs.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Noise {
    seed: u32,
}

impl Template for Noise {
    const ID: &'static str = "noise";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(64, 64);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl crate::TemplateAssets for Noise {}

impl TemplateView for Noise {
    fn view(&self, frame: Frame) -> Node {
        // xorshift32 with a fixed, never-zero seed.
        let mut state = (self.seed ^ frame.index.wrapping_mul(0x9E37_79B9)) | 1;
        let cells = (0..64 * 64).map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            div(Classes::default()).style(
                Style::new()
                    .width_px(1)
                    .height_px(1)
                    .bg_rgb(state & 0x00FF_FFFF),
            )
        });
        div(Classes::__literal("flex flex-wrap w-full h-full")).children(cells)
    }
}

/// Every frame the `Recorder` view saw, with its thread name.
static FRAMES: Mutex<Vec<(Frame, String)>> = Mutex::new(Vec::new());

/// A Template recording each [`Frame`] it is drawn with.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Recorder;

impl Template for Recorder {
    const ID: &'static str = "recorder";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(2, 2);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl crate::TemplateAssets for Recorder {}

impl TemplateView for Recorder {
    fn view(&self, frame: Frame) -> Node {
        let name = std::thread::current().name().unwrap_or("").to_owned();
        FRAMES.lock().unwrap().push((frame, name));
        div(Classes::__literal("w-full h-full bg-brand"))
    }
}

const WEBP80: AnimatedFormat = AnimatedFormat::Webp { quality: 80 };

fn noise(format: Format) -> RenderJob {
    RenderJob {
        format,
        ..RenderJob::of(&Noise { seed: 0x2545_F491 }).unwrap()
    }
}

/// Noise animated at `fps` for `ms` milliseconds as `format`.
fn anim(fps: u16, ms: u64, format: AnimatedFormat) -> RenderJob {
    RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_millis(ms), fps, format)),
        ..noise(Format::Png)
    }
}

fn with_budget(job: RenderJob, upload_budget: u64) -> RenderJob {
    RenderJob {
        upload_budget,
        ..job
    }
}

/// No resolved remote image.
fn no_remote(r: &TakumiRenderer) -> Resolved {
    Resolved::empty(r.shared.remote.fallback().clone())
}

/// The length the renderer's per-attempt encoder gives `attempt` of `job`.
fn encoded_len(r: &TakumiRenderer, job: &RenderJob, attempt: &Attempt) -> u64 {
    let view = r.view_of(job).unwrap();
    let requester = present();
    let bytes = r
        .shared
        .encoder(view, job, &no_remote(r), &requester)
        .encode(attempt)
        .unwrap();
    bytes.len() as u64
}

fn attempt(fps: Option<u16>, side: u32, quality: Option<u8>, still: bool) -> Attempt {
    Attempt {
        fps,
        size: Size::new(side, side),
        quality,
        still,
    }
}

/// Whether `bytes` hold a RIFF chunk named `fourcc`.
fn has_chunk(bytes: &[u8], fourcc: [u8; 4]) -> bool {
    bytes.windows(4).any(|w| w == fourcc)
}

#[tokio::test]
async fn a_still_over_budget_steps_down_its_size() {
    let r = renderer();
    let full = r.render(noise(Format::Png)).await.unwrap();
    assert_eq!(full.settings_used.size, Size::new(64, 64));
    let budget = full.bytes.len() as u64 - 1;
    let out = r
        .render(with_budget(noise(Format::Png), budget))
        .await
        .unwrap();
    assert!(out.bytes.len() as u64 <= budget);
    let size = out.settings_used.size;
    assert!(
        [Size::new(48, 48), Size::new(32, 32)].contains(&size),
        "{size:?}"
    );
    let (w, h, _) = decode_png(&out.bytes);
    assert_eq!((w, h), (size.width(), size.height()));
    assert_eq!(out.settings_used.frames, 1);
    assert_eq!(out.digest, *blake3::hash(&out.bytes).as_bytes());
}

#[tokio::test]
async fn a_lossy_still_steps_down_its_quality_after_its_size() {
    let r = renderer();
    let job = noise(Format::Jpeg { quality: 90 });
    let smallest_q90 = encoded_len(&r, &job, &attempt(None, 32, Some(90), false));
    let out = r.render(with_budget(job, smallest_q90 - 1)).await.unwrap();
    assert!((out.bytes.len() as u64) < smallest_q90);
    assert_eq!(out.settings_used.size, Size::new(32, 32));
    let quality = out.settings_used.quality.unwrap();
    assert!([75, 50, 25].contains(&quality), "{quality}");
    assert_eq!(&out.bytes[..2], b"\xFF\xD8");
}

#[tokio::test]
async fn nothing_fitting_is_over_budget() {
    let r = renderer();
    let err = r
        .render(with_budget(noise(Format::Png), 10))
        .await
        .unwrap_err();
    assert!(
        matches!(err, RenderError::OverBudget { budget: 10, smallest } if smallest > 10),
        "{err}"
    );
    // The smaller of the upload budget and the 2 MiB target wins.
    let err = r
        .render(with_budget(noise(Format::Png), 1024))
        .await
        .unwrap_err();
    assert!(
        matches!(err, RenderError::OverBudget { budget: 1024, smallest } if smallest > 1024),
        "{err}"
    );
}

#[tokio::test]
async fn renders_an_animated_webp() {
    let r = renderer();
    let out = r.render(anim(10, 500, WEBP80)).await.unwrap();
    assert_eq!(&out.bytes[..4], b"RIFF");
    assert_eq!(&out.bytes[8..12], b"WEBP");
    assert!(has_chunk(&out.bytes, *b"ANIM"));
    assert_eq!(out.format, noctis_ports::OutputFormat::Webp);
    assert_eq!(out.format.extension(), "webp");
    assert_eq!(
        out.settings_used,
        EncodeSettings {
            size: Size::new(64, 64),
            quality: Some(80),
            fps: Some(10),
            frames: 5,
        }
    );
    assert_eq!(out.digest, *blake3::hash(&out.bytes).as_bytes());
}

#[tokio::test]
async fn the_view_is_called_once_per_frame_on_the_pool() {
    let r = renderer();
    FRAMES.lock().unwrap().clear();
    let job = RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_millis(500), 10, WEBP80)),
        ..RenderJob::of(&Recorder).unwrap()
    };
    r.render(job).await.unwrap();
    let frames = std::mem::take(&mut *FRAMES.lock().unwrap());
    let seen: Vec<Frame> = frames.iter().map(|(f, _)| *f).collect();
    let expected: Vec<Frame> = (0..5).map(|index| Frame { index, count: 5 }).collect();
    assert_eq!(seen, expected);
    assert!(
        frames.iter().all(|(_, n)| n.starts_with("noctis-render-")),
        "{frames:?}"
    );
}

#[tokio::test]
async fn an_animation_needs_frames() {
    let r = renderer();
    for (fps, duration) in [
        (0, Duration::from_secs(1)),
        (10, Duration::ZERO),
        (10, Duration::from_micros(500)),
    ] {
        let job = RenderJob {
            animation: Some(AnimationSpec::new(duration, fps, WEBP80)),
            ..noise(Format::Png)
        };
        let err = r.render(job).await.unwrap_err();
        assert!(
            matches!(&err, RenderError::Unsupported { why, .. } if why.contains("fps >= 1")),
            "{err}"
        );
    }
}

#[cfg(not(feature = "gif"))]
#[tokio::test]
async fn gif_and_apng_need_the_feature() {
    let r = renderer();
    for format in [AnimatedFormat::Gif, AnimatedFormat::Apng] {
        let before = r.pool.admitted();
        let future = r.render(anim(10, 500, format));
        assert_eq!(r.pool.admitted(), before, "rejected before admission");
        let err = future.await.unwrap_err();
        assert!(
            matches!(&err, RenderError::Unsupported { why, .. } if why.contains("image-gif")),
            "{err}"
        );
    }
}

#[cfg(feature = "gif")]
#[tokio::test]
async fn renders_gif_and_apng() {
    let r = renderer();
    let gif = r.render(anim(10, 500, AnimatedFormat::Gif)).await.unwrap();
    assert_eq!(&gif.bytes[..6], b"GIF89a");
    assert_eq!(gif.format, noctis_ports::OutputFormat::Gif);
    assert_eq!(gif.format.extension(), "gif");
    assert_eq!(gif.settings_used.quality, None);
    assert_eq!(gif.settings_used.frames, 5);

    let apng = r.render(anim(10, 500, AnimatedFormat::Apng)).await.unwrap();
    assert_eq!(&apng.bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(has_chunk(&apng.bytes, *b"acTL"));
    assert_eq!(apng.format, noctis_ports::OutputFormat::Apng);
    assert_eq!(apng.format.extension(), "png");
    assert_eq!(apng.settings_used.quality, None);
}

#[tokio::test]
async fn an_animation_over_budget_steps_down_its_fps() {
    let r = renderer();
    let full = r.render(anim(20, 1000, WEBP80)).await.unwrap();
    assert_eq!(full.settings_used.fps, Some(20));
    assert_eq!(full.settings_used.frames, 20);
    let budget = full.bytes.len() as u64 - 1;
    let out = r
        .render(with_budget(anim(20, 1000, WEBP80), budget))
        .await
        .unwrap();
    assert!(out.bytes.len() as u64 <= budget);
    let fps = out.settings_used.fps.unwrap();
    assert!(fps <= 10, "{fps}");
    let frames = u32::try_from(budget::spans(Duration::from_secs(1), fps).len()).unwrap();
    assert_eq!(out.settings_used.frames, frames);
}

/// The animation of the still-fallback rows: 90 frames, the default cap.
/// It runs 5 s rather than 1 s:
/// at 1 s its smallest animated attempt (5 lossy frames of 32×32) is smaller
/// than the lossless 32×32 PNG still, so no budget lies between them.
fn long_anim() -> RenderJob {
    anim(18, 5000, WEBP80)
}

/// The budget between the still fallback and the smallest animated attempt.
fn still_only_budget(r: &TakumiRenderer, job: &RenderJob) -> u64 {
    let attempts = budget::ladder(job, true);
    let (still, animated) = attempts.split_last().unwrap();
    assert!(still.still);
    let smallest = animated
        .iter()
        .map(|a| encoded_len(r, job, a))
        .min()
        .unwrap();
    // Draw at the fps the fallback follows (the last animated attempt's).
    let view = r.view_of(job).unwrap();
    let requester = present();
    let remote = no_remote(r);
    let mut encoder = r.shared.encoder(view, job, &remote, &requester);
    encoder.encode(animated.last().unwrap()).unwrap();
    let still_len = encoder.encode(still).unwrap().len() as u64;
    assert!(
        still_len < smallest,
        "still {still_len} vs animated {smallest}"
    );
    still_len
}

#[tokio::test]
async fn an_animation_falls_back_to_a_still() {
    let r = renderer();
    let job = long_anim();
    let budget = still_only_budget(&r, &job);
    let out = r.render(with_budget(job, budget)).await.unwrap();
    assert_eq!(out.settings_used.frames, 1);
    assert_eq!(out.settings_used.fps, None);
    assert_eq!(out.format, noctis_ports::OutputFormat::Png);
    assert_eq!(out.format.extension(), "png");
    let (w, h, _) = decode_png(&out.bytes);
    assert_eq!(Size::new(w, h), out.settings_used.size);
}

#[tokio::test]
async fn without_the_fallback_it_is_over_budget() {
    let r = renderer_budget(2, 64, ClassCheckMode::Deny, false);
    let job = long_anim();
    let budget = still_only_budget(&renderer(), &job);
    let err = r.render(with_budget(job, budget)).await.unwrap_err();
    assert!(
        matches!(err, RenderError::OverBudget { budget: b, .. } if b == budget),
        "{err}"
    );
}

#[test]
fn a_size_step_keeps_translucent_edges_their_colour() {
    let r = renderer();
    let job = RenderJob::of(&Edge {}).unwrap();
    let view = r.view_of(&job).unwrap();
    let full = r
        .shared
        .frame(view, &job, &no_remote(&r), Frame::STILL, 0)
        .unwrap();
    let small = resized(&full, Size::new(2, 1)).unwrap();
    let edges: Vec<&[u8]> = small
        .as_raw()
        .chunks_exact(4)
        .filter(|px| px[3] > 0 && px[3] < 255)
        .collect();
    assert!(!edges.is_empty(), "{:?}", small.as_raw());
    for px in edges {
        assert!(px[0] >= 250 && px[1] <= 5 && px[2] <= 5, "{px:?}");
    }
}

/// The canvas size of a VP8X (extended) WebP.
fn webp_canvas(bytes: &[u8]) -> Size {
    assert_eq!(&bytes[12..16], b"VP8X");
    let side = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], 0]) + 1;
    Size::new(side(24), side(27))
}

#[tokio::test]
async fn an_animation_over_budget_steps_down_its_size() {
    let r = renderer();
    let job = anim(5, 1000, WEBP80);
    let full_len = encoded_len(&r, &job, &attempt(Some(5), 64, Some(80), false));
    let out = r.render(with_budget(job, full_len - 1)).await.unwrap();
    assert!((out.bytes.len() as u64) < full_len);
    let size = out.settings_used.size;
    assert_ne!(size, Size::new(64, 64));
    assert_eq!(webp_canvas(&out.bytes), size);
    assert_eq!(out.settings_used.frames, 5);
}

#[tokio::test]
async fn an_animation_over_budget_steps_down_its_quality_after_its_size() {
    let r = renderer();
    let job = anim(5, 1000, WEBP80);
    let smallest_q80 = encoded_len(&r, &job, &attempt(Some(5), 32, Some(80), false));
    let out = r.render(with_budget(job, smallest_q80 - 1)).await.unwrap();
    assert!((out.bytes.len() as u64) < smallest_q80);
    assert_eq!(out.settings_used.size, Size::new(32, 32));
    assert!(
        out.settings_used.quality < Some(80),
        "{:?}",
        out.settings_used
    );
    assert_eq!(webp_canvas(&out.bytes), Size::new(32, 32));
}

#[tokio::test]
async fn a_lossy_still_fallback_takes_the_still_quality() {
    let r = renderer();
    let job = RenderJob {
        format: Format::Jpeg { quality: 90 },
        ..long_anim()
    };
    let budget = still_only_budget(&r, &job);
    let out = r.render(with_budget(job, budget)).await.unwrap();
    assert_eq!(out.settings_used.quality, Some(25));
    assert_eq!(out.settings_used.frames, 1);
    assert_eq!(&out.bytes[..2], b"\xFF\xD8");
}

#[tokio::test]
async fn a_huge_duration_is_over_cap_before_admission() {
    let r = renderer();
    let job = RenderJob {
        animation: Some(AnimationSpec::new(Duration::MAX, 90, WEBP80)),
        ..noise(Format::Png)
    };
    let before = r.pool.admitted();
    let future = r.render(job);
    assert_eq!(r.pool.admitted(), before, "rejected before admission");
    let err = future.await.unwrap_err();
    assert!(
        matches!(
            &err,
            RenderError::OverCap {
                cap: noctis_ports::RenderCap::Duration,
                limit: 6000,
                ..
            }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn the_bounds_are_checked_before_the_caps() {
    let r = renderer();
    let seven = Duration::from_secs(7);
    let job = RenderJob {
        animation: Some(AnimationSpec::new(seven, 0, WEBP80)),
        ..noise(Format::Png)
    };
    let err = r.render(job).await.unwrap_err();
    assert!(
        matches!(&err, RenderError::Unsupported { why, .. } if why.contains("fps >= 1")),
        "{err}"
    );
    #[cfg(not(feature = "gif"))]
    {
        let job = RenderJob {
            animation: Some(AnimationSpec::new(seven, 10, AnimatedFormat::Gif)),
            ..noise(Format::Png)
        };
        let err = r.render(job).await.unwrap_err();
        assert!(
            matches!(&err, RenderError::Unsupported { why, .. } if why.contains("image-gif")),
            "{err}"
        );
    }
    let err = r.render(anim(10, 7000, WEBP80)).await.unwrap_err();
    assert!(
        matches!(
            &err,
            RenderError::OverCap {
                cap: noctis_ports::RenderCap::Duration,
                limit: 6000,
                actual: 7000,
                ..
            }
        ),
        "{err}"
    );
}

/// A requester that never leaves, for driving an [`Encoder`] directly.
fn present() -> Requester<'static> {
    fn never() -> bool {
        false
    }
    Requester::new(&never)
}

/// Defines a 2×2 probe Template `$name`: its view logs `(tag, frame index)`
/// to `$log`, and on frame 0 of tag 0 it signals `$started` and blocks on
/// `$gate` (when set).
macro_rules! probe {
    ($name:ident, $id:literal, $log:ident, $started:ident, $gate:ident) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct $name {
            tag: u8,
        }

        static $log: Mutex<Vec<(u8, u32)>> = Mutex::new(Vec::new());
        static $started: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
        static $gate: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);

        impl Template for $name {
            const ID: &'static str = $id;
            const VERSION: u32 = 1;
            const SIZE: Size = Size::new(2, 2);
            const FORMAT: Format = Format::Png;
            const ANIMATION: Option<AnimationSpec> = None;

            fn view() -> ErasedView {
                erased_view::<Self>()
            }
        }

        impl crate::TemplateAssets for $name {}

        impl TemplateView for $name {
            fn view(&self, frame: Frame) -> Node {
                $log.lock().unwrap().push((self.tag, frame.index));
                if self.tag == 0 && frame.index == 0 {
                    if let Some(started) = $started.lock().unwrap().as_ref() {
                        started.send(()).unwrap();
                    }
                    if let Some(gate) = $gate.lock().unwrap().as_ref() {
                        gate.recv_timeout(Duration::from_secs(10)).unwrap();
                    }
                }
                div(Classes::default())
            }
        }
    };
}

probe!(
    FirstProbe,
    "first-probe",
    FIRST_LOG,
    FIRST_STARTED,
    FIRST_GATE
);
probe!(StopProbe, "stop-probe", STOP_LOG, STOP_STARTED, STOP_GATE);
probe!(DropProbe, "drop-probe", DROP_LOG, DROP_STARTED, DROP_GATE);
probe!(
    ShareProbe,
    "share-probe",
    SHARE_LOG,
    SHARE_STARTED,
    SHARE_GATE
);

/// Arms a probe: returns (open the gate, wait for frame 0 of tag 0).
fn arm(
    started: &Mutex<Option<mpsc::Sender<()>>>,
    gate: &Mutex<Option<mpsc::Receiver<()>>>,
) -> (mpsc::Sender<()>, mpsc::Receiver<()>) {
    let (open, gate_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    *gate.lock().unwrap() = Some(gate_rx);
    *started.lock().unwrap() = Some(started_tx);
    (open, started_rx)
}

/// Waits (up to 5 s) until `r`'s pool has no admitted units.
async fn drained(r: &TakumiRenderer) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while r.pool.admitted() != 0 {
        assert!(std::time::Instant::now() < deadline, "{:?}", r.pool);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// `job` animated for 100 ms at 10 fps: one frame.
fn one_frame(job: RenderJob) -> RenderJob {
    RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_millis(100), 10, WEBP80)),
        ..job
    }
}

#[tokio::test]
async fn a_still_renders_before_animations_queued_earlier() {
    let r = renderer_with(1, 16, ClassCheckMode::Deny);
    let (open, started) = arm(&FIRST_STARTED, &FIRST_GATE);
    let job = |tag| RenderJob::of(&FirstProbe { tag }).unwrap();
    let first = r.render(one_frame(job(0)));
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    let animations = [1, 2, 3].map(|tag| r.render(one_frame(job(tag))));
    let still = r.render(job(9));
    open.send(()).unwrap();
    first.await.unwrap();
    still.await.unwrap();
    for animation in animations {
        animation.await.unwrap();
    }
    let tags: Vec<u8> = FIRST_LOG.lock().unwrap().iter().map(|(t, _)| *t).collect();
    assert_eq!(tags, [0, 9, 1, 2, 3]);
}

#[tokio::test]
async fn a_dropped_future_stops_an_animation_between_frames() {
    let r = renderer_with(1, 16, ClassCheckMode::Deny);
    let (open, started) = arm(&STOP_STARTED, &STOP_GATE);
    let job = RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_secs(1), 10, WEBP80)),
        ..RenderJob::of(&StopProbe { tag: 0 }).unwrap()
    };
    let future = r.render(job);
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(future);
    open.send(()).unwrap();
    drained(&r).await;
    assert_eq!(*STOP_LOG.lock().unwrap(), [(0, 0)]);
}

#[tokio::test]
async fn a_future_dropped_before_its_job_runs_is_skipped() {
    let r = renderer_with(1, 16, ClassCheckMode::Deny);
    let (open, started) = arm(&DROP_STARTED, &DROP_GATE);
    let blocker = r.render(RenderJob::of(&DropProbe { tag: 0 }).unwrap());
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    drop(r.render(RenderJob::of(&DropProbe { tag: 1 }).unwrap()));
    open.send(()).unwrap();
    blocker.await.unwrap();
    drained(&r).await;
    assert_eq!(*DROP_LOG.lock().unwrap(), [(0, 0)]);
}

#[tokio::test]
async fn animations_fill_their_share_and_a_still_is_still_admitted() {
    let r = renderer_with(1, 16, ClassCheckMode::Deny);
    let (open, started) = arm(&SHARE_STARTED, &SHARE_GATE);
    let job = |tag| RenderJob::of(&ShareProbe { tag }).unwrap();
    // 6000 ms at 15 fps: 90 frames, weighing ceil(90 / 8) = 12 units.
    let ninety = |tag| RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_secs(6), 15, WEBP80)),
        ..job(tag)
    };
    let first = r.render(ninety(0));
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(r.pool.admitted(), 12);
    assert!(matches!(
        r.render(ninety(1)).await,
        Err(RenderError::QueueFull)
    ));
    let still = r.render(job(2));
    assert_eq!(r.pool.admitted(), 13);
    open.send(()).unwrap();
    first.await.unwrap();
    still.await.unwrap();
    drained(&r).await;
}

#[test]
fn the_theme_stylesheet_declares_the_animations_and_keyframes() {
    let css = theme_css(&theme());
    let (inside, after) = css.split_once('}').unwrap();
    assert!(
        inside.contains("  --animate-fade: fade 1s linear infinite;\n"),
        "{css}"
    );
    assert!(
        after.contains("@keyframes fade {from { opacity: 0 } to { opacity: 1 }}"),
        "{css}"
    );
    assert!(theme_stylesheet(&theme()).is_ok());
}

/// Every frame the `Fade` view saw.
static FADE_FRAMES: Mutex<Vec<Frame>> = Mutex::new(Vec::new());

/// A 4×4 still Template that fades in through the theme animation; it
/// records each frame and draws the same tree for every one.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Fade;

impl Template for Fade {
    const ID: &'static str = "fade";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(4, 4);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl crate::TemplateAssets for Fade {}

impl TemplateView for Fade {
    fn view(&self, frame: Frame) -> Node {
        FADE_FRAMES.lock().unwrap().push(frame);
        div(Classes::__literal("animate-fade w-full h-full bg-brand"))
    }
}

template!(
    FadeArbitrary {},
    "fade-arbitrary",
    Size::new(4, 4),
    Format::Png,
    |_| {
        div(Classes::__literal(
            "animate-[fade_1s_linear_infinite] w-full h-full bg-brand",
        ))
    }
);
template!(Pulse {}, "pulse", Size::new(4, 4), Format::Png, |_| {
    div(Classes::__literal("animate-pulse w-full h-full bg-brand"))
});
template!(
    NarrowStill {},
    "narrow-still",
    Size::new(8, 8),
    Format::Png,
    |_| { div(Classes::__literal("bg-brand")).style(Style::new().width_px(4)) }
);

/// The alpha at (0, 0) of `job`'s frame at `time_ms`.
fn alpha_at(r: &TakumiRenderer, job: &RenderJob, time_ms: u64) -> u8 {
    let view = r.view_of(job).unwrap();
    let bitmap = r
        .shared
        .frame(view, job, &no_remote(r), Frame::STILL, time_ms)
        .unwrap();
    bitmap.as_raw()[3]
}

#[track_caller]
fn assert_fades_in(r: &TakumiRenderer, job: &RenderJob) {
    let (start, middle, end) = (
        alpha_at(r, job, 0),
        alpha_at(r, job, 500),
        alpha_at(r, job, 999),
    );
    assert!(start <= 10, "{start}");
    assert!((118..=138).contains(&middle), "{middle}");
    assert!(end >= 245, "{end}");
}

#[test]
fn a_theme_animation_plays_over_time() {
    let r = renderer();
    assert_fades_in(&r, &RenderJob::of(&Fade).unwrap());
}

#[test]
fn arbitrary_and_builtin_animations_play_over_time() {
    let r = renderer();
    assert_fades_in(&r, &RenderJob::of(&FadeArbitrary {}).unwrap());
    let pulse = RenderJob::of(&Pulse {}).unwrap();
    assert_eq!(alpha_at(&r, &pulse, 0), 255);
    let half = alpha_at(&r, &pulse, 1000);
    assert!((118..=138).contains(&half), "{half}");
}

#[tokio::test]
async fn a_css_animation_renders_one_frame_per_span() {
    let r = renderer();
    FADE_FRAMES.lock().unwrap().clear();
    let job = RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_secs(1), 10, WEBP80)),
        ..RenderJob::of(&Fade).unwrap()
    };
    let out = r.render(job).await.unwrap();
    assert!(has_chunk(&out.bytes, *b"ANIM"));
    assert_eq!(out.settings_used.frames, 10);
    let seen = std::mem::take(&mut *FADE_FRAMES.lock().unwrap());
    let expected: Vec<Frame> = (0..10).map(|index| Frame { index, count: 10 }).collect();
    assert_eq!(seen, expected);
}

#[tokio::test]
async fn a_css_animation_as_a_still_draws_its_first_frame() {
    let out = renderer()
        .render(RenderJob::of(&FadeArbitrary {}).unwrap())
        .await
        .unwrap();
    let (w, _, rgba) = decode_png(&out.bytes);
    assert!(pixel(&rgba, w, 0, 0)[3] <= 10);
}

#[tokio::test]
async fn the_fps_cap_comes_after_the_bounds_and_before_the_caps() {
    let r = renderer();
    let before = r.pool.admitted();
    let err = r.render(anim(0, 1000, WEBP80)).await.unwrap_err();
    assert!(matches!(err, RenderError::Unsupported { .. }), "{err}");
    #[cfg(not(feature = "gif"))]
    {
        let err = r
            .render(anim(60, 1000, AnimatedFormat::Gif))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, RenderError::Unsupported { why, .. } if why.contains("image-gif")),
            "{err}"
        );
    }
    #[cfg(feature = "gif")]
    {
        let err = r
            .render(anim(60, 1000, AnimatedFormat::Gif))
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                RenderError::OverCap {
                    cap: noctis_ports::RenderCap::Fps,
                    limit: 50,
                    actual: 60,
                    ..
                }
            ),
            "{err}"
        );
    }
    let err = r.render(anim(120, 7000, WEBP80)).await.unwrap_err();
    assert!(
        matches!(
            err,
            RenderError::OverCap {
                cap: noctis_ports::RenderCap::Fps,
                limit: 90,
                actual: 120,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(r.pool.admitted(), before);
}

/// An 8×8 Template whose root declares the size `case` names; case 0 sets
/// a 4 px width on frame 2 only.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RootSized {
    case: u8,
}

impl Template for RootSized {
    const ID: &'static str = "sized";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(8, 8);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl crate::TemplateAssets for RootSized {}

impl TemplateView for RootSized {
    fn view(&self, frame: Frame) -> Node {
        let classes = |c: &'static str| div(Classes::__literal(c));
        match self.case {
            0 if frame.index == 2 => classes("").style(Style::new().width_px(4)),
            0 => classes(""),
            1 => classes("w-[9px]"),
            2 => classes("size-[8px]"),
            3 => classes("sm:w-[4px]"),
            4 => classes("!h-[3px]"),
            5 => classes("w-full h-full"),
            6 => classes("w-64"),
            7 => classes("").style(Style::new().width_px(8).height_px(8)),
            _ => classes("").style(Style::new().width_pct(50.0)),
        }
    }
}

#[tokio::test]
async fn an_animation_frame_with_another_root_size_is_a_mismatch() {
    let r = renderer();
    let job = |case| RenderJob {
        animation: Some(AnimationSpec::new(Duration::from_millis(500), 10, WEBP80)),
        ..RenderJob::of(&RootSized { case }).unwrap()
    };
    for (case, frame, declared) in [(0, 2, (4, 8)), (1, 0, (9, 8)), (4, 0, (8, 3))] {
        let err = r.render(job(case)).await.unwrap_err();
        assert!(
            matches!(
                &err,
                RenderError::SizeMismatch { id, frame: f, expected, declared: d }
                    if id == "sized" && *f == frame && *expected == Size::new(8, 8) && *d == declared
            ),
            "case {case}: {err}"
        );
    }
    for case in [2, 3, 5, 6, 7, 8] {
        assert!(r.render(job(case)).await.is_ok(), "case {case}");
    }
}

#[tokio::test]
async fn a_still_is_not_size_checked() {
    let out = renderer()
        .render(RenderJob::of(&NarrowStill {}).unwrap())
        .await
        .unwrap();
    assert_eq!(out.settings_used.size, Size::new(8, 8));
}

#[test]
fn img_01_register_without_font_fails() {
    let pool = Runtime::for_test()
        .resource_or_init::<RenderPool>(PoolConfig {
            threads: 1,
            queue_capacity: 4,
        })
        .unwrap();
    let theme = theme();
    let stylesheet = theme_stylesheet(&theme).unwrap();
    let renderer = TakumiRenderer::new(RendererConfig {
        pool,
        fonts: test_fonts(Vec::new()),
        images: StaticImages::default(),
        theme,
        stylesheet,
        mode: ClassCheckMode::Deny,
        size_target: crate::plugin::DEFAULT_SIZE_TARGET,
        still_fallback: true,
        caps: RenderCaps::new(),
        remote: AssetFetcher::new(&crate::RemoteConfig::new()).unwrap(),
    });
    assert!(renderer.register(&[]).is_ok());
    let err = renderer
        .register(&[TemplateDescriptor::of::<Card>()])
        .unwrap_err();
    assert!(matches!(err, RenderError::NoFont), "{err:?}");
    assert!(
        err.to_string()
            .starts_with("[IMG-01] no font is registered")
    );
}

mod remote;
