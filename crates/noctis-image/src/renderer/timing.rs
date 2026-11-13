//! Frame timing of an animation (FR-69): merging pixel-identical
//! consecutive frames, the start and end holds, each format's longest
//! frame and the play count each format stores.

use std::borrow::Cow;
use std::time::Duration;

use noctis_ports::{AnimatedFormat, AnimationSpec, Plays};
use takumi::prelude::{AnimationFrame, Bitmap};

use crate::budget;

/// The longest frame animated WebP stores: a 24-bit millisecond duration
/// (takumi clamps a longer one).
pub(super) const WEBP_MAX_FRAME_MS: u32 = 0xFF_FFFF;

/// Merges every run of pixel-identical consecutive `frames` into its first
/// frame, with the run's summed duration, so a hold drawn as repeated
/// frames costs no bytes in any format.
pub(super) fn merge_identical(frames: Vec<AnimationFrame>) -> Vec<AnimationFrame> {
    let mut merged: Vec<AnimationFrame> = Vec::with_capacity(frames.len());
    for frame in frames {
        match merged.last_mut() {
            Some(last) if same_pixels(&last.image, &frame.image) => {
                last.duration_ms = last.duration_ms.saturating_add(frame.duration_ms);
            }
            _ => merged.push(frame),
        }
    }
    merged
}

/// Whether `a` and `b` have the same size and pixels.
fn same_pixels(a: &Bitmap, b: &Bitmap) -> bool {
    (a.width(), a.height()) == (b.width(), b.height()) && a.as_raw() == b.as_raw()
}

/// Lengthens the first frame by `spec.hold_start` and the last by
/// `spec.hold_end` (one frame gets both); adds no frame.
pub(super) fn apply_holds(frames: &mut [AnimationFrame], spec: &AnimationSpec) {
    if let Some(first) = frames.first_mut() {
        first.duration_ms = first.duration_ms.saturating_add(whole_ms(spec.hold_start));
    }
    if let Some(last) = frames.last_mut() {
        last.duration_ms = last.duration_ms.saturating_add(whole_ms(spec.hold_end));
    }
}

/// `duration` in whole milliseconds, saturating at `u32::MAX`.
fn whole_ms(duration: Duration) -> u32 {
    u32::try_from(budget::total_ms(duration)).unwrap_or(u32::MAX)
}

/// The longest frame `format` stores, in milliseconds: WebP's 24-bit
/// duration, GIF's `u16` centiseconds and APNG's `u16` milliseconds (as
/// takumi writes its delay over 1000).
pub(super) const fn max_frame_ms(format: AnimatedFormat) -> u32 {
    match format {
        AnimatedFormat::Webp { .. } => WEBP_MAX_FRAME_MS,
        AnimatedFormat::Gif => u16::MAX as u32 * 10,
        AnimatedFormat::Apng => u16::MAX as u32,
    }
}

/// `frames` with every frame longer than `max_ms` split into identical
/// frames of at most `max_ms` each; borrowed when no frame is longer.
pub(super) fn split_long(
    frames: Cow<'_, [AnimationFrame]>,
    max_ms: u32,
) -> Cow<'_, [AnimationFrame]> {
    let max_ms = max_ms.max(1);
    if frames.iter().all(|f| f.duration_ms <= max_ms) {
        return frames;
    }
    let mut split = Vec::with_capacity(frames.len() + 1);
    for frame in frames.iter() {
        let mut left = frame.duration_ms;
        while left > max_ms {
            split.push(AnimationFrame::new(frame.image.clone(), max_ms));
            left -= max_ms;
        }
        split.push(AnimationFrame::new(frame.image.clone(), left));
    }
    Cow::Owned(split)
}

/// The `loop_count` takumi writes for `plays` in `format`, normalized to
/// total plays: WebP and APNG store `n` (0 is forever), GIF stores a
/// NETSCAPE repeat of `n - 1` (`Some(0)` writes no NETSCAPE extension, which
/// plays once; `None` writes a repeat of 0, which loops forever).
pub(super) const fn loop_count(plays: Plays, format: AnimatedFormat) -> Option<u16> {
    match (plays, format) {
        (Plays::Forever, _) => None,
        (Plays::Times(n), AnimatedFormat::Gif) => Some(n.get() - 1),
        (Plays::Times(n), AnimatedFormat::Webp { .. } | AnimatedFormat::Apng) => Some(n.get()),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU16;

    use super::*;

    fn frame(px: u8, ms: u32) -> AnimationFrame {
        let bitmap = Bitmap::from_raw(1, 1, vec![px, px, px, 255]).expect("a 1x1 bitmap");
        AnimationFrame::new(bitmap, ms)
    }

    fn timing(frames: &[AnimationFrame]) -> Vec<(u8, u32)> {
        frames
            .iter()
            .map(|f| (f.image.as_raw()[0], f.duration_ms))
            .collect()
    }

    #[test]
    fn fr_69_identical_runs_merge_with_their_summed_duration() {
        let frames = vec![
            frame(1, 100),
            frame(1, 100),
            frame(2, 100),
            frame(1, 100),
            frame(1, 100),
            frame(1, 33),
        ];
        assert_eq!(
            timing(&merge_identical(frames)),
            [(1, 200), (2, 100), (1, 233)]
        );
        assert!(merge_identical(Vec::new()).is_empty());
    }

    #[test]
    fn fr_69_holds_lengthen_the_first_and_last_frame() {
        let spec = AnimationSpec::new(Duration::from_secs(1), 10, AnimatedFormat::DEFAULT)
            .hold_start(Duration::from_millis(250))
            .hold_end(Duration::from_micros(1_999_999));
        let mut frames = vec![frame(1, 100), frame(2, 100), frame(3, 100)];
        apply_holds(&mut frames, &spec);
        assert_eq!(timing(&frames), [(1, 350), (2, 100), (3, 2099)]);
        let mut one = vec![frame(1, 100)];
        apply_holds(&mut one, &spec);
        assert_eq!(timing(&one), [(1, 2349)]);
    }

    #[test]
    fn fr_69_long_frames_split_at_the_format_maximum() {
        let frames = vec![frame(1, 10), frame(2, 25)];
        let split = split_long(Cow::Borrowed(&frames), 10);
        assert_eq!(timing(&split), [(1, 10), (2, 10), (2, 10), (2, 5)]);
        assert!(matches!(
            split_long(Cow::Borrowed(&frames), 25),
            Cow::Borrowed(_)
        ));
        assert_eq!(max_frame_ms(AnimatedFormat::Gif), 655_350);
        assert_eq!(max_frame_ms(AnimatedFormat::Apng), 65_535);
    }

    #[test]
    fn fr_69_loop_count_is_normalized_to_total_plays() {
        let webp = AnimatedFormat::DEFAULT;
        let three = Plays::Times(NonZeroU16::new(3).expect("3 is not zero"));
        for format in [webp, AnimatedFormat::Gif, AnimatedFormat::Apng] {
            assert_eq!(loop_count(Plays::Forever, format), None);
        }
        assert_eq!(loop_count(three, webp), Some(3));
        assert_eq!(loop_count(three, AnimatedFormat::Apng), Some(3));
        assert_eq!(loop_count(three, AnimatedFormat::Gif), Some(2));
        assert_eq!(loop_count(Plays::ONCE, AnimatedFormat::Gif), Some(0));
        assert_eq!(loop_count(Plays::ONCE, AnimatedFormat::Apng), Some(1));
    }
}
