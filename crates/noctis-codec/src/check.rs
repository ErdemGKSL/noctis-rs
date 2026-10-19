//! Round-trip and worst-case-width checks for [`Field`] impls.
//!
//! Call these from the tests of every custom Payload field:
//!
//! ```
//! use noctis_codec::check::{assert_round_trip, assert_worst_case, round_trip, worst_case, CheckError};
//!
//! assert_round_trip(&-7i32);
//! assert_worst_case(&u8::MAX);
//! assert_eq!(
//!     worst_case(&1u8),
//!     Err(CheckError::WidthMismatch { max: Some(2), actual: 1 })
//! );
//! assert_eq!(round_trip(&"é".to_owned()), Ok(()));
//! ```

use core::fmt::Debug;

use crate::{Field, FieldDecode, FieldError};

/// Why a value failed a field check.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckError {
    /// `encode` returned an error while writing into a `String`.
    #[error("encode returned an error")]
    Encode,
    /// The encoding has a character outside printable ASCII.
    #[error("encoding has non-printable or non-ASCII character {ch:?} at byte {index}")]
    NotPrintable {
        /// Byte index of the character.
        index: usize,
        /// The character.
        ch: char,
    },
    /// The encoding has a raw `,`, `:` or `~`.
    #[error("encoding has reserved character {ch:?} at byte {index}")]
    ReservedChar {
        /// Byte index of the character.
        index: usize,
        /// The character.
        ch: char,
    },
    /// The encoding is longer than `MAX_LEN`.
    #[error("encoding is {actual} characters, MAX_LEN is {max}")]
    TooLong {
        /// The field's `MAX_LEN`.
        max: usize,
        /// The encoding's length.
        actual: usize,
    },
    /// Decoding the encoding failed.
    #[error("decoding {encoded:?} failed: {error}")]
    Decode {
        /// The encoding.
        encoded: String,
        /// The decode error.
        error: FieldError,
    },
    /// The encoding decoded to a different value.
    #[error("decoding {encoded:?} gave a different value")]
    Mismatch {
        /// The encoding.
        encoded: String,
    },
    /// The value's encoding is not exactly `MAX_LEN` characters.
    #[error("worst-case value encodes to {actual} characters, MAX_LEN is {max:?}")]
    WidthMismatch {
        /// The field's `MAX_LEN`.
        max: Option<usize>,
        /// The encoding's length.
        actual: usize,
    },
}

fn encode_checked<T: Field>(value: &T) -> Result<String, CheckError> {
    let mut encoded = String::new();
    value.encode(&mut encoded).map_err(|_| CheckError::Encode)?;
    for (index, ch) in encoded.char_indices() {
        if !matches!(ch, ' '..='~') {
            return Err(CheckError::NotPrintable { index, ch });
        }
        if matches!(ch, ',' | ':' | '~') {
            return Err(CheckError::ReservedChar { index, ch });
        }
    }
    if let Some(max) = T::MAX_LEN
        && encoded.len() > max
    {
        return Err(CheckError::TooLong {
            max,
            actual: encoded.len(),
        });
    }
    Ok(encoded)
}

/// Checks that `value` encodes to a valid field and decodes back to itself.
///
/// # Errors
///
/// The first broken rule: a non-printable or reserved character, an encoding
/// over `MAX_LEN`, a decode error or a decoded value that differs.
pub fn round_trip<T>(value: &T) -> Result<(), CheckError>
where
    T: Field + for<'de> FieldDecode<'de> + PartialEq + Debug,
{
    let encoded = encode_checked(value)?;
    match T::decode(&encoded) {
        Ok(decoded) if decoded == *value => Ok(()),
        Ok(_) => Err(CheckError::Mismatch { encoded }),
        Err(error) => Err(CheckError::Decode { encoded, error }),
    }
}

/// Checks [`round_trip`] and that `value` encodes to exactly `MAX_LEN`
/// characters; pass the field's worst-case value.
///
/// # Errors
///
/// Any [`round_trip`] error, then [`CheckError::WidthMismatch`] when the
/// width differs from `MAX_LEN` (always for an unbounded field).
pub fn worst_case<T>(value: &T) -> Result<(), CheckError>
where
    T: Field + for<'de> FieldDecode<'de> + PartialEq + Debug,
{
    round_trip(value)?;
    let actual = encode_checked(value)?.len();
    if T::MAX_LEN == Some(actual) {
        Ok(())
    } else {
        Err(CheckError::WidthMismatch {
            max: T::MAX_LEN,
            actual,
        })
    }
}

/// [`round_trip`] for tests.
///
/// # Panics
///
/// Panics with the [`CheckError`] message when the check fails.
#[track_caller]
pub fn assert_round_trip<T>(value: &T)
where
    T: Field + for<'de> FieldDecode<'de> + PartialEq + Debug,
{
    if let Err(e) = round_trip(value) {
        panic!("{e}");
    }
}

/// [`worst_case`] for tests.
///
/// # Panics
///
/// Panics with the [`CheckError`] message when the check fails.
#[track_caller]
pub fn assert_worst_case<T>(value: &T)
where
    T: Field + for<'de> FieldDecode<'de> + PartialEq + Debug,
{
    if let Err(e) = worst_case(value) {
        panic!("{e}");
    }
}

#[cfg(test)]
mod tests {
    use core::fmt::{self, Write};

    use super::{CheckError, assert_round_trip, assert_worst_case, round_trip, worst_case};
    use crate::{Field, FieldDecode, FieldError};

    /// An author field whose encoding is whatever the test puts in it.
    #[derive(Debug, Clone, PartialEq)]
    struct Raw(&'static str, Option<usize>);

    impl Field for Raw {
        const MAX_LEN: Option<usize> = Some(2);
        fn encode<W: Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
            out.write_str(self.0)
        }
    }

    impl FieldDecode<'_> for Raw {
        fn decode(raw: &str) -> Result<Self, FieldError> {
            match raw {
                "ok" => Ok(Self("ok", None)),
                "xx" => Ok(Self("xx", Some(1))),
                _ => Err(FieldError::Empty),
            }
        }
    }

    #[test]
    fn good_values_pass() {
        assert_eq!(round_trip(&255u8), Ok(()));
        assert_eq!(worst_case(&u8::MAX), Ok(()));
        assert_eq!(round_trip(&Raw("ok", None)), Ok(()));
    }

    #[test]
    fn bad_author_impls_fail() {
        assert_eq!(
            round_trip(&Raw("a,", None)),
            Err(CheckError::ReservedChar { index: 1, ch: ',' })
        );
        assert_eq!(
            round_trip(&Raw("é", None)),
            Err(CheckError::NotPrintable { index: 0, ch: 'é' })
        );
        assert_eq!(
            round_trip(&Raw("abc", None)),
            Err(CheckError::TooLong { max: 2, actual: 3 })
        );
        assert_eq!(
            round_trip(&Raw("xx", None)),
            Err(CheckError::Mismatch {
                encoded: "xx".to_owned()
            })
        );
        assert_eq!(
            round_trip(&Raw("no", None)),
            Err(CheckError::Decode {
                encoded: "no".to_owned(),
                error: FieldError::Empty
            })
        );
    }

    #[test]
    fn worst_case_widths() {
        assert_eq!(
            worst_case(&1u8),
            Err(CheckError::WidthMismatch {
                max: Some(2),
                actual: 1
            })
        );
        assert_eq!(
            worst_case(&String::new()),
            Err(CheckError::WidthMismatch {
                max: None,
                actual: 0
            })
        );
    }

    #[test]
    fn asserts_pass() {
        assert_round_trip(&1u8);
        assert_worst_case(&u8::MAX);
    }

    #[test]
    #[should_panic(expected = "MAX_LEN is Some(2)")]
    fn assert_worst_case_panics_with_message() {
        assert_worst_case(&1u8);
    }

    #[test]
    #[should_panic(expected = "reserved character")]
    fn assert_round_trip_panics_with_message() {
        assert_round_trip(&Raw(",", None));
    }
}
