//! The errors of Payload field decoding, `custom_id` encoding and decoding.

use crate::{BodyKind, Flags};

/// Why a Payload field could not be decoded.
///
/// Decoding is canonical: every input that is not exactly the encoding of some
/// value is one of these errors. Byte indices are into the decoded `&str`,
/// except for an error in a `Vec` element: there the index is relative to that
/// element after the list is unescaped.
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
    #[error("field is {actual} bytes, at most {max} allowed")]
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

/// A fixed-width section of the `custom_id` layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Section {
    /// The 1-character schema version `S`.
    Schema,
    /// The 5-character View envelope `E`.
    Envelope,
    /// The 2-character select nonce `N`.
    Nonce,
    /// The 6-character integrity tag `T`.
    Tag,
}

impl core::fmt::Display for Section {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Schema => "schema",
            Self::Envelope => "view envelope",
            Self::Nonce => "select nonce",
            Self::Tag => "tag",
        })
    }
}

/// Why a `custom_id` is malformed.
///
/// [`decode`](crate::decode) returns the layout variants; the field variants
/// come from decoding an inline body against a Payload. Byte indices are into
/// the whole `custom_id`, except inside [`CodecError::Field`], whose `error`
/// indices are relative to that field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    /// The id is empty.
    #[error("empty custom_id")]
    Empty,
    /// The id is longer than 100 bytes (CID-01).
    #[error("custom_id is {len} bytes, at most 100 allowed")]
    TooLong {
        /// The length in bytes.
        len: usize,
    },
    /// The byte at `index` is not ASCII (CID-07).
    #[error("non-ascii byte at {index}")]
    NotAscii {
        /// Byte index of the first non-ASCII byte.
        index: usize,
    },
    /// The byte at `index` is not allowed there.
    #[error("invalid character at byte {index}")]
    InvalidChar {
        /// Byte index of the invalid byte.
        index: usize,
    },
    /// The format digit names a layout generation this codec does not know.
    #[error("unknown layout generation {generation}")]
    Generation {
        /// The generation (`F / 16`).
        generation: u8,
    },
    /// The route is not followed by `:`.
    #[error("missing ':' after the route")]
    MissingSeparator,
    /// The route is empty.
    #[error("empty route")]
    EmptyRoute,
    /// The route is longer than 24 characters.
    #[error("route is {len} characters, at most 24 allowed")]
    RouteTooLong {
        /// The route's length.
        len: usize,
    },
    /// The id ends inside a fixed-width section its flags announce.
    #[error("custom_id ends inside the {section}")]
    Truncated {
        /// The cut-off section.
        section: Section,
    },
    /// The schema digit is `0`, which is written by leaving `S` out.
    #[error("schema digit is zero")]
    ZeroSchema,
    /// A Ref body carries a tag; only inline bodies are signed.
    #[error("ref body carries a tag")]
    TagOnRef,
    /// A Ref is not 11 characters long.
    #[error("ref is {len} characters, 11 required")]
    RefLength {
        /// The length after `~`.
        len: usize,
    },
    /// The inline body has fewer fields than the Payload, and field `field`
    /// has no absent value.
    #[error("missing field {field}")]
    MissingField {
        /// Zero-based index of the first missing field.
        field: usize,
    },
    /// The inline body has more fields than the Payload.
    #[error("unexpected field {field}")]
    ExtraField {
        /// Zero-based index of the first extra field.
        field: usize,
    },
    /// Field `field` did not decode.
    #[error("field {field}: {error}")]
    Field {
        /// Zero-based field index.
        field: usize,
        /// The field's error; its indices are relative to the field.
        error: FieldError,
    },
    /// A [`RefRecord`](crate::RefRecord) announces a section only ids carry
    /// (`E`, `N` or `T`); the first such section is named.
    #[error("ref record carries a {section} section")]
    UnexpectedSection {
        /// The first announced section a record never carries.
        section: Section,
    },
}

/// Why a well-formed `custom_id` does not match the Route it was decoded
/// against, because the Route changed since the id was sent (FR-60).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StaleReason {
    /// The schema version differs.
    #[error("schema {found}, route expects {expected}")]
    Schema {
        /// The Route's schema.
        expected: u8,
        /// The id's schema.
        found: u8,
    },
    /// The body kind (inline or Ref) is not one the Route uses.
    #[error("body kind does not match, route expects {expected:?}")]
    Body {
        /// The Route's body kind.
        expected: BodyKind,
    },
    /// The view, select or tag flags differ.
    #[error("flags {found:?}, route expects {expected:?}")]
    Flags {
        /// The Route's `E`/`N`/`T` flags for this body.
        expected: Flags,
        /// The id's `E`/`N`/`T` flags.
        found: Flags,
    },
}

/// Why a `custom_id` cannot have come from this App's Route (FR-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ForgedReason {
    /// The id names a different route.
    #[error("route does not match")]
    RouteMismatch,
    /// The stored Ref record names a different route than the `custom_id`
    /// (CID-05).
    #[error("ref record route does not match")]
    RecordRouteMismatch,
    /// A signed Route's inline id has no integrity tag `T`.
    #[error("integrity tag is missing")]
    MissingTag,
    /// The integrity tag `T` does not verify against any configured key.
    #[error("integrity tag does not verify")]
    BadTag,
}

/// The outcome of decoding a `custom_id` against a Route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DecodeError {
    /// The id is not a well-formed `custom_id` of this Route's Payload.
    #[error("malformed custom_id: {0}")]
    Malformed(#[from] CodecError),
    /// The id is well formed but from an older shape of the Route.
    #[error("stale custom_id: {0}")]
    Stale(StaleReason),
    /// The id cannot belong to the Route.
    #[error("forged custom_id: {0}")]
    Forged(ForgedReason),
}

/// Why a Payload could not be encoded into a `custom_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// The Route's schema is over 61.
    #[error("schema {schema} is over 61")]
    InvalidSchema {
        /// The Route's schema.
        schema: u8,
    },
    /// A `view` Route was encoded without a View envelope.
    #[error("view route needs a view envelope")]
    MissingEnvelope,
    /// A View envelope was given for a Route that is not `view`.
    #[error("route is not a view route")]
    UnexpectedEnvelope,
    /// A `select` Route was encoded without a select nonce.
    #[error("select route needs a select nonce")]
    MissingNonce,
    /// A select nonce was given for a Route that is not `select`.
    #[error("route is not a select route")]
    UnexpectedNonce,
    /// A `signed` Route was encoded without a signer.
    #[error("signed route needs a signer")]
    MissingSigner,
    /// A signer was given for a Route that is not `signed`.
    #[error("route is not a signed route")]
    UnexpectedSigner,
    /// The Payload must be stored by Ref (a `by_ref` Route, or a `spill`
    /// Route whose inline encoding does not fit).
    #[error("payload must be stored by ref")]
    NeedsRef,
    /// The inline encoding of an inline-only Route is over 100 characters.
    #[error("encoded custom_id is over 100 characters")]
    TooLong,
    /// A Payload field's `encode` returned an error of its own.
    #[error("a payload field failed to encode")]
    Field,
    /// A non-default [`RefPolicy`](crate::RefPolicy) was given for a Route
    /// whose body is always inline.
    #[error("route never stores its payload by ref")]
    UnexpectedRefPolicy,
    /// The [`RefPolicy`](crate::RefPolicy) TTL is zero.
    #[error("ref ttl is zero")]
    ZeroTtl,
    /// A [`RefRecord`](crate::RefRecord) route is not 1 to 24 characters of
    /// `[a-z0-9._-]`.
    #[error("invalid route id")]
    InvalidRoute,
    /// A [`RefRecord`](crate::RefRecord) payload byte is outside printable
    /// ASCII or is `:` or `~`.
    #[error("invalid payload byte at {index}")]
    InvalidBody {
        /// Byte index into the payload.
        index: usize,
    },
}

/// Why a [`Ref`](crate::Ref) could not be minted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RefError {
    /// The OS random source failed.
    #[error("os random source failed: {0}")]
    Rng(getrandom::Error),
    /// The random source gave too few usable bytes in 8 fills.
    #[error("random source exhausted")]
    Exhausted,
}

/// Why a [`PayloadKey`](crate::PayloadKey) could not be parsed from hex.
///
/// It never carries or prints key characters (AD-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    /// The input is not exactly 64 bytes long.
    #[error("payload key is {len} bytes, 64 hex digits expected")]
    Length {
        /// The input's length in bytes.
        len: usize,
    },
    /// The input has a byte that is not an ASCII hex digit.
    #[error("payload key has a non-hex character at {index}")]
    InvalidChar {
        /// Byte index of the first non-hex byte.
        index: usize,
    },
}

/// Why a View envelope could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EnvelopeError {
    /// The action is neither `_` nor an ASCII letter or digit.
    #[error("invalid view action {0:?}")]
    InvalidAction(char),
    /// The tab is over 61.
    #[error("tab {0} is over 61")]
    TabOutOfRange(u8),
    /// The page is over 3843.
    #[error("page {0} is over 3843")]
    PageOutOfRange(u16),
    /// The state version is over 61.
    #[error("state version {0} is over 61")]
    VersionOutOfRange(u8),
}

#[cfg(test)]
mod tests {
    use super::{CodecError, DecodeError, FieldError};

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
            "field is 3 bytes, at most 2 allowed"
        );
    }

    #[test]
    fn layout_messages_are_lower_case() {
        assert_eq!(
            CodecError::TooLong { len: 101 }.to_string(),
            "custom_id is 101 bytes, at most 100 allowed"
        );
        assert_eq!(
            DecodeError::from(CodecError::Empty),
            DecodeError::Malformed(CodecError::Empty)
        );
    }
}
