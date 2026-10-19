//! The typed fixed-width sections of the `custom_id` layout: flags, View
//! envelope, select nonce and integrity tag (AD-5, AD-20).

use core::fmt;
use core::ops::BitOr;

use crate::EnvelopeError;
use crate::base62::{self, digit, digit_value};

/// Largest value of one base62 digit.
pub(crate) const DIGIT_MAX: u8 = 61;
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
    /// The View envelope `E` is present.
    pub const E: Self = Self(2);
    /// The select nonce `N` is present.
    pub const N: Self = Self(4);
    /// The integrity tag `T` is present.
    pub const T: Self = Self(8);

    /// Flags from their bits; bits above `0b1111` are dropped.
    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b1111)
    }

    /// The flag bits (`0..=15`).
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

/// Lists the set letters, for example `Flags(EN)` or `Flags(-)`.
impl fmt::Debug for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Flags(")?;
        if self.0 == 0 {
            f.write_str("-")?;
        }
        for (flag, letter) in [
            (Self::S, 'S'),
            (Self::E, 'E'),
            (Self::N, 'N'),
            (Self::T, 'T'),
        ] {
            if self.contains(flag) {
                fmt::Write::write_char(f, letter)?;
            }
        }
        f.write_str(")")
    }
}

/// The 5-character View envelope `E`: action, tab, page and state version.
///
/// ```
/// use noctis_codec::ViewEnvelope;
///
/// let env = ViewEnvelope::new('t', 1, 63, 2)?;
/// assert_eq!(env.to_string(), "t1112");
/// assert_eq!(env.page(), 63);
/// # Ok::<(), noctis_codec::EnvelopeError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewEnvelope {
    action: u8,
    tab: u8,
    page: u16,
    state_version: u8,
}

impl ViewEnvelope {
    /// The largest page, the largest value of two base62 digits.
    pub const MAX_PAGE: u16 = TWO_DIGIT_MAX;

    /// Builds an envelope.
    ///
    /// # Errors
    ///
    /// Checked in this order: `action` is neither `_` nor an ASCII letter or
    /// digit; `tab` is over 61; `page` is over 3843; `state_version` is over 61.
    pub fn new(action: char, tab: u8, page: u16, state_version: u8) -> Result<Self, EnvelopeError> {
        let action_byte = u8::try_from(action)
            .ok()
            .filter(|&b| is_action(b))
            .ok_or(EnvelopeError::InvalidAction(action))?;
        if tab > DIGIT_MAX {
            return Err(EnvelopeError::TabOutOfRange(tab));
        }
        if page > Self::MAX_PAGE {
            return Err(EnvelopeError::PageOutOfRange(page));
        }
        if state_version > DIGIT_MAX {
            return Err(EnvelopeError::VersionOutOfRange(state_version));
        }
        Ok(Self {
            action: action_byte,
            tab,
            page,
            state_version,
        })
    }

    /// Parses the 5 envelope bytes, reporting the first bad byte's offset.
    pub(crate) fn parse(raw: [u8; 5]) -> Result<Self, usize> {
        let [action, tab, hi, lo, version] = raw;
        if !is_action(action) {
            return Err(0);
        }
        let tab = digit_value(tab).ok_or(1_usize)?;
        let hi = digit_value(hi).ok_or(2_usize)?;
        let lo = digit_value(lo).ok_or(3_usize)?;
        let state_version = digit_value(version).ok_or(4_usize)?;
        Ok(Self {
            action,
            tab,
            page: u16::from(hi) * 62 + u16::from(lo),
            state_version,
        })
    }

    /// The action character (`_` or an ASCII letter or digit).
    pub fn action(&self) -> char {
        char::from(self.action)
    }

    /// The tab (`0..=61`).
    pub const fn tab(&self) -> u8 {
        self.tab
    }

    /// The page (`0..=3843`).
    pub const fn page(&self) -> u16 {
        self.page
    }

    /// The state version (`0..=61`).
    pub const fn state_version(&self) -> u8 {
        self.state_version
    }
}

const fn is_action(b: u8) -> bool {
    b == b'_' || digit_value(b).is_some()
}

impl fmt::Display for ViewEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut page = [0; 2];
        base62::write_fixed(u64::from(self.page), &mut page);
        let [hi, lo] = page;
        for b in [
            self.action,
            digit(self.tab),
            hi,
            lo,
            digit(self.state_version),
        ] {
            fmt::Write::write_char(f, char::from(b))?;
        }
        Ok(())
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
/// The encoder passes exactly the characters before `T`. Story 2.4
/// implements this for the App's payload key.
pub trait Sign {
    /// The tag for `signed`, every character before the tag.
    fn sign(&self, signed: &str) -> Tag;
}

#[cfg(test)]
mod tests {
    use super::{Flags, SelectNonce, Tag, ViewEnvelope};
    use crate::EnvelopeError;

    #[test]
    fn envelope_display_and_getters() {
        let env = ViewEnvelope::new('t', 1, 63, 2).unwrap();
        assert_eq!(env.to_string(), "t1112");
        assert_eq!(
            (env.action(), env.tab(), env.page(), env.state_version()),
            ('t', 1, 63, 2)
        );
        let max = ViewEnvelope::new('_', 61, 3843, 61).unwrap();
        assert_eq!(max.to_string(), "_zzzz");
        assert_eq!(ViewEnvelope::parse(*b"_zzzz"), Ok(max));
        assert_eq!(ViewEnvelope::parse(*b"__zzz"), Err(1));
        assert_eq!(ViewEnvelope::parse(*b"!zzzz"), Err(0));
        assert_eq!(ViewEnvelope::parse(*b"_zz!z"), Err(3));
        assert_eq!(ViewEnvelope::parse(*b"_zzz!"), Err(4));
    }

    #[test]
    fn envelope_errors_in_order() {
        assert_eq!(
            ViewEnvelope::new('-', 62, 3844, 62),
            Err(EnvelopeError::InvalidAction('-'))
        );
        assert_eq!(
            ViewEnvelope::new('é', 0, 0, 0),
            Err(EnvelopeError::InvalidAction('é'))
        );
        assert_eq!(
            ViewEnvelope::new('a', 62, 3844, 62),
            Err(EnvelopeError::TabOutOfRange(62))
        );
        assert_eq!(
            ViewEnvelope::new('a', 0, 3844, 62),
            Err(EnvelopeError::PageOutOfRange(3844))
        );
        assert_eq!(
            ViewEnvelope::new('a', 0, 0, 62),
            Err(EnvelopeError::VersionOutOfRange(62))
        );
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
        let all = Flags::S | Flags::E | Flags::N | Flags::T;
        assert_eq!(all.bits(), 15);
        assert!(all.contains(Flags::E | Flags::T));
        assert!(!Flags::E.contains(Flags::T));
        assert_eq!(format!("{all:?}"), "Flags(SENT)");
        assert_eq!(format!("{:?}", Flags::EMPTY), "Flags(-)");
        assert_eq!(Flags::from_bits(0xF2), Flags::E);
        assert_eq!(all.intersection(Flags::N), Flags::N);
    }
}
