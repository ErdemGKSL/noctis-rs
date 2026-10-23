//! The size budget (FR-33, ATT-01): the degradation ladder a render walks
//! until its encoding fits `min(RenderJob::upload_budget, size_target)`.
//!
//! The ladder is cumulative, each step keeping the earlier steps' most
//! degraded values:
//!
//! 1. the requested settings;
//! 2. fps (animated only): `F/2`, then `F/4`, each kept only when it is at
//!    least [`MIN_FPS`] and lower than the current fps;
//! 3. size: 3/4, then 1/2 of the job size (floored, each side at least 1),
//!    skipping a step equal to the current size;
//! 4. quality (lossy only): [`QUALITY_STEPS`], each kept only when it is
//!    lower than the current quality;
//! 5. a still frame (animated, when the fallback is on): the last size,
//!    encoded with the job's still format, at quality `min(q, 25)` for a
//!    lossy one.
//!
//! Everything here is pure; the Renderer supplies the encoder.

use std::time::Duration;

use noctis_ports::{AnimatedFormat, Format, RenderError, RenderJob, Size};

/// The lowest fps an fps step may produce.
pub(crate) const MIN_FPS: u16 = 5;
/// The fps divisors of the fps steps.
const FPS_DIVISORS: [u16; 2] = [2, 4];
/// The size steps as `(numerator, denominator)` of the job size.
const SIZE_STEPS: [(u64, u64); 2] = [(3, 4), (1, 2)];
/// The quality steps of lossy formats.
pub(crate) const QUALITY_STEPS: [u8; 3] = [75, 50, 25];
/// The highest quality of a lossy still fallback.
const STILL_QUALITY: u8 = 25;

/// One encoding the ladder tries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Attempt {
    /// The fps of an animated attempt; `None` for a still.
    pub(crate) fps: Option<u16>,
    /// The output size.
    pub(crate) size: Size,
    /// The quality, for lossy formats.
    pub(crate) quality: Option<u8>,
    /// Whether this is the still fallback of an animation.
    pub(crate) still: bool,
}

/// The quality `format` encodes with, `None` for a lossless one.
fn still_quality(format: Format) -> Option<u8> {
    match format {
        Format::Jpeg { quality } | Format::Webp { quality } => Some(quality),
        Format::Png | Format::WebpLossless => None,
    }
}

/// The quality `format` encodes with, `None` for a lossless one.
fn animated_quality(format: AnimatedFormat) -> Option<u8> {
    match format {
        AnimatedFormat::Webp { quality } => Some(quality),
        AnimatedFormat::Gif | AnimatedFormat::Apng => None,
    }
}

/// `side * num / den`, floored and at least 1.
fn scale(side: u32, num: u64, den: u64) -> u32 {
    let scaled = u64::from(side) * num / den;
    u32::try_from(scaled).unwrap_or(u32::MAX).max(1)
}

/// The attempts `job` walks, in order; never empty.
pub(crate) fn ladder(job: &RenderJob, still_fallback: bool) -> Vec<Attempt> {
    let mut current = Attempt {
        fps: job.animation.map(|a| a.fps),
        size: job.size,
        quality: job
            .animation
            .map_or_else(|| still_quality(job.format), |a| animated_quality(a.format)),
        still: false,
    };
    let mut attempts = vec![current];

    if let Some(fps) = current.fps {
        for divisor in FPS_DIVISORS {
            let step = fps / divisor;
            if step >= MIN_FPS && Some(step) < current.fps {
                current.fps = Some(step);
                attempts.push(current);
            }
        }
    }

    for (num, den) in SIZE_STEPS {
        let size = Size::new(
            scale(job.size.width(), num, den),
            scale(job.size.height(), num, den),
        );
        if size != current.size {
            current.size = size;
            attempts.push(current);
        }
    }

    if let Some(mut quality) = current.quality {
        for step in QUALITY_STEPS {
            if step < quality {
                quality = step;
                current.quality = Some(step);
                attempts.push(current);
            }
        }
    }

    if job.animation.is_some() && still_fallback {
        attempts.push(Attempt {
            fps: None,
            size: current.size,
            quality: still_quality(job.format).map(|q| q.min(STILL_QUALITY)),
            still: true,
        });
    }
    attempts
}

/// The first of `attempts` whose encoding fits `budget`, with its bytes.
///
/// # Errors
///
/// The first error `encode` returns (no later attempt is encoded), or
/// [`RenderError::OverBudget`] with the smallest length seen when none fits.
pub(crate) fn fit(
    attempts: &[Attempt],
    budget: u64,
    mut encode: impl FnMut(&Attempt) -> Result<Vec<u8>, RenderError>,
) -> Result<(Vec<u8>, Attempt), RenderError> {
    let mut smallest = u64::MAX;
    for attempt in attempts {
        let bytes = encode(attempt)?;
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len <= budget {
            return Ok((bytes, *attempt));
        }
        smallest = smallest.min(len);
    }
    Err(RenderError::OverBudget { budget, smallest })
}

/// One frame of an animation: its start and displayed duration in ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Span {
    /// The start offset in milliseconds.
    pub(crate) start_ms: u64,
    /// The displayed duration in milliseconds, at least 1.
    pub(crate) duration_ms: u32,
}

/// `duration` in whole milliseconds, saturating.
fn total_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// How many frames [`spans`] returns, computed without allocating them.
///
/// Every frame lasts at least 1 ms, and for `fps <= 1000` no frame rounds to
/// zero, so the count is `min(ceil(total * fps / 1000), total)`.
pub(crate) fn span_count(duration: Duration, fps: u16) -> u64 {
    let total = total_ms(duration);
    total
        .saturating_mul(u64::from(fps))
        .div_ceil(1000)
        .min(total)
}

/// The frame schedule of `duration` at `fps`, as takumi's `frame_spans`
/// computes it: frame `i` starts at `i*1000/fps` and lasts until the next
/// start (or the end), and zero-length frames are dropped.
pub(crate) fn spans(duration: Duration, fps: u16) -> Vec<Span> {
    let total = total_ms(duration);
    if total == 0 || fps == 0 {
        return Vec::new();
    }
    let fps = u64::from(fps);
    let count = total.saturating_mul(fps).div_ceil(1000);
    (0..count)
        .filter_map(|i| {
            let start_ms = i.saturating_mul(1000) / fps;
            let end_ms = ((i + 1).saturating_mul(1000) / fps).min(total);
            let duration_ms = end_ms.saturating_sub(start_ms);
            (duration_ms != 0).then(|| Span {
                start_ms,
                duration_ms: u32::try_from(duration_ms).unwrap_or(u32::MAX),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
