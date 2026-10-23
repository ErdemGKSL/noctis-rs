//! The Renderer rows that need no App: register, render, encode, settings,
//! errors, panics, the queue and the class check.

use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;

use noctis_core::Runtime;
use noctis_ports::{AnimatedFormat, AnimationSpec, ErasedView, Size, Template};
use serde::{Deserialize, Serialize};

use super::*;
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

fn theme() -> Theme {
    Theme::parse("@theme { --color-brand: #5865f2; }").unwrap()
}

fn renderer_with(threads: usize, queue_capacity: usize, mode: ClassCheckMode) -> TakumiRenderer {
    let pool = Runtime::for_test()
        .resource_or_init::<RenderPool>(PoolConfig {
            threads,
            queue_capacity,
        })
        .unwrap();
    let theme = theme();
    let stylesheet = theme_stylesheet(&theme).unwrap();
    let renderer = TakumiRenderer::new(pool, theme, stylesheet, mode);
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
fn register_rejects_foreign_and_animated_views() {
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
    let animated = r
        .register(&[TemplateDescriptor::of::<Spinner>()])
        .unwrap_err();
    assert!(
        matches!(&animated, RenderError::Unsupported { id, why } if id == "spinner" && why.contains("not rendered yet")),
        "{animated}"
    );
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
async fn an_oversized_canvas_fails_and_the_pool_survives() {
    let r = renderer();
    let err = r
        .render(RenderJob::of(&Huge {}).unwrap())
        .await
        .unwrap_err();
    let RenderError::Failed(source) = &err else {
        panic!("{err}");
    };
    assert!(source.to_string().contains("megapixels"), "{source}");
    assert!(r.render(card("Ada")).await.is_ok());
}

#[tokio::test]
async fn unicode_text_renders() {
    assert!(renderer().render(card("Ada 😀 ğüş")).await.is_ok());
}

#[tokio::test]
async fn unknown_ids_versions_and_animated_jobs_fail() {
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
    let animated = RenderJob {
        animation: Spinner::ANIMATION,
        ..card("Ada")
    };
    assert!(matches!(
        r.render(animated).await,
        Err(RenderError::Unsupported { id, .. }) if id == "card"
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
