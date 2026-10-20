//! The typed fixed-width sections of the `custom_id` layout: flags, select
//! nonce and integrity tag (AD-5).

use core::fmt;
use core::ops::BitOr;

use crate::base62;

/// Largest value of one base62 digit.
pub(crate) const DIGIT_MAX: u8 = 61;

// The schema version `S` is one base62 digit.
const _: () = assert!(DIGIT_MAX == noctis_rules::cid::SCHEMA_MAX);
/// Largest value of two base62 digits.
const TWO_DIGIT_MAX: u16 = 62 * 62 - 1;

/// The section flags of a `custom_id`'s format digit `F`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Flags(u8);

impl Flags {
    /// No section.
    pub const EMPTY: Self = Self(0);
    /// The schema version `S` is present.
    pub const S: Self = Self(1);
    /// The select nonce `N` is present.
    pub const N: Self = Self(4);
    /// The integrity tag `T` is present.
    pub const T: Self = Self(8);

    /// The reserved flag value 2 (`0b0010`) of the format digit; the
    /// decoder rejects a `custom_id` that sets it.
    pub(crate) const RESERVED: u8 = 2;

    /// Flags from their bits; the reserved flag value 2 (`0b0010`) and bits
    /// above `0b1111` are dropped.
    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b1101)
    }

    /// The flag bits (a subset of `0b1101`).
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether every flag of `other` is set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// `self` with every flag of `other` set too.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `self` with only the flags that are also in `other`.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl BitOr for Flags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// Lists the set letters, for example `Flags(SN)` or `Flags(-)`.
impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Flags(")?;
        if self.0 == 0 {
            f.write_str("-")?;
        }
        for (flag, letter) in [(Self::S, 'S'), (Self::N, 'N'), (Self::T, 'T')] {
            if self.contains(flag) {
                fmt::Write::write_char(f, letter)?;
            }
        }
        f.write_str(")")
    }
}

/// The 2-character per-render select nonce `N` (FR-25).
///
/// ```
/// use noctis_codec::SelectNonce;
///
/// assert_eq!(SelectNonce::new(3843).map(|n| n.to_string()).as_deref(), Some("zz"));
/// assert_eq!(SelectNonce::new(3844), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectNonce(u16);

impl SelectNonce {
    /// The largest nonce, the largest value of two base62 digits.
    pub const MAX: u16 = TWO_DIGIT_MAX;

    /// A nonce, or `None` when `value` is over [`Self::MAX`].
    pub const fn new(value: u16) -> Option<Self> {
        if value > Self::MAX {
            None
        } else {
            Some(Self(value))
        }
    }

    /// The nonce value.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for SelectNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = [0; 2];
        base62::write_fixed(u64::from(self.0), &mut out);
        let [hi, lo] = out;
        fmt::Write::write_char(f, char::from(hi))?;
        fmt::Write::write_char(f, char::from(lo))
    }
}

/// The 6-character integrity tag `T`: 6 base62 digits.
///
/// ```
/// use noctis_codec::Tag;
///
/// assert_eq!(Tag::from_u64(1).as_str(), "000001");
/// assert_eq!(Tag::from_u64(u64::MAX).as_str(), "16AHYF");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tag([u8; 6]);

impl Tag {
    /// The lowest 6 base62 digits of `n` (`n` modulo 62^6).
    pub const fn from_u64(n: u64) -> Self {
        let mut out = [0; 6];
        base62::write_fixed(n, &mut out);
        Self(out)
    }

    /// Takes 6 bytes the caller has checked to be base62 digits.
    pub(crate) const fn from_digits(digits: [u8; 6]) -> Self {
        Self(digits)
    }

    /// Compares two tags in constant time: every byte is compared, with no
    /// early exit.
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "mirrors `PartialEq::eq`; the spec fixes this signature"
    )]
    pub(crate) fn ct_eq(&self, other: &Tag) -> bool {
        let diff = self
            .0
            .iter()
            .zip(other.0.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b));
        core::hint::black_box(diff) == 0
    }

    /// The tag's 6 characters.
    pub fn as_str(&self) -> &str {
        // Every byte is a base62 digit, so the bytes are ASCII.
        core::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Tag").field(&self.as_str()).finish()
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Computes the integrity tag of a signed Route's `custom_id`.
///
/// The encoder passes exactly the characters before `T`.
/// [`PayloadKey`](crate::PayloadKey) and [`PayloadKeys`](crate::PayloadKeys)
/// implement it with keyed BLAKE3.
pub trait Sign {
    /// The tag for `signed`, every character before the tag.
    fn sign(&self, signed: &str) -> Tag;
}

/// Checks the integrity tag of a signed Route's `custom_id`.
///
/// [`PayloadKey`](crate::PayloadKey) and [`PayloadKeys`](crate::PayloadKeys)
/// implement it in constant time.
pub trait Verify {
    /// Whether `tag` is a valid tag for `signed`, every character before the
    /// tag.
    fn verify(&self, signed: &str, tag: Tag) -> bool;
}

#[cfg(test)]
mod tests {
    use super::{Flags, SelectNonce, Tag};

    #[test]
    fn tag_ct_eq() {
        let a = Tag::from_digits(*b"abcdef");
        assert!(a.ct_eq(&Tag::from_digits(*b"abcdef")));
        assert!(!a.ct_eq(&Tag::from_digits(*b"Xbcdef")));
        assert!(!a.ct_eq(&Tag::from_digits(*b"abcdeX")));
    }

    #[test]
    fn select_nonce() {
        assert_eq!(SelectNonce::new(0).unwrap().to_string(), "00");
        assert_eq!(SelectNonce::new(3843).unwrap().to_string(), "zz");
        assert_eq!(SelectNonce::new(3843).unwrap().get(), 3843);
        assert_eq!(SelectNonce::new(3844), None);
    }

    #[test]
    fn tag_from_u64() {
        assert_eq!(Tag::from_u64(0).as_str(), "000000");
        assert_eq!(Tag::from_u64(1).as_str(), "000001");
        assert_eq!(Tag::from_u64(56_800_235_583).as_str(), "zzzzzz");
        assert_eq!(Tag::from_u64(56_800_235_584).as_str(), "000000");
        assert_eq!(Tag::from_u64(u64::MAX).to_string(), "16AHYF");
        assert_eq!(format!("{:?}", Tag::from_u64(1)), "Tag(\"000001\")");
    }

    #[test]
    fn flags() {
        let all = Flags::S | Flags::N | Flags::T;
        assert_eq!(all.bits(), 13);
        assert!(all.contains(Flags::N | Flags::T));
        assert!(!Flags::N.contains(Flags::T));
        assert_eq!(format!("{all:?}"), "Flags(SNT)");
        assert_eq!(format!("{:?}", Flags::EMPTY), "Flags(-)");
        assert_eq!(Flags::from_bits(0xF2), Flags::EMPTY);
        assert_eq!(Flags::from_bits(0xFF), all);
        assert_eq!(all.intersection(Flags::N), Flags::N);
    }
}
