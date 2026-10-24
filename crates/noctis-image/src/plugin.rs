//! [`ImageConfig`], [`ImagePlugin`] and [`AppBuilderImageExt`]: how an App
//! gets its Renderer.

use std::sync::Arc;
use std::time::Duration;

use noctis_core::{AppBuilder, BuildViolation, Plugin};

use crate::ClassCheckMode;
use crate::assets::{StaticImage, StaticImages};
use crate::caps::RenderCaps;
use crate::fonts::{self, Font, FontRegistry, FontsConfig};
use crate::pool::{PoolConfig, RenderPool};
use crate::remote::{AssetFetcher, RemoteConfig};
use crate::renderer::{RendererConfig, TakumiRenderer, theme_stylesheet};
use crate::tw::Theme;

/// The accepted render pool thread counts.
const THREADS: std::ops::RangeInclusive<usize> = 1..=256;
/// The accepted render queue capacities.
const QUEUE_CAPACITY: std::ops::RangeInclusive<usize> = 1..=4096;
/// The default render queue capacity.
const DEFAULT_QUEUE_CAPACITY: usize = 64;
/// The smallest accepted size target in bytes.
const MIN_SIZE_TARGET: u64 = 1024;
/// The default size target: 2 MiB.
pub(crate) const DEFAULT_SIZE_TARGET: u64 = 2_097_152;
/// The accepted pixel caps, up to takumi's canvas limit (`64 << 20` pixels).
const CAP_PIXELS: std::ops::RangeInclusive<u64> = 1..=67_108_864;

/// The render settings of an App.
///
/// The render pool settings are frozen per process [`Runtime`]: every App on
/// one Runtime must pass equal ones (another one is a
/// `BuildViolation::ResourceConflict` naming the pool).
///
/// - `threads` (1..=256): the pool's thread count; by default half the
///   available parallelism, at least 1 (1 when it is unknown).
/// - `queue_capacity` (1..=4096, default 64): the admission units admitted
///   at once, queued plus running. A still weighs 1 unit, an animation
///   `ceil(frames / 8)` units (at most its share). A quarter of the capacity,
///   floored, is reserved for stills, so animations hold at most the other
///   three quarters, and a free pool thread always takes a queued still
///   before any animation. A job that does not fit fails at once with
///   `RenderError::QueueFull`.
///
/// The size budget settings and the caps are per App; Apps differing only in
/// them share one pool.
///
/// - `size_target` (at least 1024 bytes, default 2 MiB): every output fits
///   `min(Upload Budget, size_target)`, degrading fps, then size, then
///   quality, then (optionally) falling back to a still frame;
///   `RenderError::OverBudget` when nothing fits.
/// - `still_fallback` (default `true`): whether an animation that does not
///   fit may fall back to a still frame as the ladder's last step.
/// - `caps` (default [`RenderCaps::new`]: 4 MP per job, 1 MP per frame, 90
///   frames, 6 s): the per-job caps; a job over one fails before admission
///   with `RenderError::OverCap`. The pixel caps accept 1..=67108864, the
///   frame cap at least 1 and the duration cap at least 1 ms.
///
/// Out-of-range values fail `App::build` with a plugin violation.
///
/// Fonts ([`font`](Self::font), IMG-01) are process-scoped too: they form
/// the Runtime's font registry, so every App on one Runtime must register
/// equal fonts in the same order (another list, or none, is a
/// `BuildViolation::ResourceConflict` naming `FontRegistry`). Order is
/// priority. Registering a Template with no font at all (and `default-font`
/// off) fails `App::build` with `[IMG-01] no font is registered`.
///
/// Static images ([`image`](Self::image), IMG-02) are per App, decoded once
/// when the App builds.
///
/// Remote images ([`remote`](Self::remote), IMG-03) are per App too: the
/// [`RemoteConfig`] sets the origin allow-list (the Discord CDN by default),
/// the timeout, byte, pixel and per-job caps, the fetch concurrency, the
/// `AssetStore` TTL and the fallback image.
///
/// [`Runtime`]: noctis_core::Runtime
///
/// ```
/// use noctis_image::{Font, ImageConfig, RenderCaps, StaticImage};
///
/// static INTER: &[u8] = b"..."; // include_bytes!("../fonts/Inter.ttf")
/// const LOGO: StaticImage = StaticImage::new("logo", b"..."); // include_bytes!("logo.png")
///
/// let config = ImageConfig::new()
///     .threads(2)
///     .queue_capacity(16)
///     .size_target(1_048_576)
///     .still_fallback(false)
///     .caps(RenderCaps::new().frames(120))
///     .font(Font::from_static(INTER))
///     .image(LOGO);
/// assert_ne!(config, ImageConfig::default());
/// assert_eq!(ImageConfig::new(), ImageConfig::default());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageConfig {
    threads: usize,
    queue_capacity: usize,
    size_target: u64,
    still_fallback: bool,
    caps: RenderCaps,
    fonts: Vec<Font>,
    images: Vec<StaticImage>,
    remote: RemoteConfig,
}

impl ImageConfig {
    /// The defaults: half the available parallelism (at least 1) threads,
    /// a queue of 64 units, a 2 MiB size target, the still fallback on,
    /// the default [`RenderCaps`], no fonts or static images, and the
    /// default [`RemoteConfig`].
    pub fn new() -> Self {
        // Clamped so a very large host never builds an out-of-range default.
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| (n.get() / 2).clamp(1, *THREADS.end()));
        Self {
            threads,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            size_target: DEFAULT_SIZE_TARGET,
            still_fallback: true,
            caps: RenderCaps::new(),
            fonts: Vec::new(),
            images: Vec::new(),
            remote: RemoteConfig::new(),
        }
    }

    /// The render pool's thread count, 1..=256.
    #[must_use]
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    /// The admission units admitted at once (queued plus running),
    /// 1..=4096: a still weighs 1, an animation `ceil(frames / 8)`, and a
    /// quarter (floored) is reserved for stills.
    #[must_use]
    pub fn queue_capacity(mut self, queue_capacity: usize) -> Self {
        self.queue_capacity = queue_capacity;
        self
    }

    /// The largest output in bytes, at least 1024 (default 2 MiB); a render
    /// fits the smaller of it and the interaction's Upload Budget. The budget
    /// is per file (ATT-01); ATT-02 (25 MiB per request) still limits a whole
    /// message, and the 2 MiB default keeps up to 10 renders under it.
    #[must_use]
    pub fn size_target(mut self, bytes: u64) -> Self {
        self.size_target = bytes;
        self
    }

    /// Whether an animation that does not fit the size budget may fall back
    /// to a still frame (default `true`).
    #[must_use]
    pub fn still_fallback(mut self, enabled: bool) -> Self {
        self.still_fallback = enabled;
        self
    }

    /// The per-job caps (default [`RenderCaps::new`]); a job over one fails
    /// before admission with `RenderError::OverCap`.
    #[must_use]
    pub fn caps(mut self, caps: RenderCaps) -> Self {
        self.caps = caps;
        self
    }

    /// Registers `font` after the fonts already added: order is priority.
    ///
    /// Text with no `font-*` family draws with the first font, and every
    /// glyph a font lacks (or a [`Font::unicode_range`] excludes) falls back
    /// to the next family in order of first appearance, then to the bundled
    /// `"Inter"` (`default-font`), so mixed-script text needs no Template
    /// change. A Template's own family (`font-display`, …) is tried first.
    /// A font that cannot be loaded, an inverted range or a family name
    /// that is empty, over 128 bytes or has control characters fails
    /// `App::build` with `[IMG-01]`.
    #[must_use]
    pub fn font(mut self, font: Font) -> Self {
        self.fonts.push(font);
        self
    }

    /// Registers the static image `image` for `<img src={..} />`: PNG, JPEG
    /// or WebP, at most 16 MP (16777216 pixels). Its bytes are decoded once
    /// when the App builds; another format, a corrupt file, a larger one or
    /// one id registered with other bytes fails `App::build` with
    /// `[IMG-02]`. Registering the same handle twice is a no-op.
    #[must_use]
    pub fn image(mut self, image: StaticImage) -> Self {
        self.images.push(image);
        self
    }

    /// How the App resolves the remote images its Templates declare in
    /// `#[asset]` fields (default [`RemoteConfig::new`]: the Discord CDN
    /// allow-list, 5 s, 8 MiB, 8 at once, 16 per job, 4 MP, a 1 day cache
    /// TTL and the built-in fallback). Invalid origins and out-of-range
    /// values fail `App::build` with `[IMG-03]`.
    #[must_use]
    pub fn remote(mut self, remote: RemoteConfig) -> Self {
        self.remote = remote;
        self
    }

    fn pool_config(&self) -> PoolConfig {
        PoolConfig {
            threads: self.threads,
            queue_capacity: self.queue_capacity,
        }
    }
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The plugin that installs the App's takumi Renderer; added by
/// [`AppBuilderImageExt::image`] and [`AppBuilderImageExt::theme`].
///
/// Its install checks the [`ImageConfig`] ranges, font list and remote
/// config, gets the process-scoped render pool and font registry from the
/// App's Runtime, decodes the App's static images, builds its remote-image
/// fetcher (and fallback), parses the Theme into the renderer's stylesheet
/// and sets the Renderer. On a `Runtime::for_test()` App the
/// runtime class check (TW-01) is Deny: an unknown `Classes::dynamic` class
/// fails the render; elsewhere it warns once per distinct string.
#[derive(Debug)]
pub struct ImagePlugin {
    config: ImageConfig,
    theme: Theme,
}

impl ImagePlugin {
    /// Reports every out-of-range setting and font list problem; whether
    /// there was none.
    fn check_config(&self, app: &mut AppBuilder) -> bool {
        let ImageConfig {
            threads,
            queue_capacity,
            size_target,
            still_fallback: _,
            caps,
            ref fonts,
            images: _,
            ref remote,
        } = self.config;
        let mut in_range = true;
        if !THREADS.contains(&threads) {
            in_range = false;
            app.report(BuildViolation::plugin::<Self>(format!(
                "render pool threads {threads} is out of range: use 1..=256"
            )));
        }
        if !QUEUE_CAPACITY.contains(&queue_capacity) {
            in_range = false;
            app.report(BuildViolation::plugin::<Self>(format!(
                "render pool queue capacity {queue_capacity} is out of range: use 1..=4096"
            )));
        }
        if size_target < MIN_SIZE_TARGET {
            in_range = false;
            app.report(BuildViolation::plugin::<Self>(format!(
                "render size target {size_target} is out of range: use at least 1024 bytes"
            )));
        }
        for (name, pixels) in [
            ("job_pixels", caps.job_pixels),
            ("frame_pixels", caps.frame_pixels),
        ] {
            if !CAP_PIXELS.contains(&pixels) {
                in_range = false;
                app.report(BuildViolation::plugin::<Self>(format!(
                    "render cap {name} {pixels} is out of range: use 1..=67108864"
                )));
            }
        }
        if caps.frames == 0 {
            in_range = false;
            app.report(BuildViolation::plugin::<Self>(
                "render cap frames 0 is out of range: use at least 1",
            ));
        }
        if caps.duration < Duration::from_millis(1) {
            in_range = false;
            app.report(BuildViolation::plugin::<Self>(format!(
                "render cap duration {:?} is out of range: use at least 1 ms",
                caps.duration
            )));
        }
        for problem in fonts::validate(fonts).into_iter().chain(remote.validate()) {
            in_range = false;
            app.report(BuildViolation::plugin::<Self>(problem));
        }
        in_range
    }
}

impl Plugin for ImagePlugin {
    fn install(&self, app: &mut AppBuilder) {
        if !self.check_config(app) {
            return;
        }
        let ImageConfig {
            size_target,
            still_fallback,
            caps,
            ref fonts,
            ref images,
            ref remote,
            ..
        } = self.config;
        let pool = match app
            .current_runtime()
            .resource_or_init::<RenderPool>(self.config.pool_config())
        {
            Ok(pool) => pool,
            Err(conflict) => {
                app.report(BuildViolation::ResourceConflict(conflict));
                return;
            }
        };
        if let Some(error) = pool.build_error() {
            app.report(BuildViolation::plugin::<Self>(format!(
                "the render pool could not be started: {error}"
            )));
            return;
        }
        let registry = match app
            .current_runtime()
            .resource_or_init::<FontRegistry>(FontsConfig {
                fonts: fonts.clone(),
                bundled: cfg!(feature = "default-font"),
            }) {
            Ok(registry) => registry,
            Err(conflict) => {
                app.report(BuildViolation::ResourceConflict(conflict));
                return;
            }
        };
        if let Some(error) = registry.build_error() {
            app.report(BuildViolation::plugin::<Self>(error.to_owned()));
            return;
        }
        let images = match StaticImages::decode(images) {
            Ok(images) => images,
            Err(error) => {
                app.report(BuildViolation::plugin::<Self>(error));
                return;
            }
        };
        let remote = match AssetFetcher::new(remote) {
            Ok(remote) => remote,
            Err(error) => {
                app.report(BuildViolation::plugin::<Self>(error));
                return;
            }
        };
        let stylesheet = match theme_stylesheet(&self.theme) {
            Ok(stylesheet) => stylesheet,
            Err(error) => {
                app.report(BuildViolation::plugin::<Self>(format!(
                    "the Theme is not valid CSS for the renderer: {error}"
                )));
                return;
            }
        };
        let mode = if app.current_runtime().is_test() {
            ClassCheckMode::Deny
        } else {
            ClassCheckMode::Warn
        };
        app.renderer(Arc::new(TakumiRenderer::new(RendererConfig {
            pool,
            fonts: registry,
            images,
            theme: self.theme.clone(),
            stylesheet,
            mode,
            size_target,
            still_fallback,
            caps,
            remote,
        })));
    }
}

mod sealed {
    pub trait Sealed {}

    impl Sealed for noctis_core::AppBuilder {}
}

/// `.image(..)` and `.theme(..)` on the [`AppBuilder`]: each call updates
/// the App's one [`ImagePlugin`], adding it (default [`ImageConfig`],
/// [`Theme::empty`]) on first use. The last call of each kind wins, their
/// order does not matter, and there is never a `DuplicatePlugin`.
///
/// Sealed: only the `AppBuilder` implements it.
///
/// ```
/// use noctis_core::App;
/// use noctis_image::tw::Theme;
/// use noctis_image::{AppBuilderImageExt, ImageConfig};
///
/// let theme = Theme::parse("@theme { --color-brand: #5865f2; }")?;
/// let mut builder = App::builder();
/// builder.theme(theme).image(ImageConfig::new().threads(2));
/// # Ok::<(), noctis_image::tw::ThemeError>(())
/// ```
pub trait AppBuilderImageExt: sealed::Sealed {
    /// Sets the render config of the App's [`ImagePlugin`].
    fn image(&mut self, config: ImageConfig) -> &mut Self;

    /// Sets the Theme the App's Renderer draws with.
    fn theme(&mut self, theme: Theme) -> &mut Self;
}

impl AppBuilderImageExt for AppBuilder {
    fn image(&mut self, config: ImageConfig) -> &mut Self {
        match self.plugin_mut::<ImagePlugin>() {
            Some(plugin) => plugin.config = config,
            None => {
                self.plugin(ImagePlugin {
                    config,
                    theme: Theme::empty(),
                });
            }
        }
        self
    }

    fn theme(&mut self, theme: Theme) -> &mut Self {
        match self.plugin_mut::<ImagePlugin>() {
            Some(plugin) => plugin.theme = theme,
            None => {
                self.plugin(ImagePlugin {
                    config: ImageConfig::new(),
                    theme,
                });
            }
        }
        self
    }
}

#[cfg(test)]
mod tests;
