//! The testkit rows of Story 10.11's matrix: tolerance, names, env flags,
//! PNG normalization and every `check` outcome, with a fake Renderer.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use bytes::Bytes;
use noctis_ports::{
    AnimatedFormat, AnimationSpec, BoxFuture, EncodeSettings, ErasedView, RenderOutput,
    TemplateDescriptor,
};
use serde::{Deserialize, Serialize};

use super::*;

/// Encodes `pixels` (rows of `channels`-byte pixels) as a PNG.
fn encode(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
    }
    out
}

/// A `width`x`height` RGBA8 image, every pixel `[10, 20, 30, 255]`.
fn base(width: u32, height: u32) -> Vec<u8> {
    [10, 20, 30, 255].repeat((width * height) as usize)
}

fn rgba(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    encode(
        width,
        height,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        pixels,
    )
}

/// A canned output: the bytes, their format and the size used.
type Canned = (Vec<u8>, OutputFormat, Size);

/// A fake Renderer answering every job with a canned output, recording the
/// last job.
struct Fake {
    answer: Result<Canned, fn(&RenderJob) -> RenderError>,
    last: Mutex<Option<RenderJob>>,
}

impl Fake {
    fn png(bytes: Vec<u8>, size: Size) -> Arc<dyn Renderer> {
        Self::with(bytes, OutputFormat::Png, size)
    }

    fn with(bytes: Vec<u8>, format: OutputFormat, size: Size) -> Arc<dyn Renderer> {
        Arc::new(Self {
            answer: Ok((bytes, format, size)),
            last: Mutex::new(None),
        })
    }
}

impl Renderer for Fake {
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>> {
        let answer = match &self.answer {
            Ok((bytes, format, size)) => Ok(RenderOutput {
                bytes: Bytes::from(bytes.clone()),
                digest: [0; 32],
                format: *format,
                settings_used: EncodeSettings {
                    size: *size,
                    quality: None,
                    fps: None,
                    frames: 1,
                },
            }),
            Err(error) => Err(error(&job)),
        };
        *self.last.lock().unwrap() = Some(job);
        Box::pin(async move { answer })
    }

    fn register(&self, _: &[TemplateDescriptor]) -> Result<(), RenderError> {
        Ok(())
    }
}

/// A Template whose own settings are a JPEG and an animation.
#[derive(Serialize, Deserialize)]
struct Animated;

impl Template for Animated {
    const ID: &'static str = "animated";
    const VERSION: u32 = 1;
    const SIZE: Size = Size::new(10, 10);
    const FORMAT: Format = Format::Jpeg { quality: 80 };
    const ANIMATION: Option<AnimationSpec> = Some(AnimationSpec::new(
        Duration::from_secs(1),
        10,
        AnimatedFormat::Gif,
    ));

    fn view() -> ErasedView {
        ErasedView::new(())
    }
}

fn job() -> RenderJob {
    RenderJob::of(&Animated).unwrap()
}

/// A fresh, unique directory under the system temp dir.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("noctis-snapshot-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const LOCAL: SnapshotEnv = SnapshotEnv {
    update: false,
    ci: false,
};

async fn run(
    renderer: &Arc<dyn Renderer>,
    name: &str,
    tolerance: Tolerance,
    dir: &Dir,
    env: SnapshotEnv,
) -> Result<Outcome, SnapshotError> {
    check(
        Some(renderer),
        name,
        job(),
        Size::new(10, 10),
        tolerance,
        &dir.0,
        env,
    )
    .await
}

/// Stores `stored` as `card.png`, then renders `rendered`.
async fn compare(
    stored: &[u8],
    rendered: Vec<u8>,
    tolerance: Tolerance,
) -> (Result<Outcome, SnapshotError>, Dir) {
    let dir = Dir::new();
    write(&dir.file("card.png"), stored).unwrap();
    let fake = Fake::png(rendered, Size::new(10, 10));
    (run(&fake, "card", tolerance, &dir, LOCAL).await, dir)
}

/// `base(10, 10)` with the first `n` pixels' red channel raised by `delta`.
fn changed(n: usize, delta: u8) -> Vec<u8> {
    let mut pixels = base(10, 10);
    for pixel in pixels.chunks_exact_mut(4).take(n) {
        pixel[0] += delta;
    }
    rgba(10, 10, &pixels)
}

#[tokio::test]
async fn identical_passes_and_removes_a_stale_new_png() {
    let bytes = rgba(10, 10, &base(10, 10));
    let dir = Dir::new();
    write(&dir.file("card.png"), &bytes).unwrap();
    write(&dir.file("card.new.png"), b"stale").unwrap();
    let fake = Fake::png(bytes, Size::new(10, 10));
    let outcome = run(&fake, "card", Tolerance::EXACT, &dir, LOCAL)
        .await
        .unwrap();
    assert_eq!(outcome, Outcome::Matched { differing: 0 });
    assert!(!dir.file("card.new.png").exists());
}

#[tokio::test]
async fn one_of_100_within_a_ratio_of_001_passes() {
    let (outcome, _dir) = compare(
        &rgba(10, 10, &base(10, 10)),
        changed(1, 1),
        Tolerance::ratio(0.01),
    )
    .await;
    assert_eq!(outcome.unwrap(), Outcome::Matched { differing: 1 });
}

#[tokio::test]
async fn two_of_100_over_a_ratio_of_001_fails_and_writes_new_png() {
    let rendered = changed(2, 1);
    let (outcome, dir) = compare(
        &rgba(10, 10, &base(10, 10)),
        rendered.clone(),
        Tolerance::ratio(0.01),
    )
    .await;
    let message = outcome.unwrap_err().to_string();
    assert!(message.contains("2/100"), "{message}");
    assert!(message.contains("2.00 %"), "{message}");
    assert!(
        message.contains("1.00 % of the pixels (channel delta 0)"),
        "{message}"
    );
    let new = dir.file("card.new.png");
    assert!(message.contains(&new.display().to_string()), "{message}");
    assert_eq!(std::fs::read(new).unwrap(), rendered);
}

#[tokio::test]
async fn exact_default_fails_on_one_pixel() {
    let (outcome, _dir) = compare(
        &rgba(10, 10, &base(10, 10)),
        changed(1, 1),
        Tolerance::default(),
    )
    .await;
    assert!(matches!(
        outcome,
        Err(SnapshotError::Mismatch {
            differing: 1,
            total: 100,
            ..
        })
    ));
}

#[tokio::test]
async fn ratio_one_passes_when_every_pixel_differs() {
    let (outcome, _dir) = compare(
        &rgba(10, 10, &base(10, 10)),
        changed(100, 50),
        Tolerance::ratio(1.0),
    )
    .await;
    assert_eq!(outcome.unwrap(), Outcome::Matched { differing: 100 });
}

#[tokio::test]
async fn channel_delta_three_passes_and_four_fails() {
    let stored = rgba(10, 10, &base(10, 10));
    let tolerance = Tolerance::ratio(0.0).channel(3);
    let (outcome, _dir) = compare(&stored, changed(1, 3), tolerance).await;
    assert_eq!(outcome.unwrap(), Outcome::Matched { differing: 0 });
    let (outcome, _dir) = compare(&stored, changed(1, 4), tolerance).await;
    assert!(matches!(
        outcome,
        Err(SnapshotError::Mismatch { differing: 1, .. })
    ));
}

#[tokio::test]
async fn alpha_only_differences_count() {
    let mut pixels = base(10, 10);
    pixels[3] = 254;
    let (outcome, _dir) = compare(
        &rgba(10, 10, &base(10, 10)),
        rgba(10, 10, &pixels),
        Tolerance::EXACT,
    )
    .await;
    assert!(matches!(
        outcome,
        Err(SnapshotError::Mismatch { differing: 1, .. })
    ));
}

#[tokio::test]
async fn a_size_mismatch_fails_naming_both_sizes() {
    let dir = Dir::new();
    write(&dir.file("card.png"), &rgba(10, 10, &base(10, 10))).unwrap();
    let fake = Fake::png(rgba(10, 11, &base(10, 11)), Size::new(10, 11));
    let outcome = check(
        Some(&fake),
        "card",
        job(),
        Size::new(10, 11),
        Tolerance::ratio(1.0),
        &dir.0,
        LOCAL,
    )
    .await;
    let message = outcome.unwrap_err().to_string();
    assert!(
        message.contains("10x10") && message.contains("10x11"),
        "{message}"
    );
    assert!(dir.file("card.new.png").exists());
}

#[tokio::test]
async fn a_downscaled_render_fails() {
    let dir = Dir::new();
    let fake = Fake::png(rgba(5, 5, &base(5, 5)), Size::new(5, 5));
    let outcome = run(&fake, "card", Tolerance::ratio(1.0), &dir, LOCAL).await;
    let message = outcome.unwrap_err().to_string();
    assert!(
        message.contains("the size budget downscaled the render"),
        "{message}"
    );
    assert!(!dir.file("card.png").exists());
}

#[tokio::test]
async fn gray_rgb_and_16_bit_snapshots_are_normalized() {
    let rendered = rgba(10, 10, &[7, 7, 7, 255].repeat(100));
    let gray = encode(
        10,
        10,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[7; 100],
    );
    let rgb = encode(10, 10, png::ColorType::Rgb, png::BitDepth::Eight, &[7; 300]);
    let wide: Vec<u8> = [7, 0, 7, 0, 7, 0, 255, 255].repeat(100);
    let wide = encode(10, 10, png::ColorType::Rgba, png::BitDepth::Sixteen, &wide);
    for stored in [gray, rgb, wide] {
        let (outcome, _dir) = compare(&stored, rendered.clone(), Tolerance::EXACT).await;
        assert_eq!(outcome.unwrap(), Outcome::Matched { differing: 0 });
    }
}

#[test]
fn decode_normalizes_gray_alpha() {
    let bytes = encode(
        2,
        1,
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Eight,
        &[1, 2, 3, 4],
    );
    let image = decode(&bytes).unwrap();
    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(image.pixels, [1, 1, 1, 2, 3, 3, 3, 4]);
}

#[tokio::test]
async fn a_corrupt_stored_snapshot_fails_and_update_mode_overwrites_it() {
    let rendered = rgba(10, 10, &base(10, 10));
    let (outcome, dir) = compare(b"junk", rendered.clone(), Tolerance::EXACT).await;
    let message = outcome.unwrap_err().to_string();
    assert!(
        message.contains("cannot decode stored snapshot"),
        "{message}"
    );
    assert_eq!(std::fs::read(dir.file("card.new.png")).unwrap(), rendered);

    let fake = Fake::png(rendered.clone(), Size::new(10, 10));
    let env = SnapshotEnv {
        update: true,
        ci: false,
    };
    assert_eq!(
        run(&fake, "card", Tolerance::EXACT, &dir, env)
            .await
            .unwrap(),
        Outcome::Updated
    );
    assert_eq!(std::fs::read(dir.file("card.png")).unwrap(), rendered);
}

#[tokio::test]
async fn a_missing_snapshot_is_written_locally() {
    let rendered = rgba(10, 10, &base(10, 10));
    let dir = Dir::new();
    let fake = Fake::png(rendered.clone(), Size::new(10, 10));
    let nested = Dir(dir.0.join("nested"));
    let outcome = run(&fake, "card", Tolerance::EXACT, &nested, LOCAL)
        .await
        .unwrap();
    assert_eq!(outcome, Outcome::Written);
    assert_eq!(std::fs::read(nested.file("card.png")).unwrap(), rendered);
}

#[tokio::test]
async fn a_missing_snapshot_fails_on_ci() {
    let rendered = rgba(10, 10, &base(10, 10));
    let dir = Dir::new();
    let fake = Fake::png(rendered.clone(), Size::new(10, 10));
    let env = SnapshotEnv {
        update: false,
        ci: true,
    };
    let message = run(&fake, "card", Tolerance::EXACT, &dir, env)
        .await
        .unwrap_err()
        .to_string();
    assert!(message.contains("missing snapshot (CI)"), "{message}");
    assert_eq!(std::fs::read(dir.file("card.new.png")).unwrap(), rendered);
    assert!(!dir.file("card.png").exists());
}

#[tokio::test]
async fn update_mode_overwrites_a_differing_snapshot_even_on_ci() {
    for ci in [false, true] {
        let rendered = changed(50, 9);
        let (_, dir) = compare(
            &rgba(10, 10, &base(10, 10)),
            rendered.clone(),
            Tolerance::EXACT,
        )
        .await;
        let fake = Fake::png(rendered.clone(), Size::new(10, 10));
        let env = SnapshotEnv { update: true, ci };
        assert_eq!(
            run(&fake, "card", Tolerance::EXACT, &dir, env)
                .await
                .unwrap(),
            Outcome::Updated
        );
        assert_eq!(std::fs::read(dir.file("card.png")).unwrap(), rendered);
        assert!(!dir.file("card.new.png").exists());
    }
}

#[test]
fn env_flags_parse() {
    for off in [None, Some(""), Some("0"), Some("false")] {
        assert!(!flag(off.map(OsStr::new)), "{off:?}");
    }
    for on in ["1", "true", "yes"] {
        assert!(flag(Some(OsStr::new(on))), "{on}");
    }
}

#[test]
fn good_names_are_accepted() {
    for name in ["a", &"a".repeat(64), "profile_card-2"] {
        assert!(check_name(name).is_ok(), "{name}");
    }
}

#[tokio::test]
async fn bad_names_fail_before_rendering() {
    let long = "a".repeat(65);
    for name in ["", long.as_str(), "a/b", "..", "a b", "x.new", "kart.ş"] {
        let fake = Arc::new(Fake {
            answer: Ok((Vec::new(), OutputFormat::Png, Size::new(10, 10))),
            last: Mutex::new(None),
        });
        let renderer: Arc<dyn Renderer> = fake.clone();
        let dir = Dir::new();
        let message = run(&renderer, name, Tolerance::EXACT, &dir, LOCAL)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("1..=64 bytes of [A-Za-z0-9_-]"),
            "{message}"
        );
        assert!(fake.last.lock().unwrap().is_none(), "{name:?} rendered");
    }
}

#[test]
fn tolerance_accepts_zero_and_negative_zero() {
    assert_eq!(Tolerance::ratio(0.0), Tolerance::EXACT);
    assert_eq!(Tolerance::ratio(-0.0), Tolerance::EXACT);
    assert_eq!(Tolerance::from(0.5).channel(7).channel_delta(), 7);
}

#[test]
fn bad_tolerances_panic() {
    for bad in [-0.1, 1.000_000_1, f64::NAN, f64::INFINITY] {
        let message = *std::panic::catch_unwind(|| Tolerance::ratio(bad))
            .unwrap_err()
            .downcast::<String>()
            .unwrap();
        assert_eq!(
            message,
            format!("tolerance must be a finite ratio in 0.0..=1.0, got {bad}")
        );
    }
}

#[tokio::test]
async fn no_renderer_fails() {
    let dir = Dir::new();
    let outcome = check(
        None,
        "card",
        job(),
        Size::new(10, 10),
        Tolerance::EXACT,
        &dir.0,
        LOCAL,
    )
    .await;
    assert_eq!(
        outcome.unwrap_err().to_string(),
        "no Renderer: build the App with the `image` feature and `.image(..)`/`.theme(..)`"
    );
}

#[tokio::test]
async fn a_render_error_is_the_message() {
    let renderer: Arc<dyn Renderer> = Arc::new(Fake {
        answer: Err(|job| RenderError::UnknownTemplate {
            id: job.template_id.to_owned(),
            version: job.template_version,
        }),
        last: Mutex::new(None),
    });
    let dir = Dir::new();
    let message = run(&renderer, "card", Tolerance::EXACT, &dir, LOCAL)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("unknown template `animated` version 1"),
        "{message}"
    );
}

#[tokio::test]
async fn non_png_output_fails() {
    for format in [OutputFormat::Webp, OutputFormat::Apng] {
        let dir = Dir::new();
        let fake = Fake::with(rgba(10, 10, &base(10, 10)), format, Size::new(10, 10));
        let message = run(&fake, "card", Tolerance::EXACT, &dir, LOCAL)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("a pixel snapshot needs a still PNG"),
            "{message}"
        );
    }
}

#[tokio::test]
async fn the_job_is_forced_to_a_still_png_without_a_budget() {
    let fake = Arc::new(Fake {
        answer: Ok((
            rgba(10, 10, &base(10, 10)),
            OutputFormat::Png,
            Size::new(10, 10),
        )),
        last: Mutex::new(None),
    });
    let renderer: Arc<dyn Renderer> = fake.clone();
    let dir = Dir::new();
    run(&renderer, "card", Tolerance::EXACT, &dir, LOCAL)
        .await
        .unwrap();
    let last = fake.last.lock().unwrap().take().unwrap();
    assert_eq!(last.format, Format::Png);
    assert_eq!(last.animation, None);
    assert_eq!(last.upload_budget, u64::MAX);
}
