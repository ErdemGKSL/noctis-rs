//! Remote images through `TakumiRenderer::render` (IMG-03): resolution
//! before admission, the fallback, required fields, dedupe, the count cap,
//! undeclared images, binding, cancellation and animated avatars.

use std::sync::atomic::{AtomicUsize, Ordering};

use image::AnimationDecoder;
use noctis_ports::memory::MemoryAssetStore;
use noctis_ports::{AssetStore, RenderCap, RendererPorts};

use super::*;
use crate::remote::test_server::{Reply, Server, animated_webp, png};
use crate::{AssetField, DeclaredImages, RemoteConfig, RemoteImage, TemplateAssets, img};

const RED: [u8; 4] = [255, 0, 0, 255];
const SLATE: [u8; 4] = [0x47, 0x55, 0x69, 0xFF];

/// A still Template declaring `images` (required when `required`) and
/// drawing `draw`, each over the whole canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pic {
    images: Vec<RemoteImage>,
    draw: Vec<RemoteImage>,
    required: bool,
}

/// How many times each Template's view ran.
static PIC_VIEWS: AtomicUsize = AtomicUsize::new(0);

impl Template for Pic {
    const ID: &'static str = "pic";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(4, 4);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl TemplateAssets for Pic {
    const DECLARES: bool = true;

    fn declare(&self, out: &mut DeclaredImages) {
        AssetField::declare(&self.images, "images", self.required, out);
    }
}

fn draw_all(draw: &[RemoteImage]) -> Node {
    draw.iter().fold(
        div(Classes::__literal("flex w-full h-full")),
        |root, image| root.child(img(image, Classes::__literal("w-full h-full"))),
    )
}

impl TemplateView for Pic {
    fn view(&self, _: Frame) -> Node {
        PIC_VIEWS.fetch_add(1, Ordering::SeqCst);
        draw_all(&self.draw)
    }
}

/// A Template without `#[asset]` fields that draws a remote image anyway.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sneaky {
    draw: RemoteImage,
}

impl Template for Sneaky {
    const ID: &'static str = "sneaky";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(4, 4);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        erased_view::<Self>()
    }
}

impl TemplateAssets for Sneaky {}

impl TemplateView for Sneaky {
    fn view(&self, _: Frame) -> Node {
        draw_all(std::slice::from_ref(&self.draw))
    }
}

/// An animated Template (2 fps over 1 s) declaring and drawing `avatar`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Spin {
    avatar: RemoteImage,
}

impl Template for Spin {
    const ID: &'static str = "spin";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(4, 4);
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

impl TemplateAssets for Spin {
    const DECLARES: bool = true;

    fn declare(&self, out: &mut DeclaredImages) {
        AssetField::declare(&self.avatar, "avatar", false, out);
    }
}

impl TemplateView for Spin {
    fn view(&self, _: Frame) -> Node {
        draw_all(std::slice::from_ref(&self.avatar))
    }
}

/// A renderer whose remote images resolve with `config`.
fn remote_renderer(config: &RemoteConfig) -> TakumiRenderer {
    let pool = Runtime::for_test()
        .resource_or_init::<RenderPool>(PoolConfig {
            threads: 2,
            queue_capacity: 64,
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
        mode: ClassCheckMode::Deny,
        size_target: crate::plugin::DEFAULT_SIZE_TARGET,
        still_fallback: true,
        caps: RenderCaps::new(),
        remote: AssetFetcher::new(config).unwrap(),
    });
    renderer
        .register(&[
            TemplateDescriptor::of::<Pic>(),
            TemplateDescriptor::of::<Sneaky>(),
            TemplateDescriptor::of::<Spin>(),
            TemplateDescriptor::of::<Card>(),
        ])
        .unwrap();
    renderer
}

fn for_server(server: &Server) -> TakumiRenderer {
    remote_renderer(&RemoteConfig::new().discord_cdn(server.origin()))
}

fn pic(images: Vec<RemoteImage>, draw: Vec<RemoteImage>, required: bool) -> RenderJob {
    RenderJob::of(&Pic {
        images,
        draw,
        required,
    })
    .unwrap()
}

/// The top-left pixel of a PNG.
fn corner(bytes: &[u8]) -> [u8; 4] {
    let (width, _, rgba) = decode_png(bytes);
    pixel(&rgba, width, 0, 0)
}

#[tokio::test]
async fn img_03_a_declared_image_is_fetched_before_the_job_is_submitted() {
    let requested = Arc::new(AtomicUsize::new(0));
    let hits = Arc::clone(&requested);
    let server = Server::start(move |_| {
        hits.fetch_add(1, Ordering::SeqCst);
        Reply::Ok(png(2, 2, RED))
    })
    .await;
    let r = for_server(&server);
    let a = RemoteImage::url(&server.url("/a.png")).unwrap();
    let render = r.render(pic(vec![a.clone()], vec![a], false));
    // Nothing is fetched or queued before the future runs.
    assert_eq!(requested.load(Ordering::SeqCst), 0);
    let out = render.await.unwrap();
    assert_eq!(corner(&out.bytes), RED);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn img_03_failures_draw_the_fallback_and_required_fails() {
    let server = Server::start(|_| Reply::Status(404)).await;
    let r = for_server(&server);
    let a = RemoteImage::url(&server.url("/a.png?token=secret")).unwrap();
    let out = r
        .render(pic(vec![a.clone()], vec![a.clone()], false))
        .await
        .unwrap();
    assert_eq!(corner(&out.bytes), SLATE);
    let views = PIC_VIEWS.load(Ordering::SeqCst);
    let error = r
        .render(pic(vec![a.clone()], vec![a], true))
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "[IMG-03] template `pic` requires remote image `{}/a.png`, which could not be used: \
             HTTP 404",
            server.origin()
        )
    );
    assert!(
        PIC_VIEWS.load(Ordering::SeqCst) <= views + 1,
        "no job after a required failure"
    );
}

#[tokio::test]
async fn img_03_not_allow_listed_draws_the_fallback() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let r = remote_renderer(&RemoteConfig::new());
    let a = RemoteImage::url(&server.url("/a.png")).unwrap();
    let out = r
        .render(pic(vec![a.clone()], vec![a], false))
        .await
        .unwrap();
    assert_eq!(corner(&out.bytes), SLATE);
    assert_eq!(server.requests().len(), 0);
}

#[tokio::test]
async fn img_03_the_placeholder_always_draws_the_fallback() {
    let r = remote_renderer(&RemoteConfig::new());
    let p = RemoteImage::placeholder();
    let out = r.render(pic(vec![p.clone()], vec![p], true)).await.unwrap();
    assert_eq!(corner(&out.bytes), SLATE);
    // Even a Template without `#[asset]` fields may draw it.
    let out = r
        .render(
            RenderJob::of(&Sneaky {
                draw: RemoteImage::placeholder(),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(corner(&out.bytes), SLATE);
}

#[tokio::test]
async fn img_03_duplicates_are_fetched_once_and_option_vec_shapes() {
    let server = Server::start(|_| Reply::Ok(png(2, 2, RED))).await;
    let r = for_server(&server);
    let a = RemoteImage::url(&server.url("/a.png")).unwrap();
    let out = r
        .render(pic(vec![a.clone(), a.clone()], vec![a.clone(), a], false))
        .await
        .unwrap();
    assert_eq!(corner(&out.bytes), RED);
    assert_eq!(server.requests().len(), 1);
    // `vec![]` declares nothing.
    assert!(r.render(pic(vec![], vec![], false)).await.is_ok());
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn img_03_over_the_count_cap_fails_before_any_fetch() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let r = for_server(&server);
    let images: Vec<RemoteImage> = (0..17)
        .map(|i| RemoteImage::url(&server.url(&format!("/{i}.png"))).unwrap())
        .collect();
    let error = r.render(pic(images, vec![], false)).await.unwrap_err();
    assert!(
        matches!(
            error,
            RenderError::OverCap {
                cap: RenderCap::RemoteImages,
                limit: 16,
                actual: 17,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(server.requests().len(), 0);
}

#[tokio::test]
async fn img_03_an_undeclared_image_is_an_error() {
    let server = Server::start(|_| Reply::Ok(png(1, 1, RED))).await;
    let r = for_server(&server);
    let a = RemoteImage::url(&server.url("/a.png?x=1")).unwrap();
    let b = RemoteImage::url(&server.url("/b.png")).unwrap();
    let error = r.render(pic(vec![a], vec![b], false)).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "[IMG-03] template `pic` draws remote image `{}/b.png`, which no `#[asset]` field \
             declares",
            server.origin()
        )
    );
    let sneaky = RenderJob::of(&Sneaky {
        draw: RemoteImage::url(&server.url("/c.png")).unwrap(),
    })
    .unwrap();
    assert!(matches!(
        r.render(sneaky).await,
        Err(RenderError::UndeclaredImage { .. })
    ));
    assert_eq!(server.requests().len(), 1, "only the declared image");
}

#[tokio::test]
async fn img_03_a_bound_store_caches_and_unbound_fetches_every_time() {
    let server = Server::start(|_| Reply::Ok(png(2, 2, RED))).await;
    let a = RemoteImage::url(&server.url("/a.png")).unwrap();
    let unbound = for_server(&server);
    for _ in 0..2 {
        unbound
            .render(pic(vec![a.clone()], vec![a.clone()], false))
            .await
            .unwrap();
    }
    assert_eq!(server.requests().len(), 2);
    let bound = for_server(&server);
    let store = Arc::new(MemoryAssetStore::new());
    bound.bind(RendererPorts::new(Arc::clone(&store) as Arc<dyn AssetStore>));
    // A second bind is ignored (the first wins).
    bound.bind(RendererPorts::new(Arc::new(MemoryAssetStore::new())));
    for _ in 0..2 {
        let out = bound
            .render(pic(vec![a.clone()], vec![a.clone()], false))
            .await
            .unwrap();
        assert_eq!(corner(&out.bytes), RED);
    }
    assert_eq!(server.requests().len(), 3);
    let key = format!(
        "img:v1:url:{}",
        blake3::hash(server.url("/a.png").as_bytes()).to_hex()
    );
    assert!(store.get(&key).await.unwrap().is_some());
}

#[tokio::test]
async fn img_03_dropping_the_render_aborts_the_fetch_and_submits_nothing() {
    let server = Server::start(|_| {
        Reply::After(Duration::from_secs(30), Box::new(Reply::Ok(png(1, 1, RED))))
    })
    .await;
    let r = for_server(&server);
    let a = RemoteImage::url(&server.url("/stall.png")).unwrap();
    let render = r.render(RenderJob::of(&Spin { avatar: a }).unwrap());
    // A Template with `#[asset]` fields is admitted after its fetches.
    assert_eq!(r.pool.admitted(), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), render)
            .await
            .is_err()
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while server.aborted() == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the fetch was not aborted"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(r.pool.admitted(), 0, "the pool got no job");
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
async fn img_03_an_animated_avatar_animates() {
    const HASH: &str = "a_1269e74af4df7417b13759eae50c83dc";
    let user = |avatar: &str| twilight_model::user::User {
        accent_color: None,
        avatar: Some(avatar.parse().unwrap()),
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator: 0,
        email: None,
        flags: None,
        global_name: None,
        id: twilight_model::id::Id::new(1),
        locale: None,
        mfa_enabled: None,
        name: "u".to_owned(),
        premium_type: None,
        primary_guild: None,
        public_flags: None,
        system: None,
        verified: None,
    };
    let server = Server::start(|target| {
        if target.contains("animated=true") {
            Reply::Ok(animated_webp())
        } else {
            Reply::Ok(png(2, 2, RED))
        }
    })
    .await;
    let r = for_server(&server);
    let avatar = RemoteImage::avatar(&user(HASH), 64);
    let out = r
        .render(
            RenderJob::of(&Spin {
                avatar: avatar.clone(),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    let requests = server.requests();
    assert_eq!(
        requests[0].target,
        format!("/avatars/1/{HASH}.webp?size=64&animated=true")
    );
    let frames = webp_frames(&out.bytes);
    assert_eq!(frames.len(), 2);
    let [r0, _, b0, _] = frames[0].get_pixel(0, 0).0;
    let [r1, _, b1, _] = frames[1].get_pixel(0, 0).0;
    assert!(
        r0 > 200 && b0 < 60,
        "frame 0 is red: {:?}",
        frames[0].get_pixel(0, 0)
    );
    assert!(
        b1 > 200 && r1 < 60,
        "frame 1 is blue: {:?}",
        frames[1].get_pixel(0, 0)
    );
    // A still Template requests the same avatar without `animated`.
    let still = r
        .render(pic(vec![avatar.clone()], vec![avatar], false))
        .await
        .unwrap();
    assert_eq!(corner(&still.bytes), RED);
    assert_eq!(
        server.requests()[1].target,
        format!("/avatars/1/{HASH}.webp?size=64")
    );
}
