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
use twilight_model::id::Id;
use twilight_model::id::marker::GuildMarker;

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
///
/// // An application emoji is at most 256 KiB.
/// let emoji = RenderOptions::new().max_bytes(262_144);
/// assert_ne!(emoji, RenderOptions::default());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderOptions {
    animation: Option<AnimationSpec>,
    max_bytes: Option<u64>,
}

impl RenderOptions {
    /// No options: the Template renders as it declares (its `ANIMATION`,
    /// if any), within the call's Upload Budget and the App's
    /// `size_target`.
    pub const fn new() -> Self {
        Self {
            animation: None,
            max_bytes: None,
        }
    }

    /// Renders this call as `spec`, in place of the Template's own
    /// `ANIMATION` (or its still image). The view is called once per frame
    /// with `Frame { index, count }`; `spec.fps` must be within the
    /// format's cap ([`AnimatedFormat::max_fps`](noctis_ports::AnimatedFormat::max_fps)).
    #[must_use]
    pub const fn animation(self, spec: AnimationSpec) -> Self {
        Self {
            animation: Some(spec),
            ..self
        }
    }

    /// Caps this call's output at `bytes` (FR-33, ATT-01): the job's budget
    /// is `min(bytes, the call's Upload Budget, the App's size_target)`, and
    /// the Renderer degrades fps, then size, then quality, then falls back
    /// to a still frame until the output fits, exactly as for the App's
    /// budget. A render that cannot fit fails with
    /// [`RenderError::OverBudget`]. Use it for a render meant for a 256 KiB
    /// application emoji or a small thumbnail without lowering the whole
    /// App's `size_target`.
    ///
    /// An animation that only fits as its still fallback comes back with
    /// `settings_used.frames == 1` and the still format: read
    /// [`RenderReport`] to refuse it.
    ///
    /// `max_bytes(0)` is never "no limit": the render fails with
    /// [`RenderError::Unsupported`] before the Renderer is called.
    #[must_use]
    pub const fn max_bytes(self, bytes: u64) -> Self {
        Self {
            max_bytes: Some(bytes),
            ..self
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

/// Where a render runs: the App, the Upload Budget and the Render Error
/// Sink report fields of the calling context.
pub(crate) struct RenderSite<'a> {
    /// `None` for the detached `__testing` seams (`NoRenderer`).
    pub(crate) app: Option<&'a Arc<AppShared>>,
    /// The interaction's Upload Budget; `None` outside an interaction, where
    /// the App's `Limits::attachment_bytes` applies.
    pub(crate) upload_budget: Option<usize>,
    pub(crate) shard: Option<u32>,
    pub(crate) guild_id: Option<Id<GuildMarker>>,
    pub(crate) module: Option<&'static str>,
    pub(crate) handler: Option<&'static str>,
    pub(crate) extensions: Option<&'a crate::Extensions>,
}

impl<'a> RenderSite<'a> {
    /// A render of `app` outside any run (`App::render`).
    pub(crate) fn app(app: &'a Arc<AppShared>) -> Self {
        Self {
            app: Some(app),
            upload_budget: None,
            shard: None,
            guild_id: None,
            module: None,
            handler: None,
            extensions: None,
        }
    }
}

/// Renders `template` through the App's Renderer with `options`, from an
/// interaction context.
pub(crate) async fn render<T: Template>(
    inner: &CtxInner,
    template: T,
    options: RenderOptions,
) -> HandlerResult<RenderReport> {
    let site = RenderSite {
        app: inner.app.as_ref(),
        upload_budget: inner.shared.as_ref().map(|s| s.upload_budget()),
        shard: Some(inner.shard),
        guild_id: inner.guild_id,
        module: Some(inner.module),
        handler: Some(inner.path),
        extensions: Some(inner.extensions()),
    };
    render_at(site, template, options).await
}

/// Renders `template` through the App's Renderer with `options`; every
/// context's `render`, `render_with` and `render_report` lands here.
///
/// Without an App or a Renderer it fails with [`RenderError::NoRenderer`]
/// (no metric, no report). An animation over its format's fps cap fails
/// with [`RenderError::OverCap`] before the Renderer is called; an empty
/// output fails with [`RenderError::Failed`] and one over the job's Upload
/// Budget with [`RenderError::OverBudget`]. Every other
/// failure is a [`RenderError`] inside the returned [`HandlerError`]; all
/// of them except `QueueFull` are reported to the [`ErrorOrigin::Render`]
/// sink.
pub(crate) async fn render_at<T: Template>(
    site: RenderSite<'_>,
    template: T,
    options: RenderOptions,
) -> HandlerResult<RenderReport> {
    let Some((app, renderer)) = site
        .app
        .and_then(|app| app.renderer.as_ref().map(|renderer| (app, renderer)))
    else {
        return Err(HandlerError::new(RenderError::NoRenderer));
    };
    let span = tracing::info_span!("noctis.render", app = %app.app, template = T::ID);
    async {
        let result = match RenderJob::of(&template) {
            Ok(mut job) => {
                // The render fits what this interaction's upload accepts, or
                // the App's attachment limit outside one (ATT-01), and the
                // call's own cap; the Renderer folds in its `size_target`.
                let budget = site.upload_budget.unwrap_or(app.limits.attachment_bytes);
                job.upload_budget = job_budget(budget, options.max_bytes);
                if let Some(spec) = options.animation {
                    job.animation = Some(spec);
                }
                if options.max_bytes == Some(0) {
                    Err(RenderError::Unsupported {
                        id: T::ID.to_owned(),
                        why: "RenderOptions::max_bytes(0) leaves no room for any output; \
                              omit max_bytes for the default budget"
                            .into(),
                    })
                } else {
                    match job.animation.map(|spec| spec.check_fps(T::ID)) {
                        Some(Err(error)) => Err(error),
                        _ => render_job(&app.app, renderer.as_ref(), T::ID, job).await,
                    }
                }
            }
            Err(error) => Err(RenderError::failed(error)),
        };
        result.map_err(|error| fail(&site, app, error))
    }
    .instrument(span)
    .await
}

/// The job's `upload_budget`: the call's Upload Budget, capped by
/// `RenderOptions::max_bytes` when it is set (FR-33).
fn job_budget(upload_budget: usize, max_bytes: Option<u64>) -> u64 {
    let upload = u64::try_from(upload_budget).unwrap_or(u64::MAX);
    max_bytes.map_or(upload, |cap| cap.min(upload))
}

/// `render`, `render_with` and `render_report` of a context outside an
/// interaction (`EventCtx`, `HookCtx`): the `impl` block calls
/// `self.render_site()` for its [`RenderSite`].
macro_rules! scope_render_methods {
    ($ty:ident, $import:literal) => {
        impl $ty {
            /// Renders `template` through the App's Renderer (feature
            /// `image`: `.theme(..)` or `.image(..)`) into an
            /// [`Attachment`](noctis_ui::Attachment) named by its render
            /// digest, exactly as `CommandCtx::render` does: the same
            /// `noctis.render{app, template}` span, the
            /// `noctis_render_duration_seconds`,
            /// `noctis_render_queue_depth` and
            /// `noctis_render_rejected_total` metrics and the Render Error
            /// Sink. There is no interaction here, so the job's
            /// `upload_budget` is the App's `Limits::attachment_bytes`
            /// (ATT-01). Post the attachment with
            /// `ctx.messages().send(channel, view)`.
            ///
            /// The render is awaited in the handler: dropping the future
            /// (a timeout, the App shutting down) cancels it.
            ///
            /// ```no_run
            /// use noctis_ports::{AnimationSpec, ErasedView, Format, Size, Template};
            /// use noctis_core::HandlerResult;
            #[doc = concat!("use ", $import, ";")]
            /// use noctis_ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};
            /// use twilight_model::id::Id;
            /// use twilight_model::id::marker::ChannelMarker;
            ///
            /// #[derive(serde::Serialize, serde::Deserialize)]
            /// struct Welcome {
            ///     name: String,
            /// }
            ///
            /// impl Template for Welcome {
            ///     const ID: &'static str = "welcome";
            ///     const VERSION: u32 = 1;
            ///     const SIZE: Size = Size::new(600, 200);
            ///     const FORMAT: Format = Format::Png;
            ///     const ANIMATION: Option<AnimationSpec> = None;
            ///     fn view() -> ErasedView {
            ///         ErasedView::new(())
            ///     }
            /// }
            ///
            #[doc = concat!("async fn welcome(ctx: ", stringify!($ty), ", channel: Id<ChannelMarker>) -> HandlerResult<()> {")]
            ///     let mut files = AttachmentSet::new();
            ///     let image = files.add(ctx.render(Welcome { name: "Ada".into() }).await?);
            ///     let gallery = MediaGallery::new([MediaItem::new(image).alt("Welcome")]);
            ///     let msg = V2Message::new(Components::new().push(gallery)).attachments(files);
            ///     ctx.messages().send(channel, msg).await?;
            ///     Ok(())
            /// }
            /// ```
            ///
            /// # Errors
            ///
            /// The errors of `CommandCtx::render`: a
            /// [`HandlerError`](crate::HandlerError) wrapping the
            /// [`RenderError`](noctis_ports::RenderError) (`NoRenderer`
            /// without a Renderer, `QueueFull`, `UnknownTemplate`,
            /// `Unsupported`, `OverCap`, `OverBudget`, `SizeMismatch`,
            /// `UnknownImage`, `RemoteImage`, `Failed`, `Panicked`).
            pub fn render<T: ::noctis_ports::Template>(
                &self,
                template: T,
            ) -> impl ::std::future::Future<
                Output = $crate::HandlerResult<::noctis_ui::Attachment>,
            > + Send
                   + '_ {
                self.render_with(template, $crate::RenderOptions::new())
            }

            /// [`render`](Self::render) with per-call
            /// [`RenderOptions`](crate::RenderOptions) (`.animation(spec)`,
            /// `.max_bytes(n)`).
            ///
            /// # Errors
            ///
            /// The errors of [`render`](Self::render).
            pub fn render_with<T: ::noctis_ports::Template>(
                &self,
                template: T,
                options: $crate::RenderOptions,
            ) -> impl ::std::future::Future<
                Output = $crate::HandlerResult<::noctis_ui::Attachment>,
            > + Send
                   + '_ {
                async move {
                    $crate::render::render_at(self.render_site(), template, options)
                        .await
                        .map(|report| report.attachment)
                }
            }

            /// [`render_with`](Self::render_with), returning a
            /// [`RenderReport`](crate::RenderReport): the attachment plus
            /// the encoded `format` and the `settings_used`.
            ///
            /// # Errors
            ///
            /// The errors of [`render`](Self::render).
            pub fn render_report<T: ::noctis_ports::Template>(
                &self,
                template: T,
                options: $crate::RenderOptions,
            ) -> impl ::std::future::Future<
                Output = $crate::HandlerResult<$crate::RenderReport>,
            > + Send
                   + '_ {
                async move { $crate::render::render_at(self.render_site(), template, options).await }
            }
        }
    };
}

pub(crate) use scope_render_methods;

/// Renders the art of one drawn application emoji for Publish (FR-82):
/// as PNG, or as GIF when the Template animates (EMO-04), within
/// `Limits::emoji_bytes` (EMO-02), in the `noctis.render` span with the
/// render metrics. A still fallback of an animation is an error, not an
/// upload. A failure is counted and reported like any render's, and its
/// message is returned.
pub(crate) async fn render_emoji(
    app: &Arc<AppShared>,
    art: crate::emoji::EmojiArt,
) -> Result<RenderReport, String> {
    let template = art.descriptor.id;
    let Some(renderer) = app.renderer.as_ref() else {
        return Err(RenderError::NoRenderer.to_string());
    };
    let span = tracing::info_span!("noctis.render", app = %app.app, template);
    async {
        let result = match art.job {
            Ok(mut job) => {
                let budget = u64::try_from(app.limits.emoji_bytes).unwrap_or(u64::MAX);
                job.upload_budget = budget;
                job.format = noctis_ports::Format::Png;
                if let Some(spec) = &mut job.animation {
                    spec.format = noctis_ports::AnimatedFormat::Gif;
                }
                let animated = job.animation.is_some();
                match job.animation.map(|spec| spec.check_fps(template)) {
                    Some(Err(error)) => Err(error),
                    _ => render_job(&app.app, renderer.as_ref(), template, job).await,
                }
                .map(|report| (report, animated))
            }
            Err(error) => Err(RenderError::failed(error)),
        };
        match result {
            Ok((report, true)) if report.settings_used.frames <= 1 => Err(format!(
                "the animation fits {} bytes only as a still frame; an animated emoji \
                 must stay animated: shorten it, lower its fps or simplify it",
                app.limits.emoji_bytes
            )),
            Ok((report, _)) => Ok(report),
            Err(error) => {
                let message = crate::app::error_chain(&error);
                let _ = fail(&RenderSite::app(app), app, error);
                Err(message)
            }
        }
    }
    .instrument(span)
    .await
}

/// Renders `job`, checks the output against the job's Upload Budget and
/// wraps it, measuring the accepted render.
///
/// Every Renderer's output is checked here, so a custom Renderer cannot hand
/// the Responder an empty or over-budget file (ATT-01): an empty output is
/// [`RenderError::Failed`], one longer than [`RenderJob::upload_budget`] is
/// [`RenderError::OverBudget`].
async fn render_job(
    app: &Arc<str>,
    renderer: &dyn Renderer,
    template: &'static str,
    job: RenderJob,
) -> Result<RenderReport, RenderError> {
    let budget = job.upload_budget;
    let started = Instant::now();
    let output = {
        let _in_flight = InFlight::enter(app);
        renderer.render(job).await?
    };
    if output.bytes.is_empty() {
        return Err(RenderError::failed("the Renderer returned no bytes"));
    }
    let len = u64::try_from(output.bytes.len()).unwrap_or(u64::MAX);
    if len > budget {
        return Err(RenderError::OverBudget {
            budget,
            smallest: len,
        });
    }
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
/// `OverBudget`, `SizeMismatch`, `UnknownImage`, `RemoteImage` and
/// `UndeclaredImage` are handled like `Failed`: reported, with no
/// rejected-metric reason (an `OverCap` for `RemoteImages` is `over_cap`).
fn fail(site: &RenderSite<'_>, app: &AppShared, error: RenderError) -> HandlerError {
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
            shard: site.shard,
            guild_id: site.guild_id,
            module: site.module,
            handler: site.handler,
            event_type: None,
            extensions: site.extensions,
            middleware: None,
        });
    }
    error
}

#[cfg(test)]
mod tests;
