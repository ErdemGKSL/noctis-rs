//! The takumi [`Renderer`]: one per App, bound to its Theme, rendering stills
//! and animations within its per-job caps ([`RenderCaps`]), fitted to the
//! size budget (`crate::budget`), on the process-scoped [`RenderPool`].

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
use crate::caps::RenderCaps;
use crate::lower::lower;
use crate::pool::{Lane, RenderPool, Requester};
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
    /// The App's `ImageConfig::caps`, checked before admission.
    caps: RenderCaps,
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
/// over every namespace's tokens and then every `--animate-<name>`, followed
/// by the Theme's `@keyframes` blocks. takumi reads `@theme` as `:root`, so
/// theme utilities (`bg-brand`, `animate-rise`) resolve their
/// `var(--color-brand)` through it, and animations find their keyframes.
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
    for (name, value) in theme.animations() {
        css.push_str("  --animate-");
        css.push_str(name);
        css.push_str(": ");
        css.push_str(value);
        css.push_str(";\n");
    }
    css.push('}');
    for (name, block) in theme.keyframes() {
        css.push_str("\n@keyframes ");
        css.push_str(name);
        css.push_str(" {");
        css.push_str(block);
        css.push('}');
    }
    css
}

impl TakumiRenderer {
    /// A Renderer drawing with `theme` (already parsed into `stylesheet`)
    /// on `pool`; `mode` is Deny on test Runtimes, Warn otherwise. Each
    /// output fits `min(job.upload_budget, size_target)`, an animation
    /// falling back to a still frame when `still_fallback` is on; a job
    /// over one of `caps` fails before admission.
    pub(crate) fn new(
        pool: Arc<RenderPool>,
        theme: Theme,
        stylesheet: StyleSheet,
        mode: ClassCheckMode,
        size_target: u64,
        still_fallback: bool,
        caps: RenderCaps,
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
                caps,
            }),
            views: RwLock::new(HashMap::new()),
        }
    }

    /// The view of `job`, checked against the registered version, the
    /// animations this Renderer can encode, the format's fps cap and then
    /// its per-job caps.
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
            spec.check_fps(job.template_id)?;
        }
        self.shared.caps.check(job)?;
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
    /// Looks the Template up and checks the caps, then admits the job to
    /// the pool before it returns the future, so admission order is call
    /// order. Dropping the future gives the job up: a queued job is skipped,
    /// a running one stops before its next frame or encode attempt.
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let admitted = self.view_of(&job).and_then(|view| {
            let shared = Arc::clone(&self.shared);
            let lane = match job.animation {
                None => Lane::Still,
                Some(spec) => Lane::Animated {
                    frames: budget::span_count(spec.duration, spec.fps),
                },
            };
            self.pool
                .try_submit(lane, move |requester| shared.draw(view, &job, requester))
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
    /// The per-attempt encoder of `job`, drawing with `view` for
    /// `requester`.
    fn encoder<'a>(
        &'a self,
        view: ViewFn,
        job: &'a RenderJob,
        requester: &'a Requester<'a>,
    ) -> Encoder<'a> {
        Encoder {
            shared: self,
            view,
            job,
            requester,
            frames: None,
        }
    }

    /// Renders `job` with `view` and fits it to the size budget; runs on a
    /// pool thread and stops once `requester` is gone.
    fn draw(
        &self,
        view: ViewFn,
        job: &RenderJob,
        requester: &Requester<'_>,
    ) -> Result<RenderOutput, RenderError> {
        let budget = job.upload_budget.min(self.size_target);
        let attempts = budget::ladder(job, self.still_fallback);
        let mut encoder = self.encoder(view, job, requester);
        let (bytes, attempt) = budget::fit(&attempts, budget, |a| {
            if requester.gone() {
                return Err(requester_gone());
            }
            encoder.encode(a)
        })?;
        let (format, frames) = match job.animation {
            Some(spec) if !attempt.still => {
                // `Encoder::draw` already bounded the count to `u32`.
                let count = encoder.frames.as_ref().map_or(0, |f| f.full.len());
                (spec.format.into(), u32::try_from(count).unwrap_or(u32::MAX))
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

    /// Draws one frame of `job` at its full size; a frame of an animation
    /// whose root declares another explicit size fails with
    /// [`RenderError::SizeMismatch`].
    fn frame(
        &self,
        view: ViewFn,
        job: &RenderJob,
        frame: Frame,
        time_ms: u64,
    ) -> Result<Bitmap, RenderError> {
        let node = view.call(&job.data, frame)?;
        if job.animation.is_some()
            && let Some(declared) = node.root_size_mismatch(job.size)
        {
            return Err(RenderError::SizeMismatch {
                id: job.template_id.to_owned(),
                frame: frame.index,
                expected: job.size,
                declared,
            });
        }
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

/// The error a run stops with once its requester is gone; nobody sees it.
fn requester_gone() -> RenderError {
    RenderError::failed("the requester is gone")
}

/// The `Unsupported` error of an animation with more than `u32::MAX` frames.
fn too_many_frames(job: &RenderJob) -> RenderError {
    RenderError::Unsupported {
        id: job.template_id.to_owned(),
        why: "an animation has at most u32::MAX frames".into(),
    }
}

/// The full-size frames of one fps (`None` for a still Template), plus the
/// frames last resampled from them, so quality steps at one size reuse them.
struct Frames {
    fps: Option<u16>,
    full: Vec<AnimationFrame>,
    resampled: Option<(Size, Vec<AnimationFrame>)>,
}

/// Encodes one job's [`Attempt`]s. Frames are drawn at full size once per
/// distinct fps; a size step resamples them instead of drawing again.
pub(crate) struct Encoder<'a> {
    shared: &'a Shared,
    view: ViewFn,
    job: &'a RenderJob,
    /// Asked before each frame: a gone requester stops the draw.
    requester: &'a Requester<'a>,
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
        let mut bytes = Vec::new();
        match job.animation {
            Some(spec) if !attempt.still => {
                let frames = self.frames_sized(fps, attempt.size)?;
                write_animated(
                    spec.format,
                    attempt.quality,
                    Cow::Borrowed(frames),
                    &mut bytes,
                    job,
                )?;
            }
            _ => {
                let frames = self.frames_at(fps)?;
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
            self.frames = Some(Frames {
                fps,
                full: frames,
                resampled: None,
            });
        }
        Ok(self.frames.as_ref().map_or(&[], |f| &f.full))
    }

    /// The frames at `fps` resampled to `size`, cached for the last size.
    fn frames_sized(
        &mut self,
        fps: Option<u16>,
        size: Size,
    ) -> Result<&[AnimationFrame], RenderError> {
        self.frames_at(fps)?;
        let Some(cache) = self.frames.as_mut() else {
            return Ok(&[]);
        };
        let full = cache
            .full
            .first()
            .is_none_or(|f| (f.image.width(), f.image.height()) == (size.width(), size.height()));
        if full {
            return Ok(&cache.full);
        }
        if cache.resampled.as_ref().is_none_or(|(s, _)| *s != size) {
            let frames = cache
                .full
                .iter()
                .map(|f| {
                    resized(&f.image, size)
                        .map(|b| AnimationFrame::new(b.into_owned(), f.duration_ms))
                })
                .collect::<Result<Vec<_>, _>>()?;
            cache.resampled = Some((size, frames));
        }
        Ok(cache.resampled.as_ref().map_or(&[], |(_, f)| f))
    }

    /// Draws every frame at `fps` (one still frame for `None`), stopping
    /// before a frame once the requester is gone.
    fn draw(&self, fps: Option<u16>) -> Result<Vec<AnimationFrame>, RenderError> {
        let (shared, view, job) = (self.shared, self.view, self.job);
        if self.requester.gone() {
            return Err(requester_gone());
        }
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
                if self.requester.gone() {
                    return Err(requester_gone());
                }
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
    // takumi's pixels are straight RGBA; resampling them as-is would bleed
    // the black of clear pixels into translucent edges.
    let mut raw = bitmap.as_raw().to_vec();
    premultiply(&mut raw);
    let source = image::RgbaImage::from_raw(bitmap.width(), bitmap.height(), raw)
        .ok_or_else(|| RenderError::failed("a rendered frame has an invalid pixel buffer"))?;
    let mut scaled =
        image::imageops::resize(&source, width, height, FilterType::Triangle).into_raw();
    demultiply(&mut scaled);
    Bitmap::from_raw(width, height, scaled)
        .map(Cow::Owned)
        .ok_or_else(|| RenderError::failed("a resampled frame has an invalid pixel buffer"))
}

/// Premultiplies straight RGBA pixels by their alpha, in place.
fn premultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(px[3]);
        for c in &mut px[..3] {
            // At most 255 * 255 + 127, so the quotient fits a byte.
            *c = u8::try_from((u16::from(*c) * alpha + 127) / 255).unwrap_or(u8::MAX);
        }
    }
}

/// Turns premultiplied RGBA pixels back into straight ones, in place.
fn demultiply(pixels: &mut [u8]) {
    for px in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(px[3]);
        for c in &mut px[..3] {
            // A clear pixel (alpha 0) has no colour left: 0.
            *c = (u32::from(*c) * 255 + alpha / 2)
                .checked_div(alpha)
                .map_or(0, |v| u8::try_from(v).unwrap_or(u8::MAX));
        }
    }
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
