//! Play count, holds and merged identical frames (FR-69), read back from
//! the encoded bytes of every animated format.

use std::num::NonZeroU16;

use noctis_ports::{Plays, RenderCap};

use super::*;

const THREE: Plays = Plays::Times(NonZeroU16::new(3).unwrap());

/// `Steps` (5 red frames, then 5 blue ones) animated by `spec`.
fn steps(spec: AnimationSpec) -> RenderJob {
    RenderJob {
        animation: Some(spec),
        ..RenderJob::of(&Steps).unwrap()
    }
}

/// One second at 10 fps as `format`.
fn second(format: AnimatedFormat) -> AnimationSpec {
    AnimationSpec::new(Duration::from_secs(1), 10, format)
}

/// The chunks of a RIFF (WebP) file: fourcc and payload.
fn riff_chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&bytes[..4], b"RIFF");
    let mut chunks = Vec::new();
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let fourcc: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        chunks.push((fourcc, &bytes[at + 8..at + 8 + len]));
        at += 8 + len + (len & 1);
    }
    chunks
}

/// An animated WebP's loop count (0 is forever) and frame durations in ms.
fn webp_timing(bytes: &[u8]) -> (u16, Vec<u32>) {
    let chunks = riff_chunks(bytes);
    let anim = chunks
        .iter()
        .find(|(c, _)| c == b"ANIM")
        .expect("an ANIM chunk")
        .1;
    let loops = u16::from_le_bytes([anim[4], anim[5]]);
    let durations = chunks
        .iter()
        .filter(|(c, _)| c == b"ANMF")
        .map(|(_, p)| u32::from_le_bytes([p[12], p[13], p[14], 0]))
        .collect();
    (loops, durations)
}

/// The chunks of a PNG file: type and data.
#[cfg(feature = "gif")]
fn png_chunks(bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut chunks = Vec::new();
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
        chunks.push((kind, &bytes[at + 8..at + 8 + len]));
        at += 12 + len;
    }
    chunks
}

/// An APNG's play count (0 is forever) and frame delays in ms.
#[cfg(feature = "gif")]
fn apng_timing(bytes: &[u8]) -> (u32, Vec<u32>) {
    let chunks = png_chunks(bytes);
    let actl = chunks
        .iter()
        .find(|(c, _)| c == b"acTL")
        .expect("an acTL chunk")
        .1;
    let plays = u32::from_be_bytes(actl[4..8].try_into().unwrap());
    let delays = chunks
        .iter()
        .filter(|(c, _)| c == b"fcTL")
        .map(|(_, d)| {
            let num = u32::from(u16::from_be_bytes([d[20], d[21]]));
            let den = u32::from(u16::from_be_bytes([d[22], d[23]]));
            num * 1000 / den
        })
        .collect::<Vec<_>>();
    // The png decoder reads the same play count and frame count.
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let control = reader.info().animation_control.expect("an animated PNG");
    assert_eq!(control.num_plays, plays);
    assert_eq!(control.num_frames as usize, delays.len());
    (plays, delays)
}

/// The data sub-blocks of a GIF block starting at `*at`, which ends past
/// the terminator.
#[cfg(feature = "gif")]
fn gif_sub_blocks<'a>(bytes: &'a [u8], at: &mut usize) -> Vec<&'a [u8]> {
    let mut blocks = Vec::new();
    loop {
        let len = usize::from(bytes[*at]);
        *at += 1;
        if len == 0 {
            return blocks;
        }
        blocks.push(&bytes[*at..*at + len]);
        *at += len;
    }
}

/// A GIF's NETSCAPE repeat (`None` without the extension) and frame delays
/// in centiseconds.
#[cfg(feature = "gif")]
fn gif_timing(bytes: &[u8]) -> (Option<u16>, Vec<u16>) {
    assert_eq!(&bytes[..6], b"GIF89a");
    let table = |flags: u8| {
        if flags & 0x80 == 0 {
            0
        } else {
            3 << ((flags & 7) + 1)
        }
    };
    let mut at = 13 + table(bytes[10]);
    let (mut repeat, mut delays) = (None, Vec::new());
    loop {
        match bytes[at] {
            0x21 => {
                let label = bytes[at + 1];
                at += 2;
                let blocks = gif_sub_blocks(bytes, &mut at);
                match label {
                    0xF9 => delays.push(u16::from_le_bytes([blocks[0][1], blocks[0][2]])),
                    0xFF if blocks[0] == b"NETSCAPE2.0" => {
                        assert_eq!(blocks[1][0], 1);
                        repeat = Some(u16::from_le_bytes([blocks[1][1], blocks[1][2]]));
                    }
                    _ => {}
                }
            }
            0x2C => {
                at += 10 + table(bytes[at + 9]) + 1;
                gif_sub_blocks(bytes, &mut at);
            }
            0x3B => return (repeat, delays),
            other => panic!("unexpected GIF block {other:#04x} at {at}"),
        }
    }
}

#[tokio::test]
async fn fr_69_webp_stores_the_total_plays() {
    let r = renderer();
    for (plays, stored) in [(Plays::Forever, 0), (Plays::ONCE, 1), (THREE, 3)] {
        let out = r.render(steps(second(WEBP80).plays(plays))).await.unwrap();
        assert_eq!(webp_timing(&out.bytes).0, stored, "{plays:?}");
    }
}

#[cfg(feature = "gif")]
#[tokio::test]
async fn fr_69_gif_and_apng_store_the_total_plays() {
    let r = renderer();
    // GIF: NETSCAPE repeat 0 loops forever, repeat n - 1 plays n times, and
    // no extension at all plays once.
    for (plays, stored) in [
        (Plays::Forever, Some(0)),
        (Plays::ONCE, None),
        (THREE, Some(2)),
    ] {
        let spec = second(AnimatedFormat::Gif).plays(plays);
        let out = r.render(steps(spec)).await.unwrap();
        assert_eq!(gif_timing(&out.bytes).0, stored, "{plays:?}");
    }
    for (plays, stored) in [(Plays::Forever, 0), (Plays::ONCE, 1), (THREE, 3)] {
        let spec = second(AnimatedFormat::Apng).plays(plays);
        let out = r.render(steps(spec)).await.unwrap();
        assert_eq!(apng_timing(&out.bytes).0, stored, "{plays:?}");
    }
}

#[tokio::test]
async fn fr_69_identical_frames_merge_and_holds_lengthen_the_ends() {
    let r = renderer();
    let held = |spec: AnimationSpec| {
        spec.hold_start(Duration::from_millis(300))
            .hold_end(Duration::from_millis(700))
    };
    let plain = r.render(steps(second(WEBP80))).await.unwrap();
    assert_eq!(webp_timing(&plain.bytes).1, [500, 500]);
    let out = r.render(steps(held(second(WEBP80)))).await.unwrap();
    assert_eq!(webp_timing(&out.bytes).1, [800, 1200]);
    assert_eq!(out.settings_used.frames, 10, "the frames drawn");
    #[cfg(feature = "gif")]
    {
        let gif = r.render(steps(second(AnimatedFormat::Gif))).await.unwrap();
        assert_eq!(gif_timing(&gif.bytes).1, [50, 50]);
        assert_eq!(gif.settings_used.frames, 10);
        let gif = r
            .render(steps(held(second(AnimatedFormat::Gif))))
            .await
            .unwrap();
        assert_eq!(gif_timing(&gif.bytes).1, [80, 120]);
        let apng = r
            .render(steps(held(second(AnimatedFormat::Apng))))
            .await
            .unwrap();
        assert_eq!(apng_timing(&apng.bytes).1, [800, 1200]);
        assert_eq!(apng.settings_used.frames, 10);
    }
}

#[tokio::test]
async fn fr_69_holds_draw_no_frame_and_the_view_sees_the_same_frames() {
    let r = renderer();
    FRAMES.lock().unwrap().clear();
    let spec = AnimationSpec::new(Duration::from_millis(500), 10, WEBP80)
        .once()
        .hold_start(Duration::from_secs(1))
        .hold_end(Duration::from_secs(2));
    let job = RenderJob {
        animation: Some(spec),
        ..RenderJob::of(&Recorder).unwrap()
    };
    let out = r.render(job).await.unwrap();
    let seen: Vec<Frame> = std::mem::take(&mut *FRAMES.lock().unwrap())
        .into_iter()
        .map(|(f, _)| f)
        .collect();
    let expected: Vec<Frame> = (0..5).map(|index| Frame { index, count: 5 }).collect();
    assert_eq!(seen, expected);
    assert_eq!(out.settings_used.frames, 5);
    // Recorder draws the same pixels every frame: one frame of 3.5 s.
    assert_eq!(webp_timing(&out.bytes), (1, vec![3500]));
}

#[tokio::test]
async fn fr_70_the_duration_cap_counts_the_holds() {
    let r = renderer();
    let spec = second(WEBP80).hold_start(Duration::from_secs(2));
    let at_cap = spec.hold_end(Duration::from_secs(3));
    let out = r.render(steps(at_cap)).await.unwrap();
    assert_eq!(webp_timing(&out.bytes).1, [2500, 3500]);
    let over = spec.hold_end(Duration::from_millis(3001));
    let before = r.pool.admitted();
    let future = r.render(steps(over));
    assert_eq!(r.pool.admitted(), before, "rejected before admission");
    let err = future.await.unwrap_err();
    assert!(
        matches!(
            &err,
            RenderError::OverCap {
                cap: RenderCap::Duration,
                limit: 6000,
                actual: 6001,
                ..
            }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn fr_69_a_webp_longer_than_one_frame_can_last_is_unsupported() {
    let mut r = renderer();
    // Only a duration cap raised past 4.6 hours lets such a job through.
    Arc::get_mut(&mut r.shared).unwrap().caps = RenderCaps::new().duration(Duration::MAX);
    let spec = second(WEBP80).hold_end(Duration::from_millis(16_777_215));
    let err = r.render(steps(spec)).await.unwrap_err();
    assert!(
        matches!(&err, RenderError::Unsupported { why, .. } if why.contains("16777215")),
        "{err}"
    );
}

#[tokio::test]
async fn fr_33_the_ladder_keeps_plays_and_holds() {
    let r = renderer();
    let spec = AnimationSpec::new(Duration::from_secs(1), 20, WEBP80)
        .once()
        .hold_end(Duration::from_secs(2));
    let job = RenderJob {
        animation: Some(spec),
        ..noise(Format::Png)
    };
    let full = r.render(job.clone()).await.unwrap();
    assert_eq!(full.settings_used.fps, Some(20));
    let out = r
        .render(with_budget(job, full.bytes.len() as u64 - 1))
        .await
        .unwrap();
    let fps = out.settings_used.fps.unwrap();
    assert!(fps < 20, "{fps}");
    let (loops, durations) = webp_timing(&out.bytes);
    assert_eq!(loops, 1);
    assert_eq!(durations.iter().sum::<u32>(), 3000);
    assert!(*durations.last().unwrap() > 2000, "{durations:?}");
}

#[test]
fn fr_36_plays_and_holds_change_the_render_key() {
    let r = renderer();
    let base = second(WEBP80);
    let keys: std::collections::HashSet<_> = [
        base,
        base.once(),
        base.plays(THREE),
        base.hold_start(Duration::from_millis(1)),
        base.hold_end(Duration::from_millis(1)),
    ]
    .into_iter()
    .map(|spec| r.key(&steps(spec)).0)
    .collect();
    assert_eq!(keys.len(), 5);
}
