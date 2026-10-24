//! Emoji through `TakumiRenderer` (IMG-04): custom emoji found in Template
//! data, fetched and drawn inline, their cap, fallback, cache and lookup;
//! the bundled Unicode emoji font and the startup warning.

use image::AnimationDecoder;
use noctis_ports::memory::MemoryAssetStore;
use noctis_ports::{AssetStore, RendererPorts};

use super::*;
use crate::fonts::FontsConfig;
use crate::remote::test_server::{Reply, Server, animated_webp, capture_lines, png};
use crate::{AssetField, DeclaredImages, RemoteConfig, RemoteImage, TemplateAssets, text};

const ID: u64 = 123_456_789_012_345_678;
const RED: [u8; 4] = [255, 0, 0, 255];
const SLATE: [u8; 3] = [0x47, 0x55, 0x69];
const WHITE: u32 = 0xFF_FFFF;

static JP400: &[u8] =
    include_bytes!("../../../tests/fixtures/fonts/noto-sans-jp-119-400-normal.woff2");

/// A still Template `$name` of `$size` drawing `$view` over a white root.
macro_rules! say {
    ($name:ident, $id:literal, $size:expr, $view:expr) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        struct $name {
            text: String,
        }

        impl Template for $name {
            const ID: &'static str = $id;
            const VERSION: u32 = 1;
            const SIZE: Size = $size;
            const FORMAT: Format = Format::Png;
            const ANIMATION: Option<AnimationSpec> = None;

            fn view() -> ErasedView {
                erased_view::<Self>()
            }
        }

        impl TemplateAssets for $name {}

        impl TemplateView for $name {
            fn view(&self, _: Frame) -> Node {
                let view: fn(&str) -> Node = $view;
                div(Classes::__literal("flex w-full h-full items-center"))
                    .style(Style::new().bg_rgb(WHITE))
                    .child(view(&self.text))
            }
        }
    };
}

say!(Say, "say", Size::new(240, 48), |t| text(
    t.to_owned(),
    Classes::default()
));
say!(SayGreen, "say-green", Size::new(64, 48), |t| text(
    t.to_owned(),
    Classes::default()
)
.style(Style::new().color_rgb(0x00_FF00)));
say!(Shout, "shout", Size::new(64, 48), |t| text(
    t.to_uppercase(),
    Classes::default()
));
say!(Forge, "forge", Size::new(64, 48), |_| text(
    format!("<:x1:{FORGED}>"),
    Classes::default()
));
say!(Big, "big", Size::new(96, 96), |t| text(
    t.to_owned(),
    Classes::__literal("text-[64px]")
));

/// An emoji id no Template data holds.
const FORGED: u64 = 98_765_432_109_876_543;

/// An animated Template (2 fps over 1 s) drawing its text.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SayAnim {
    text: String,
}

impl Template for SayAnim {
    const ID: &'static str = "say-anim";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(48, 48);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = Some(AnimationSpec::new(
        Duration::from_secs(1),
        2,
        AnimatedFormat::Webp { quality: 100 },
    ));

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl TemplateAssets for SayAnim {}

impl TemplateView for SayAnim {
    fn view(&self, _: Frame) -> Node {
        div(Classes::__literal("flex w-full h-full items-center"))
            .child(text(self.text.clone(), Classes::default()))
    }
}

/// `#[asset]` images plus text with emoji.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Mixed {
    images: Vec<RemoteImage>,
    text: String,
}

impl Template for Mixed {
    const ID: &'static str = "mixed";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(64, 16);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl TemplateAssets for Mixed {
    const DECLARES: bool = true;

    fn declare(&self, out: &mut DeclaredImages) {
        AssetField::declare(&self.images, "images", false, out);
    }
}

impl TemplateView for Mixed {
    fn view(&self, _: Frame) -> Node {
        div(Classes::__literal("flex w-full h-full"))
            .child(text(self.text.clone(), Classes::default()))
    }
}

fn templates() -> [TemplateDescriptor; 7] {
    [
        TemplateDescriptor::of::<Say>(),
        TemplateDescriptor::of::<SayGreen>(),
        TemplateDescriptor::of::<Shout>(),
        TemplateDescriptor::of::<Forge>(),
        TemplateDescriptor::of::<Big>(),
        TemplateDescriptor::of::<SayAnim>(),
        TemplateDescriptor::of::<Mixed>(),
    ]
}

/// A renderer drawing with `fonts` (on its own test Runtime) whose remote
/// images resolve with `config`.
fn renderer_of(config: &RemoteConfig, fonts: FontsConfig) -> TakumiRenderer {
    let runtime = Runtime::for_test();
    let pool = runtime
        .resource_or_init::<RenderPool>(PoolConfig {
            threads: 2,
            queue_capacity: 64,
        })
        .unwrap();
    let theme = theme();
    let stylesheet = theme_stylesheet(&theme).unwrap();
    let renderer = TakumiRenderer::new(RendererConfig {
        pool,
        fonts: runtime.resource_or_init::<FontRegistry>(fonts).unwrap(),
        images: StaticImages::default(),
        theme,
        stylesheet,
        mode: ClassCheckMode::Deny,
        size_target: crate::plugin::DEFAULT_SIZE_TARGET,
        still_fallback: true,
        caps: RenderCaps::new(),
        remote: AssetFetcher::new(config).unwrap(),
    });
    renderer.register(&templates()).unwrap();
    renderer
}

fn inter(emoji: bool) -> FontsConfig {
    FontsConfig {
        fonts: vec![Font::from_static(INTER)],
        bundled: false,
        emoji,
    }
}

fn for_server(server: &Server, config: RemoteConfig) -> TakumiRenderer {
    renderer_of(&config.discord_cdn(server.origin()), inter(false))
}

fn job<T: Template + Serialize>(t: &T) -> RenderJob {
    RenderJob::of(t).unwrap()
}

fn say(t: &str) -> RenderJob {
    job(&Say { text: t.to_owned() })
}

fn token(id: u64) -> String {
    format!("<:pepe:{id}>")
}

fn is_red(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] < 60 && p[2] < 60
}

fn is_slate(p: [u8; 4]) -> bool {
    p[..3].iter().zip(SLATE).all(|(&a, b)| a.abs_diff(b) <= 12)
}

fn is_ink(p: [u8; 4]) -> bool {
    p[..3].iter().all(|&c| c < 110)
}

/// The `(x, y)` of every pixel of a PNG that matches `pred`.
fn where_(bytes: &[u8], pred: fn([u8; 4]) -> bool) -> Vec<(u32, u32)> {
    let (width, height, rgba) = decode_png(bytes);
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .filter(|&(x, y)| pred(pixel(&rgba, width, x, y)))
        .collect()
}

#[tokio::test]
async fn img_04_a_custom_emoji_is_fetched_and_drawn_inline() {
    let server = Server::start(|_| Reply::Ok(png(4, 4, RED))).await;
    let r = for_server(&server, RemoteConfig::new());
    let out = r
        .render(say(&format!("ab {} cd", token(ID))))
        .await
        .unwrap();
    let targets: Vec<String> = server.requests().into_iter().map(|q| q.target).collect();
    assert_eq!(targets, [format!("/emojis/{ID}.webp?size=128")]);

    let red = where_(&out.bytes, is_red);
    assert!(
        red.len() > 50,
        "the emoji is drawn: {} red pixels",
        red.len()
    );
    let (min_x, max_x) = (
        red.iter().map(|p| p.0).min().unwrap(),
        red.iter().map(|p| p.0).max().unwrap(),
    );
    let (min_y, max_y) = (
        red.iter().map(|p| p.1).min().unwrap(),
        red.iter().map(|p| p.1).max().unwrap(),
    );
    let ink = where_(&out.bytes, is_ink);
    let on_line = |&&(_, y): &&(u32, u32)| (min_y..=max_y).contains(&y);
    assert!(
        ink.iter().filter(on_line).any(|&(x, _)| x < min_x),
        "`ab` is drawn left of the emoji"
    );
    assert!(
        ink.iter().filter(on_line).any(|&(x, _)| x > max_x),
        "`cd` is drawn right of the emoji"
    );
    // 1.375em at the default 16px is 22px.
    assert!((18..=24).contains(&(max_x - min_x + 1)), "{min_x}..{max_x}");
}

#[tokio::test]
async fn img_04_no_glyph_of_the_markup_is_drawn() {
    let server = Server::start(|_| Reply::Ok(png(4, 4, RED))).await;
    let r = for_server(&server, RemoteConfig::new());
    let out = r.render(job(&SayGreen { text: token(ID) })).await.unwrap();
    assert!(!where_(&out.bytes, is_red).is_empty());
    let green = where_(&out.bytes, |p| p[1] > 100 && p[0] < 150 && p[2] < 150);
    assert!(green.is_empty(), "{} green pixels", green.len());
}

/// The frames of an animated WebP, as RGBA.
fn webp_frames(bytes: &[u8]) -> Vec<image::RgbaImage> {
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes)).unwrap();
    decoder
        .into_frames()
        .map(|frame| frame.unwrap().into_buffer())
        .collect()
}

#[tokio::test]
async fn img_04_an_animated_emoji_animates_only_in_an_animated_job() {
    let server = Server::start(|target| {
        if target.contains("animated=true") {
            Reply::Ok(animated_webp())
        } else {
            Reply::Ok(png(2, 2, RED))
        }
    })
    .await;
    let r = for_server(&server, RemoteConfig::new());
    let store = Arc::new(MemoryAssetStore::new());
    r.bind(RendererPorts::new(store.clone()));
    let animated = format!("<a:x1:{ID}>");
    let out = r
        .render(job(&SayAnim {
            text: animated.clone(),
        }))
        .await
        .unwrap();
    assert_eq!(
        server.requests()[0].target,
        format!("/emojis/{ID}.webp?size=128&animated=true")
    );
    let frames = webp_frames(&out.bytes);
    assert_eq!(frames.len(), 2);
    assert_ne!(frames[0], frames[1], "the emoji's frames differ");

    r.render(say(&animated)).await.unwrap();
    assert_eq!(
        server.requests()[1].target,
        format!("/emojis/{ID}.webp?size=128")
    );
    let key = format!("img:v1:emoji:{ID}:128:s");
    assert!(store.get(&key).await.unwrap().is_some(), "{key}");
}

#[tokio::test(flavor = "current_thread")]
async fn img_04_failures_draw_the_fallback_with_a_warning() {
    let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec();
    let replies = [
        Reply::Status(404),
        Reply::After(Duration::from_secs(5), Box::new(Reply::Ok(png(4, 4, RED)))),
        Reply::Ok(gif),
    ];
    for reply in replies {
        let server = Server::start(move |_| reply.clone()).await;
        let r = for_server(
            &server,
            RemoteConfig::new().timeout(Duration::from_millis(100)),
        );
        let (lines, _guard) = capture_lines();
        let out = r
            .render(say(&format!("ab {} cd", token(ID))))
            .await
            .unwrap();
        assert!(
            where_(&out.bytes, is_slate).len() > 50,
            "the fallback is drawn inline"
        );
        let warnings = lines.warnings();
        assert_eq!(warnings.len(), 1, "{warnings:#?}");
        assert!(
            warnings[0].contains(&format!("emoji/{ID}")),
            "{}",
            warnings[0]
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn img_04_emoji_over_the_cap_draw_the_fallback() {
    let server = Server::start(|_| Reply::Ok(png(4, 4, RED))).await;
    let text: String = (0..33).map(|i| token(ID + i)).collect();

    let r = for_server(&server, RemoteConfig::new());
    let (lines, guard) = capture_lines();
    r.render(say(&text)).await.unwrap();
    assert_eq!(server.requests().len(), 32);
    let warnings = lines.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains("1 custom emoji over the per-job cap of 32 draw the fallback"),
        "{}",
        warnings[0]
    );
    drop(guard);

    let none = for_server(&server, RemoteConfig::new().max_emoji(0));
    let three: String = (0..3).map(|i| token(ID + 100 + i)).collect();
    let out = none.render(say(&three)).await.unwrap();
    assert_eq!(server.requests().len(), 32, "max_emoji(0) fetches none");
    assert!(where_(&out.bytes, is_slate).len() > 50);
    assert!(where_(&out.bytes, is_red).is_empty());
}

#[tokio::test]
async fn img_04_emoji_never_count_against_max_per_job() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let r = for_server(&server, RemoteConfig::new());
    let images = (0..16)
        .map(|i| RemoteImage::url(&server.url(&format!("/{i}.png"))).unwrap())
        .collect();
    let text = (0..20).map(|i| token(ID + i)).collect();
    r.render(job(&Mixed { images, text })).await.unwrap();
    assert_eq!(server.requests().len(), 36);
}

#[tokio::test]
async fn img_04_a_bound_store_caches_an_emoji() {
    let server = Server::start(|_| Reply::Ok(png(4, 4, RED))).await;
    let r = for_server(&server, RemoteConfig::new());
    let store = Arc::new(MemoryAssetStore::new());
    r.bind(RendererPorts::new(store.clone()));
    for _ in 0..2 {
        let out = r.render(say(&token(ID))).await.unwrap();
        assert!(!where_(&out.bytes, is_red).is_empty());
    }
    assert_eq!(server.requests().len(), 1);
    let key = format!("img:v1:emoji:{ID}:128:s");
    assert!(store.get(&key).await.unwrap().is_some(), "{key}");
}

#[tokio::test]
async fn img_04_an_emoji_only_the_view_writes_is_undeclared() {
    let server = Server::start(|_| Reply::Ok(png(4, 4, RED))).await;
    let r = for_server(&server, RemoteConfig::new());
    let error = r.render(job(&Forge { text: token(ID) })).await.unwrap_err();
    assert!(
        matches!(&error, RenderError::UndeclaredImage { id, image }
            if id == "forge" && *image == format!("emoji/{FORGED}")),
        "{error:?}"
    );
}

#[tokio::test]
async fn img_04_the_lookup_ignores_the_name() {
    let server = Server::start(|_| Reply::Ok(png(4, 4, RED))).await;
    let r = for_server(&server, RemoteConfig::new());
    let out = r.render(job(&Shout { text: token(ID) })).await.unwrap();
    assert!(!where_(&out.bytes, is_red).is_empty());
}

#[tokio::test]
async fn img_04_data_without_a_token_is_admitted_at_call() {
    let r = renderer_of(&RemoteConfig::new(), inter(false));
    let render = r.render(say("a < b, <:p:1>"));
    assert_eq!(r.pool.admitted(), 1, "admitted before the future runs");
    render.await.unwrap();
}

/// Pixels within ±24 of Twemoji's face yellow `#FFCC4D`.
fn yellow(bytes: &[u8]) -> usize {
    where_(bytes, |p| {
        p[..3]
            .iter()
            .zip([0xFF, 0xCC, 0x4D])
            .all(|(&a, b): (&u8, u8)| a.abs_diff(b) <= 24)
    })
    .len()
}

fn kana(emoji: bool) -> FontsConfig {
    FontsConfig {
        fonts: vec![Font::from_static(JP400)],
        bundled: false,
        emoji,
    }
}

#[cfg(feature = "emoji")]
#[tokio::test]
async fn img_04_unicode_emoji_draw_in_color() {
    let r = renderer_of(&RemoteConfig::new(), kana(true));
    let out = r
        .render(job(&Big {
            text: "😀".into()
        }))
        .await
        .unwrap();
    let count = yellow(&out.bytes);
    assert!(count >= 200, "{count} yellow pixels");
}

#[tokio::test]
async fn img_04_unicode_emoji_without_the_font_draw_no_color() {
    let r = renderer_of(&RemoteConfig::new(), kana(false));
    let out = r
        .render(job(&Big {
            text: "😀".into()
        }))
        .await
        .unwrap();
    assert_eq!(yellow(&out.bytes), 0);
}

#[cfg(feature = "emoji")]
#[tokio::test]
async fn img_04_the_emoji_font_leaves_text_unchanged() {
    let with = renderer_of(&RemoteConfig::new(), inter(true));
    let without = renderer_of(&RemoteConfig::new(), inter(false));
    let a = with.render(say("Ab12 #*")).await.unwrap();
    let b = without.render(say("Ab12 #*")).await.unwrap();
    assert_eq!(a.bytes, b.bytes);
}

#[cfg(feature = "emoji")]
#[test]
fn img_04_the_emoji_font_alone_is_no_font() {
    let runtime = Runtime::for_test();
    let r = TakumiRenderer::new(RendererConfig {
        pool: runtime
            .resource_or_init::<RenderPool>(PoolConfig {
                threads: 1,
                queue_capacity: 4,
            })
            .unwrap(),
        fonts: runtime
            .resource_or_init::<FontRegistry>(FontsConfig {
                fonts: Vec::new(),
                bundled: false,
                emoji: true,
            })
            .unwrap(),
        images: StaticImages::default(),
        theme: theme(),
        stylesheet: theme_stylesheet(&theme()).unwrap(),
        mode: ClassCheckMode::Deny,
        size_target: crate::plugin::DEFAULT_SIZE_TARGET,
        still_fallback: true,
        caps: RenderCaps::new(),
        remote: AssetFetcher::new(&RemoteConfig::new()).unwrap(),
    });
    assert!(matches!(r.register(&templates()), Err(RenderError::NoFont)));
}

#[test]
fn img_04_register_warns_once_without_the_emoji_font() {
    let without = renderer_of(&RemoteConfig::new(), inter(false));
    let (lines, guard) = capture_lines();
    without.register(&templates()).unwrap();
    let warnings = lines.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:#?}");
    assert!(
        warnings[0].contains("[IMG-04]") && warnings[0].contains("image-emoji"),
        "{}",
        warnings[0]
    );
    without.register(&[]).unwrap();
    assert_eq!(lines.warnings().len(), 1, "no Templates: no warning");
    drop(guard);

    #[cfg(feature = "emoji")]
    {
        let with = renderer_of(&RemoteConfig::new(), inter(true));
        let (lines, _guard) = capture_lines();
        with.register(&templates()).unwrap();
        assert!(lines.warnings().is_empty());
    }
}
