//! Story 10.3: `ctx.render` with the real `ImagePlugin` through
//! `noctis::testkit::TestApp`: the uploaded file is the Template's PNG, and a
//! panicking view reaches the Render Error Sink while the pool keeps
//! rendering.

#![cfg(all(feature = "image", feature = "testkit"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};

use noctis::image::{Frame, Node, RenderError, Template, TemplateView, theme, view};
use noctis::prelude::*;
use noctis::testkit::{Response, TestApp, TestError};
use noctis::ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
use noctis::{ErrorReport, ReportKind};
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

#[derive(Template, Serialize, Deserialize)]
#[template(id = "boom", version = 1, size = (4, 4), format = Png)]
struct Boom;

impl TemplateView for Boom {
    fn view(&self, _: Frame) -> Node {
        panic!("boom")
    }
}

/// Responds with `image` in a Media Gallery.
async fn show(ctx: &CommandCtx, image: noctis::ui::Attachment) -> HandlerResult<Responded> {
    let mut files = AttachmentSet::new();
    let image = files.add(image);
    let gallery = MediaGallery::new([MediaItem::new(image).alt("Card")]);
    ctx.respond(V2Message::new(Components::new().push(gallery)).attachments(files))
        .await
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
        show(&ctx, image).await
    }
}

#[derive(SlashCommand)]
#[command("boom", description = "Render a panicking card")]
struct ShowBoom;

impl ShowBoom {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        match ctx.render(Boom).await {
            Ok(image) => show(&ctx, image).await,
            Err(err) => match err.as_error().downcast_ref::<RenderError>() {
                Some(RenderError::Panicked { message }) => {
                    ctx.reply_text(format!("panicked: {message}")).await
                }
                _ => Err(err),
            },
        }
    }
}

/// The Render sink's reports: (origin, kind, handler).
#[derive(Default)]
struct Reports(Mutex<Vec<(String, String, String)>>);

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Panic(message) => format!("panic:{message}"),
            ReportKind::Error(error) => format!("error:{error}"),
            _ => "other".to_owned(),
        };
        self.0.lock().unwrap().push((
            report.origin.to_string(),
            kind,
            report.handler.unwrap_or("-").to_owned(),
        ));
    }
}

async fn start(reports: &Arc<Reports>) -> TestApp {
    let mut builder = App::builder();
    builder
        .theme(theme!())
        .error_sink(ErrorOrigin::Render, Arc::clone(reports))
        .module(
            Module::new("cards")
                .template::<Card>()
                .template::<Boom>()
                .add::<ShowCard>()
                .add::<ShowBoom>(),
        );
    TestApp::build(&mut builder).await.unwrap()
}

/// The PNG uploaded by the run's one callback (or its `@original` edit).
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

#[tokio::test(flavor = "multi_thread")]
async fn ctx_render_uploads_the_templates_png() -> Result<(), TestError> {
    let reports = Arc::new(Reports::default());
    let app = start(&reports).await;
    let response = app.command("card").run().await?;
    let (name, bytes) = uploaded(&response);
    let digest = blake3::hash(&bytes);
    let hex = digest.to_hex()[..16].to_owned();
    assert_eq!(name, format!("{hex}.png"));
    let decoder = png::Decoder::new(std::io::Cursor::new(&bytes[..]));
    let reader = decoder.read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (64, 32));
    assert!(reports.0.lock().unwrap().is_empty());
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panicking_view_reaches_the_render_sink_and_the_next_render_works()
-> Result<(), TestError> {
    let reports = Arc::new(Reports::default());
    let app = start(&reports).await;
    let response = app.command("boom").run().await?;
    assert_eq!(response.text(), "panicked: boom");
    assert_eq!(
        *reports.0.lock().unwrap(),
        [(
            "render".to_owned(),
            "panic:boom".to_owned(),
            "boom".to_owned()
        )]
    );
    let next = app.command("card").run().await?;
    let (name, _) = uploaded(&next);
    assert_eq!(name.len(), "0123456789abcdef.png".len(), "{name}");
    assert_eq!(&name[16..], ".png");
    app.stop().await
}
