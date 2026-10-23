//! Story 10.3: `ctx.render` end to end through `TestApp` with a fake
//! Renderer (FR-21, FR-34): no Renderer, a fast render, a render slower than
//! the ack deadline, and the Render Error Sink.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use noctis::prelude::*;
use noctis::ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
use noctis::{ErrorReport, ReportKind};
use noctis_ports::{
    AnimationSpec, BoxFuture, EncodeSettings, ErasedView, Format, OutputFormat, RenderError,
    RenderJob, RenderOutput, Renderer, Size, Template, TemplateDescriptor,
};
use noctis_testkit::{HttpCall, Response, TestApp, TestError};
use serde::{Deserialize, Serialize};

/// A hand-written Template (the fake Renderer never draws it).
#[derive(Serialize, Deserialize)]
struct Card {
    name: String,
}

impl Template for Card {
    const ID: &'static str = "card";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(64, 32);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = None;

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

/// The fake's image bytes and digest.
const BYTES: &[u8] = b"\x89PNG fake card";
const DIGEST: [u8; 32] = [0x5A; 32];

/// How the fake answers.
#[derive(Clone, Copy)]
enum Answer {
    Now,
    After(Duration),
    Failed,
    Unknown,
}

struct Fake(Answer);

impl Renderer for Fake {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let answer = self.0;
        Box::pin(async move {
            match answer {
                Answer::Now => {}
                Answer::After(delay) => tokio::time::sleep(delay).await,
                Answer::Failed => return Err(RenderError::failed("fake failure")),
                Answer::Unknown => {
                    return Err(RenderError::UnknownTemplate {
                        id: job.template_id.to_owned(),
                        version: job.template_version,
                    });
                }
            }
            Ok(RenderOutput {
                bytes: Bytes::from_static(BYTES),
                digest: DIGEST,
                format: OutputFormat::Png,
                settings_used: EncodeSettings {
                    size: job.size,
                    quality: None,
                    fps: None,
                    frames: 1,
                },
            })
        })
    }

    fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

#[derive(SlashCommand)]
#[command("card", description = "Show the card")]
struct ShowCard;

impl ShowCard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let image = match ctx
            .render(Card {
                name: "Ada".to_owned(),
            })
            .await
        {
            Ok(image) => image,
            Err(err) => {
                let kind = match err.as_error().downcast_ref::<RenderError>() {
                    Some(RenderError::NoRenderer) => "no-renderer",
                    Some(RenderError::QueueFull) => "busy",
                    Some(_) => "render-error",
                    None => "other",
                };
                return ctx.reply_text(kind).await;
            }
        };
        let mut files = AttachmentSet::new();
        let image = files.add(image);
        let gallery = MediaGallery::new([MediaItem::new(image).alt("Card")]);
        ctx.respond(V2Message::new(Components::new().push(gallery)).attachments(files))
            .await
    }
}

/// The reports of the Render sink.
#[derive(Default)]
struct Reports(Mutex<Vec<String>>);

impl Reports {
    fn lines(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl ErrorSink for Reports {
    fn report(&self, report: ErrorReport<'_>) {
        let kind = match report.kind {
            ReportKind::Error(error) => format!("error:{error}"),
            ReportKind::Panic(message) => format!("panic:{message}"),
            _ => "other".to_owned(),
        };
        self.0.lock().unwrap().push(format!(
            "{} {kind} {}",
            report.origin,
            report.handler.unwrap_or("-")
        ));
    }
}

async fn start(answer: Option<Answer>, reports: &Arc<Reports>, deadline: Duration) -> TestApp {
    let mut builder = App::builder();
    builder
        .ack_deadline(deadline)
        .error_sink(ErrorOrigin::Render, Arc::clone(reports))
        .module(Module::new("cards").add::<ShowCard>());
    if let Some(answer) = answer {
        builder.renderer(Arc::new(Fake(answer)));
    }
    TestApp::build(&mut builder).await.unwrap()
}

/// The digest-named file: 16 hex digits of the digest plus `.png`.
fn file_name() -> String {
    format!("{}.png", "5a".repeat(8))
}

fn calls(response: &Response, suffix: &str) -> Vec<HttpCall> {
    response
        .http_calls()
        .iter()
        .filter(|c| c.path().ends_with(suffix))
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_renderer_render_is_no_renderer_and_unreported() -> Result<(), TestError> {
    let reports = Arc::new(Reports::default());
    let app = start(None, &reports, Duration::from_secs(2)).await;
    let response = app.command("card").run().await?;
    assert_eq!(response.text(), "no-renderer");
    assert!(reports.lines().is_empty(), "{:?}", reports.lines());
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fast_render_is_uploaded_with_the_reply() -> Result<(), TestError> {
    let reports = Arc::new(Reports::default());
    let app = start(Some(Answer::Now), &reports, Duration::from_secs(2)).await;
    let response = app.command("card").run().await?;
    let callbacks = calls(&response, "/callback");
    assert_eq!(callbacks.len(), 1, "{:?}", response.http_calls());
    assert_eq!(callbacks[0].json().unwrap()["type"], 4);
    let files = callbacks[0].files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "files[0]");
    assert_eq!(files[0].filename, file_name());
    assert_eq!(&files[0].bytes[..], BYTES);
    assert!(reports.lines().is_empty());
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_render_is_auto_deferred_then_edited_in() -> Result<(), TestError> {
    let reports = Arc::new(Reports::default());
    let app = start(
        Some(Answer::After(Duration::from_millis(300))),
        &reports,
        Duration::from_millis(50),
    )
    .await;
    let response = app.command("card").run().await?;
    let callbacks = calls(&response, "/callback");
    assert_eq!(callbacks.len(), 1, "{:?}", response.http_calls());
    assert_eq!(
        *callbacks[0].json().unwrap(),
        serde_json::json!({ "type": 5 })
    );
    let edits = calls(&response, "/messages/@original");
    assert_eq!(edits.len(), 1, "{:?}", response.http_calls());
    assert_eq!(edits[0].method(), "PATCH");
    let files = edits[0].files();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].filename, file_name());
    app.stop().await
}

#[tokio::test(flavor = "multi_thread")]
async fn render_failures_are_reported_once_each() -> Result<(), TestError> {
    for (answer, expected) in [
        (Answer::Failed, "render error:render failed card"),
        (
            Answer::Unknown,
            "render error:unknown template `card` version 1 card",
        ),
    ] {
        let reports = Arc::new(Reports::default());
        let app = start(Some(answer), &reports, Duration::from_secs(2)).await;
        let response = app.command("card").run().await?;
        assert_eq!(response.text(), "render-error");
        assert_eq!(reports.lines(), [expected]);
        app.stop().await?;
    }
    Ok(())
}
