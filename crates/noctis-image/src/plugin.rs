//! [`ImageConfig`], [`ImagePlugin`] and [`AppBuilderImageExt`]: how an App
//! gets its Renderer.

use std::sync::Arc;

use noctis_core::{AppBuilder, BuildViolation, Plugin};

use crate::ClassCheckMode;
use crate::pool::{PoolConfig, RenderPool};
use crate::renderer::{TakumiRenderer, theme_stylesheet};
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

/// The render settings of an App.
///
/// The render pool settings are frozen per process [`Runtime`]: every App on
/// one Runtime must pass equal ones (another one is a
/// `BuildViolation::ResourceConflict` naming the pool).
///
/// - `threads` (1..=256): the pool's thread count; by default half the
///   available parallelism, at least 1 (1 when it is unknown).
/// - `queue_capacity` (1..=4096, default 64): the most renders admitted at
///   once, queued plus running; one more fails at once with
///   `RenderError::QueueFull`.
///
/// The size budget settings are per App; Apps differing only in them share
/// one pool.
///
/// - `size_target` (at least 1024 bytes, default 2 MiB): every output fits
///   `min(Upload Budget, size_target)`, degrading fps, then size, then
///   quality, then (optionally) falling back to a still frame;
///   `RenderError::OverBudget` when nothing fits.
/// - `still_fallback` (default `true`): whether an animation that does not
///   fit may fall back to a still frame as the ladder's last step.
///
/// Out-of-range values fail `App::build` with a plugin violation.
///
/// [`Runtime`]: noctis_core::Runtime
///
/// ```
/// use noctis_image::ImageConfig;
///
/// let config = ImageConfig::new()
///     .threads(2)
///     .queue_capacity(16)
///     .size_target(1_048_576)
///     .still_fallback(false);
/// assert_ne!(config, ImageConfig::default());
/// assert_eq!(ImageConfig::new(), ImageConfig::default());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageConfig {
    threads: usize,
    queue_capacity: usize,
    size_target: u64,
    still_fallback: bool,
}

impl ImageConfig {
    /// The defaults: half the available parallelism (at least 1) threads,
    /// a queue of 64, a 2 MiB size target and the still fallback on.
    pub fn new() -> Self {
        // Clamped so a very large host never builds an out-of-range default.
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| (n.get() / 2).clamp(1, *THREADS.end()));
        Self {
            threads,
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            size_target: DEFAULT_SIZE_TARGET,
            still_fallback: true,
        }
    }

    /// The render pool's thread count, 1..=256.
    #[must_use]
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    /// The most renders admitted at once (queued plus running), 1..=4096.
    #[must_use]
    pub fn queue_capacity(mut self, queue_capacity: usize) -> Self {
        self.queue_capacity = queue_capacity;
        self
    }

    /// The largest output in bytes, at least 1024 (default 2 MiB); a render
    /// fits the smaller of it and the interaction's Upload Budget.
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
/// Its install checks the [`ImageConfig`] ranges, gets the process-scoped
/// render pool from the App's Runtime, parses the Theme into the renderer's
/// stylesheet and sets the Renderer. On a `Runtime::for_test()` App the
/// runtime class check (TW-01) is Deny: an unknown `Classes::dynamic` class
/// fails the render; elsewhere it warns once per distinct string.
#[derive(Debug)]
pub struct ImagePlugin {
    config: ImageConfig,
    theme: Theme,
}

impl Plugin for ImagePlugin {
    fn install(&self, app: &mut AppBuilder) {
        let ImageConfig {
            threads,
            queue_capacity,
            size_target,
            still_fallback,
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
        if !in_range {
            return;
        }
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
        app.renderer(Arc::new(TakumiRenderer::new(
            pool,
            self.theme.clone(),
            stylesheet,
            mode,
            size_target,
            still_fallback,
        )));
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
