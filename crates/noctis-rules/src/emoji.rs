//! Application emoji rules (EMO-01..04): the name grammar, the upload
//! formats sniffed by magic bytes, and the per-image byte cap.
//!
//! `#[derive(AppEmojis)]` runs these checks at `cargo check` on every
//! declared name and embedded file; `AppBuilder::emojis` runs
//! [`validate_emoji_set`] on the names of every registered set.
//!
//! ```
//! use noctis_rules::emoji::{EmojiFormat, validate_emoji_image, validate_emoji_name};
//! use noctis_rules::{Limits, RuleId};
//!
//! assert!(validate_emoji_name("st_online").is_ok());
//! assert_eq!(validate_emoji_name("x").unwrap_err().rule, RuleId::EMO_03);
//!
//! let png = b"\x89PNG\r\n\x1a\n....";
//! assert_eq!(validate_emoji_image("check", png, &Limits::default()), Ok(EmojiFormat::Png));
//! let webp = b"RIFF\0\0\0\0WEBPVP8 ";
//! assert_eq!(validate_emoji_image("check", webp, &Limits::default()).unwrap_err().rule, RuleId::EMO_04);
//! ```

use std::collections::HashSet;
use std::fmt;

use crate::{Actual, Bound, Limits, RuleId, Violation};

/// The shortest emoji name (EMO-03).
pub const NAME_MIN: usize = 2;
/// The longest emoji name (EMO-03).
pub const NAME_MAX: usize = 32;

/// An image format Noctis uploads as an application emoji (EMO-04).
///
/// Discord's "Image Data" names only PNG, JPEG and GIF data URIs, so these
/// are the three Noctis uploads; a GIF is an animated emoji.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmojiFormat {
    /// PNG (`\x89PNG\r\n\x1a\n`).
    Png,
    /// JPEG (`FF D8 FF`).
    Jpeg,
    /// GIF (`GIF87a` or `GIF89a`); the emoji is animated.
    Gif,
}

impl EmojiFormat {
    /// The format `bytes` start with, by magic bytes, or `None` for any
    /// other content (WebP, AVIF, SVG, text, …).
    ///
    /// ```
    /// use noctis_rules::emoji::EmojiFormat;
    ///
    /// assert_eq!(EmojiFormat::sniff(b"GIF89a\x01\x00"), Some(EmojiFormat::Gif));
    /// assert_eq!(EmojiFormat::sniff(b"<svg"), None);
    /// ```
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else {
            None
        }
    }

    /// Whether an emoji of this format is animated (only GIF).
    pub const fn is_animated(self) -> bool {
        matches!(self, Self::Gif)
    }

    /// The media type of the upload's data URI (`image/png`, …).
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
        }
    }

    /// The file extensions `#[derive(AppEmojis)]` looks for, in order.
    pub const EXTENSIONS: [&'static str; 3] = ["png", "gif", "jpg"];
}

impl fmt::Display for EmojiFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Gif => "GIF",
        })
    }
}

/// EMO-03: `name` is 2..=32 of `[A-Za-z0-9_]` (the IMG-04 grammar).
///
/// # Errors
///
/// An EMO-03 [`Violation`] whose path is the name (boxed: it is large).
pub fn validate_emoji_name(name: &str) -> Result<(), Box<Violation>> {
    let len = name.len();
    if !(NAME_MIN..=NAME_MAX).contains(&len) || !name.is_ascii() {
        return Err(Box::new(
            Violation::new(
                RuleId::EMO_03,
                name,
                Bound::Between {
                    min: NAME_MIN,
                    max: NAME_MAX,
                },
                Actual::Count(name.chars().count()),
            )
            .with_help("an emoji name is 2 to 32 of A-Z, a-z, 0-9 and _"),
        ));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
        return Err(Box::new(
            Violation::new(
                RuleId::EMO_03,
                name,
                Bound::Describe("only A-Z, a-z, 0-9 and _"),
                Actual::Text(bad.to_string()),
            )
            .with_help("an emoji name is 2 to 32 of A-Z, a-z, 0-9 and _"),
        ));
    }
    Ok(())
}

/// EMO-04 then EMO-02: the image of emoji `name` is PNG, JPEG or GIF
/// (sniffed by magic bytes) of at most [`Limits::emoji_bytes`] bytes.
///
/// # Errors
///
/// A boxed EMO-04 [`Violation`] for any other content, else an EMO-02 one for an
/// image over the cap.
pub fn validate_emoji_image(
    name: &str,
    bytes: &[u8],
    limits: &Limits,
) -> Result<EmojiFormat, Box<Violation>> {
    let Some(format) = EmojiFormat::sniff(bytes) else {
        return Err(Box::new(
            Violation::new(
                RuleId::EMO_04,
                name,
                Bound::Describe("a PNG, JPEG or GIF image"),
                Actual::Text(describe_head(bytes)),
            )
            .with_help("export the emoji as PNG (or GIF for an animated one)"),
        ));
    };
    if bytes.len() > limits.emoji_bytes {
        return Err(Box::new(
            Violation::new(
                RuleId::EMO_02,
                name,
                Bound::AtMost(limits.emoji_bytes),
                Actual::Count(bytes.len()),
            )
            .with_note("bytes per emoji image (256 KiB)")
            .with_help("shrink the image: a flat 128x128 PNG is about 1 KB"),
        ));
    }
    Ok(format)
}

/// The first bytes of an unsupported file, for a violation.
fn describe_head(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "an empty file".to_owned();
    }
    let head: String = bytes
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("a file starting {head}")
}

/// EMO-03 then EMO-01 over every name an application declares: each name
/// valid and unique, and at most [`Limits::application_emojis`] of them.
///
/// Returns every violation, in input order, the count last.
pub fn validate_emoji_set<'a>(
    names: impl IntoIterator<Item = &'a str>,
    limits: &Limits,
) -> Vec<Violation> {
    let mut seen = HashSet::new();
    let mut violations = Vec::new();
    for name in names {
        if let Err(v) = validate_emoji_name(name) {
            violations.push(*v);
        } else if !seen.insert(name) {
            violations.push(
                Violation::new(
                    RuleId::EMO_03,
                    name,
                    Bound::Describe("unique within the application"),
                    Actual::Text(name.to_owned()),
                )
                .with_help("give each emoji its own name"),
            );
        }
    }
    if seen.len() > limits.application_emojis {
        violations.push(Violation::new(
            RuleId::EMO_01,
            "emojis",
            Bound::AtMost(limits.application_emojis),
            Actual::Count(seen.len()),
        ));
    }
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emo_03_names() {
        for ok in ["ab", "st_online", "A1_b2", &"x".repeat(32)] {
            assert!(validate_emoji_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a", &"x".repeat(33), "no-dash", "sp ace", "çek"] {
            let v = validate_emoji_name(bad).unwrap_err();
            assert_eq!(v.rule, RuleId::EMO_03, "{bad}");
        }
        assert_eq!(
            validate_emoji_name("a-b").unwrap_err().to_string(),
            "[EMO-03] a-b: limit only A-Z, a-z, 0-9 and _, actual `-`\n  \
             = help: an emoji name is 2 to 32 of A-Z, a-z, 0-9 and _"
        );
    }

    #[test]
    fn emo_04_formats_are_sniffed() {
        assert_eq!(
            EmojiFormat::sniff(b"\x89PNG\r\n\x1a\nrest"),
            Some(EmojiFormat::Png)
        );
        assert_eq!(
            EmojiFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(EmojiFormat::Jpeg)
        );
        assert_eq!(EmojiFormat::sniff(b"GIF87a"), Some(EmojiFormat::Gif));
        assert_eq!(EmojiFormat::sniff(b"GIF89a"), Some(EmojiFormat::Gif));
        for bad in [
            &b""[..],
            b"RIFF\0\0\0\0WEBP",
            b"\0\0\0\x1cftypavif",
            b"<svg",
            b"\x89PN",
        ] {
            assert_eq!(EmojiFormat::sniff(bad), None);
        }
        assert!(EmojiFormat::Gif.is_animated());
        assert!(!EmojiFormat::Png.is_animated());
        assert_eq!(EmojiFormat::Jpeg.mime(), "image/jpeg");
        assert_eq!(EmojiFormat::Gif.to_string(), "GIF");
        let v = validate_emoji_image("x", b"<svg>", &Limits::default()).unwrap_err();
        assert_eq!(v.rule, RuleId::EMO_04);
        assert!(v.to_string().contains("3c 73 76 67 3e"), "{v}");
        let v = validate_emoji_image("x", b"", &Limits::default()).unwrap_err();
        assert!(v.to_string().contains("an empty file"), "{v}");
    }

    #[test]
    fn emo_02_byte_cap() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.resize(262_144, 0);
        assert_eq!(
            validate_emoji_image("x", &png, &Limits::default()),
            Ok(EmojiFormat::Png)
        );
        png.push(0);
        let v = validate_emoji_image("x", &png, &Limits::default()).unwrap_err();
        assert_eq!(v.rule, RuleId::EMO_02);
        assert!(
            v.to_string()
                .starts_with("[EMO-02] x: limit at most 262144, actual 262145")
        );
    }

    #[test]
    fn emo_01_and_emo_03_set_checks() {
        let limits = Limits::default();
        assert!(validate_emoji_set(["ab", "cd"], &limits).is_empty());
        let v = validate_emoji_set(["ab", "ab", "x"], &limits);
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|v| v.rule == RuleId::EMO_03));
        let names: Vec<String> = (0..2001).map(|i| format!("e{i}")).collect();
        let v = validate_emoji_set(names.iter().map(String::as_str), &limits);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, RuleId::EMO_01);
        assert_eq!(v[0].actual, Actual::Count(2001));
    }
}
