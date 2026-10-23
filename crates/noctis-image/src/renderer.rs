//! The takumi [`Renderer`]: one per App, bound to its Theme, rendering stills
//! and animations, fitted to the size budget (`crate::budget`), on the
//! process-scoped [`RenderPool`].

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use bytes::Bytes;
use image::imageops::FilterType;
use noctis_ports::{
    AnimatedFormat, BoxFuture, EncodeSettings, Format, Frame, RenderError, RenderJob, RenderOutput,
    Renderer, Size, TemplateDescriptor,
};
#[cfg(feature = "gif")]
use takumi::prelude::{AnimatedGifOptions, AnimatedPngOptions};
use takumi::prelude::{
    AnimatedWebpOptions, AnimationFrame, Bitmap, Fonts, Quality, RenderOptions, StyleSheet,
    Viewport,
};

use crate::budget::{self, Attempt};
use crate::lower::lower;
use crate::pool::RenderPool;
use crate::tw::Theme;
use crate::{ClassCheck, ClassCheckMode, ViewFn};

/// The registered Templates: id → (version, view).
type Views = HashMap<&'static str, (u32, ViewFn)>;

/// The per-App takumi Renderer.
pub(crate) struct TakumiRenderer {
    pool: Arc<RenderPool>,
    shared: Arc<Shared>,
    views: RwLock<Views>,
}

/// What every render job of one Renderer reads.
struct Shared {
    theme: Theme,
    /// The `@theme { … }` stylesheet built once from `theme`.
    stylesheet: Arc<StyleSheet>,
    check: ClassCheck,
    /// No fonts are registered yet (Story 10.7).
    fonts: Fonts,
    /// The App's `ImageConfig::size_target`.
    size_target: u64,
    /// Whether an animation that does not fit may fall back to a still.
    still_fallback: bool,
}

/// Why an animation with fps 0 or a duration under 1 ms is unsupported.
const ANIMATION_BOUNDS: &str = "an animation needs fps >= 1 and a duration of at least 1 ms";
/// Why GIF/APNG output is unsupported without the `gif` feature.
#[cfg(not(feature = "gif"))]
const GIF_FEATURE: &str = "GIF and APNG output need the `image-gif` feature";

const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TakumiRenderer>();
};

/// The takumi stylesheet of `theme`: `@theme { --<prefix><token>: <value>; … }`
/// over every namespace's tokens. takumi reads `@theme` as `:root`, so theme
/// utilities (`bg-brand`) resolve their `var(--color-brand)` through it.
///
/// # Errors
///
/// takumi's parse error when a value is not valid CSS for it.
pub(crate) fn theme_stylesheet(
    theme: &Theme,
) -> Result<StyleSheet, takumi_core::error::StyleSheetParseError> {
    StyleSheet::parse_list([theme_css(theme)])
}

/// The `@theme { … }` CSS text [`theme_stylesheet`] parses.
fn theme_css(theme: &Theme) -> String {
    let mut css = String::from("@theme {\n");
    for (name, value) in theme.custom_properties() {
        css.push_str("  ");
        css.push_str(&name);
        css.push_str(": ");
        css.push_str(value);
        css.push_str(";\n");
    }
    css.push('}');
    css
}

impl TakumiRenderer {
    /// A Renderer drawing with `theme` (already parsed into `stylesheet`)
    /// on `pool`; `mode` is Deny on test Runtimes, Warn otherwise. Each
    /// output fits `min(job.upload_budget, size_target)`, an animation
    /// falling back to a still frame when `still_fallback` is on.
    pub(crate) fn new(
        pool: Arc<RenderPool>,
        theme: Theme,
        stylesheet: StyleSheet,
        mode: ClassCheckMode,
        size_target: u64,
        still_fallback: bool,
    ) -> Self {
        Self {
            pool,
            shared: Arc::new(Shared {
                theme,
                stylesheet: Arc::new(stylesheet),
                check: ClassCheck::new(mode),
                fonts: Fonts::default(),
                size_target,
                still_fallback,
            }),
            views: RwLock::new(HashMap::new()),
        }
    }

    /// The view of `job`, checked against the registered version and the
    /// animations this Renderer can encode.
    fn view_of(&self, job: &RenderJob) -> Result<ViewFn, RenderError> {
        let views = self.views.read().unwrap_or_else(PoisonError::into_inner);
        let view = match views.get(job.template_id) {
            Some(&(version, view)) if version == job.template_version => view,
            _ => {
                return Err(RenderError::UnknownTemplate {
                    id: job.template_id.to_owned(),
                    version: job.template_version,
                });
            }
        };
        if let Some(spec) = job.animation {
            let unsupported = |why: &'static str| RenderError::Unsupported {
                id: job.template_id.to_owned(),
                why: why.into(),
            };
            if spec.fps == 0 || spec.duration < Duration::from_millis(1) {
                return Err(unsupported(ANIMATION_BOUNDS));
            }
            #[cfg(not(feature = "gif"))]
            if matches!(spec.format, AnimatedFormat::Gif | AnimatedFormat::Apng) {
                return Err(unsupported(GIF_FEATURE));
            }
            if u32::try_from(budget::span_count(spec.duration, spec.fps)).is_err() {
                return Err(too_many_frames(job));
            }
        }
        Ok(view)
    }
}

impl std::fmt::Debug for TakumiRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let views = self.views.read().unwrap_or_else(PoisonError::into_inner);
        let mut ids: Vec<&str> = views.keys().copied().collect();
        ids.sort_unstable();
        f.debug_struct("TakumiRenderer")
            .field("pool", &self.pool)
            .field("templates", &ids)
            .finish_non_exhaustive()
    }
}

impl Renderer for TakumiRenderer {
    /// Looks the Template up, then admits the job to the pool before it
    /// returns the future, so admission order is call order.
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let admitted = self.view_of(&job).and_then(|view| {
            let shared = Arc::clone(&self.shared);
            self.pool.try_submit(move || shared.draw(view, &job))
        });
        Box::pin(async move {
            admitted?
                .await
                .map_err(|_| RenderError::failed("render pool dropped the job"))?
        })
    }

    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError> {
        let mut views = Views::with_capacity(templates.len());
        for descriptor in templates {
            let Some(view) = descriptor.view.downcast_ref::<ViewFn>() else {
                return Err(RenderError::Unsupported {
                    id: descriptor.id.to_owned(),
                    why: "its view was not made by `#[derive(Template)]`".into(),
                });
            };
            views.insert(descriptor.id, (descriptor.version, *view));
        }
        *self.views.write().unwrap_or_else(PoisonError::into_inner) = views;
        Ok(())
    }
}

impl Shared {
    /// The per-attempt encoder of `job`, drawing with `view`.
    fn encoder<'a>(&'a self, view: ViewFn, job: &'a RenderJob) -> Encoder<'a> {
        Encoder {
            shared: self,
            view,
            job,
            frames: None,
        }
    }

    /// Renders `job` with `view` and fits it to the size budget; runs on a
    /// pool thread.
    fn draw(&self, view: ViewFn, job: &RenderJob) -> Result<RenderOutput, RenderError> {
        let budget = job.upload_budget.min(self.size_target);
        let attempts = budget::ladder(job, self.still_fallback);
        let mut encoder = self.encoder(view, job);
        let (bytes, attempt) = budget::fit(&attempts, budget, |a| encoder.encode(a))?;
        let (format, frames) = match job.animation {
            Some(spec) if !attempt.still => {
                let fps = attempt.fps.unwrap_or(spec.fps);
                let frames = u32::try_from(budget::span_count(spec.duration, fps))
                    .map_err(|_| too_many_frames(job))?;
                (spec.format.into(), frames)
            }
            _ => (job.format.into(), 1),
        };
        let digest = *blake3::hash(&bytes).as_bytes();
        Ok(RenderOutput {
            bytes: Bytes::from(bytes),
            digest,
            format,
            settings_used: EncodeSettings {
                size: attempt.size,
                quality: attempt.quality,
                fps: attempt.fps,
                frames,
            },
        })
    }

    /// Draws one frame of `job` at its full size.
    fn frame(
        &self,
        view: ViewFn,
        job: &RenderJob,
        frame: Frame,
        time_ms: u64,
    ) -> Result<Bitmap, RenderError> {
        let node = view.call(&job.data, frame)?;
        self.check
            .check(&node, &self.theme)
            .map_err(RenderError::failed)?;
        let root = lower(&node)?;
        let (width, height) = (job.size.width(), job.size.height());
        let options = RenderOptions::builder()
            .viewport(Viewport::new((width, height)))
            .fonts(&self.fonts)
            .node(root)
            .stylesheet(Arc::clone(&self.stylesheet))
            .time_ms(time_ms)
            .build();
        let bitmap = takumi::render(options).map_err(RenderError::failed)?;
        if (bitmap.width(), bitmap.height()) != (width, height) {
            return Err(RenderError::failed(format!(
                "takumi drew {}x{} pixels for a {width}x{height} Template",
                bitmap.width(),
                bitmap.height()
            )));
        }
        Ok(bitmap)
    }
}

/// The `Unsupported` error of an animation with more than `u32::MAX` frames.
fn too_many_frames(job: &RenderJob) -> RenderError {
    RenderError::Unsupported {
        id: job.template_id.to_owned(),
        why: "an animation has at most u32::MAX frames".into(),
    }
}

/// The full-size frames of one fps; `fps` is `None` for a still Template.
struct Frames {
    fps: Option<u16>,
    frames: Vec<AnimationFrame>,
}

/// Encodes one job's [`Attempt`]s. Frames are drawn at full size once per
/// distinct fps; a size step resamples them instead of drawing again.
pub(crate) struct Encoder<'a> {
    shared: &'a Shared,
    view: ViewFn,
    job: &'a RenderJob,
    frames: Option<Frames>,
}

impl Encoder<'_> {
    /// The encoded bytes of `attempt`.
    ///
    /// # Errors
    ///
    /// Any draw or encode error.
    pub(crate) fn encode(&mut self, attempt: &Attempt) -> Result<Vec<u8>, RenderError> {
        let job = self.job;
        let fps = match job.animation {
            None => None,
            Some(spec) if !attempt.still => Some(attempt.fps.unwrap_or(spec.fps)),
            // A still fallback takes frame 0 of whatever was drawn last.
            Some(spec) => self.frames.as_ref().map_or(Some(spec.fps), |f| f.fps),
        };
        let frames = self.frames_at(fps)?;
        let mut bytes = Vec::new();
        match job.animation {
            Some(spec) if !attempt.still => {
                let frames = resized_frames(frames, attempt.size)?;
                write_animated(spec.format, attempt.quality, frames, &mut bytes, job)?;
            }
            _ => {
                let first = &frames[0].image;
                let image = resized(first, attempt.size)?;
                let format = still_encoding(job.format, attempt.quality);
                takumi::write_image(image.as_ref(), &mut bytes, format)
                    .map_err(RenderError::failed)?;
            }
        }
        Ok(bytes)
    }

    /// The full-size frames at `fps`, drawn on first use.
    fn frames_at(&mut self, fps: Option<u16>) -> Result<&[AnimationFrame], RenderError> {
        if self.frames.as_ref().is_none_or(|f| f.fps != fps) {
            let frames = self.draw(fps)?;
            self.frames = Some(Frames { fps, frames });
        }
        Ok(self.frames.as_ref().map_or(&[], |f| &f.frames))
    }

    /// Draws every frame at `fps` (one still frame for `None`).
    fn draw(&self, fps: Option<u16>) -> Result<Vec<AnimationFrame>, RenderError> {
        let (shared, view, job) = (self.shared, self.view, self.job);
        let (Some(fps), Some(spec)) = (fps, job.animation) else {
            let still = shared.frame(view, job, Frame::STILL, 0)?;
            return Ok(vec![AnimationFrame::new(still, 0)]);
        };
        let spans = budget::spans(spec.duration, fps);
        let count = u32::try_from(spans.len()).map_err(|_| too_many_frames(job))?;
        if count == 0 {
            return Err(RenderError::Unsupported {
                id: job.template_id.to_owned(),
                why: ANIMATION_BOUNDS.into(),
            });
        }
        spans
            .iter()
            .zip(0..count)
            .map(|(span, index)| {
                let frame = Frame { index, count };
                let bitmap = shared.frame(view, job, frame, span.start_ms)?;
                Ok(AnimationFrame::new(bitmap, span.duration_ms))
            })
            .collect()
    }
}

/// `bitmap` resampled to `size` (Triangle), or borrowed when it already is.
fn resized(bitmap: &Bitmap, size: Size) -> Result<Cow<'_, Bitmap>, RenderError> {
    let (width, height) = (size.width(), size.height());
    if (bitmap.width(), bitmap.height()) == (width, height) {
        return Ok(Cow::Borrowed(bitmap));
    }
    let source =
        image::RgbaImage::from_raw(bitmap.width(), bitmap.height(), bitmap.as_raw().to_vec())
            .ok_or_else(|| RenderError::failed("a rendered frame has an invalid pixel buffer"))?;
    let scaled = image::imageops::resize(&source, width, height, FilterType::Triangle);
    Bitmap::from_raw(width, height, scaled.into_raw())
        .map(Cow::Owned)
        .ok_or_else(|| RenderError::failed("a resampled frame has an invalid pixel buffer"))
}

/// `frames` resampled to `size`, or borrowed when they already are.
fn resized_frames(
    frames: &[AnimationFrame],
    size: Size,
) -> Result<Cow<'_, [AnimationFrame]>, RenderError> {
    let full = frames
        .first()
        .is_none_or(|f| (f.image.width(), f.image.height()) == (size.width(), size.height()));
    if full {
        return Ok(Cow::Borrowed(frames));
    }
    frames
        .iter()
        .map(|f| {
            resized(&f.image, size).map(|b| AnimationFrame::new(b.into_owned(), f.duration_ms))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Cow::Owned)
}

/// Encodes `frames` as `format` into `out`.
fn write_animated(
    format: AnimatedFormat,
    quality: Option<u8>,
    frames: Cow<'_, [AnimationFrame]>,
    out: &mut Vec<u8>,
    #[cfg_attr(
        feature = "gif",
        expect(unused_variables, reason = "only the GIF-less build names the job")
    )]
    job: &RenderJob,
) -> Result<(), RenderError> {
    match format {
        AnimatedFormat::Webp { quality: requested } => {
            let options = AnimatedWebpOptions::builder()
                .lossless(false)
                .quality(quality.unwrap_or(requested))
                .build();
            takumi::write_animated_webp(frames, out, options).map_err(RenderError::failed)
        }
        #[cfg(feature = "gif")]
        AnimatedFormat::Gif => {
            takumi::write_animated_gif(frames, out, AnimatedGifOptions::default())
                .map_err(RenderError::failed)
        }
        #[cfg(feature = "gif")]
        AnimatedFormat::Apng => {
            takumi::write_animated_png(&frames, out, AnimatedPngOptions::default())
                .map_err(RenderError::failed)
        }
        #[cfg(not(feature = "gif"))]
        AnimatedFormat::Gif | AnimatedFormat::Apng => Err(RenderError::Unsupported {
            id: job.template_id.to_owned(),
            why: GIF_FEATURE.into(),
        }),
    }
}

/// takumi's output format for the still `format` at `quality` (the
/// attempt's; `None` for lossless formats).
fn still_encoding(format: Format, quality: Option<u8>) -> takumi::prelude::OutputFormat {
    use takumi::prelude::OutputFormat;

    match format {
        Format::Png => OutputFormat::Png,
        Format::Jpeg { quality: requested } => OutputFormat::Jpeg {
            quality: Quality::new(quality.unwrap_or(requested)),
        },
        Format::Webp { quality: requested } => OutputFormat::WebP {
            quality: Quality::new(quality.unwrap_or(requested)),
        },
        Format::WebpLossless => OutputFormat::WebPLossless,
    }
}

#[cfg(test)]
mod tests;
