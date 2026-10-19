//! Minimal-digit base62 (`0-9A-Za-z`).

use crate::FieldError;

/// Digit `i` of base62 is `ALPHABET[i]`.
pub(crate) const ALPHABET: &[u8; 62] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Base62 digits needed for `u64::MAX`.
pub(crate) const U64_WIDTH: usize = 11;

/// Writes `n` as minimal-digit base62 into `buf` and returns the used tail.
pub(crate) fn encode_u64(mut n: u64, buf: &mut [u8; U64_WIDTH]) -> &str {
    let mut start = U64_WIDTH;
    loop {
        start -= 1;
        // `n % 62 < 62` always fits `usize`.
        let digit = usize::try_from(n % 62).unwrap_or_default();
        buf[start] = ALPHABET[digit];
        n /= 62;
        if n == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[start..]).unwrap_or_default()
}

/// The value of one base62 digit, if `b` is one.
pub(crate) const fn digit_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'Z' => Some(b - b'A' + 10),
        b'a'..=b'z' => Some(b - b'a' + 36),
        _ => None,
    }
}

/// Decodes canonical minimal-digit base62 of at most `max_len` digits.
pub(crate) fn decode_u64(raw: &str, max_len: usize) -> Result<u64, FieldError> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() {
        return Err(FieldError::Empty);
    }
    if bytes.len() > max_len {
        return Err(FieldError::TooLong {
            max: max_len,
            actual: bytes.len(),
        });
    }
    let mut value: u64 = 0;
    let mut overflow = false;
    for (index, &b) in bytes.iter().enumerate() {
        let Some(d) = digit_value(b) else {
            return Err(FieldError::InvalidChar { index });
        };
        match value
            .checked_mul(62)
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return Err(FieldError::LeadingZero);
    }
    if overflow {
        return Err(FieldError::Overflow);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{U64_WIDTH, decode_u64, encode_u64};
    use crate::FieldError;

    #[test]
    fn encodes_minimal_digits() {
        let mut buf = [0; U64_WIDTH];
        assert_eq!(encode_u64(0, &mut buf), "0");
        assert_eq!(encode_u64(61, &mut buf), "z");
        assert_eq!(encode_u64(62, &mut buf), "10");
        assert_eq!(encode_u64(255, &mut buf), "47");
        assert_eq!(encode_u64(u64::MAX, &mut buf), "LygHa16AHYF");
    }

    #[test]
    fn decodes_and_rejects() {
        assert_eq!(decode_u64("47", 2), Ok(255));
        assert_eq!(decode_u64("", 2), Err(FieldError::Empty));
        assert_eq!(
            decode_u64("100", 2),
            Err(FieldError::TooLong { max: 2, actual: 3 })
        );
        assert_eq!(decode_u64("01", 2), Err(FieldError::LeadingZero));
        assert_eq!(
            decode_u64("1 ", 2),
            Err(FieldError::InvalidChar { index: 1 })
        );
        assert_eq!(decode_u64("zzzzzzzzzzz", 11), Err(FieldError::Overflow));
        assert_eq!(decode_u64("LygHa16AHYG", 11), Err(FieldError::Overflow));
    }
}
