//! Template pixel snapshots (FR-51): [`Tolerance`] and the runtime behind
//! [`assert_render_snapshot!`](crate::assert_render_snapshot).
//!
//! A snapshot renders a Template through the App's `Renderer` port (never
//! `noctis-image` itself, AD-24) as a still PNG, decodes it with `png` and
//! compares it with `<dir>/<name>.png` pixel by pixel.

use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use noctis_ports::{Format, OutputFormat, RenderError, RenderJob, Renderer, Size, Template};

/// How far a render may drift from its stored pixel snapshot.
///
/// A pixel *differs* when any of its R, G, B or A values differs by more
/// than [`channel_delta`](Self::channel_delta); a snapshot matches when both
/// images have the same size and at most
/// [`max_ratio`](Self::max_ratio) of their pixels differ.
///
/// ```
/// use noctis_testkit::Tolerance;
///
/// let tolerance = Tolerance::ratio(0.01).channel(2);
/// assert_eq!(tolerance.max_ratio(), 0.01);
/// assert_eq!(tolerance.channel_delta(), 2);
/// assert_eq!(Tolerance::default(), Tolerance::EXACT);
/// assert_eq!(Tolerance::from(0.01), Tolerance::ratio(0.01));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    max_ratio: f64,
    channel: u8,
}

impl Tolerance {
    /// No differing pixel at all (ratio 0.0, channel delta 0).
    pub const EXACT: Self = Self {
        max_ratio: 0.0,
        channel: 0,
    };

    /// At most `max_ratio` (0.0..=1.0) of the pixels may differ, with a
    /// channel delta of 0.
    ///
    /// # Panics
    ///
    /// When `max_ratio` is not finite or outside 0.0..=1.0.
    #[track_caller]
    #[must_use]
    pub fn ratio(max_ratio: f64) -> Self {
        assert!(
            max_ratio.is_finite() && (0.0..=1.0).contains(&max_ratio),
            "tolerance must be a finite ratio in 0.0..=1.0, got {max_ratio}"
        );
        Self {
            max_ratio,
            channel: 0,
        }
    }

    /// The same ratio, where a pixel differs only when one of its channels
    /// differs by more than `delta`.
    #[must_use]
    pub const fn channel(self, delta: u8) -> Self {
        Self {
            channel: delta,
            ..self
        }
    }

    /// The largest share of differing pixels, 0.0..=1.0.
    #[must_use]
    pub const fn max_ratio(self) -> f64 {
        self.max_ratio
    }

    /// The largest per-channel difference that does not count.
    #[must_use]
    pub const fn channel_delta(self) -> u8 {
        self.channel
    }
}

impl Default for Tolerance {
    fn default() -> Self {
        Self::EXACT
    }
}

impl From<f64> for Tolerance {
    #[track_caller]
    fn from(max_ratio: f64) -> Self {
        Self::ratio(max_ratio)
    }
}

impl fmt::Display for Tolerance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.2} % of the pixels (channel delta {})",
            self.max_ratio * 100.0,
            self.channel
        )
    }
}

/// The environment switches of a snapshot run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SnapshotEnv {
    /// `NOCTIS_UPDATE_SNAPSHOTS`: always (re)write the snapshot and pass.
    pub(crate) update: bool,
    /// `CI`: a missing snapshot fails instead of being written.
    pub(crate) ci: bool,
}

impl SnapshotEnv {
    /// Reads `NOCTIS_UPDATE_SNAPSHOTS` and `CI`.
    pub(crate) fn from_env() -> Self {
        Self {
            update: flag(std::env::var_os("NOCTIS_UPDATE_SNAPSHOTS").as_deref()),
            ci: flag(std::env::var_os("CI").as_deref()),
        }
    }
}

/// A flag is on when it is set and not `""`, `"0"` or `"false"`.
fn flag(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !matches!(value.to_str(), Some("" | "0" | "false")))
}

/// What a passing snapshot did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The render matches the stored snapshot within the tolerance.
    Matched {
        /// The pixels that differ.
        differing: u64,
    },
    /// No snapshot existed (locally): the render was stored.
    Written,
    /// Update mode: the render replaced the stored snapshot.
    Updated,
}

/// Why a snapshot failed.
#[derive(Debug)]
pub(crate) enum SnapshotError {
    InvalidName {
        name: String,
        why: &'static str,
    },
    NoRenderer,
    Render(RenderError),
    NotStillPng(OutputFormat),
    Downscaled {
        declared: Size,
        used: Size,
    },
    DecodeRendered {
        error: String,
        new: PathBuf,
    },
    DecodeStored {
        path: PathBuf,
        error: String,
        new: PathBuf,
    },
    MissingInCi {
        path: PathBuf,
        new: PathBuf,
    },
    SizeMismatch {
        name: String,
        stored: (u32, u32),
        rendered: (u32, u32),
        new: PathBuf,
    },
    Mismatch {
        name: String,
        differing: u64,
        total: u64,
        tolerance: Tolerance,
        new: PathBuf,
    },
    Io {
        path: PathBuf,
        error: io::Error,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UPDATE: &str = "set NOCTIS_UPDATE_SNAPSHOTS=1 to accept it";
        match self {
            Self::InvalidName { name, why } => write!(
                f,
                "invalid snapshot name {name:?}: {why}; a name is 1..=64 bytes of [A-Za-z0-9_-]"
            ),
            Self::NoRenderer => f.write_str(
                "no Renderer: build the App with the `image` feature and `.image(..)`/`.theme(..)`",
            ),
            Self::Render(error) => write!(f, "the render failed: {error}"),
            Self::NotStillPng(format) => write!(
                f,
                "a pixel snapshot needs a still PNG, but the Renderer returned {format:?}"
            ),
            Self::Downscaled { declared, used } => write!(
                f,
                "the size budget downscaled the render from {}x{} to {}x{}",
                declared.width(),
                declared.height(),
                used.width(),
                used.height()
            ),
            Self::DecodeRendered { error, new } => write!(
                f,
                "cannot decode the rendered PNG: {error}; its bytes are at {}",
                new.display()
            ),
            Self::DecodeStored { path, error, new } => write!(
                f,
                "cannot decode stored snapshot {}: {error}; the actual render is at {} ({UPDATE})",
                path.display(),
                new.display()
            ),
            Self::MissingInCi { path, new } => write!(
                f,
                "missing snapshot (CI): {} does not exist and CI never writes one; the actual \
                 render is at {} (commit it as the snapshot after review)",
                path.display(),
                new.display()
            ),
            Self::SizeMismatch {
                name,
                stored,
                rendered,
                new,
            } => write!(
                f,
                "pixel snapshot `{name}`: the stored image is {}x{} but the render is {}x{}; \
                 the actual render is at {} ({UPDATE})",
                stored.0,
                stored.1,
                rendered.0,
                rendered.1,
                new.display()
            ),
            Self::Mismatch {
                name,
                differing,
                total,
                tolerance,
                new,
            } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a percentage for a message; pixel counts are far below 2^52"
                )]
                let percent = *differing as f64 * 100.0 / *total as f64;
                write!(
                    f,
                    "pixel snapshot `{name}` differs: {differing}/{total} pixels ({percent:.2} %) \
                     are over the tolerance of {tolerance}; the actual render is at {} ({UPDATE})",
                    new.display()
                )
            }
            Self::Io { path, error } => write!(f, "cannot access {}: {error}", path.display()),
        }
    }
}

/// Checks a snapshot name: 1..=64 bytes of `[A-Za-z0-9_-]`.
fn check_name(name: &str) -> Result<(), SnapshotError> {
    let why = if name.is_empty() {
        "it is empty"
    } else if name.len() > 64 {
        "it is over 64 bytes"
    } else if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        "it has a byte outside [A-Za-z0-9_-]"
    } else {
        return Ok(());
    };
    Err(SnapshotError::InvalidName {
        name: name.to_owned(),
        why,
    })
}

/// A decoded image as 8-bit RGBA.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Rgba {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

/// Decodes a PNG (its first frame) to 8-bit RGBA: palettes and low bit
/// depths are expanded, 16-bit channels stripped, gray replicated, and a
/// missing alpha is 255.
pub(crate) fn decode(bytes: &[u8]) -> Result<Rgba, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "the image is too large".to_owned())?;
    let mut buf = vec![0; size];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let (color, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return Err(format!("unsupported bit depth {depth:?}"));
    }
    let pixels: Vec<u8> = match color {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("an unexpanded palette".to_owned()),
    };
    let expected = u64::from(info.width) * u64::from(info.height) * 4;
    if pixels.len() as u64 != expected {
        return Err(format!(
            "{} bytes of pixels for {}x{}",
            pixels.len(),
            info.width,
            info.height
        ));
    }
    Ok(Rgba {
        width: info.width,
        height: info.height,
        pixels,
    })
}

/// The pixels of two same-size images that differ by more than `channel`
/// in any of R, G, B or A.
fn differing(a: &[u8], b: &[u8], channel: u8) -> u64 {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(p, q)| {
            p.iter()
                .zip(q.iter())
                .any(|(x, y)| x.abs_diff(*y) > channel)
        })
        .count() as u64
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    let io = |error| SnapshotError::Io {
        path: path.to_owned(),
        error,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    std::fs::write(path, bytes).map_err(io)
}

fn remove_stale(path: &Path) -> Result<(), SnapshotError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SnapshotError::Io {
            path: path.to_owned(),
            error,
        }),
    }
}

/// Renders `job` as a still PNG and compares it with `<dir>/<name>.png`.
///
/// The job is forced to [`Format::Png`], no animation and an unlimited
/// upload budget; `declared` is the Template's size, which the render must
/// keep.
pub(crate) async fn check(
    renderer: Option<&Arc<dyn Renderer>>,
    name: &str,
    mut job: RenderJob,
    declared: Size,
    tolerance: Tolerance,
    dir: &Path,
    env: SnapshotEnv,
) -> Result<Outcome, SnapshotError> {
    check_name(name)?;
    let renderer = renderer.ok_or(SnapshotError::NoRenderer)?;
    job.format = Format::Png;
    job.animation = None;
    job.upload_budget = u64::MAX;
    let output = renderer.render(job).await.map_err(SnapshotError::Render)?;
    if output.format != OutputFormat::Png {
        return Err(SnapshotError::NotStillPng(output.format));
    }
    if output.settings_used.size != declared {
        return Err(SnapshotError::Downscaled {
            declared,
            used: output.settings_used.size,
        });
    }
    let path = dir.join(format!("{name}.png"));
    let new = dir.join(format!("{name}.new.png"));
    let bytes = &output.bytes[..];
    let actual = match decode(bytes) {
        Ok(actual) => actual,
        Err(error) => {
            write(&new, bytes)?;
            return Err(SnapshotError::DecodeRendered { error, new });
        }
    };
    if env.update {
        write(&path, bytes)?;
        remove_stale(&new)?;
        return Ok(Outcome::Updated);
    }
    let stored = match std::fs::read(&path) {
        Ok(stored) => stored,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if env.ci {
                write(&new, bytes)?;
                return Err(SnapshotError::MissingInCi { path, new });
            }
            write(&path, bytes)?;
            remove_stale(&new)?;
            return Ok(Outcome::Written);
        }
        Err(error) => return Err(SnapshotError::Io { path, error }),
    };
    let stored = match decode(&stored) {
        Ok(stored) => stored,
        Err(error) => {
            write(&new, bytes)?;
            return Err(SnapshotError::DecodeStored { path, error, new });
        }
    };
    if (stored.width, stored.height) != (actual.width, actual.height) {
        write(&new, bytes)?;
        return Err(SnapshotError::SizeMismatch {
            name: name.to_owned(),
            stored: (stored.width, stored.height),
            rendered: (actual.width, actual.height),
            new,
        });
    }
    let total = u64::from(actual.width) * u64::from(actual.height);
    let differing = differing(&stored.pixels, &actual.pixels, tolerance.channel);
    #[allow(
        clippy::cast_precision_loss,
        reason = "pixel counts are far below 2^52, so the comparison is exact"
    )]
    let within = differing as f64 <= tolerance.max_ratio * total as f64;
    if !within {
        write(&new, bytes)?;
        return Err(SnapshotError::Mismatch {
            name: name.to_owned(),
            differing,
            total,
            tolerance,
            new,
        });
    }
    remove_stale(&new)?;
    Ok(Outcome::Matched { differing })
}

/// The runtime of [`assert_render_snapshot!`](crate::assert_render_snapshot):
/// renders `template` through `renderer` and compares it with
/// `<dir>/<name>.png`.
///
/// # Panics
///
/// On every failure, like `assert_eq!`: an invalid name, no Renderer, a
/// render error, a non-PNG or downscaled output, a missing snapshot on CI,
/// an undecodable stored snapshot, or pixels over `tolerance`.
#[doc(hidden)]
pub async fn render_snapshot<'a, T: Template>(
    renderer: impl Into<Option<&'a Arc<dyn Renderer>>>,
    name: &str,
    template: T,
    tolerance: Tolerance,
    dir: &str,
) {
    if let Err(error) = check_name(name) {
        panic!("{error}");
    }
    let job = match RenderJob::of(&template) {
        Ok(job) => job,
        Err(error) => panic!("pixel snapshot `{name}`: cannot encode the Template: {error}"),
    };
    let outcome = check(
        renderer.into(),
        name,
        job,
        T::SIZE,
        tolerance,
        Path::new(dir),
        SnapshotEnv::from_env(),
    )
    .await;
    if let Err(error) = outcome {
        panic!("{error}");
    }
}

/// Renders a Template through the App's `Renderer` port as a still PNG and
/// compares it with the stored pixel snapshot `tests/snapshots/<name>.png`
/// of the calling crate (FR-51). Valid only inside an async fn.
///
/// ```text
/// assert_render_snapshot!(renderer, "name", template);                    // Tolerance::EXACT
/// assert_render_snapshot!(renderer, "name", template, tolerance = 0.01);  // or a Tolerance
/// ```
///
/// - `renderer` is an `Option<&Arc<dyn Renderer>>` (`app.renderer()`) or a
///   `&Arc<dyn Renderer>`; `template` is a Template value registered on that
///   App (`Module::template::<T>()`).
/// - The job is forced to a still PNG with no animation and no upload
///   budget, so the view is drawn once with `Frame::STILL`; a JPEG, WebP or
///   animated Template snapshots its still PNG pixels.
/// - `name` is 1..=64 bytes of `[A-Za-z0-9_-]`, unique in the crate.
/// - The first run writes `<name>.png` (creating the directory). On a
///   mismatch, the render is written to `<name>.new.png` next to it; a match
///   removes a stale one. With `NOCTIS_UPDATE_SNAPSHOTS` on, the snapshot is
///   always rewritten. With `CI` on, a missing snapshot fails. A variable is
///   on when it is set and not `""`, `"0"` or `"false"`.
///
/// # Panics
///
/// On every failure, like `assert_eq!`.
#[macro_export]
macro_rules! assert_render_snapshot {
    ($renderer:expr, $name:expr, $template:expr, tolerance = $tolerance:expr $(,)?) => {
        $crate::__private::render_snapshot(
            $renderer,
            $name,
            $template,
            ::core::convert::Into::<$crate::Tolerance>::into($tolerance),
            ::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/tests/snapshots"),
        )
        .await
    };
    ($renderer:expr, $name:expr, $template:expr $(,)?) => {
        $crate::__private::render_snapshot(
            $renderer,
            $name,
            $template,
            $crate::Tolerance::EXACT,
            ::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/tests/snapshots"),
        )
        .await
    };
}

#[cfg(test)]
mod tests;
