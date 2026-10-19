//! The payload key and key ring that sign and verify integrity tags (AD-5,
//! AD-29).

#![deny(clippy::indexing_slicing, clippy::string_slice)]

use core::fmt;
use core::str::FromStr;

use crate::{KeyError, Sign, Tag, Verify};

/// The tag of `signed` under `key`: the first 8 bytes of keyed BLAKE3,
/// little-endian, reduced to 6 base62 digits.
fn tag(key: &[u8; 32], signed: &str) -> Tag {
    let hash = blake3::keyed_hash(key, signed.as_bytes());
    let mut first = [0u8; 8];
    for (dst, src) in first.iter_mut().zip(hash.as_bytes().iter()) {
        *dst = *src;
    }
    Tag::from_u64(u64::from_le_bytes(first))
}

/// The value of one ASCII hex digit.
const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The 32-byte secret that signs and verifies integrity tags (AD-5).
///
/// It is redacted in `Debug` and `Display` and has no byte getter, so it
/// cannot be logged or handed to an adapter (AD-29, NFR-6).
///
/// ```
/// use noctis_codec::{PayloadKey, Sign, Verify};
///
/// let key: PayloadKey = "01".repeat(32).parse()?;
/// let tag = key.sign("8sig:1");
/// assert_eq!(tag.as_str(), "UU2sw1");
/// assert!(key.verify("8sig:1", tag));
/// assert!(!key.verify("8sig:2", tag));
/// assert_eq!(format!("{key:?}"), "PayloadKey(<redacted>)");
/// # Ok::<(), noctis_codec::KeyError>(())
/// ```
#[derive(Clone)]
pub struct PayloadKey([u8; 32]);

impl PayloadKey {
    /// A key from 32 raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses the `NOCTIS_PAYLOAD_KEY` format: exactly 64 ASCII hex digits,
    /// upper or lower case, with no `0x` prefix and no surrounding
    /// whitespace.
    ///
    /// # Errors
    ///
    /// [`KeyError::Length`] when `s` is not 64 bytes long, then
    /// [`KeyError::InvalidChar`] at the first byte that is not a hex digit.
    pub fn from_hex(s: &str) -> Result<Self, KeyError> {
        let bytes = s.as_bytes();
        if bytes.len() != 64 {
            return Err(KeyError::Length { len: bytes.len() });
        }
        let mut key = [0u8; 32];
        for (i, (out, pair)) in key.iter_mut().zip(bytes.chunks_exact(2)).enumerate() {
            let index = i * 2;
            let mut value = 0u8;
            for (offset, &b) in pair.iter().enumerate() {
                let digit = hex_value(b).ok_or(KeyError::InvalidChar {
                    index: index + offset,
                })?;
                value = (value << 4) | digit;
            }
            *out = value;
        }
        Ok(Self(key))
    }
}

impl FromStr for PayloadKey {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl fmt::Debug for PayloadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PayloadKey(<redacted>)")
    }
}

impl fmt::Display for PayloadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Sign for PayloadKey {
    fn sign(&self, signed: &str) -> Tag {
        tag(&self.0, signed)
    }
}

impl Verify for PayloadKey {
    fn verify(&self, signed: &str, tag: Tag) -> bool {
        self.sign(signed).ct_eq(&tag)
    }
}

/// The App's key ring: the current key signs; the current key or one
/// previous key verifies, so ids signed before a rotation keep working.
///
/// ```
/// use noctis_codec::{PayloadKey, PayloadKeys, Sign, Verify};
///
/// let old = PayloadKey::from_bytes([1; 32]);
/// let new = PayloadKey::from_bytes([2; 32]);
/// let tag = old.sign("8sig:1");
/// assert!(PayloadKeys::new(new.clone()).with_previous(old).verify("8sig:1", tag));
/// assert!(!PayloadKeys::new(new).verify("8sig:1", tag));
/// ```
#[derive(Clone, Debug)]
pub struct PayloadKeys {
    current: PayloadKey,
    previous: Option<PayloadKey>,
}

impl PayloadKeys {
    /// A ring with only the current key.
    #[must_use]
    pub const fn new(current: PayloadKey) -> Self {
        Self {
            current,
            previous: None,
        }
    }

    /// Also accepts tags made with `previous` (key rotation).
    #[must_use]
    pub fn with_previous(self, previous: PayloadKey) -> Self {
        Self {
            previous: Some(previous),
            ..self
        }
    }
}

impl Sign for PayloadKeys {
    fn sign(&self, signed: &str) -> Tag {
        self.current.sign(signed)
    }
}

impl Verify for PayloadKeys {
    fn verify(&self, signed: &str, tag: Tag) -> bool {
        let current = self.current.verify(signed, tag);
        let previous = self
            .previous
            .as_ref()
            .is_some_and(|k| k.verify(signed, tag));
        // Non-short-circuit `|`: both comparisons always run.
        current | previous
    }
}

#[cfg(test)]
mod tests {
    use super::{PayloadKey, PayloadKeys};
    use crate::{KeyError, Sign, Verify};

    #[test]
    fn payload_key_is_redacted() {
        let k = PayloadKey::from_hex(&"ab".repeat(32)).unwrap();
        assert_eq!(format!("{k:?}"), "PayloadKey(<redacted>)");
        assert_eq!(format!("{k}"), "<redacted>");
        assert_eq!(format!("{k:#?}"), "PayloadKey(<redacted>)");
        let ring = format!("{:?}", PayloadKeys::new(k.clone()).with_previous(k));
        assert_eq!(ring.matches("<redacted>").count(), 2);
        for bad in ["ab", "AB", "171", "["] {
            assert!(!ring.contains(bad), "{ring}");
        }
    }

    #[test]
    fn from_hex_matrix() {
        let hex63 = "0".repeat(63);
        let hex62 = "0".repeat(62);
        let ok = |s: &str| PayloadKey::from_hex(s).map(|k| k.sign("x"));
        assert_eq!(
            ok(&"01".repeat(32)).unwrap(),
            PayloadKey::from_bytes([1; 32]).sign("x")
        );
        assert_eq!(ok(&"AB".repeat(32)), ok(&"ab".repeat(32)));
        assert!(ok(&"AB".repeat(32)).is_ok());
        assert!(ok(&"00".repeat(32)).is_ok());
        let cases: [(String, KeyError); 13] = [
            (String::new(), KeyError::Length { len: 0 }),
            (hex63.clone(), KeyError::Length { len: 63 }),
            ("0".repeat(65), KeyError::Length { len: 65 }),
            ("0".repeat(128), KeyError::Length { len: 128 }),
            (format!("{hex62}é"), KeyError::InvalidChar { index: 62 }),
            (format!("0x{hex62}"), KeyError::InvalidChar { index: 1 }),
            (format!("{hex63}g"), KeyError::InvalidChar { index: 63 }),
            (format!(" {hex63}"), KeyError::InvalidChar { index: 0 }),
            (format!("{hex63}\n"), KeyError::InvalidChar { index: 63 }),
            ("é".repeat(32), KeyError::InvalidChar { index: 0 }),
            (format!("{hex63}G"), KeyError::InvalidChar { index: 63 }),
            (format!("g{hex63}"), KeyError::InvalidChar { index: 0 }),
            (format!("{hex62}-0"), KeyError::InvalidChar { index: 62 }),
        ];
        for (input, want) in cases {
            let got = PayloadKey::from_hex(&input).unwrap_err();
            assert_eq!(got, want, "{input:?}");
            assert_eq!(input.parse::<PayloadKey>().unwrap_err(), want);
            // The messages are fixed text plus a number: no key character.
            let (debug, display) = match want {
                KeyError::Length { len } => (
                    format!("Length {{ len: {len} }}"),
                    format!("payload key is {len} bytes, 64 hex digits expected"),
                ),
                KeyError::InvalidChar { index } => (
                    format!("InvalidChar {{ index: {index} }}"),
                    format!("payload key has a non-hex character at {index}"),
                ),
            };
            assert_eq!(format!("{got:?}"), debug);
            assert_eq!(got.to_string(), display);
        }
    }

    #[test]
    fn golden_tags() {
        for (bytes, signed, want) in [
            ([1; 32], "8sig:1", "UU2sw1"),
            ([2; 32], "8sig:1", "a0cQ9C"),
            ([3; 32], "8sig:1", "DMzSul"),
            ([1; 32], "8sig:2", "ktZWEa"),
        ] {
            let k = PayloadKey::from_bytes(bytes);
            let tag = k.sign(signed);
            assert_eq!(tag.as_str(), want);
            assert!(k.verify(signed, tag));
        }
    }
}
