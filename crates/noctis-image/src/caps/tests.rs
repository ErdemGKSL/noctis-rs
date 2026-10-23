//! `RenderCaps`: defaults, setters and every cap row, in check order.

use std::time::Duration;

use bytes::Bytes;
use noctis_ports::{AnimatedFormat, AnimationSpec, Format, Size};

use super::*;

const WEBP: AnimatedFormat = AnimatedFormat::Webp { quality: 80 };

fn still(width: u32, height: u32) -> RenderJob {
    RenderJob {
        template_id: "card",
        template_version: 1,
        data: Bytes::new(),
        size: Size::new(width, height),
        format: Format::Png,
        animation: None,
        upload_budget: RenderJob::DEFAULT_UPLOAD_BUDGET,
    }
}

fn animated(width: u32, height: u32, fps: u16, duration: Duration) -> RenderJob {
    RenderJob {
        animation: Some(AnimationSpec::new(duration, fps, WEBP)),
        ..still(width, height)
    }
}

/// A 2×2 animation at `fps` for `ms` milliseconds.
fn anim(fps: u16, ms: u64) -> RenderJob {
    animated(2, 2, fps, Duration::from_millis(ms))
}

/// The cap, limit and actual value of an `OverCap`, or `None` when `job` passes.
fn over(caps: &RenderCaps, job: &RenderJob) -> Option<(RenderCap, u64, u64)> {
    match caps.check(job) {
        Ok(()) => None,
        Err(RenderError::OverCap {
            id,
            cap,
            limit,
            actual,
        }) => {
            assert_eq!(id, "card");
            Some((cap, limit, actual))
        }
        Err(other) => panic!("{other}"),
    }
}

#[test]
fn defaults_and_setters() {
    let caps = RenderCaps::new();
    assert_eq!(caps, RenderCaps::default());
    assert_eq!(
        (
            caps.job_pixels,
            caps.frame_pixels,
            caps.frames,
            caps.duration
        ),
        (4_000_000, 1_000_000, 90, Duration::from_secs(6))
    );
    let changed = RenderCaps::new()
        .job_pixels(1)
        .frame_pixels(2)
        .frames(3)
        .duration(Duration::from_millis(4));
    assert_eq!(
        (
            changed.job_pixels,
            changed.frame_pixels,
            changed.frames,
            changed.duration
        ),
        (1, 2, 3, Duration::from_millis(4))
    );
}

#[test]
fn the_pixel_caps() {
    let caps = RenderCaps::new();
    let second = Duration::from_secs(1);
    for (job, expected) in [
        (still(2000, 2000), None),
        (
            still(2001, 2000),
            Some((RenderCap::JobPixels, 4_000_000, 4_002_000)),
        ),
        (
            still(10_000, 10_000),
            Some((RenderCap::JobPixels, 4_000_000, 100_000_000)),
        ),
        (animated(1000, 1000, 10, second), None),
        (
            animated(1001, 1000, 10, second),
            Some((RenderCap::FramePixels, 1_000_000, 1_001_000)),
        ),
        // A still is bounded by the job cap alone.
        (still(1001, 1000), None),
        // The job cap is checked first.
        (
            animated(2001, 2000, 10, second),
            Some((RenderCap::JobPixels, 4_000_000, 4_002_000)),
        ),
    ] {
        assert_eq!(over(&caps, &job), expected, "{:?}", job.size);
    }
}

#[test]
fn the_time_caps() {
    let caps = RenderCaps::new();
    for (job, expected) in [
        (anim(15, 6000), None),
        (anim(1, 6001), Some((RenderCap::Duration, 6000, 6001))),
        // Floored to 6000 ms.
        (animated(2, 2, 1, Duration::from_micros(6_000_900)), None),
        (anim(18, 5000), None),
        (anim(20, 4550), Some((RenderCap::Frames, 90, 91))),
    ] {
        assert_eq!(over(&caps, &job), expected, "{:?}", job.animation);
    }
    let max = over(&caps, &animated(2, 2, 1000, Duration::MAX)).unwrap();
    assert_eq!((max.0, max.1), (RenderCap::Duration, 6000));
    assert!(max.2 > 6000);
}

#[test]
fn custom_caps() {
    let more = RenderCaps::new().frames(200);
    assert_eq!(over(&more, &anim(20, 5000)), None);
    assert_eq!(
        over(&RenderCaps::new(), &anim(20, 5000)),
        Some((RenderCap::Frames, 90, 100))
    );
    let one = RenderCaps::new().frames(1);
    assert_eq!(over(&one, &anim(10, 100)), None);
    assert_eq!(over(&one, &anim(10, 200)), Some((RenderCap::Frames, 1, 2)));
    // A still has no frame or time cap.
    assert_eq!(
        over(&one.duration(Duration::from_millis(1)), &still(9, 9)),
        None
    );
}
