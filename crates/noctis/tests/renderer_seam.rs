//! Story 15.1 (FR-37): the Renderer seam. One Module and handler render
//! through a custom Renderer built only from `noctis::image::unstable`, and
//! through the takumi Renderer, with no source change; a second Renderer
//! is a `BuildViolation::DuplicateRenderer`. The project template's
//! `FixedRenderer` compiles against the real API and serves the Module too.

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

#[path = "../../../examples/project-template/src/adapters/fixed_renderer.rs"]
mod fixed_renderer;

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use noctis::image::unstable::{
    ErasedView, RenderJob, RenderOutput, Renderer, RendererPorts, TemplateDescriptor,
    decode_template,
};
use noctis::image::{
    EncodeSettings, Font, Frame, ImageConfig, Node, OutputFormat, RenderError, Template,
    TemplateView, theme, view,
};
use noctis::ports::BoxFuture;
use noctis::prelude::*;
use noctis::testkit::{Response, TestApp, TestError};
use noctis::ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
use noctis::{BuildViolation, Runtime};
use serde::{Deserialize, Serialize};

#[derive(Template, Serialize, Deserialize)]
#[template(id = "card", version = 1, size = (64, 32), format = Png)]
struct Card {
    name: String,
}

impl TemplateView for Card {
    fn view(&self, _: Frame) -> Node {
        view! { <div tw="flex w-full h-full bg-brand">{&self.name}</div> }
    }
}

#[derive(SlashCommand)]
#[command("card", description = "Show the card")]
struct ShowCard;

impl ShowCard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let image = ctx
            .render(Card {
                name: "Ada".to_owned(),
            })
            .await?;
        let mut files = AttachmentSet::new();
        let image = files.add(image);
        let gallery = MediaGallery::new([MediaItem::new(image).alt("Card")]);
        ctx.respond(V2Message::new(Components::new().push(gallery)).attachments(files))
            .await
    }
}

/// The one Module both Apps mount.
fn cards() -> Module {
    Module::new("cards").template::<Card>().add::<ShowCard>()
}

/// A Renderer answering fixed bytes, recording the jobs it saw.
struct Fixed {
    bytes: Bytes,
    jobs: Mutex<Vec<RenderJob>>,
    registered: Mutex<Vec<(&'static str, u32)>>,
    bound: Mutex<usize>,
}

impl Fixed {
    const ID: &'static [u8] = b"renderer-seam.fixed=1";

    fn new(bytes: &'static [u8]) -> Arc<Self> {
        Arc::new(Self {
            bytes: Bytes::from_static(bytes),
            jobs: Mutex::new(Vec::new()),
            registered: Mutex::new(Vec::new()),
            bound: Mutex::new(0),
        })
    }
}

impl Renderer for Fixed {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let out = RenderOutput {
            bytes: self.bytes.clone(),
            digest: job.key(Self::ID),
            format: OutputFormat::Png,
            settings_used: EncodeSettings {
                size: job.size,
                quality: None,
                fps: None,
                frames: 1,
            },
        };
        self.jobs.lock().unwrap().push(job);
        Box::pin(async move { Ok(out) })
    }

    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        self.registered
            .lock()
            .unwrap()
            .extend(templates.iter().map(|t| (t.id, t.version)));
        Ok(())
    }

    fn bind(&self, _: RendererPorts) {
        *self.bound.lock().unwrap() += 1;
    }
}

fn font() -> Font {
    Font::from_static(include_bytes!(
        "../../noctis-image/fonts/InterVariable.woff2"
    ))
}

/// The file uploaded by the run's one callback.
fn uploaded(response: &Response) -> (String, Vec<u8>) {
    let file = response
        .http_calls()
        .iter()
        .flat_map(|call| call.files().iter())
        .next()
        .expect("one uploaded file")
        .clone();
    (file.filename, file.bytes.to_vec())
}

fn hex16(digest: [u8; 32]) -> String {
    use std::fmt::Write as _;

    digest[..8].iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn fr_37_a_custom_renderer_serves_the_same_module() -> Result<(), TestError> {
    let fixed = Fixed::new(b"\x89PNG fixed bytes");
    let mut builder = App::builder();
    builder.renderer(Arc::clone(&fixed)).module(cards());
    let app = TestApp::build(&mut builder).await?;
    assert_eq!(*fixed.registered.lock().unwrap(), [("card", 1)]);
    assert_eq!(*fixed.bound.lock().unwrap(), 1);

    let response = app.command("card").run().await?;
    let (name, bytes) = uploaded(&response);
    assert_eq!(bytes, b"\x89PNG fixed bytes");
    let job = fixed.jobs.lock().unwrap().pop().expect("one job");
    assert_eq!(job.template_id, "card");
    assert_eq!(job.template_version, 1);
    assert_eq!(job.upload_budget, RenderJob::DEFAULT_UPLOAD_BUDGET);
    let card: Card = decode_template(&job.data).unwrap();
    assert_eq!(card.name, "Ada");
    assert_eq!(name, format!("{}.png", hex16(job.key(Fixed::ID))));
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn fr_37_the_same_module_renders_through_takumi() -> Result<(), TestError> {
    let mut builder = App::builder();
    builder
        .theme(theme!())
        .image(ImageConfig::new().font(font()))
        .module(cards());
    let app = TestApp::build(&mut builder).await?;
    let response = app.command("card").run().await?;
    let (name, bytes) = uploaded(&response);
    assert_eq!(
        name.rsplit_once('.').map(|(_, ext)| ext),
        Some("png"),
        "{name}"
    );
    let decoder = png::Decoder::new(std::io::Cursor::new(&bytes[..]));
    let reader = decoder.read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (64, 32));
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn fr_37_a_custom_renderer_and_the_image_plugin_are_duplicate_renderers() {
    let fixed = Fixed::new(b"x");
    let err = App::builder()
        .runtime(&Runtime::for_test())
        .renderer(Arc::clone(&fixed))
        .theme(theme!())
        .image(ImageConfig::new().font(font()))
        .module(cards())
        .build()
        .unwrap_err();
    let [BuildViolation::DuplicateRenderer { first, second }] = err.violations() else {
        panic!("expected one DuplicateRenderer, got {err}");
    };
    assert_eq!(*first, std::any::type_name::<Arc<Fixed>>());
    assert!(second.ends_with("TakumiRenderer"), "{second}");
    // The first Renderer is kept: it got the Templates.
    assert_eq!(*fixed.registered.lock().unwrap(), [("card", 1)]);
}

#[test]
fn fr_37_renderer_types_are_only_unstable() {
    // Every renderer-facing type is reachable from `noctis::image::unstable`.
    let view = ErasedView::new(());
    assert!(view.downcast_ref::<()>().is_some());
    let _: fn(&[u8]) -> Result<Card, noctis::ports::ValueError> = decode_template::<Card>;
    let descriptor = TemplateDescriptor::of::<Card>();
    assert_eq!(descriptor.id, "card");
    // The snapshot lists none of them on a stable path.
    let snapshot = include_str!("../../../xtask/public-api.txt");
    for name in [
        "Renderer",
        "RendererPorts",
        "RenderJob",
        "RenderOutput",
        "TemplateDescriptor",
        "ErasedView",
        "decode_template",
    ] {
        for stable in ["noctis::ports::", "noctis::image::"] {
            let path = format!("{stable}{name}");
            let leaked = snapshot.lines().any(|line| {
                line.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                    .any(|token| token == path)
            });
            assert!(!leaked, "`{path}` is in xtask/public-api.txt");
        }
        assert!(
            snapshot.contains(&format!("noctis::image::unstable::{name}")),
            "`noctis::image::unstable::{name}` is missing from xtask/public-api.txt"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fr_37_the_template_fixed_renderer_serves_the_module() -> Result<(), TestError> {
    use fixed_renderer::FixedRenderer;

    let job = RenderJob::of(&Card {
        name: "Ada".to_owned(),
    })
    .unwrap();
    let renderer = FixedRenderer::new(Bytes::from_static(b"GIF89a"), OutputFormat::Gif);
    let output = renderer.render(job.clone()).await.unwrap();
    assert_eq!(output.bytes, Bytes::from_static(b"GIF89a"));
    assert_eq!(output.digest, job.key(FixedRenderer::ID));
    assert_eq!(output.format, OutputFormat::Gif);

    let mut builder = App::builder();
    builder.module(cards()).renderer(FixedRenderer::new(
        Bytes::from_static(b"GIF89a"),
        OutputFormat::Gif,
    ));
    let app = TestApp::build(&mut builder).await?;
    let response = app.command("card").run().await?;
    let (name, bytes) = uploaded(&response);
    assert_eq!(bytes, b"GIF89a");
    assert_eq!(name, format!("{}.gif", hex16(job.key(FixedRenderer::ID))));
    app.stop().await
}
