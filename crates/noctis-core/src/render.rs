//! `ctx.render`, `ctx.render_with` and `ctx.render_report`: a Template value
//! through the App's Renderer into an [`Attachment`] (FR-34, FR-69, AD-22,
//! AD-29).
//!
//! Core owns the observability of a render, because the Renderer port knows
//! no App: the `noctis.render{app, template}` span, the
//! `noctis_render_duration_seconds{app, template}`,
//! `noctis_render_queue_depth{app}` and
//! `noctis_render_rejected_total{app, reason}` metrics, and the Render Error
//! Sink reports.

use std::sync::Arc;
use std::time::Instant;

use noctis_ports::{
    AnimationSpec, EncodeSettings, OutputFormat, RenderError, RenderJob, Renderer, Template,
};
use noctis_ui::Attachment;
use tracing::Instrument as _;

use crate::command::CtxInner;
use crate::handler::AppShared;
use crate::{ErrorOrigin, ErrorReport, HandlerError, HandlerResult, ReportKind};

/// Per-call options of `ctx.render_with` and `ctx.render_report`.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_core::RenderOptions;
/// use noctis_ports::{AnimatedFormat, AnimationSpec};
///
/// let spin = AnimationSpec::new(Duration::from_secs(1), 20, AnimatedFormat::DEFAULT);
/// let options = RenderOptions::new().animation(spin);
/// assert_ne!(options, RenderOptions::default());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    animation: Option<AnimationSpec>,
}

impl RenderOptions {
    /// No options: the Template renders as it declares (its `ANIMATION`,
    /// if any).
    pub const fn new() -> Self {
        Self { animation: None }
    }

    /// Renders this call as `spec`, in place of the Template's own
    /// `ANIMATION` (or its still image). The view is called once per frame
    /// with `Frame { index, count }`; `spec.fps` must be within the
    /// format's cap ([`AnimatedFormat::max_fps`](noctis_ports::AnimatedFormat::max_fps)).
    #[must_use]
    pub const fn animation(self, spec: AnimationSpec) -> Self {
        Self {
            animation: Some(spec),
        }
    }
}

/// What `ctx.render_report` returns: the attachment and how it was encoded.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RenderReport {
    /// The rendered image, named by its render digest.
    pub attachment: Attachment,
    /// The encoded format.
    pub format: OutputFormat,
    /// The settings the output was encoded with, after any degradation to
    /// fit the size budget.
    pub settings_used: EncodeSettings,
}

/// Renders `template` through the App's Renderer with `options`.
///
/// Without an App or a Renderer it fails with [`RenderError::NoRenderer`]
/// (no metric, no report). An animation over its format's fps cap fails
/// with [`RenderError::OverCap`] before the Renderer is called. Every other
/// failure is a [`RenderError`] inside the returned [`HandlerError`]; all
/// of them except `QueueFull` are reported to the [`ErrorOrigin::Render`]
/// sink.
pub(crate) async fn render<T: Template>(
    inner: &CtxInner,
    template: T,
    options: RenderOptions,
) -> HandlerResult<RenderReport> {
    let Some((app, renderer)) = inner
        .app
        .as_ref()
        .and_then(|app| app.renderer.as_ref().map(|renderer| (app, renderer)))
    else {
        return Err(HandlerError::new(RenderError::NoRenderer));
    };
    let span = tracing::info_span!("noctis.render", app = %app.app, template = T::ID);
    async {
        let result = match RenderJob::of(&template) {
            Ok(mut job) => {
                // The render fits what this interaction's upload accepts (ATT-01).
                let budget = inner
                    .shared
                    .as_ref()
                    .map_or(app.limits.attachment_bytes, |s| s.upload_budget());
                job.upload_budget = u64::try_from(budget).unwrap_or(u64::MAX);
                if let Some(spec) = options.animation {
                    job.animation = Some(spec);
                }
                match job.animation.map(|spec| spec.check_fps(T::ID)) {
                    Some(Err(error)) => Err(error),
                    _ => render_job(&app.app, renderer.as_ref(), T::ID, job).await,
                }
            }
            Err(error) => Err(RenderError::failed(error)),
        };
        result.map_err(|error| fail(inner, app, error))
    }
    .instrument(span)
    .await
}

/// Renders `job` and wraps the output, measuring the render.
async fn render_job(
    app: &Arc<str>,
    renderer: &dyn Renderer,
    template: &'static str,
    job: RenderJob,
) -> Result<RenderReport, RenderError> {
    let started = Instant::now();
    let output = {
        let _in_flight = InFlight::enter(app);
        renderer.render(job).await?
    };
    metrics::histogram!(
        "noctis_render_duration_seconds",
        "app" => Arc::clone(app),
        "template" => template
    )
    .record(started.elapsed().as_secs_f64());
    let attachment = Attachment::rendered(output.bytes, output.digest, output.format.extension())
        .map_err(RenderError::failed)?;
    Ok(RenderReport {
        attachment,
        format: output.format,
        settings_used: output.settings_used,
    })
}

/// Counts one render in `noctis_render_queue_depth{app}` while alive, so a
/// dropped render future leaves the gauge right.
struct InFlight(Arc<str>);

impl InFlight {
    fn enter(app: &Arc<str>) -> Self {
        metrics::gauge!("noctis_render_queue_depth", "app" => Arc::clone(app)).increment(1.0);
        Self(Arc::clone(app))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        metrics::gauge!("noctis_render_queue_depth", "app" => Arc::clone(&self.0)).decrement(1.0);
    }
}

/// Counts and reports `error`, then wraps it for the handler. The rejected
/// reasons are `queue_full`, `unknown_template`, `unsupported` and
/// `over_cap`; every error but `QueueFull` and `NoRenderer` is reported.
/// `OverBudget` and `SizeMismatch` are handled like `Failed`: reported, with
/// no rejected-metric reason.
fn fail(inner: &CtxInner, app: &AppShared, error: RenderError) -> HandlerError {
    let reason = match &error {
        RenderError::QueueFull => Some("queue_full"),
        RenderError::UnknownTemplate { .. } => Some("unknown_template"),
        RenderError::Unsupported { .. } => Some("unsupported"),
        RenderError::OverCap { .. } => Some("over_cap"),
        _ => None,
    };
    if let Some(reason) = reason {
        metrics::counter!(
            "noctis_render_rejected_total",
            "app" => Arc::clone(&app.app),
            "reason" => reason
        )
        .increment(1);
    }
    let panic = match &error {
        RenderError::Panicked { message } => Some(message.clone()),
        _ => None,
    };
    let reported = !matches!(error, RenderError::QueueFull | RenderError::NoRenderer);
    let error = HandlerError::new(error);
    if reported {
        let kind = match &panic {
            Some(message) => ReportKind::Panic(message),
            None => ReportKind::Error(&error),
        };
        app.sinks.report(ErrorReport {
            origin: ErrorOrigin::Render,
            kind,
            app: &app.app,
            shard: Some(inner.shard),
            guild_id: inner.guild_id,
            module: Some(inner.module),
            handler: Some(inner.path),
            event_type: None,
            extensions: Some(inner.extensions()),
            middleware: None,
        });
    }
    error
}

#[cfg(test)]
mod tests;
