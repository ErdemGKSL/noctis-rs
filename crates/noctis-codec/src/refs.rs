//! Refs: 11 base62 characters from the OS CSPRNG (CID-05, AD-18).
#![deny(clippy::indexing_slicing, clippy::string_slice)]

use core::fmt;

use crate::RefError;
use crate::base62::digit;
use crate::layout::REF_LEN;

/// Bytes below this map uniformly onto a base62 digit (`4 * 62`).
const ACCEPT_BELOW: u8 = 248;
/// Random bytes drawn per fill.
const FILL_LEN: usize = 16;
/// Fills tried before giving up on a broken source.
const MAX_FILLS: usize = 8;

/// A Ref: the 11 base62 characters a by-ref `custom_id` stores its Payload
/// under (≈ 65.5 bits of entropy).
///
/// A Ref comes only from the OS CSPRNG ([`Ref::mint`]) or from a decoded
/// `custom_id` ([`RawBody::Ref`](crate::RawBody::Ref)); it is never derived
/// from interaction data, a counter or a clock. Its field is private, so it
/// cannot be built from chosen bytes:
///
/// ```compile_fail
/// let r = noctis_codec::Ref([b'0'; 11]); // [CID-05]
/// ```
///
/// ```compile_fail
/// let r = noctis_codec::Ref::from_digits(*b"0123456789A"); // [CID-05]
/// ```
///
/// ```
/// let r = noctis_codec::Ref::mint()?;
/// assert_eq!(r.as_str().len(), 11);
/// assert!(r.as_str().bytes().all(|b| b.is_ascii_alphanumeric()));
/// # Ok::<(), noctis_codec::RefError>(())
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ref([u8; REF_LEN]);

impl Ref {
    /// Mints a fresh Ref from the OS CSPRNG (`getrandom`).
    ///
    /// Each character is drawn by unbiased rejection sampling: a random byte
    /// below 248 is the digit `byte % 62`, any other byte is skipped.
    ///
    /// # Errors
    ///
    /// [`RefError::Rng`] when the OS source fails; [`RefError::Exhausted`]
    /// when 8 fills of 16 bytes did not yield 11 usable bytes, which a real
    /// CSPRNG never does.
    pub fn mint() -> Result<Self, RefError> {
        Self::mint_from(getrandom::fill)
    }

    /// Mints a Ref from the bytes `fill` writes (see [`Ref::mint`]).
    pub(crate) fn mint_from(
        mut fill: impl FnMut(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Result<Self, RefError> {
        let mut out = [0; REF_LEN];
        let mut filled = 0;
        let mut buf = [0; FILL_LEN];
        for _ in 0..MAX_FILLS {
            fill(&mut buf).map_err(RefError::Rng)?;
            for &b in buf.iter().filter(|&&b| b < ACCEPT_BELOW) {
                let Some(slot) = out.get_mut(filled) else {
                    break;
                };
                *slot = digit(b % 62);
                filled += 1;
            }
            if filled == REF_LEN {
                return Ok(Self(out));
            }
        }
        Err(RefError::Exhausted)
    }

    /// Takes 11 bytes the caller has checked to be base62 digits.
    pub(crate) const fn from_digits(digits: [u8; REF_LEN]) -> Self {
        Self(digits)
    }

    /// The all-`0` Ref the testkit renders unresolved by-Ref ids with
    /// (snapshots mask it anyway); never stored. Not public API.
    ///
    /// ```
    /// assert_eq!(noctis_codec::Ref::PLACEHOLDER.as_str(), "00000000000");
    /// ```
    #[doc(hidden)]
    pub const PLACEHOLDER: Self = Self([b'0'; REF_LEN]);

    /// The Ref's 11 characters.
    pub fn as_str(&self) -> &str {
        // Every byte is a base62 digit, so the bytes are ASCII.
        core::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl fmt::Debug for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Ref").field(&self.as_str()).finish()
    }
}

impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Ref;
    use crate::RefError;

    /// A source that writes `bytes` in order, then `then` forever, counting
    /// its fills.
    fn source(
        bytes: Vec<u8>,
        then: u8,
        fills: &mut usize,
    ) -> impl FnMut(&mut [u8]) -> Result<(), getrandom::Error> + '_ {
        let mut it = bytes.into_iter();
        move |buf| {
            *fills += 1;
            for b in buf.iter_mut() {
                *b = it.next().unwrap_or(then);
            }
            Ok(())
        }
    }

    fn mint(bytes: Vec<u8>, then: u8) -> Result<Ref, RefError> {
        let mut fills = 0;
        Ref::mint_from(source(bytes, then, &mut fills))
    }

    fn first(byte: u8) -> char {
        mint(vec![byte], 0)
            .unwrap()
            .as_str()
            .chars()
            .next()
            .unwrap()
    }

    #[test]
    fn cid_05_mint_rejection_sampling() {
        assert_eq!(mint((0..=10).collect(), 0).unwrap().as_str(), "0123456789A");
        assert_eq!(first(247), 'z');
        assert_eq!(first(61), 'z');
        assert_eq!(first(62), '0');

        let mut bytes = vec![248, 249, 250, 251, 252, 253, 254, 255];
        bytes.extend(0..20);
        assert!(mint(bytes, 0).unwrap().as_str().starts_with("012"));

        let mut bytes = vec![255; 16];
        bytes.extend(0..20);
        assert_eq!(mint(bytes, 0).unwrap().as_str(), "0123456789A");

        let mut fills = 0;
        assert_eq!(
            Ref::mint_from(source(Vec::new(), 255, &mut fills)),
            Err(RefError::Exhausted)
        );
        assert_eq!(fills, 8);

        assert_eq!(
            Ref::mint_from(|_| Err(getrandom::Error::UNSUPPORTED)),
            Err(RefError::Rng(getrandom::Error::UNSUPPORTED))
        );

        let minted = Ref::mint().unwrap();
        assert_eq!(minted.as_str().len(), 11);
        assert!(minted.as_str().bytes().all(|b| b.is_ascii_alphanumeric()));
    }

    #[test]
    fn ref_display_and_debug() {
        let r = Ref::from_digits(*b"0123456789A");
        assert_eq!(format!("{r:?}"), "Ref(\"0123456789A\")");
        assert_eq!(r.to_string(), "0123456789A");
        assert_eq!(r.as_str(), "0123456789A");
    }
}
