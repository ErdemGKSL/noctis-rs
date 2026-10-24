//! The render contract (AD-22): what core hands a [`Renderer`] and what it
//! gets back, with no dependency on the image stack.
//!
//! A [`Template`] is a typed value; [`RenderJob::of`] turns it into an erased
//! job whose `data` is plain postcard (no schema byte: `template_version`
//! already versions it). The renderer reads it back with [`decode_template`].

use std::any::Any;
use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::BoxFuture;
use crate::value::{ValueError, decode_postcard};

/// An image Template: a typed value plus its fixed render settings.
pub trait Template: Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The Template id, unique per App.
    const ID: &'static str;
    /// The data version; bump it when the fields change.
    const VERSION: u32;
    /// The output size in pixels.
    const SIZE: Size;
    /// The still output format.
    const FORMAT: Format;
    /// The animation, if the Template is animated.
    const ANIMATION: Option<AnimationSpec>;

    /// The type-erased view the renderer draws this Template with.
    fn view() -> ErasedView;
}

/// A type-erased, shareable view value; the renderer downcasts it.
///
/// Cloning shares the value. `Debug` does not print the contents.
#[derive(Clone)]
pub struct ErasedView(Arc<dyn Any + Send + Sync>);

impl ErasedView {
    /// Erases `view`.
    pub fn new<T: Any + Send + Sync>(view: T) -> Self {
        Self(Arc::new(view))
    }

    /// The view as a `T`, if it is one.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl fmt::Debug for ErasedView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErasedView").finish_non_exhaustive()
    }
}

/// What a [`Renderer`] needs to know about one Template ahead of time.
#[derive(Debug, Clone)]
pub struct TemplateDescriptor {
    /// [`Template::ID`].
    pub id: &'static str,
    /// [`Template::VERSION`].
    pub version: u32,
    /// [`Template::view`].
    pub view: ErasedView,
}

impl TemplateDescriptor {
    /// The descriptor of `T`.
    pub fn of<T: Template>() -> Self {
        Self {
            id: T::ID,
            version: T::VERSION,
            view: T::view(),
        }
    }
}

/// One render request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderJob {
    /// [`Template::ID`].
    pub template_id: &'static str,
    /// [`Template::VERSION`].
    pub template_version: u32,
    /// The Template value as plain postcard (no schema byte).
    pub data: Bytes,
    /// The output size.
    pub size: Size,
    /// The still output format.
    pub format: Format,
    /// The animation, if any.
    pub animation: Option<AnimationSpec>,
    /// The largest output in bytes the upload accepts (ATT-01).
    pub upload_budget: u64,
}

impl RenderJob {
    /// The [`upload_budget`](Self::upload_budget) [`RenderJob::of`] sets:
    /// 20 MiB, Discord's default attachment limit.
    pub const DEFAULT_UPLOAD_BUDGET: u64 = 20_971_520;

    /// The job that renders `template`, with the
    /// [`DEFAULT_UPLOAD_BUDGET`](Self::DEFAULT_UPLOAD_BUDGET).
    ///
    /// # Errors
    ///
    /// [`ValueError::Serialize`] when `template` cannot be serialized.
    pub fn of<T: Template>(template: &T) -> Result<Self, ValueError> {
        let data = postcard::to_extend(template, Vec::new()).map_err(|_| ValueError::Serialize)?;
        Ok(Self {
            template_id: T::ID,
            template_version: T::VERSION,
            data: Bytes::from(data),
            size: T::SIZE,
            format: T::FORMAT,
            animation: T::ANIMATION,
            upload_budget: Self::DEFAULT_UPLOAD_BUDGET,
        })
    }
}

/// Reads a Template value back from [`RenderJob::data`].
///
/// # Errors
///
/// [`ValueError::Malformed`] when `data` does not decode as `T`, and
/// [`ValueError::Trailing`] when bytes are left after it.
pub fn decode_template<T: Template>(data: &[u8]) -> Result<T, ValueError> {
    decode_postcard(data)
}

/// An image size in pixels; both sides are at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    width: u32,
    height: u32,
}

impl Size {
    /// A size of `width` x `height`.
    ///
    /// # Panics
    ///
    /// When a side is 0; in a `const` this is a compile error.
    pub const fn new(width: u32, height: u32) -> Self {
        match Self::try_new(width, height) {
            Some(size) => size,
            None => panic!("Size sides must be at least 1"),
        }
    }

    /// A size of `width` x `height`, or `None` when a side is 0.
    pub const fn try_new(width: u32, height: u32) -> Option<Self> {
        if width == 0 || height == 0 {
            None
        } else {
            Some(Self { width, height })
        }
    }

    /// The width in pixels.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// The height in pixels.
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// A still output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Lossless PNG.
    Png,
    /// JPEG.
    Jpeg {
        /// Quality, 1..=100.
        quality: u8,
    },
    /// Lossy WebP.
    Webp {
        /// Quality, 1..=100.
        quality: u8,
    },
    /// Lossless WebP.
    WebpLossless,
}

/// An animated output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimatedFormat {
    /// Animated WebP.
    Webp {
        /// Quality, 1..=100.
        quality: u8,
    },
    /// GIF.
    Gif,
    /// Animated PNG.
    Apng,
}

impl AnimatedFormat {
    /// The default animated format: animated WebP at quality 80.
    pub const DEFAULT: Self = Self::Webp { quality: 80 };

    /// The highest frame rate the format plays: 50 for GIF (its delays are
    /// whole centiseconds, so 20 ms is the shortest frame), 90 for WebP and
    /// APNG.
    #[must_use]
    pub const fn max_fps(self) -> u16 {
        match self {
            Self::Webp { .. } | Self::Apng => 90,
            Self::Gif => 50,
        }
    }
}

/// How a Template animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnimationSpec {
    /// The total duration.
    pub duration: Duration,
    /// Frames per second.
    pub fps: u16,
    /// The animated output format.
    pub format: AnimatedFormat,
}

impl AnimationSpec {
    /// An animation of `duration` at `fps` frames per second.
    pub const fn new(duration: Duration, fps: u16, format: AnimatedFormat) -> Self {
        Self {
            duration,
            fps,
            format,
        }
    }

    /// Checks the frame rate against the format's cap
    /// ([`AnimatedFormat::max_fps`]).
    ///
    /// # Errors
    ///
    /// [`RenderError::OverCap`] with [`RenderCap::Fps`], naming `template_id`,
    /// when `fps` is over the cap.
    pub fn check_fps(&self, template_id: &str) -> Result<(), RenderError> {
        let limit = self.format.max_fps();
        if self.fps > limit {
            return Err(RenderError::OverCap {
                id: template_id.to_owned(),
                cap: RenderCap::Fps,
                limit: u64::from(limit),
                actual: u64::from(self.fps),
            });
        }
        Ok(())
    }
}

/// Which frame of how many a view is drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Frame {
    /// The 0-based frame index.
    pub index: u32,
    /// The number of frames.
    pub count: u32,
}

impl Frame {
    /// The only frame of a still image.
    pub const STILL: Self = Self { index: 0, count: 1 };
}

/// The format of encoded output bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormat {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// WebP, still or animated.
    Webp,
    /// GIF.
    Gif,
    /// Animated PNG.
    Apng,
}

impl OutputFormat {
    /// The file extension Discord displays the format with; APNG is `png`.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png | Self::Apng => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
        }
    }
}

impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Png => Self::Png,
            Format::Jpeg { .. } => Self::Jpeg,
            Format::Webp { .. } | Format::WebpLossless => Self::Webp,
        }
    }
}

impl From<AnimatedFormat> for OutputFormat {
    fn from(format: AnimatedFormat) -> Self {
        match format {
            AnimatedFormat::Webp { .. } => Self::Webp,
            AnimatedFormat::Gif => Self::Gif,
            AnimatedFormat::Apng => Self::Apng,
        }
    }
}

/// The settings an output was actually encoded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EncodeSettings {
    /// The output size.
    pub size: Size,
    /// The quality, for lossy formats.
    pub quality: Option<u8>,
    /// Frames per second, for animations.
    pub fps: Option<u16>,
    /// The number of frames (1 for a still).
    pub frames: u32,
}

/// A rendered image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutput {
    /// The encoded bytes.
    pub bytes: Bytes,
    /// The blake3 digest of `bytes`.
    pub digest: [u8; 32],
    /// The encoded format.
    pub format: OutputFormat,
    /// The settings used.
    pub settings_used: EncodeSettings,
}

/// One of the Renderer's per-job caps (FR-70), named by
/// [`RenderError::OverCap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RenderCap {
    /// The canvas of every job, in pixels (`w*h`).
    JobPixels,
    /// Each frame of an animation, in pixels (`w*h`).
    FramePixels,
    /// The number of frames of an animation.
    Frames,
    /// The duration of an animation, in whole milliseconds.
    Duration,
    /// The frame rate of an animation, capped per animated format.
    Fps,
}

impl RenderCap {
    /// The cap's stable name: `job_pixels`, `frame_pixels`, `frames`,
    /// `duration_ms` or `fps`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JobPixels => "job_pixels",
            Self::FramePixels => "frame_pixels",
            Self::Frames => "frames",
            Self::Duration => "duration_ms",
            Self::Fps => "fps",
        }
    }
}

impl fmt::Display for RenderCap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a render failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    /// No Template with this id and version is registered.
    #[error("unknown template `{id}` version {version}")]
    UnknownTemplate {
        /// The Template id.
        id: String,
        /// The Template version.
        version: u32,
    },
    /// The renderer cannot render this Template.
    #[error("template `{id}` is unsupported: {why}")]
    Unsupported {
        /// The Template id.
        id: String,
        /// Why it is unsupported.
        why: Cow<'static, str>,
    },
    /// No encoding of the image fits the size budget, even after degrading
    /// fps, size, quality and (when enabled) falling back to a still frame.
    #[error(
        "the rendered image does not fit the size budget: the smallest encoding is {smallest} \
         bytes, the budget {budget} bytes"
    )]
    OverBudget {
        /// The budget in bytes.
        budget: u64,
        /// The length of the smallest encoding tried, in bytes.
        smallest: u64,
    },
    /// The job is over one of the Renderer's per-job caps (FR-70); it never
    /// entered the queue.
    #[error("template `{id}` exceeds the {cap} cap: {actual} is over the limit {limit}")]
    OverCap {
        /// The Template id.
        id: String,
        /// The cap exceeded first.
        cap: RenderCap,
        /// The cap's limit: pixels, frames or whole milliseconds.
        limit: u64,
        /// The job's value, in the same unit.
        actual: u64,
    },
    /// A frame of an animation declared an explicit root size other than
    /// the job's size (FR-69).
    #[error(
        "template `{id}` frame {frame} declares a {}x{} root, but every frame of an animation \
         has the Template's size {}x{}",
        declared.0,
        declared.1,
        expected.width(),
        expected.height()
    )]
    SizeMismatch {
        /// The Template id.
        id: String,
        /// The 0-based index of the frame.
        frame: u32,
        /// The job's size.
        expected: Size,
        /// The root size the frame declared: (width, height), with the job's
        /// side where only the other one differs.
        declared: (u32, u32),
    },
    /// The render queue is full; retry later.
    #[error("render queue is full")]
    QueueFull,
    /// The render failed.
    #[error("render failed")]
    Failed(#[source] Box<dyn Error + Send + Sync>),
    /// The App has no [`Renderer`]; only core returns it.
    #[error(
        "no Renderer is installed: call `.theme(..)` or `.image(..)` (feature `image`) on the \
         AppBuilder"
    )]
    NoRenderer,
    /// The Template's view panicked; the render pool survives.
    #[error("render panicked: {message}")]
    Panicked {
        /// The panic's `&str`/`String` message, or `"<non-string panic>"`.
        message: String,
    },
    /// Templates are registered but no font is (IMG-01): their text would
    /// draw nothing. Returned by `Renderer::register`, so `App::build` fails.
    #[error(
        "[IMG-01] no font is registered: add one with `ImageConfig::font(..)` or enable the \
         `default-font` feature"
    )]
    NoFont,
    /// A Template drew a static image the App did not register (IMG-02).
    #[error(
        "template `{id}` uses static image `{image}`, which is not registered: add it with \
         `ImageConfig::image(..)`"
    )]
    UnknownImage {
        /// The Template id.
        id: String,
        /// The static image id.
        image: String,
    },
}

impl RenderError {
    /// A render failure; use it as `map_err(RenderError::failed)`.
    pub fn failed(err: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Failed(err.into())
    }
}

/// Turns [`RenderJob`]s into [`RenderOutput`]s (a driven port, not a store).
pub trait Renderer: Send + Sync + 'static {
    /// Renders `job`.
    fn render(&self, job: RenderJob) -> BoxFuture<'_, Result<RenderOutput, RenderError>>;

    /// Registers Templates ahead of their first render.
    ///
    /// # Errors
    ///
    /// A [`RenderError`] when a Template cannot be registered.
    fn register(&self, templates: &[TemplateDescriptor]) -> Result<(), RenderError>;
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    struct CardView;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Card {
        name: String,
        level: u32,
        tags: Vec<String>,
    }

    impl Template for Card {
        const ID: &'static str = "card";
        const VERSION: u32 = 3;
        const SIZE: Size = Size::new(800, 240);
        const FORMAT: Format = Format::Webp { quality: 90 };
        const ANIMATION: Option<AnimationSpec> = Some(AnimationSpec::new(
            Duration::from_secs(1),
            10,
            AnimatedFormat::Gif,
        ));

        fn view() -> ErasedView {
            ErasedView::new(CardView)
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Blank;

    impl Template for Blank {
        const ID: &'static str = "blank";
        const VERSION: u32 = 1;
        const SIZE: Size = Size::new(1, 1);
        const FORMAT: Format = Format::Png;
        const ANIMATION: Option<AnimationSpec> = None;

        fn view() -> ErasedView {
            ErasedView::new(())
        }
    }

    fn card() -> Card {
        Card {
            name: "Ada 😀".to_owned(),
            level: u32::MAX,
            tags: vec!["a".to_owned(), String::new()],
        }
    }

    #[test]
    fn template_round_trips_through_a_job() {
        let job = RenderJob::of(&card()).unwrap();
        assert_eq!(job.template_id, Card::ID);
        assert_eq!(job.template_version, Card::VERSION);
        assert_eq!(job.size, Card::SIZE);
        assert_eq!(job.format, Card::FORMAT);
        assert_eq!(job.animation, Card::ANIMATION);
        assert_eq!(job.data.as_ref(), postcard::to_allocvec(&card()).unwrap());
        assert_eq!(decode_template::<Card>(&job.data), Ok(card()));
    }

    #[test]
    fn template_data_trailing_and_empty() {
        let mut data = RenderJob::of(&card()).unwrap().data.to_vec();
        data.push(0);
        assert_eq!(
            decode_template::<Card>(&data),
            Err(ValueError::Trailing { extra: 1 })
        );
        assert_eq!(decode_template::<Card>(b""), Err(ValueError::Malformed));
    }

    #[test]
    fn zero_field_template() {
        assert!(RenderJob::of(&Blank).unwrap().data.is_empty());
        assert_eq!(decode_template::<Blank>(b""), Ok(Blank));
    }

    #[test]
    fn size_bounds() {
        for (w, h) in [(1, 1), (u32::MAX, u32::MAX)] {
            let size = Size::new(w, h);
            assert_eq!((size.width(), size.height()), (w, h));
        }
        assert_eq!(Size::try_new(0, 5), None);
        assert_eq!(Size::try_new(5, 0), None);
        assert_eq!(Size::try_new(0, 0), None);
    }

    #[test]
    #[should_panic(expected = "at least 1")]
    fn size_new_panics_on_zero() {
        let _ = Size::new(0, 5);
    }

    #[test]
    fn frame_still() {
        assert_eq!(Frame::STILL, Frame { index: 0, count: 1 });
    }

    #[test]
    fn extensions() {
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Jpeg.extension(), "jpg");
        assert_eq!(OutputFormat::Webp.extension(), "webp");
        assert_eq!(OutputFormat::Gif.extension(), "gif");
        assert_eq!(OutputFormat::Apng.extension(), "png");
    }

    #[test]
    fn formats_map_to_output_formats() {
        assert_eq!(OutputFormat::from(Format::Png), OutputFormat::Png);
        assert_eq!(
            OutputFormat::from(Format::Jpeg { quality: 80 }),
            OutputFormat::Jpeg
        );
        assert_eq!(
            OutputFormat::from(Format::Webp { quality: 80 }),
            OutputFormat::Webp
        );
        assert_eq!(OutputFormat::from(Format::WebpLossless), OutputFormat::Webp);
        assert_eq!(
            OutputFormat::from(AnimatedFormat::Webp { quality: 80 }),
            OutputFormat::Webp
        );
        assert_eq!(OutputFormat::from(AnimatedFormat::Gif), OutputFormat::Gif);
        assert_eq!(OutputFormat::from(AnimatedFormat::Apng), OutputFormat::Apng);
    }

    #[test]
    fn animation_spec_is_const() {
        const SPEC: AnimationSpec =
            AnimationSpec::new(Duration::from_secs(2), 20, AnimatedFormat::Gif);
        assert_eq!(SPEC.duration, Duration::from_secs(2));
        assert_eq!(SPEC.fps, 20);
        assert_eq!(SPEC.format, AnimatedFormat::Gif);
    }

    #[test]
    fn erased_view() {
        let view = ErasedView::new(5u8);
        assert_eq!(view.downcast_ref::<u8>(), Some(&5));
        assert_eq!(view.downcast_ref::<u16>(), None);
        let clone = view.clone();
        assert!(std::ptr::eq(
            clone.downcast_ref::<u8>().unwrap(),
            view.downcast_ref::<u8>().unwrap()
        ));
        assert!(!format!("{view:?}").contains('5'));
    }

    #[test]
    fn template_descriptor_of() {
        let descriptor = TemplateDescriptor::of::<Card>();
        assert_eq!(descriptor.id, Card::ID);
        assert_eq!(descriptor.version, Card::VERSION);
        assert!(descriptor.view.downcast_ref::<CardView>().is_some());
    }

    #[test]
    fn render_error_no_renderer_and_panicked() {
        assert_eq!(
            RenderError::NoRenderer.to_string(),
            "no Renderer is installed: call `.theme(..)` or `.image(..)` (feature `image`) on \
             the AppBuilder"
        );
        let panicked = RenderError::Panicked {
            message: "boom".to_owned(),
        };
        assert_eq!(panicked.to_string(), "render panicked: boom");
        assert!(panicked.source().is_none());
    }

    #[test]
    fn job_of_sets_the_default_upload_budget() {
        let job = RenderJob::of(&card()).unwrap();
        assert_eq!(job.upload_budget, RenderJob::DEFAULT_UPLOAD_BUDGET);
        assert_eq!(RenderJob::DEFAULT_UPLOAD_BUDGET, 20_971_520);
    }

    #[test]
    fn animated_format_default() {
        assert_eq!(
            AnimatedFormat::DEFAULT,
            AnimatedFormat::Webp { quality: 80 }
        );
    }

    #[test]
    fn render_error_over_budget() {
        let err = RenderError::OverBudget {
            budget: 10,
            smallest: 20,
        };
        assert_eq!(
            err.to_string(),
            "the rendered image does not fit the size budget: the smallest encoding is 20 bytes, \
             the budget 10 bytes"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn render_error_over_cap_and_cap_names() {
        let err = RenderError::OverCap {
            id: "card".to_owned(),
            cap: RenderCap::Frames,
            limit: 90,
            actual: 91,
        };
        assert_eq!(
            err.to_string(),
            "template `card` exceeds the frames cap: 91 is over the limit 90"
        );
        assert!(err.source().is_none());
        for (cap, name) in [
            (RenderCap::JobPixels, "job_pixels"),
            (RenderCap::FramePixels, "frame_pixels"),
            (RenderCap::Frames, "frames"),
            (RenderCap::Duration, "duration_ms"),
            (RenderCap::Fps, "fps"),
        ] {
            assert_eq!(cap.as_str(), name);
            assert_eq!(cap.to_string(), name);
        }
    }

    #[test]
    fn render_error_size_mismatch() {
        let err = RenderError::SizeMismatch {
            id: "card".to_owned(),
            frame: 3,
            expected: Size::new(934, 282),
            declared: (500, 282),
        };
        assert_eq!(
            err.to_string(),
            "template `card` frame 3 declares a 500x282 root, but every frame of an animation \
             has the Template's size 934x282"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn fps_caps_per_format() {
        assert_eq!(AnimatedFormat::Gif.max_fps(), 50);
        assert_eq!(AnimatedFormat::Apng.max_fps(), 90);
        assert_eq!(AnimatedFormat::DEFAULT.max_fps(), 90);
        let check =
            |fps, format| AnimationSpec::new(Duration::from_secs(1), fps, format).check_fps("card");
        let over = |fps, format| match check(fps, format) {
            Err(RenderError::OverCap {
                id,
                cap: RenderCap::Fps,
                limit,
                actual,
            }) if id == "card" => (limit, actual),
            other => panic!("expected an fps OverCap, got {other:?}"),
        };
        let webp = AnimatedFormat::Webp { quality: 80 };
        assert!(check(50, AnimatedFormat::Gif).is_ok());
        assert_eq!(over(51, AnimatedFormat::Gif), (50, 51));
        assert!(check(90, webp).is_ok());
        assert_eq!(over(91, webp), (90, 91));
        assert!(check(90, AnimatedFormat::Apng).is_ok());
        assert_eq!(over(91, AnimatedFormat::Apng), (90, 91));
        assert_eq!(over(u16::MAX, webp), (90, 65535));
    }

    #[test]
    fn img_01_no_font_message() {
        assert_eq!(
            RenderError::NoFont.to_string(),
            "[IMG-01] no font is registered: add one with `ImageConfig::font(..)` or enable the \
             `default-font` feature"
        );
    }

    #[test]
    fn img_02_unknown_image_message() {
        let err = RenderError::UnknownImage {
            id: "card".into(),
            image: "logo".into(),
        };
        assert_eq!(
            err.to_string(),
            "template `card` uses static image `logo`, which is not registered: add it with \
             `ImageConfig::image(..)`"
        );
        assert!(err.source().is_none());
    }

    #[test]
    fn render_error_failed() {
        let err = RenderError::failed(fmt::Error);
        assert!(err.source().is_some());
        assert!(!err.to_string().is_empty());
    }
}
