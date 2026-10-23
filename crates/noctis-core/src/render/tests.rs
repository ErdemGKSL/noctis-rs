//! `ctx.render`: `NoRenderer`, the metrics, the span and the Render Error
//! Sink, through a fake Renderer.

use std::sync::Mutex;

use bytes::Bytes;
use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use noctis_ports::{
    AnimationSpec, BoxFuture, EncodeSettings, ErasedView, Format, OutputFormat, RenderOutput, Size,
    TemplateDescriptor,
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
}

impl Fake {
    fn new(answer: Answer) -> Arc<Self> {
        Arc::new(Self {
            answer,
            snapshotter: None,
            depth: Mutex::new(None),
            budget: Mutex::new(None),
        })
    }
}

impl Renderer for Fake {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        tracing::info!(template = job.template_id, "fake renders");
        *self.budget.lock().unwrap() = Some(job.upload_budget);
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
                    settings_used: EncodeSettings {
                        size: job.size,
                        quality: None,
                        fps: None,
                        frames: 1,
                    },
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
    let err = render(&parts(&app), card()).await.unwrap_err();
    assert!(matches!(render_error(&err), RenderError::NoRenderer));
    // A detached context (no App) too.
    let detached = CtxInner::detached_parts(Ephemerality::Public);
    let err = render(&detached, card()).await.unwrap_err();
    assert!(matches!(render_error(&err), RenderError::NoRenderer));
    assert!(sink.seen().is_empty());
}

#[tokio::test]
async fn ok_records_the_duration_and_the_depth() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let fake = Arc::new(Fake {
        answer: Answer::Ok,
        snapshotter: Some(snapshotter.clone()),
        depth: Mutex::new(None),
        budget: Mutex::new(None),
    });
    let sink = Arc::new(Recording::default());
    let app = app(Some(Arc::clone(&fake)), &sink);
    let attachment = render(&parts(&app), card()).await.unwrap();
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
    ] {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let sink = Arc::new(Recording::default());
        let app = app(Some(Fake::new(answer)), &sink);
        let err = render(&parts(&app), card()).await.unwrap_err();
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
        let err = render(&parts(&app), card()).await.unwrap_err();
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
    render(&parts(&app), card()).await.unwrap();
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
    render(&parts(&app), card()).await.unwrap();
    assert_eq!(*fake.budget.lock().unwrap(), Some(20_971_520));

    let fake = Fake::new(Answer::Ok);
    let mut limits = Limits::default();
    limits.attachment_bytes = 1000;
    let app = app_with(Some(Arc::clone(&fake)), &sink, limits);
    render(&parts(&app), card()).await.unwrap();
    assert_eq!(*fake.budget.lock().unwrap(), Some(1000));
}

#[tokio::test]
async fn over_budget_is_reported_without_a_rejected_reason() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let _guard = metrics::set_default_local_recorder(&recorder);
    let sink = Arc::new(Recording::default());
    let app = app(Some(Fake::new(Answer::OverBudget)), &sink);
    let err = render(&parts(&app), card()).await.unwrap_err();
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
