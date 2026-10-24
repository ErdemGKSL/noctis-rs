//! `ctx.render`, `render_with` and `render_report`: options, the fps cap,
//! the report, `NoRenderer`, the metrics, the span and the Render Error
//! Sink, through a fake Renderer.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_ports::{
    AnimatedFormat, AnimationSpec, BoxFuture, EncodeSettings, ErasedView, Format, OutputFormat,
    RenderOutput, Size, TemplateDescriptor,
};
use serde::{Deserialize, Serialize};
use twilight_model::id::Id;

use super::*;
use crate::responder::Ephemerality;
use crate::test_support::{Capture, Recording};
use crate::{App, Runtime};
use noctis_rules::Limits;

#[derive(Debug, Serialize, Deserialize)]
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

/// An animated Template: 1 s at 10 fps, the default format.
#[derive(Debug, Serialize, Deserialize)]
struct Reel;

const REEL: AnimationSpec = AnimationSpec::new(Duration::from_secs(1), 10, AnimatedFormat::DEFAULT);

impl Template for Reel {
    const ID: &'static str = "reel";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(8, 8);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = Some(REEL);

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

fn card() -> Card {
    Card {
        name: "Ada".to_owned(),
    }
}

/// What the fake answers.
#[derive(Clone, Copy)]
enum Answer {
    Ok,
    QueueFull,
    Unknown,
    Unsupported,
    Failed,
    Panicked,
    OverBudget,
    OverCap,
    SizeMismatch,
}

/// A Renderer answering `answer` at once; it snapshots the metrics and
/// logs one event while it renders.
struct Fake {
    answer: Answer,
    snapshotter: Option<Snapshotter>,
    /// `noctis_render_queue_depth` while rendering.
    depth: Mutex<Option<f64>>,
    /// The `upload_budget` of the last job it received.
    budget: Mutex<Option<u64>>,
    /// The last job it received.
    job: Mutex<Option<RenderJob>>,
    /// How often it was called.
    calls: AtomicUsize,
}

impl Fake {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Self::with(answer, None))
    }

    fn with(answer: Answer, snapshotter: Option<Snapshotter>) -> Self {
        Self {
            answer,
            snapshotter,
            depth: Mutex::new(None),
            budget: Mutex::new(None),
            job: Mutex::new(None),
            calls: AtomicUsize::new(0),
        }
    }
}

/// The settings the fake's Ok answer reports for a job of `size`.
fn fake_settings(size: Size) -> EncodeSettings {
    EncodeSettings {
        size,
        quality: None,
        fps: None,
        frames: 1,
    }
}

impl Renderer for Fake {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        tracing::info!(template = job.template_id, "fake renders");
        *self.budget.lock().unwrap() = Some(job.upload_budget);
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.job.lock().unwrap() = Some(job.clone());
        if let Some(snapshotter) = &self.snapshotter {
            *self.depth.lock().unwrap() = gauge(snapshotter, "noctis_render_queue_depth");
        }
        let result = match self.answer {
            Answer::Ok => {
                let bytes = Bytes::from_static(b"\x89PNG fake");
                Ok(RenderOutput {
                    digest: [0xAB; 32],
                    bytes,
                    format: OutputFormat::Png,
                    settings_used: fake_settings(job.size),
                })
            }
            Answer::QueueFull => Err(RenderError::QueueFull),
            Answer::Unknown => Err(RenderError::UnknownTemplate {
                id: "card".to_owned(),
                version: 1,
            }),
            Answer::Unsupported => Err(RenderError::Unsupported {
                id: "card".to_owned(),
                why: "no".into(),
            }),
            Answer::Failed => Err(RenderError::failed("fake failure")),
            Answer::Panicked => Err(RenderError::Panicked {
                message: "boom".to_owned(),
            }),
            Answer::OverBudget => Err(RenderError::OverBudget {
                budget: 10,
                smallest: 20,
            }),
            Answer::OverCap => Err(RenderError::OverCap {
                id: "card".to_owned(),
                cap: noctis_ports::RenderCap::Frames,
                limit: 90,
                actual: 91,
            }),
            Answer::SizeMismatch => Err(RenderError::SizeMismatch {
                id: "card".to_owned(),
                frame: 3,
                expected: job.size,
                declared: (1, 1),
            }),
        };
        Box::pin(async move { result })
    }

    fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

/// A test App with `renderer` and a recording Render sink.
fn app(renderer: Option<Arc<Fake>>, sink: &Arc<Recording>) -> App {
    app_with(renderer, sink, Limits::default())
}

/// [`app`] with `limits`.
fn app_with(renderer: Option<Arc<Fake>>, sink: &Arc<Recording>, limits: Limits) -> App {
    let mut builder = App::builder();
    builder.limits(limits);
    builder
        .runtime(&Runtime::for_test())
        .error_sink(ErrorOrigin::Render, Arc::clone(sink));
    if let Some(renderer) = renderer {
        builder.renderer(renderer);
    }
    builder.build().unwrap()
}

/// The parts of a `/card show` run in guild 5, Module `cards`, on `app`.
fn parts(app: &App) -> CtxInner {
    CtxInner {
        path: "card show",
        module: "cards",
        shard: 2,
        guild_id: Some(Id::new(5)),
        app: Some(Arc::clone(app.shared())),
        ..CtxInner::detached_parts(Ephemerality::Public)
    }
}

/// One snapshot row: name, labels and value.
type Sample = (String, Vec<(String, String)>, DebugValue);

/// The value of the metric `name` in a fresh snapshot (which resets
/// gauges and counters to 0).
fn snapshot_of(snapshotter: &Snapshotter) -> Vec<Sample> {
    snapshotter
        .snapshot()
        .into_vec()
        .into_iter()
        .map(|(key, _, _, value)| {
            let labels = key
                .key()
                .labels()
                .map(|l| (l.key().to_owned(), l.value().to_owned()))
                .collect();
            (key.key().name().to_owned(), labels, value)
        })
        .collect()
}

fn gauge(snapshotter: &Snapshotter, name: &str) -> Option<f64> {
    snapshot_of(snapshotter)
        .into_iter()
        .find_map(|(n, _, value)| match value {
            DebugValue::Gauge(v) if n == name => Some(v.0),
            _ => None,
        })
}

fn render_error(error: &HandlerError) -> &RenderError {
    error
        .as_error()
        .downcast_ref::<RenderError>()
        .expect("a RenderError")
}

#[tokio::test]
async fn no_renderer_is_a_typed_error_without_report() {
    let sink = Arc::new(Recording::default());
    let app = app(None, &sink);
    let err = render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap_err();
    assert!(matches!(render_error(&err), RenderError::NoRenderer));
    // A detached context (no App) too.
    let detached = CtxInner::detached_parts(Ephemerality::Public);
    let err = render(&detached, card(), RenderOptions::new())
        .await
        .unwrap_err();
    assert!(matches!(render_error(&err), RenderError::NoRenderer));
    assert!(sink.seen().is_empty());
}

#[tokio::test]
async fn ok_records_the_duration_and_the_depth() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let fake = Arc::new(Fake::with(Answer::Ok, Some(snapshotter.clone())));
    let sink = Arc::new(Recording::default());
    let app = app(Some(Arc::clone(&fake)), &sink);
    let attachment = render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap()
        .attachment;
    assert_eq!(attachment.name(), format!("{}.png", "ab".repeat(8)));
    assert_eq!(attachment.origin(), noctis_ui::AttachmentOrigin::Rendered);
    // +1 while rendering (that snapshot reset the gauge to 0), then -1.
    assert_eq!(*fake.depth.lock().unwrap(), Some(1.0));
    let snapshot = snapshot_of(&snapshotter);
    let depth = snapshot
        .iter()
        .find(|(n, ..)| n == "noctis_render_queue_depth")
        .unwrap();
    assert_eq!(depth.1, [("app".to_owned(), "default".to_owned())]);
    assert!(matches!(depth.2, DebugValue::Gauge(v) if (v.0 + 1.0).abs() < f64::EPSILON));
    let duration = snapshot
        .iter()
        .find(|(n, ..)| n == "noctis_render_duration_seconds")
        .unwrap();
    assert_eq!(
        duration.1,
        [
            ("app".to_owned(), "default".to_owned()),
            ("template".to_owned(), "card".to_owned())
        ]
    );
    assert!(matches!(&duration.2, DebugValue::Histogram(samples) if samples.len() == 1));
    assert!(
        !snapshot
            .iter()
            .any(|(n, ..)| n == "noctis_render_rejected_total")
    );
    assert!(sink.seen().is_empty());
}

#[tokio::test]
async fn rejections_count_by_reason_and_queue_full_is_not_reported() {
    for (answer, reason, reported) in [
        (Answer::QueueFull, "queue_full", false),
        (Answer::Unknown, "unknown_template", true),
        (Answer::Unsupported, "unsupported", true),
        (Answer::OverCap, "over_cap", true),
    ] {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let sink = Arc::new(Recording::default());
        let app = app(Some(Fake::new(answer)), &sink);
        let err = render(&parts(&app), card(), RenderOptions::new())
            .await
            .unwrap_err();
        let snapshot = snapshot_of(&snapshotter);
        let rejected: Vec<_> = snapshot
            .iter()
            .filter(|(n, ..)| n == "noctis_render_rejected_total")
            .collect();
        assert_eq!(rejected.len(), 1, "{reason}");
        assert_eq!(
            rejected[0].1,
            [
                ("app".to_owned(), "default".to_owned()),
                ("reason".to_owned(), reason.to_owned())
            ]
        );
        assert!(matches!(rejected[0].2, DebugValue::Counter(1)), "{reason}");
        assert!(
            !snapshot
                .iter()
                .any(|(n, ..)| n == "noctis_render_duration_seconds")
        );
        let seen = sink.seen();
        assert_eq!(seen.len(), usize::from(reported), "{reason}: {seen:?}");
        if reported {
            assert_eq!(seen[0].kind, format!("error:{err}"));
        }
    }
}

#[tokio::test]
async fn failures_and_panics_reach_the_render_sink() {
    for (answer, kind) in [
        (Answer::Failed, "error:render failed"),
        (Answer::Panicked, "panic:boom"),
    ] {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let sink = Arc::new(Recording::default());
        let app = app(Some(Fake::new(answer)), &sink);
        let err = render(&parts(&app), card(), RenderOptions::new())
            .await
            .unwrap_err();
        let snapshot = snapshot_of(&snapshotter);
        assert!(!snapshot.iter().any(|(n, ..)| {
            n == "noctis_render_rejected_total" || n == "noctis_render_duration_seconds"
        }));
        let depth = snapshot
            .iter()
            .find(|(n, ..)| n == "noctis_render_queue_depth")
            .unwrap();
        assert!(matches!(depth.2, DebugValue::Gauge(v) if v.0.abs() < f64::EPSILON));
        match answer {
            Answer::Panicked => assert!(matches!(
                render_error(&err),
                RenderError::Panicked { message } if message == "boom"
            )),
            _ => assert!(matches!(render_error(&err), RenderError::Failed(_))),
        }
        let seen = sink.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        let report = &seen[0];
        assert_eq!(report.origin, ErrorOrigin::Render);
        assert_eq!(report.kind, kind);
        assert_eq!(report.app, "default");
        assert_eq!(report.shard, Some(2));
        assert_eq!(report.guild_id, Some(5));
        assert_eq!(report.module.as_deref(), Some("cards"));
        assert_eq!(report.handler.as_deref(), Some("card show"));
        assert_eq!(report.event_type, None);
    }
}

#[tokio::test]
async fn renders_run_in_one_noctis_render_span() {
    let capture = Capture::default();
    let _guard = tracing::subscriber::set_default(capture.subscriber());
    let sink = Arc::new(Recording::default());
    let app = app(Some(Fake::new(Answer::Ok)), &sink);
    render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap();
    let lines = capture.lines_with(&["fake renders"]);
    assert_eq!(lines.len(), 1, "{}", capture.text());
    assert!(
        lines[0].contains("noctis.render{app=default template=\"card\"}")
            || lines[0].contains("noctis.render{app=default template=card}"),
        "{}",
        lines[0]
    );
}

#[tokio::test]
async fn a_detached_render_gets_the_app_attachment_limit() {
    let sink = Arc::new(Recording::default());
    let fake = Fake::new(Answer::Ok);
    let app = app(Some(Arc::clone(&fake)), &sink);
    render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap();
    assert_eq!(*fake.budget.lock().unwrap(), Some(20_971_520));

    let fake = Fake::new(Answer::Ok);
    let mut limits = Limits::default();
    limits.attachment_bytes = 1000;
    let app = app_with(Some(Arc::clone(&fake)), &sink, limits);
    render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap();
    assert_eq!(*fake.budget.lock().unwrap(), Some(1000));
}

#[tokio::test]
async fn over_budget_is_reported_without_a_rejected_reason() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let sink = Arc::new(Recording::default());
    let app = app(Some(Fake::new(Answer::OverBudget)), &sink);
    let err = render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap_err();
    assert!(matches!(
        render_error(&err),
        RenderError::OverBudget {
            budget: 10,
            smallest: 20
        }
    ));
    let snapshot = snapshot_of(&snapshotter);
    assert!(
        !snapshot
            .iter()
            .any(|(n, ..)| n == "noctis_render_rejected_total")
    );
    let seen = sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].origin, ErrorOrigin::Render);
    assert_eq!(seen[0].kind, format!("error:{err}"));
}

#[tokio::test]
async fn options_set_the_animation_and_default_to_the_template_s() {
    assert_eq!(RenderOptions::default(), RenderOptions::new());
    let sink = Arc::new(Recording::default());
    let fake = Fake::new(Answer::Ok);
    let app = app(Some(Arc::clone(&fake)), &sink);
    let spec = AnimationSpec::new(
        Duration::from_secs(1),
        20,
        AnimatedFormat::Webp { quality: 80 },
    );
    render(&parts(&app), card(), RenderOptions::new().animation(spec))
        .await
        .unwrap();
    let job = fake.job.lock().unwrap().take().unwrap();
    assert_eq!(job.animation, Some(spec));
    render(&parts(&app), Reel, RenderOptions::new())
        .await
        .unwrap();
    let job = fake.job.lock().unwrap().take().unwrap();
    assert_eq!(job.animation, Some(REEL));
    render(&parts(&app), Reel, RenderOptions::new().animation(spec))
        .await
        .unwrap();
    let job = fake.job.lock().unwrap().take().unwrap();
    assert_eq!(job.animation, Some(spec));
}

/// An animated Template whose own `ANIMATION` is over the GIF fps cap.
#[derive(Debug, Serialize, Deserialize)]
struct GifReel;

impl Template for GifReel {
    const ID: &'static str = "gif_reel";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(8, 8);
    const FORMAT: Format = Format::Png;
    const ANIMATION: Option<AnimationSpec> = Some(AnimationSpec::new(
        Duration::from_secs(1),
        60,
        AnimatedFormat::Gif,
    ));

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

#[tokio::test]
async fn a_template_animation_over_the_fps_cap_never_reaches_the_renderer() {
    let sink = Arc::new(Recording::default());
    let fake = Fake::new(Answer::Ok);
    let app = app(Some(Arc::clone(&fake)), &sink);
    let err = render(&parts(&app), GifReel, RenderOptions::new())
        .await
        .unwrap_err();
    assert!(
        matches!(
            render_error(&err),
            RenderError::OverCap {
                id,
                cap: noctis_ports::RenderCap::Fps,
                limit: 50,
                actual: 60,
            } if id == "gif_reel"
        ),
        "{err}"
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_fps_over_the_format_cap_never_reaches_the_renderer() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let sink = Arc::new(Recording::default());
    let fake = Fake::new(Answer::Ok);
    let app = app(Some(Arc::clone(&fake)), &sink);
    let gif = AnimationSpec::new(Duration::from_secs(1), 60, AnimatedFormat::Gif);
    let err = render(&parts(&app), card(), RenderOptions::new().animation(gif))
        .await
        .unwrap_err();
    assert!(
        matches!(
            render_error(&err),
            RenderError::OverCap {
                id,
                cap: noctis_ports::RenderCap::Fps,
                limit: 50,
                actual: 60,
            } if id == "card"
        ),
        "{err}"
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    let snapshot = snapshot_of(&snapshotter);
    let rejected: Vec<_> = snapshot
        .iter()
        .filter(|(n, ..)| n == "noctis_render_rejected_total")
        .collect();
    assert_eq!(rejected.len(), 1);
    assert!(
        rejected[0]
            .1
            .contains(&("reason".to_owned(), "over_cap".to_owned()))
    );
    assert!(matches!(rejected[0].2, DebugValue::Counter(1)));
    assert_eq!(sink.seen().len(), 1);
}

#[tokio::test]
async fn render_report_returns_the_format_and_settings() {
    let sink = Arc::new(Recording::default());
    let app = app(Some(Fake::new(Answer::Ok)), &sink);
    let report = render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap();
    assert_eq!(report.attachment.name(), format!("{}.png", "ab".repeat(8)));
    assert_eq!(report.format, OutputFormat::Png);
    assert_eq!(report.settings_used, fake_settings(Card::SIZE));
}

#[tokio::test]
async fn a_size_mismatch_is_reported_without_a_rejected_reason() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let sink = Arc::new(Recording::default());
    let app = app(Some(Fake::new(Answer::SizeMismatch)), &sink);
    let err = render(&parts(&app), card(), RenderOptions::new())
        .await
        .unwrap_err();
    assert!(matches!(
        render_error(&err),
        RenderError::SizeMismatch { frame: 3, .. }
    ));
    let snapshot = snapshot_of(&snapshotter);
    assert!(
        !snapshot
            .iter()
            .any(|(n, ..)| n == "noctis_render_rejected_total")
    );
    let seen = sink.seen();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].kind, format!("error:{err}"));
}
