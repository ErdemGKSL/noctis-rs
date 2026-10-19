//! Typed values: postcard with a leading schema byte.

use bytes::Bytes;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Why a typed value did not encode or decode.
///
/// It never wraps postcard's own error, so the encoding stays an
/// implementation detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValueError {
    /// The value could not be serialized (for example, a sequence of unknown
    /// length).
    #[error("value could not be serialized")]
    Serialize,
    /// The stored bytes are empty: there is no schema byte.
    #[error("stored value is empty")]
    Empty,
    /// The stored bytes do not decode as the requested type.
    #[error("stored value is malformed")]
    Malformed,
    /// The value decoded, but `extra` bytes were left over.
    #[error("stored value has {extra} trailing bytes")]
    Trailing {
        /// The number of bytes left after the value.
        extra: usize,
    },
}

/// Encodes `value` as `[schema] ++ postcard(value)`.
///
/// # Errors
///
/// [`ValueError::Serialize`] when `value` cannot be serialized.
pub fn encode_value<T: Serialize + ?Sized>(schema: u8, value: &T) -> Result<Bytes, ValueError> {
    let out = postcard::to_extend(value, vec![schema]).map_err(|_| ValueError::Serialize)?;
    Ok(Bytes::from(out))
}

/// Decodes bytes written by [`encode_value`].
///
/// A different schema byte is a miss: `Ok(None)`.
///
/// # Errors
///
/// [`ValueError::Empty`] for empty input, [`ValueError::Malformed`] when the
/// body does not decode as `T`, and [`ValueError::Trailing`] when bytes are
/// left after it.
pub fn decode_value<T: DeserializeOwned>(schema: u8, raw: &[u8]) -> Result<Option<T>, ValueError> {
    let (&stored, body) = raw.split_first().ok_or(ValueError::Empty)?;
    if stored != schema {
        return Ok(None);
    }
    decode_postcard(body).map(Some)
}

/// Decodes a whole postcard body, rejecting trailing bytes.
pub(crate) fn decode_postcard<T: DeserializeOwned>(body: &[u8]) -> Result<T, ValueError> {
    let (value, rest) = postcard::take_from_bytes(body).map_err(|_| ValueError::Malformed)?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(ValueError::Trailing { extra: rest.len() })
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serializer};

    use super::*;
    use crate::PortError;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[expect(
        clippy::struct_field_names,
        reason = "the spec's sample type is V { n, s, v }"
    )]
    struct V {
        n: u32,
        s: String,
        v: Vec<u8>,
    }

    fn sample() -> V {
        V {
            n: 42,
            s: "a".to_owned(),
            v: vec![1, 2],
        }
    }

    #[test]
    fn round_trip_keeps_schema_byte_first() {
        let raw = encode_value(7, &sample()).unwrap();
        assert_eq!(raw[0], 7);
        assert_eq!(decode_value::<V>(7, &raw), Ok(Some(sample())));
    }

    #[test]
    fn schema_extremes_round_trip() {
        for schema in [0, 255] {
            let raw = encode_value(schema, &sample()).unwrap();
            assert_eq!(raw[0], schema);
            assert_eq!(decode_value::<V>(schema, &raw), Ok(Some(sample())));
        }
    }

    #[test]
    fn schema_mismatch_is_a_miss() {
        let raw = encode_value(3, &sample()).unwrap();
        assert_eq!(decode_value::<V>(4, &raw), Ok(None));
    }

    #[test]
    fn empty_input_is_an_error() {
        assert_eq!(decode_value::<V>(1, b""), Err(ValueError::Empty));
    }

    #[test]
    fn schema_byte_only() {
        assert_eq!(decode_value::<V>(1, &[1]), Err(ValueError::Malformed));
        let unit = encode_value(1, &()).unwrap();
        assert_eq!(unit.as_ref(), [1]);
        assert_eq!(decode_value::<()>(1, &unit), Ok(Some(())));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut raw = encode_value(1, &sample()).unwrap().to_vec();
        raw.extend_from_slice(&[0, 0]);
        assert_eq!(
            decode_value::<V>(1, &raw),
            Err(ValueError::Trailing { extra: 2 })
        );
    }

    #[test]
    fn garbage_body_is_malformed() {
        let raw = [1, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        assert_eq!(decode_value::<V>(1, &raw), Err(ValueError::Malformed));
    }

    #[test]
    fn unicode_round_trips() {
        let value = V {
            s: "ğüş 😀 é\u{0}".to_owned(),
            ..sample()
        };
        let raw = encode_value(2, &value).unwrap();
        assert_eq!(decode_value::<V>(2, &raw), Ok(Some(value)));
    }

    #[test]
    fn large_value_round_trips() {
        let value = V {
            v: vec![0xAB; 1 << 20],
            ..sample()
        };
        let raw = encode_value(9, &value).unwrap();
        let body = postcard::to_allocvec(&value).unwrap();
        assert_eq!(raw.len(), 1 + body.len());
        assert_eq!(decode_value::<V>(9, &raw), Ok(Some(value)));
    }

    struct UnknownLen;

    impl Serialize for UnknownLen {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut seq = serializer.serialize_seq(None)?;
            seq.serialize_element(&1u8)?;
            seq.end()
        }
    }

    #[test]
    fn unknown_length_sequence_fails_to_serialize() {
        assert_eq!(encode_value(1, &UnknownLen), Err(ValueError::Serialize));
    }

    #[test]
    fn converts_into_port_error() {
        let err = PortError::from(ValueError::Empty);
        assert!(matches!(err, PortError::Value(ValueError::Empty)));
        assert!(!err.to_string().is_empty());
    }
}
