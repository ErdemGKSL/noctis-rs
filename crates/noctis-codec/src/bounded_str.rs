//! `BoundedStr<N>`: printable-ASCII text of at most `N` characters.

use core::fmt::{self, Write};
use core::str::FromStr;

use crate::escape::{escape, is_printable, unescape_ascii_into};
use crate::{Field, FieldDecode, FieldError};

/// Why a string cannot become a [`BoundedStr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BoundedStrError {
    /// The string has more than `capacity` characters.
    #[error("string has {actual} characters, capacity is {capacity}")]
    TooLong {
        /// The capacity `N`.
        capacity: usize,
        /// The string's length.
        actual: usize,
    },
    /// The character `ch` at byte `index` is not printable ASCII.
    #[error("character {ch:?} at byte {index} is not printable ASCII")]
    InvalidChar {
        /// Byte index of the character.
        index: usize,
        /// The offending character.
        ch: char,
    },
}

/// Printable-ASCII text (`0x20..=0x7E`) of at most `N` characters, stored inline.
///
/// As a Payload field its worst-case width is `2 * N`: `!`, `,`, `:` and `~`
/// are escaped as `!0`..`!3`, every other character is written as is.
///
/// ```
/// use noctis_codec::{BoundedStr, Field};
///
/// const CODE: BoundedStr<8> = BoundedStr::from_static("a,b");
/// assert_eq!(CODE.as_str(), "a,b");
/// assert_eq!(<BoundedStr<8> as Field>::MAX_LEN, Some(16));
/// assert!(BoundedStr::<4>::new("çay").is_err());
/// ```
///
/// Invalid literals in a `const` are compile errors:
///
/// ```compile_fail
/// use noctis_codec::BoundedStr;
///
/// const T: BoundedStr<4> = BoundedStr::from_static("çay"); // [CID-07]
/// assert_eq!(T.len(), 3);
/// ```
///
/// ```compile_fail
/// use noctis_codec::BoundedStr;
///
/// const T: BoundedStr<2> = BoundedStr::from_static("abc");
/// assert_eq!(T.len(), 3);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BoundedStr<const N: usize> {
    /// Bytes `..len` are the text; the tail is zero.
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> BoundedStr<N> {
    /// The capacity `N`.
    pub const CAPACITY: usize = N;

    /// Creates a `BoundedStr` from `s`.
    ///
    /// # Errors
    ///
    /// [`BoundedStrError::InvalidChar`] for the first character outside
    /// printable ASCII (checked first), then [`BoundedStrError::TooLong`] when
    /// `s` has more than `N` characters.
    pub fn new(s: &str) -> Result<Self, BoundedStrError> {
        if let Some((index, ch)) = s
            .char_indices()
            .find(|&(_, c)| !(c.is_ascii() && is_printable(c as u8)))
        {
            return Err(BoundedStrError::InvalidChar { index, ch });
        }
        if s.len() > N {
            return Err(BoundedStrError::TooLong {
                capacity: N,
                actual: s.len(),
            });
        }
        let mut bytes = [0; N];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        Ok(Self {
            bytes,
            len: s.len(),
        })
    }

    /// Creates a `BoundedStr` from a literal, in `const` context.
    ///
    /// # Panics
    ///
    /// Panics when `s` has a character outside printable ASCII or more than
    /// `N` characters; in a `const` item that is a compile error.
    pub const fn from_static(s: &'static str) -> Self {
        let src = s.as_bytes();
        let mut i = 0;
        while i < src.len() {
            assert!(
                is_printable(src[i]),
                "[CID-07] BoundedStr holds printable ASCII only"
            );
            i += 1;
        }
        assert!(
            src.len() <= N,
            "BoundedStr literal is longer than its capacity"
        );
        let mut bytes = [0; N];
        let mut i = 0;
        while i < src.len() {
            bytes[i] = src[i];
            i += 1;
        }
        Self {
            bytes,
            len: src.len(),
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        self.bytes
            .get(..self.len)
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or_default()
    }

    /// The length in characters (= bytes).
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the text is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for BoundedStr<N> {
    fn default() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> fmt::Debug for BoundedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BoundedStr").field(&self.as_str()).finish()
    }
}

impl<const N: usize> fmt::Display for BoundedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const N: usize> AsRef<str> for BoundedStr<N> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<const N: usize> TryFrom<&str> for BoundedStr<N> {
    type Error = BoundedStrError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl<const N: usize> FromStr for BoundedStr<N> {
    type Err = BoundedStrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl<const N: usize> PartialEq<str> for BoundedStr<N> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl<const N: usize> PartialEq<&str> for BoundedStr<N> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl<const N: usize> Field for BoundedStr<N> {
    const MAX_LEN: Option<usize> = N.checked_mul(2);

    fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        escape(self.as_str(), out)
    }
}

impl<const N: usize> FieldDecode<'_> for BoundedStr<N> {
    fn decode(raw: &str) -> Result<Self, FieldError> {
        if let Some(max) = Self::MAX_LEN
            && raw.len() > max
        {
            return Err(FieldError::TooLong {
                max,
                actual: raw.len(),
            });
        }
        let mut bytes = [0; N];
        let len = unescape_ascii_into(raw, &mut bytes)?;
        if len > N {
            return Err(FieldError::OverCapacity {
                capacity: N,
                actual: len,
            });
        }
        // `unescape_ascii_into` writes printable ASCII only, so `bytes[..len]`
        // is valid text and the tail is still zero.
        Ok(Self { bytes, len })
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedStr, BoundedStrError};
    use crate::check::worst_case;
    use crate::{Field, FieldDecode, FieldError};

    fn enc<T: Field>(v: &T) -> String {
        let mut out = String::new();
        v.encode(&mut out).unwrap();
        out
    }

    const _: () = {
        const fn assert_copy<T: Copy>() {}
        assert_copy::<BoundedStr<8>>();
        assert_copy::<Option<BoundedStr<8>>>();
    };

    #[test]
    fn construction() {
        assert!(BoundedStr::<4>::new("ab").is_ok());
        assert!(BoundedStr::<4>::new("").is_ok());
        assert!(BoundedStr::<4>::new("abcd").is_ok());
        assert!(BoundedStr::<4>::new(",!:~").is_ok());
        assert_eq!(
            BoundedStr::<4>::new("abcde"),
            Err(BoundedStrError::TooLong {
                capacity: 4,
                actual: 5
            })
        );
        assert_eq!(
            BoundedStr::<4>::new("aé"),
            Err(BoundedStrError::InvalidChar { index: 1, ch: 'é' })
        );
        assert_eq!(
            BoundedStr::<4>::new("a\tb"),
            Err(BoundedStrError::InvalidChar { index: 1, ch: '\t' })
        );
        assert_eq!(
            BoundedStr::<1>::new("\u{7f}xx"),
            Err(BoundedStrError::InvalidChar {
                index: 0,
                ch: '\u{7f}'
            })
        );
    }

    #[test]
    fn traits() {
        let s: BoundedStr<4> = "ab".parse().unwrap();
        assert_eq!(format!("{s:?}"), r#"BoundedStr("ab")"#);
        assert_eq!(s.to_string(), "ab");
        assert_eq!(s, "ab");
        assert_eq!(s, *"ab");
        assert_eq!(s.len(), 2);
        assert!(!s.is_empty());
        assert!(BoundedStr::<4>::default().is_empty());
        assert_eq!(BoundedStr::<4>::CAPACITY, 4);
        assert_eq!(BoundedStr::<4>::try_from("ab"), Ok(s));
        assert_eq!(s.as_ref(), "ab");
    }

    #[test]
    fn encode_matrix() {
        let b = |s| BoundedStr::<8>::new(s).unwrap();
        assert_eq!(enc(&b("abc")), "abc");
        assert_eq!(enc(&b("")), "");
        assert_eq!(enc(&b("a,b!c:d~")), "a!1b!0c!2d!3");
        assert_eq!(enc(&BoundedStr::<0>::new("").unwrap()), "");
        assert_eq!(<BoundedStr<0> as Field>::MAX_LEN, Some(0));
        assert_eq!(<BoundedStr<{ usize::MAX }> as Field>::MAX_LEN, None);
        assert_eq!(
            <Option<BoundedStr<{ usize::MAX / 2 }>> as Field>::MAX_LEN,
            Some(usize::MAX)
        );
    }

    #[test]
    fn cid_01_bounded_str_worst_case_width() {
        let w = BoundedStr::<4>::new("!!!!").unwrap();
        assert_eq!(enc(&w), "!0!0!0!0");
        worst_case(&w).unwrap();
        let o = Some(BoundedStr::<2>::new(",,").unwrap());
        assert_eq!(enc(&o), "1!1!1");
        worst_case(&o).unwrap();
        worst_case(&BoundedStr::<0>::default()).unwrap();
    }

    #[test]
    fn decode_matrix() {
        assert_eq!(BoundedStr::<8>::decode("a!1b!0c!2d!3").unwrap(), "a,b!c:d~");
        for raw in ["a,b", "a:b", "a~b"] {
            assert_eq!(
                BoundedStr::<8>::decode(raw),
                Err(FieldError::InvalidChar { index: 1 })
            );
        }
        let bad = |index| Err(FieldError::BadEscape { index });
        assert_eq!(BoundedStr::<8>::decode("a!"), bad(1));
        assert_eq!(BoundedStr::<8>::decode("!4"), bad(0));
        assert_eq!(BoundedStr::<8>::decode("!a"), bad(0));
        assert_eq!(BoundedStr::<8>::decode("!u0000e9"), bad(0));
        for (raw, index) in [("\n", 0), ("é", 0), ("\u{7f}", 0)] {
            assert_eq!(
                BoundedStr::<8>::decode(raw),
                Err(FieldError::InvalidChar { index })
            );
        }
        assert_eq!(
            BoundedStr::<2>::decode("abc"),
            Err(FieldError::OverCapacity {
                capacity: 2,
                actual: 3
            })
        );
        assert_eq!(BoundedStr::<2>::decode("!0!0").unwrap(), "!!");
        assert_eq!(
            BoundedStr::<2>::decode("!0!0!0"),
            Err(FieldError::TooLong { max: 4, actual: 6 })
        );
        assert_eq!(BoundedStr::<0>::decode("").unwrap(), "");
        assert_eq!(
            BoundedStr::<0>::decode("a"),
            Err(FieldError::TooLong { max: 0, actual: 1 })
        );
    }

    #[test]
    fn cid_07_bounded_str_encodes_printable_ascii() {
        let s = BoundedStr::<8>::new("!,:~ ~,!").unwrap();
        let e = enc(&s);
        assert!(e.bytes().all(|b| (0x20..=0x7E).contains(&b)));
        assert!(!e.contains([',', ':', '~']));
    }
}
