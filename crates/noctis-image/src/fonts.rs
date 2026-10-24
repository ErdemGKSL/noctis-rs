//! Fonts (FR-35, IMG-01): [`Font`], registered in priority order with
//! `ImageConfig::font`, and the process-scoped [`FontRegistry`] Resource
//! (AD-24) every Renderer of one Runtime draws with.
//!
//! Priority is call order. A Template's own `font-*` family is tried first;
//! every glyph it lacks, and all text without a family, walks the fallback
//! chain: the registered families in order of first appearance, then the
//! bundled last-resort `"Inter"` (`default-font`). A font limited with
//! [`Font::unicode_range`] serves only those codepoints, so mixed-script text
//! needs no Template change.
//!
//! With the `emoji` feature, the bundled Twemoji Mozilla `COLRv0` font
//! (IMG-04; OFL-1.1 font, CC-BY-4.0 Twemoji graphics, see `NOTICE`) is
//! registered after Inter as a last resort under the family `"Twemoji"`.
//! It is never part of the chain: takumi sends emoji-presentation runs to
//! color fonts first and text runs to non-color fonts first, so author fonts
//! and text-presentation digits keep priority. It is not a text font, so it
//! alone never satisfies [`FontRegistry::is_empty`].

use std::fmt;
use std::ops::RangeInclusive;
use std::sync::Arc;

use takumi::prelude::{
    FontFamily, FontOverride, FontResource, FontSource, Fonts, UnicodeRange, UnicodeRangeSet,
};

/// The longest family override, in bytes.
const MAX_FAMILY_BYTES: usize = 128;

/// The family the bundled font is registered under.
#[cfg(feature = "default-font")]
pub(crate) const DEFAULT_FAMILY: &str = "Inter";

/// The bundled font: Inter 4.1, variable, WOFF2 (OFL-1.1; see `NOTICE`).
#[cfg(feature = "default-font")]
pub(crate) static DEFAULT_FONT: &[u8] = include_bytes!("../fonts/InterVariable.woff2");

/// The family the bundled emoji font is registered under.
#[cfg(feature = "emoji")]
pub(crate) const EMOJI_FAMILY: &str = "Twemoji";

/// The bundled emoji font: Twemoji Mozilla (twemoji-colr-font 15.0.3),
/// `COLRv0`, WOFF2 (OFL-1.1 font, CC-BY-4.0 graphics; see `NOTICE`).
#[cfg(feature = "emoji")]
static EMOJI_FONT: &[u8] = include_bytes!("../fonts/TwemojiMozilla.woff2");

/// A font's bytes: borrowed for the whole process, or shared.
#[derive(Clone)]
enum FontBytes {
    Static(&'static [u8]),
    Shared(Arc<[u8]>),
}

impl FontBytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Static(bytes) => bytes,
            Self::Shared(bytes) => bytes,
        }
    }

    /// The takumi source, without copying the bytes.
    fn source(&self) -> FontSource<'static> {
        match self {
            Self::Static(bytes) => FontSource::from_static(bytes),
            Self::Shared(bytes) => FontSource::from_shared(Arc::new(Arc::clone(bytes))),
        }
    }
}

/// A font file (TTF, OTF, TTC or WOFF2) for image Templates, registered
/// with `ImageConfig::font`.
///
/// Fonts are tried in registration order. [`family`](Self::family)
/// registers the font under another family name than its embedded one, and
/// [`unicode_range`](Self::unicode_range) limits the codepoints it serves
/// (every codepoint by default).
///
/// Two fonts are equal when their bytes (by blake3 digest), family override
/// and ranges are. `Debug` never prints the bytes, only their length and
/// digest prefix.
///
/// ```
/// use noctis_image::Font;
///
/// static KANA: &[u8] = b"\0\x01\0\0"; // the font file, e.g. `include_bytes!`
/// let font = Font::from_static(KANA)
///     .family("Kana")
///     .unicode_range('\u{3041}'..='\u{3096}')
///     .unicode_range('\u{30A1}'..='\u{30FA}');
/// assert_eq!(font, Font::from_bytes(KANA.to_vec()).family("Kana")
///     .unicode_range('\u{3041}'..='\u{3096}')
///     .unicode_range('\u{30A1}'..='\u{30FA}'));
/// assert!(format!("{font:?}").contains("len: 4"));
/// ```
#[derive(Clone)]
pub struct Font {
    bytes: FontBytes,
    digest: [u8; 32],
    family: Option<String>,
    ranges: Vec<RangeInclusive<char>>,
}

impl Font {
    /// A font over bytes that live as long as the process, such as
    /// `include_bytes!`; they are never copied.
    pub fn from_static(bytes: &'static [u8]) -> Self {
        Self::of(FontBytes::Static(bytes))
    }

    /// A font over shared bytes, such as a file the bot read at startup.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Self {
        Self::of(FontBytes::Shared(bytes.into()))
    }

    fn of(bytes: FontBytes) -> Self {
        let digest = *blake3::hash(bytes.as_slice()).as_bytes();
        Self {
            bytes,
            digest,
            family: None,
            ranges: Vec::new(),
        }
    }

    /// Registers the font under `name` instead of its embedded family name:
    /// 1..=128 bytes, no control characters (checked at `App::build`, IMG-01).
    #[must_use]
    pub fn family(mut self, name: impl Into<String>) -> Self {
        self.family = Some(name.into());
        self
    }

    /// Adds `range` to the codepoints the font serves; without any range it
    /// serves every codepoint. An inverted range fails `App::build` (IMG-01).
    #[must_use]
    pub fn unicode_range(mut self, range: RangeInclusive<char>) -> Self {
        self.ranges.push(range);
        self
    }

    /// The length of the font file in bytes.
    fn len(&self) -> usize {
        self.bytes.as_slice().len()
    }

    /// The takumi resource of this font.
    fn resource(&self) -> FontResource<'static> {
        let mut resource = FontResource::new(self.bytes.source());
        if let Some(family) = &self.family {
            resource = resource.override_info(FontOverride {
                family_name: Some(Arc::from(family.as_str())),
                ..FontOverride::default()
            });
        }
        if !self.ranges.is_empty() {
            resource = resource.unicode_range(
                self.ranges
                    .iter()
                    .map(|r| UnicodeRange::new(u32::from(*r.start()), u32::from(*r.end())))
                    .collect::<UnicodeRangeSet>(),
            );
        }
        resource
    }
}

impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest && self.family == other.family && self.ranges == other.ranges
    }
}

impl Eq for Font {}

/// A range as `U+XXXX..=U+YYYY`.
struct UPlus<'a>(&'a RangeInclusive<char>);

impl fmt::Display for UPlus<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "U+{:04X}..=U+{:04X}",
            u32::from(*self.0.start()),
            u32::from(*self.0.end())
        )
    }
}

impl fmt::Debug for UPlus<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// The first 8 hex characters of `digest`.
pub(crate) fn digest_prefix(digest: &[u8; 32]) -> String {
    blake3::Hash::from_bytes(*digest).to_hex()[..8].to_owned()
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("family", &self.family)
            .field("ranges", &self.ranges.iter().map(UPlus).collect::<Vec<_>>())
            .field("len", &self.len())
            .field("blake3", &digest_prefix(&self.digest))
            .finish_non_exhaustive()
    }
}

/// The checks of the `ImageConfig::font` list that run before anything is
/// loaded (IMG-01): family overrides and Unicode ranges. One message per
/// problem; `#i` is the font's 0-based position in call order.
pub(crate) fn validate(fonts: &[Font]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, font) in fonts.iter().enumerate() {
        if let Some(family) = &font.family
            && (family.is_empty()
                || family.len() > MAX_FAMILY_BYTES
                || family.chars().any(char::is_control))
        {
            problems.push(format!(
                "[IMG-01] font #{i}: family name {family:?} must be 1..=128 bytes without control \
                 characters"
            ));
        }
        for r in &font.ranges {
            if r.start() > r.end() {
                problems.push(format!(
                    "[IMG-01] font #{i}: unicode range {} is inverted",
                    UPlus(r)
                ));
            }
        }
    }
    problems
}

/// The frozen configuration of the [`FontRegistry`]: the fonts in priority
/// order, whether the bundled font is registered (`default-font`) and
/// whether the bundled emoji font is (`emoji`). Equality is order-sensitive,
/// because order is priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontsConfig {
    pub(crate) fonts: Vec<Font>,
    pub(crate) bundled: bool,
    pub(crate) emoji: bool,
}

/// The process-scoped font registry: every Renderer of one Runtime draws
/// with it.
pub(crate) struct FontRegistry {
    fonts: Fonts,
    /// The registered families in order of first appearance.
    families: Vec<String>,
    /// [`families`](Self::families) as takumi's per-render fallback chain;
    /// `None` when empty.
    chain: Option<FontFamily>,
    bundled: bool,
    /// Whether the bundled emoji font is registered.
    emoji: bool,
    /// The first load error (reported at install, since
    /// [`Resource::init`](noctis_core::Resource::init) is infallible).
    error: Option<String>,
}

impl noctis_core::Resource for FontRegistry {
    type Config = FontsConfig;

    fn init(config: &FontsConfig) -> Self {
        let mut fonts = Fonts::default();
        let mut families: Vec<String> = Vec::new();
        let mut error = None;
        for (i, font) in config.fonts.iter().enumerate() {
            let why = match fonts.register(font.resource()) {
                Ok(registered) if registered.is_empty() => "it contains no font face".to_owned(),
                Ok(registered) => {
                    for family in registered {
                        if !families.contains(&family.name) {
                            families.push(family.name);
                        }
                    }
                    continue;
                }
                Err(err) => err.to_string(),
            };
            error = Some(format!(
                "[IMG-01] font #{i} ({} bytes) could not be loaded: {why}",
                font.len()
            ));
            break;
        }
        let bundled = config.bundled && error.is_none() && register_bundled(&mut fonts, &mut error);
        let emoji = config.emoji && error.is_none() && register_emoji(&mut fonts, &mut error);
        let chain = (!families.is_empty()).then(|| FontFamily::from_names(families.clone()));
        Self {
            fonts,
            families,
            chain,
            bundled,
            emoji,
            error,
        }
    }
}

/// Registers the bundled font as the last resort; whether it registered.
#[cfg(feature = "default-font")]
fn register_bundled(fonts: &mut Fonts, error: &mut Option<String>) -> bool {
    let resource = FontResource::new(FontSource::from_static(DEFAULT_FONT))
        .override_info(FontOverride {
            family_name: Some(Arc::from(DEFAULT_FAMILY)),
            ..FontOverride::default()
        })
        .last_resort();
    match fonts.register(resource) {
        Ok(registered) if !registered.is_empty() => true,
        Ok(_) => {
            *error = Some("[IMG-01] the bundled font contains no font face".to_owned());
            false
        }
        Err(err) => {
            *error = Some(format!(
                "[IMG-01] the bundled font could not be loaded: {err}"
            ));
            false
        }
    }
}

/// Without `default-font` there is nothing to bundle.
#[cfg(not(feature = "default-font"))]
fn register_bundled(_: &mut Fonts, _: &mut Option<String>) -> bool {
    false
}

/// Registers the bundled emoji font as a last resort, after the bundled
/// text font; whether it registered.
#[cfg(feature = "emoji")]
fn register_emoji(fonts: &mut Fonts, error: &mut Option<String>) -> bool {
    let resource = FontResource::new(FontSource::from_static(EMOJI_FONT))
        .override_info(FontOverride {
            family_name: Some(Arc::from(EMOJI_FAMILY)),
            ..FontOverride::default()
        })
        .last_resort();
    match fonts.register(resource) {
        Ok(registered) if !registered.is_empty() => true,
        Ok(_) => {
            *error = Some(
                "[IMG-04] the bundled emoji font could not be loaded: it contains no font face"
                    .to_owned(),
            );
            false
        }
        Err(err) => {
            *error = Some(format!(
                "[IMG-04] the bundled emoji font could not be loaded: {err}"
            ));
            false
        }
    }
}

/// Without `emoji` there is no emoji font to bundle.
#[cfg(not(feature = "emoji"))]
fn register_emoji(_: &mut Fonts, _: &mut Option<String>) -> bool {
    false
}

impl FontRegistry {
    /// The first font that could not be loaded, as an `[IMG-01]` message.
    pub(crate) fn build_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether no text font is registered: no family and no bundled font.
    /// The emoji font does not count, so emoji alone is still
    /// `RenderError::NoFont`.
    pub(crate) fn is_empty(&self) -> bool {
        self.families().is_empty() && !self.bundled
    }

    /// Whether the bundled emoji font is registered (IMG-04).
    pub(crate) fn has_emoji(&self) -> bool {
        self.emoji
    }

    /// The registered author families in priority order (the bundled one
    /// excluded).
    pub(crate) fn families(&self) -> &[String] {
        &self.families
    }

    /// The takumi font context.
    pub(crate) fn fonts(&self) -> &Fonts {
        &self.fonts
    }

    /// The per-render fallback chain; `None` without author families.
    pub(crate) fn chain(&self) -> Option<&FontFamily> {
        self.chain.as_ref()
    }
}

impl fmt::Debug for FontRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontRegistry")
            .field("families", &self.families)
            .field("bundled", &self.bundled)
            .field("emoji", &self.emoji)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
