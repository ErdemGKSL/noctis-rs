//! The error a Payload field decode returns.

/// Why a Payload field could not be decoded.
///
/// Decoding is canonical: every input that is not exactly the encoding of some
/// value is one of these errors. Byte indices are into the decoded `&str`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FieldError {
    /// The input is empty, but this field never encodes to `""`.
    #[error("empty field")]
    Empty,
    /// The character starting at byte `index` is not allowed here.
    #[error("invalid character at byte {index}")]
    InvalidChar {
        /// Byte index of the first invalid character.
        index: usize,
    },
    /// A multi-digit integer starts with `0`.
    #[error("integer has a leading zero")]
    LeadingZero,
    /// The integer does not fit the field's type.
    #[error("integer overflows the field type")]
    Overflow,
    /// The input is longer than the field's worst-case width.
    #[error("field is {actual} characters, at most {max} allowed")]
    TooLong {
        /// The field's `MAX_LEN`.
        max: usize,
        /// The input's length in bytes.
        actual: usize,
    },
    /// The decoded string has more characters than the `BoundedStr` holds.
    #[error("string has {actual} characters, capacity is {capacity}")]
    OverCapacity {
        /// The `BoundedStr` capacity `N`.
        capacity: usize,
        /// The decoded length.
        actual: usize,
    },
    /// A Discord id decoded to `0`.
    #[error("id is zero")]
    ZeroId,
    /// The `!` escape at byte `index` is dangling, unknown or not canonical.
    #[error("bad escape at byte {index}")]
    BadEscape {
        /// Byte index of the `!` that starts the escape.
        index: usize,
    },
    /// The input continues after a complete value, from byte `index`.
    #[error("unexpected input at byte {index}")]
    TrailingInput {
        /// Byte index of the first unexpected character.
        index: usize,
    },
    /// A list does not end with its element terminator.
    #[error("list is not terminated")]
    BadList,
}

impl FieldError {
    /// Shifts index-bearing variants by `by` bytes; others are unchanged.
    pub(crate) const fn offset(self, by: usize) -> Self {
        match self {
            Self::InvalidChar { index } => Self::InvalidChar {
                index: index.saturating_add(by),
            },
            Self::BadEscape { index } => Self::BadEscape {
                index: index.saturating_add(by),
            },
            Self::TrailingInput { index } => Self::TrailingInput {
                index: index.saturating_add(by),
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FieldError;

    #[test]
    fn offset_shifts_only_indices() {
        assert_eq!(
            FieldError::InvalidChar { index: 0 }.offset(1),
            FieldError::InvalidChar { index: 1 }
        );
        assert_eq!(
            FieldError::BadEscape { index: 2 }.offset(1),
            FieldError::BadEscape { index: 3 }
        );
        assert_eq!(
            FieldError::TrailingInput { index: 1 }.offset(1),
            FieldError::TrailingInput { index: 2 }
        );
        let too_long = FieldError::TooLong { max: 2, actual: 3 };
        assert_eq!(too_long.offset(1), too_long);
    }

    #[test]
    fn messages_are_lower_case() {
        assert_eq!(FieldError::Empty.to_string(), "empty field");
        assert_eq!(
            FieldError::TooLong { max: 2, actual: 3 }.to_string(),
            "field is 3 characters, at most 2 allowed"
        );
    }
}
