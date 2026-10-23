//! The ladder, fit and spans rows of the size budget.

use bytes::Bytes;
use noctis_ports::AnimationSpec;

use super::*;

fn job(size: Size, format: Format, animation: Option<AnimationSpec>) -> RenderJob {
    RenderJob {
        template_id: "t",
        template_version: 1,
        data: Bytes::new(),
        size,
        format,
        animation,
        upload_budget: RenderJob::DEFAULT_UPLOAD_BUDGET,
    }
}

fn still(w: u32, h: u32, format: Format) -> RenderJob {
    job(Size::new(w, h), format, None)
}

fn anim(fps: u16, format: AnimatedFormat, still_format: Format) -> RenderJob {
    job(
        Size::new(64, 64),
        still_format,
        Some(AnimationSpec::new(Duration::from_secs(1), fps, format)),
    )
}

fn a(fps: Option<u16>, (w, h): (u32, u32), quality: Option<u8>) -> Attempt {
    Attempt {
        fps,
        size: Size::new(w, h),
        quality,
        still: false,
    }
}

fn fps_steps(attempts: &[Attempt]) -> Vec<u16> {
    let mut fps: Vec<u16> = attempts.iter().filter_map(|a| a.fps).collect();
    fps.dedup();
    fps
}

#[test]
fn ladder_still_png() {
    assert_eq!(
        ladder(&still(64, 32, Format::Png), true),
        [
            a(None, (64, 32), None),
            a(None, (48, 24), None),
            a(None, (32, 16), None),
        ]
    );
}

#[test]
fn ladder_jpeg() {
    assert_eq!(
        ladder(&still(64, 32, Format::Jpeg { quality: 90 }), true),
        [
            a(None, (64, 32), Some(90)),
            a(None, (48, 24), Some(90)),
            a(None, (32, 16), Some(90)),
            a(None, (32, 16), Some(75)),
            a(None, (32, 16), Some(50)),
            a(None, (32, 16), Some(25)),
        ]
    );
}

#[test]
fn ladder_low_quality_only_steps_below() {
    let steps: Vec<_> = ladder(&still(64, 32, Format::Webp { quality: 30 }), true)
        .iter()
        .map(|a| a.quality)
        .collect();
    assert_eq!(steps, [Some(30), Some(30), Some(30), Some(25)]);
    for q in [25, 1] {
        let attempts = ladder(&still(64, 32, Format::Jpeg { quality: q }), true);
        assert_eq!(attempts.len(), 3, "q{q}");
        assert!(attempts.iter().all(|a| a.quality == Some(q)), "q{q}");
    }
}

#[test]
fn ladder_lossless_has_no_quality_step() {
    let attempts = ladder(&still(64, 32, Format::WebpLossless), true);
    assert_eq!(attempts.len(), 3);
    assert!(attempts.iter().all(|a| a.quality.is_none()));
}

#[test]
fn ladder_tiny_sizes() {
    assert_eq!(
        ladder(&still(1, 1, Format::Png), true),
        [a(None, (1, 1), None)]
    );
    assert_eq!(
        ladder(&still(3, 1, Format::Png), true),
        [
            a(None, (3, 1), None),
            a(None, (2, 1), None),
            a(None, (1, 1), None),
        ]
    );
}

#[test]
fn ladder_large_sizes_do_not_overflow() {
    let attempts = ladder(&still(u32::MAX, u32::MAX, Format::Png), true);
    assert_eq!(attempts[2].size, Size::new(u32::MAX / 2, u32::MAX / 2));
}

#[test]
fn ladder_animated_webp() {
    let attempts = ladder(
        &anim(20, AnimatedFormat::Webp { quality: 80 }, Format::Png),
        true,
    );
    assert_eq!(
        attempts,
        [
            a(Some(20), (64, 64), Some(80)),
            a(Some(10), (64, 64), Some(80)),
            a(Some(5), (64, 64), Some(80)),
            a(Some(5), (48, 48), Some(80)),
            a(Some(5), (32, 32), Some(80)),
            a(Some(5), (32, 32), Some(75)),
            a(Some(5), (32, 32), Some(50)),
            a(Some(5), (32, 32), Some(25)),
            Attempt {
                fps: None,
                size: Size::new(32, 32),
                quality: None,
                still: true,
            },
        ]
    );
}

#[test]
fn ladder_fps_floor() {
    let webp = AnimatedFormat::Webp { quality: 80 };
    for (fps, expected) in [
        (9, &[9][..]),
        (5, &[5]),
        (1, &[1]),
        (90, &[90, 45, 22]),
        (20, &[20, 10, 5]),
    ] {
        let attempts = ladder(&anim(fps, webp, Format::Png), true);
        assert_eq!(fps_steps(&attempts), expected, "F = {fps}");
    }
}

#[test]
fn ladder_gif_is_lossless_and_its_still_too() {
    let attempts = ladder(&anim(10, AnimatedFormat::Gif, Format::Png), true);
    assert!(attempts.iter().all(|a| a.quality.is_none()));
    let last = attempts.last().unwrap();
    assert!(last.still);
    assert_eq!(last.fps, None);
}

#[test]
fn ladder_without_the_still_fallback() {
    let attempts = ladder(&anim(10, AnimatedFormat::Apng, Format::Png), false);
    assert!(attempts.iter().all(|a| !a.still));
    assert!(
        ladder(&still(64, 32, Format::Png), true)
            .iter()
            .all(|a| !a.still)
    );
}

#[test]
fn ladder_lossy_still_fallback_caps_quality() {
    let last = *ladder(
        &anim(10, AnimatedFormat::Gif, Format::Jpeg { quality: 90 }),
        true,
    )
    .last()
    .unwrap();
    assert_eq!(last.quality, Some(25));
    let low = *ladder(
        &anim(10, AnimatedFormat::Gif, Format::Webp { quality: 10 }),
        true,
    )
    .last()
    .unwrap();
    assert_eq!(low.quality, Some(10));
}

/// Three distinct attempts for the fit rows.
fn three() -> Vec<Attempt> {
    ladder(&still(64, 32, Format::Png), true)
}

/// An encoder returning `lens[i]` bytes for the i-th call.
fn lengths(lens: &[usize]) -> impl FnMut(&Attempt) -> Result<Vec<u8>, RenderError> + '_ {
    let mut calls = 0;
    move |_| {
        let len = lens[calls];
        calls += 1;
        Ok(vec![0; len])
    }
}

#[test]
fn fit_takes_the_first_attempt_that_fits() {
    let attempts = three();
    let (bytes, attempt) = fit(&attempts, 10, lengths(&[10, 5])).unwrap();
    assert_eq!((bytes.len(), attempt), (10, attempts[0]));
    let (bytes, attempt) = fit(&attempts, 10, lengths(&[11, 10])).unwrap();
    assert_eq!((bytes.len(), attempt), (10, attempts[1]));
}

#[test]
fn fit_reports_the_smallest_when_none_fits() {
    assert!(matches!(
        fit(&three(), 10, lengths(&[30, 20, 25])),
        Err(RenderError::OverBudget {
            budget: 10,
            smallest: 20
        })
    ));
    assert!(matches!(
        fit(&three(), 0, lengths(&[3, 2, 1])),
        Err(RenderError::OverBudget {
            budget: 0,
            smallest: 1
        })
    ));
}

#[test]
fn fit_propagates_an_encode_error_at_once() {
    let mut calls = 0;
    let err = fit(&three(), 1, |_| {
        calls += 1;
        if calls == 2 {
            Err(RenderError::QueueFull)
        } else {
            Ok(vec![0; 5])
        }
    })
    .unwrap_err();
    assert!(matches!(err, RenderError::QueueFull));
    assert_eq!(calls, 2);
}

fn durations(duration: Duration, fps: u16) -> Vec<u32> {
    spans(duration, fps).iter().map(|s| s.duration_ms).collect()
}

#[test]
fn spans_match_takumi() {
    assert_eq!(durations(Duration::from_secs(1), 3), [333, 333, 334]);
    assert_eq!(durations(Duration::from_millis(1), 90), [1]);
    assert_eq!(durations(Duration::from_millis(2500), 2), [500; 5]);
    let starts: Vec<u64> = spans(Duration::from_secs(1), 3)
        .iter()
        .map(|s| s.start_ms)
        .collect();
    assert_eq!(starts, [0, 333, 666]);
    assert!(spans(Duration::ZERO, 10).is_empty());
    assert!(spans(Duration::from_micros(500), 10).is_empty());
    assert!(spans(Duration::from_secs(1), 0).is_empty());
}

#[test]
fn span_count_matches_spans() {
    for (ms, fps) in [
        (1000, 3),
        (1, 90),
        (2500, 2),
        (1000, 2000),
        (7, 65_535),
        (999, 1000),
        (0, 5),
    ] {
        let duration = Duration::from_millis(ms);
        assert_eq!(
            span_count(duration, fps),
            spans(duration, fps).len() as u64,
            "{ms} ms at {fps} fps"
        );
    }
    assert_eq!(span_count(Duration::MAX, 1000), u64::MAX / 1000 + 1);
}
