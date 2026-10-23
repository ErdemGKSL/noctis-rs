//! [`RenderCaps`]: the per-job caps a Renderer checks before admission
//! (FR-70, NFR-1).

use std::time::Duration;

use noctis_ports::{RenderCap, RenderError, RenderJob};

use crate::budget;

/// The per-job caps of an App's Renderer (FR-70).
///
/// Every job is checked before it enters the render queue; a job over a cap
/// fails with `RenderError::OverCap` naming the first cap it exceeds. Every
/// limit is inclusive, and the checks run in this order:
///
/// 1. `job_pixels` (default 4 000 000): the canvas `w*h` of every job;
/// 2. `frame_pixels` (default 1 000 000): the canvas `w*h` of each frame of
///    an animation (a still is bounded by `job_pixels` alone);
/// 3. `duration` (default 6 s): an animation's duration, in whole
///    milliseconds;
/// 4. `frames` (default 90): an animation's frame count.
///
/// ```
/// use std::time::Duration;
///
/// use noctis_image::RenderCaps;
///
/// let caps = RenderCaps::new().frames(200).duration(Duration::from_secs(10));
/// assert_ne!(caps, RenderCaps::default());
/// assert_eq!(RenderCaps::new(), RenderCaps::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderCaps {
    pub(crate) job_pixels: u64,
    pub(crate) frame_pixels: u64,
    pub(crate) frames: u32,
    pub(crate) duration: Duration,
}

impl RenderCaps {
    /// The defaults: 4 MP per job, 1 MP per frame, 90 frames and 6 s.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            job_pixels: 4_000_000,
            frame_pixels: 1_000_000,
            frames: 90,
            duration: Duration::from_secs(6),
        }
    }

    /// The largest canvas (`w*h`) of any job, 1..=67108864 pixels.
    #[must_use]
    pub const fn job_pixels(mut self, pixels: u64) -> Self {
        self.job_pixels = pixels;
        self
    }

    /// The largest frame (`w*h`) of an animation, 1..=67108864 pixels.
    #[must_use]
    pub const fn frame_pixels(mut self, pixels: u64) -> Self {
        self.frame_pixels = pixels;
        self
    }

    /// The most frames of an animation, at least 1.
    #[must_use]
    pub const fn frames(mut self, frames: u32) -> Self {
        self.frames = frames;
        self
    }

    /// The longest animation, at least 1 ms (whole milliseconds count).
    #[must_use]
    pub const fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Checks `job` against the caps, in the documented order.
    ///
    /// # Errors
    ///
    /// [`RenderError::OverCap`] naming the first cap `job` exceeds.
    pub(crate) fn check(&self, job: &RenderJob) -> Result<(), RenderError> {
        let over = |cap: RenderCap, limit: u64, actual: u64| {
            if actual > limit {
                Err(RenderError::OverCap {
                    id: job.template_id.to_owned(),
                    cap,
                    limit,
                    actual,
                })
            } else {
                Ok(())
            }
        };
        let pixels = u64::from(job.size.width()) * u64::from(job.size.height());
        over(RenderCap::JobPixels, self.job_pixels, pixels)?;
        if let Some(spec) = job.animation {
            over(RenderCap::FramePixels, self.frame_pixels, pixels)?;
            over(
                RenderCap::Duration,
                budget::total_ms(self.duration),
                budget::total_ms(spec.duration),
            )?;
            over(
                RenderCap::Frames,
                u64::from(self.frames),
                budget::span_count(spec.duration, spec.fps),
            )?;
        }
        Ok(())
    }
}

impl Default for RenderCaps {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
