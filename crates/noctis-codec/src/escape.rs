//! The `!` escape shared by every string-like field.

use core::fmt::{self, Write};
use std::borrow::Cow;

use crate::FieldError;

/// Whether `b` is printable ASCII (`0x20..=0x7E`).
pub(crate) const fn is_printable(b: u8) -> bool {
    matches!(b, 0x20..=0x7E)
}

/// The two-character escape of a reserved printable character, if it is one.
const fn short_escape(c: char) -> Option<&'static str> {
    match c {
        '!' => Some("!0"),
        ',' => Some("!1"),
        ':' => Some("!2"),
        '~' => Some("!3"),
        _ => None,
    }
}

/// Writes one character in escaped form.
pub(crate) fn escape_char<W: Write + ?Sized>(c: char, out: &mut W) -> fmt::Result {
    if let Some(esc) = short_escape(c) {
        out.write_str(esc)
    } else if c.is_ascii() && is_printable(c as u8) {
        out.write_char(c)
    } else {
        write!(out, "!u{:06x}", u32::from(c))
    }
}

/// Writes `s` in escaped form.
pub(crate) fn escape<W: Write + ?Sized>(s: &str, out: &mut W) -> fmt::Result {
    s.chars().try_for_each(|c| escape_char(c, out))
}

/// A writer that escapes everything written through it into `inner`.
pub(crate) struct EscapeWriter<'a, W: Write + ?Sized> {
    pub(crate) inner: &'a mut W,
}

impl<W: Write + ?Sized> Write for EscapeWriter<'_, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        escape(s, self.inner)
    }
}

/// Value of a lowercase hex digit.
const fn hex_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'f' => Some((b - b'a' + 10) as u32),
        _ => None,
    }
}

/// Decodes the `!u` escape whose `!` is at `at`; returns the character.
fn unicode_escape(bytes: &[u8], at: usize) -> Option<char> {
    let digits = bytes.get(at + 2..at + 8)?;
    let mut value = 0u32;
    for &b in digits {
        value = value * 16 + hex_value(b)?;
    }
    let c = char::from_u32(value)?;
    // Printable ASCII is never escaped as `!u`, so that form is not canonical.
    (!(c.is_ascii() && is_printable(c as u8))).then_some(c)
}

/// Reverses [`escape`], borrowing when `raw` has no `!`.
///
/// With `allow_unicode == false` (`BoundedStr`) the `!u` form is rejected.
pub(crate) fn unescape(raw: &str, allow_unicode: bool) -> Result<Cow<'_, str>, FieldError> {
    let bytes = raw.as_bytes();
    let mut owned: Option<String> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'!' {
            let out = owned.get_or_insert_with(|| raw[..i].to_owned());
            let bad = FieldError::BadEscape { index: i };
            let (c, width) = match bytes.get(i + 1) {
                Some(b'0') => ('!', 2),
                Some(b'1') => (',', 2),
                Some(b'2') => (':', 2),
                Some(b'3') => ('~', 2),
                Some(b'u') if allow_unicode => (unicode_escape(bytes, i).ok_or(bad)?, 8),
                _ => return Err(bad),
            };
            out.push(c);
            i += width;
        } else if is_printable(b) && !matches!(b, b',' | b':' | b'~') {
            if let Some(out) = owned.as_mut() {
                out.push(char::from(b));
            }
            i += 1;
        } else {
            return Err(FieldError::InvalidChar { index: i });
        }
    }
    Ok(owned.map_or(Cow::Borrowed(raw), Cow::Owned))
}

/// Reverses [`escape`] for `BoundedStr` (no `!u` form) into `buf`, without
/// allocating.
///
/// Scans all of `raw`, so `InvalidChar`/`BadEscape` win over capacity. Returns
/// the full decoded length, which may exceed `buf.len()`; only the first
/// `buf.len()` decoded bytes are written.
pub(crate) fn unescape_ascii_into(raw: &str, buf: &mut [u8]) -> Result<usize, FieldError> {
    let bytes = raw.as_bytes();
    let mut len = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (out, width) = if b == b'!' {
            match bytes.get(i + 1) {
                Some(b'0') => (b'!', 2),
                Some(b'1') => (b',', 2),
                Some(b'2') => (b':', 2),
                Some(b'3') => (b'~', 2),
                _ => return Err(FieldError::BadEscape { index: i }),
            }
        } else if is_printable(b) && !matches!(b, b',' | b':' | b'~') {
            (b, 1)
        } else {
            return Err(FieldError::InvalidChar { index: i });
        };
        if let Some(slot) = buf.get_mut(len) {
            *slot = out;
        }
        len += 1;
        i += width;
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::{Cow, escape, unescape, unescape_ascii_into};
    use crate::FieldError;

    fn esc(s: &str) -> String {
        let mut out = String::new();
        escape(s, &mut out).unwrap();
        out
    }

    #[test]
    fn escape_table() {
        assert_eq!(esc("a,b!c:d~"), "a!1b!0c!2d!3");
        assert_eq!(esc("é😀"), "!u0000e9!u01f600");
        assert_eq!(esc("\n"), "!u00000a");
        assert_eq!(esc("\u{7f}"), "!u00007f");
        assert_eq!(esc(" "), " ");
    }

    #[test]
    fn unescape_borrows_without_bang() {
        assert!(matches!(unescape("abc", true), Ok(Cow::Borrowed("abc"))));
        assert!(matches!(unescape("a!1", true), Ok(Cow::Owned(s)) if s == "a,"));
    }

    #[test]
    fn unescape_errors() {
        let bad = |index| Err(FieldError::BadEscape { index });
        assert_eq!(unescape("a!", true), bad(1));
        assert_eq!(unescape("!4", true), bad(0));
        assert_eq!(unescape("!a", true), bad(0));
        assert_eq!(unescape("!u0000e9", false), bad(0));
        assert_eq!(unescape("!u0000E9", true), bad(0));
        assert_eq!(unescape("!u00e9", true), bad(0));
        assert_eq!(unescape("!u000041", true), bad(0));
        assert_eq!(unescape("!u00d800", true), bad(0));
        assert_eq!(unescape("!u110000", true), bad(0));
        assert_eq!(unescape("!uéééé", true), bad(0));
        assert_eq!(
            unescape("aé", true),
            Err(FieldError::InvalidChar { index: 1 })
        );
    }

    #[test]
    fn unescape_ascii_into_matches_unescape() {
        let mut buf = [0u8; 4];
        assert_eq!(unescape_ascii_into("a!1b!0", &mut buf), Ok(4));
        assert_eq!(&buf, b"a,b!");
        let mut small = [0u8; 2];
        assert_eq!(unescape_ascii_into("abc", &mut small), Ok(3));
        assert_eq!(
            unescape_ascii_into("abcé", &mut small),
            Err(FieldError::InvalidChar { index: 3 })
        );
        assert_eq!(
            unescape_ascii_into("abc!u0000e9", &mut small),
            Err(FieldError::BadEscape { index: 3 })
        );
    }
}
