//! The stored record of a Ref: `F route ":" [S] body` (AD-5, AD-18, CID-05).
#![deny(clippy::indexing_slicing, clippy::string_slice)]

use bytes::Bytes;

use crate::base62::digit;
use crate::layout::{
    FMT_LEN, SEP_LEN, check_inline, is_inline_byte, parse_format, parse_route, parse_schema,
};
use noctis_rules::cid::SCHEMA_MAX as MAX_SCHEMA;

use crate::parts::Flags;
use crate::route::{FieldReader, MAX_ROUTE_LEN, PayloadDecode, Route, is_route_byte};
use crate::{CodecError, DecodeError, EncodeError, ForgedReason, Section, StaleReason};

/// The record a Ref is stored with: the Route id, the schema version and the
/// Payload's inline field body.
///
/// It binds the Ref to its Route and schema, so a Ref replayed on another
/// Route is [`DecodeError::Forged`] and a Ref from an older schema is
/// [`DecodeError::Stale`] (see [`decode_record`]). It is written with the
/// `custom_id` grammar, `F route ":" [S] body`, where `F` only ever has the
/// `S` flag and the body has no length cap.
///
/// ```
/// use noctis_codec::RefRecord;
///
/// let record = RefRecord::new("r", 2, "47,3")?;
/// let bytes = record.encode();
/// assert_eq!(&bytes[..], b"1r:247,3");
/// assert_eq!(RefRecord::decode(&bytes), Ok(record));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RefRecord<'a> {
    route: &'a str,
    schema: u8,
    payload: &'a str,
}

impl<'a> RefRecord<'a> {
    /// Checks and builds a record.
    ///
    /// # Errors
    ///
    /// Checked in this order: [`EncodeError::InvalidRoute`] when `route` is
    /// not 1 to 24 characters of `[a-z0-9._-]`;
    /// [`EncodeError::InvalidSchema`] when `schema` is over 61;
    /// [`EncodeError::InvalidBody`] at the first `payload` byte outside
    /// printable ASCII or equal to `:` or `~`.
    pub fn new(route: &'a str, schema: u8, payload: &'a str) -> Result<Self, EncodeError> {
        let r = route.as_bytes();
        if r.is_empty() || r.len() > MAX_ROUTE_LEN || !r.iter().all(|&b| is_route_byte(b)) {
            return Err(EncodeError::InvalidRoute);
        }
        if schema > MAX_SCHEMA {
            return Err(EncodeError::InvalidSchema { schema });
        }
        if let Some(index) = payload.bytes().position(|b| !is_inline_byte(b)) {
            return Err(EncodeError::InvalidBody { index });
        }
        Ok(Self::new_unchecked(route, schema, payload))
    }

    /// Builds a record from parts the caller has checked.
    pub(crate) const fn new_unchecked(route: &'a str, schema: u8, payload: &'a str) -> Self {
        Self {
            route,
            schema,
            payload,
        }
    }

    /// The route id.
    pub const fn route(&self) -> &'a str {
        self.route
    }

    /// The schema version (`0` when `S` is absent).
    pub const fn schema(&self) -> u8 {
        self.schema
    }

    /// The Payload's inline field body.
    pub const fn payload(&self) -> &'a str {
        self.payload
    }

    /// Encodes the record as `F route ":" [S] body`.
    pub fn encode(&self) -> Bytes {
        let has_schema = self.schema > 0;
        let mut out = Vec::with_capacity(
            FMT_LEN + self.route.len() + SEP_LEN + usize::from(has_schema) + self.payload.len(),
        );
        let flags = if has_schema { Flags::S } else { Flags::EMPTY };
        out.push(digit(flags.bits()));
        out.extend_from_slice(self.route.as_bytes());
        out.push(b':');
        if has_schema {
            out.push(digit(self.schema));
        }
        out.extend_from_slice(self.payload.as_bytes());
        Bytes::from(out)
    }

    /// Decodes a record.
    ///
    /// Total and injective: every input gives `Ok` or a [`CodecError`],
    /// never a panic, and `decode(b)` = `Ok(r)` implies `r.encode() == b`.
    ///
    /// # Errors
    ///
    /// The first failed check, in this order: empty; a non-ASCII byte; a
    /// bad format digit, a generation other than 0 or the reserved flag
    /// value 2 (`0b0010`, [`CodecError::ReservedFlag`]); an `N` or `T` flag
    /// ([`CodecError::UnexpectedSection`], naming the first); a bad,
    /// unterminated, empty or over-long route; a missing, zero or bad schema
    /// digit; a body byte outside printable ASCII or equal to `:` or `~`.
    /// Indices are byte indices into `raw`.
    pub fn decode(raw: &'a [u8]) -> Result<Self, CodecError> {
        if raw.is_empty() {
            return Err(CodecError::Empty);
        }
        if let Some(index) = raw.iter().position(|b| !b.is_ascii()) {
            return Err(CodecError::NotAscii { index });
        }
        // Every byte is ASCII, so this never fails.
        let text = core::str::from_utf8(raw).map_err(|e| CodecError::NotAscii {
            index: e.valid_up_to(),
        })?;
        let flags = parse_format(raw)?;
        for (flag, section) in [(Flags::N, Section::Nonce), (Flags::T, Section::Tag)] {
            if flags.contains(flag) {
                return Err(CodecError::UnexpectedSection { section });
            }
        }
        let route_end = parse_route(raw)?;
        let (schema, pos) = parse_schema(raw, route_end + SEP_LEN, flags)?;
        let body = raw.get(pos..).unwrap_or_default();
        check_inline(body, pos)?;
        Ok(Self {
            route: text.get(FMT_LEN..route_end).unwrap_or_default(),
            schema,
            payload: text.get(pos..).unwrap_or_default(),
        })
    }
}

/// Decodes the stored record of a Ref against Route `R` (phase 3).
///
/// Call it only after [`Decoded::payload`](crate::Decoded::payload) for `R`
/// returned [`PayloadSource::Ref`](crate::PayloadSource::Ref), which already
/// proved the `custom_id`'s route is `R::ID`; comparing the record with
/// `R::ID` is then comparing it with the id's route.
///
/// ```
/// use noctis_codec::{DecodeError, ForgedReason, Route, decode_record};
/// # use core::fmt;
/// # use noctis_codec::{CodecError, FieldReader, FieldWriter, Payload, PayloadDecode, RouteId, RouteShape};
/// # #[derive(Debug, PartialEq)]
/// # struct Qty(u8);
/// # impl Payload for Qty {
/// #     const MAX_ENCODED_LEN: Option<usize> = <u8 as noctis_codec::Field>::MAX_LEN;
/// #     fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result { out.field(&self.0) }
/// # }
/// # impl<'de> PayloadDecode<'de> for Qty {
/// #     fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError> { Ok(Self(f.field()?)) }
/// # }
/// struct Stored;
/// impl Route for Stored {
///     const ID: RouteId = RouteId::new("r");
///     const SHAPE: RouteShape = RouteShape::BY_REF;
///     const SCHEMA: u8 = 0;
///     type Payload = Qty;
/// }
///
/// assert_eq!(decode_record::<Stored>(b"0r:3"), Ok(Qty(3)));
/// assert_eq!(
///     decode_record::<Stored>(b"0x:3"),
///     Err(DecodeError::Forged(ForgedReason::RecordRouteMismatch))
/// );
/// ```
///
/// # Errors
///
/// Checked in this order: a malformed record is [`DecodeError::Malformed`];
/// a record of another route is [`DecodeError::Forged`] with
/// [`ForgedReason::RecordRouteMismatch`]; a different schema is
/// [`DecodeError::Stale`]; a body that is not the Payload's encoding is
/// [`DecodeError::Malformed`].
pub fn decode_record<'r, R: Route>(record: &'r [u8]) -> Result<R::Payload, DecodeError>
where
    R::Payload: PayloadDecode<'r>,
{
    let record = RefRecord::decode(record)?;
    if record.route() != R::ID.as_str() {
        return Err(DecodeError::Forged(ForgedReason::RecordRouteMismatch));
    }
    if record.schema() != R::SCHEMA {
        return Err(DecodeError::Stale(StaleReason::Schema {
            expected: R::SCHEMA,
            found: record.schema(),
        }));
    }
    let mut reader = FieldReader::new(record.payload());
    let payload = R::Payload::decode_fields(&mut reader)?;
    reader.finish()?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::RefRecord;
    use crate::{CodecError, EncodeError, Section};

    fn enc(route: &str, schema: u8, payload: &str) -> Result<Vec<u8>, EncodeError> {
        RefRecord::new(route, schema, payload).map(|r| r.encode().to_vec())
    }

    fn err(raw: &[u8]) -> CodecError {
        match RefRecord::decode(raw) {
            Ok(r) => panic!("{raw:?} decoded to {r:?}"),
            Err(e) => e,
        }
    }

    #[test]
    fn record_new_and_encode() {
        use EncodeError as E;

        assert_eq!(enc("r", 0, "47,3").unwrap(), b"0r:47,3");
        assert_eq!(enc("r", 2, "47,3").unwrap(), b"1r:247,3");
        assert_eq!(enc("r", 61, "").unwrap(), b"1r:z");
        assert_eq!(enc("_x", 0, "").unwrap(), b"0_x:");
        assert!(enc(&"a".repeat(24), 0, "").is_ok());

        for route in ["", &"a".repeat(25), "R", "a:b", "é"] {
            assert_eq!(enc(route, 0, ""), Err(E::InvalidRoute), "{route:?}");
        }
        assert_eq!(enc("r", 62, ""), Err(E::InvalidSchema { schema: 62 }));
        assert_eq!(enc("r", 255, ""), Err(E::InvalidSchema { schema: 255 }));
        for (payload, index) in [("a~", 1), ("a:b", 1), ("\t", 0), ("é", 0), ("\u{7f}", 0)] {
            assert_eq!(
                enc("r", 0, payload),
                Err(E::InvalidBody { index }),
                "{payload:?}"
            );
        }
        assert_eq!(enc("", 62, ""), Err(E::InvalidRoute));
    }

    #[test]
    fn record_decode_matrix() {
        use CodecError as E;
        let bad = |index| E::InvalidChar { index };

        assert_eq!(err(b""), E::Empty);
        assert_eq!(err(b"0r:\xc3\xa9"), E::NotAscii { index: 3 });
        assert_eq!(err(b"\xff"), E::NotAscii { index: 0 });
        assert_eq!(err(b"!r:"), bad(0));
        assert_eq!(err(b"Gr:"), E::Generation { generation: 1 });
        assert_eq!(err(b"2r:x"), E::ReservedFlag);
        assert_eq!(err(b"Fr:z"), E::ReservedFlag);
        for (raw, section) in [
            (&b"4r:00"[..], Section::Nonce),
            (b"8r:000000", Section::Tag),
        ] {
            assert_eq!(err(raw), E::UnexpectedSection { section });
        }
        assert_eq!(err(b"0r"), E::MissingSeparator);
        assert_eq!(err(b"0:1"), E::EmptyRoute);
        assert_eq!(
            err(format!("0{}:", "a".repeat(25)).as_bytes()),
            E::RouteTooLong { len: 25 }
        );
        assert_eq!(err(b"0R:1"), bad(1));
        assert_eq!(
            err(b"1r:"),
            E::Truncated {
                section: Section::Schema
            }
        );
        assert_eq!(err(b"1r:0"), E::ZeroSchema);
        assert_eq!(err(b"1r:!"), bad(3));
        assert_eq!(err(b"0r:~0123456789A"), bad(3));
        assert_eq!(err(b"0r:a:b"), bad(4));
        assert_eq!(err(b"0r:\t"), bad(3));

        assert_eq!(
            RefRecord::decode(b"0r:"),
            Ok(RefRecord::new("r", 0, "").unwrap())
        );
        let long = format!("0r:{}", "x".repeat(10_000));
        let record = RefRecord::decode(long.as_bytes()).unwrap();
        assert_eq!(record.payload().len(), 10_000);
        assert_eq!(record.route(), "r");
        assert_eq!(record.schema(), 0);
        let s2 = RefRecord::decode(b"1r:247,3").unwrap();
        assert_eq!((s2.route(), s2.schema(), s2.payload()), ("r", 2, "47,3"));
    }
}
