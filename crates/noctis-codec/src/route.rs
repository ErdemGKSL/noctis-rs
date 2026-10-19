//! The `Route`, `Payload` and `PayloadDecode` traits, route ids and shapes,
//! and the field writer and reader a Payload's fields go through (AD-5, AD-19).

use core::fmt;

use crate::{CodecError, Field, FieldDecode};

/// Longest route id, in characters.
pub(crate) const MAX_ROUTE_LEN: usize = 24;

/// Whether `b` may appear in a route id (`[a-z0-9._-]`).
pub(crate) const fn is_route_byte(b: u8) -> bool {
    matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')
}

/// A route id: 1 to 24 characters of `[a-z0-9._-]`. Ids starting with `_`
/// are reserved for Noctis.
///
/// ```
/// use noctis_codec::RouteId;
///
/// const BUY: RouteId = RouteId::new("shop.buy");
/// assert_eq!(BUY.as_str(), "shop.buy");
/// assert!(!BUY.is_reserved());
/// ```
///
/// A bad id fails to compile when the `RouteId` is a constant:
///
/// ```compile_fail
/// use noctis_codec::RouteId;
///
/// const _: RouteId = RouteId::new("Shop");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteId(&'static str);

impl RouteId {
    /// Checks `id` against the route grammar.
    ///
    /// # Panics
    ///
    /// When `id` is empty, longer than 24 characters, or has a character
    /// outside `[a-z0-9._-]`; in a `const` this is a compile error.
    pub const fn new(id: &'static str) -> Self {
        let bytes = id.as_bytes();
        assert!(!bytes.is_empty(), "route id is empty");
        assert!(
            bytes.len() <= MAX_ROUTE_LEN,
            "route id is longer than 24 characters"
        );
        let mut i = 0;
        while i < bytes.len() {
            assert!(
                is_route_byte(bytes[i]),
                "route id has a character outside [a-z0-9._-]"
            );
            i += 1;
        }
        Self(id)
    }

    /// The id.
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// Whether the id starts with `_`, which is reserved for Noctis (CID-03).
    pub const fn is_reserved(self) -> bool {
        matches!(self.0.as_bytes().first(), Some(b'_'))
    }
}

impl fmt::Display for RouteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// How a Route's body is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyKind {
    /// Always inline in the `custom_id`.
    Inline,
    /// Always by Ref (`~` + 11 characters).
    ByRef,
    /// Inline when it fits, by Ref otherwise (`overflow = spill`).
    Spill,
}

/// The codec shape of a Route: its body kind and which of the View envelope,
/// select nonce and integrity tag it carries.
///
/// ```
/// use noctis_codec::{BodyKind, RouteShape};
///
/// const SHAPE: RouteShape = RouteShape::SPILL.signed();
/// assert_eq!(SHAPE.body(), BodyKind::Spill);
/// assert!(SHAPE.is_signed() && !SHAPE.has_view());
/// ```
///
/// A `by_ref` Route cannot be signed (the Ref is already unguessable):
///
/// ```compile_fail
/// use noctis_codec::RouteShape;
///
/// const _: RouteShape = RouteShape::BY_REF.signed();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RouteShape {
    body: BodyKind,
    view: bool,
    select: bool,
    signed: bool,
}

impl RouteShape {
    /// An inline body and no other section.
    pub const INLINE: Self = Self::with_body(BodyKind::Inline);
    /// A Ref body and no other section.
    pub const BY_REF: Self = Self::with_body(BodyKind::ByRef);
    /// An inline body that spills to a Ref, and no other section.
    pub const SPILL: Self = Self::with_body(BodyKind::Spill);

    const fn with_body(body: BodyKind) -> Self {
        Self {
            body,
            view: false,
            select: false,
            signed: false,
        }
    }

    /// The shape with a View envelope.
    #[must_use]
    pub const fn view(self) -> Self {
        Self { view: true, ..self }
    }

    /// The shape with a select nonce.
    #[must_use]
    pub const fn select(self) -> Self {
        Self {
            select: true,
            ..self
        }
    }

    /// The shape with an integrity tag on inline bodies.
    ///
    /// # Panics
    ///
    /// When the body is [`BodyKind::ByRef`]; in a `const` this is a compile
    /// error.
    #[must_use]
    pub const fn signed(self) -> Self {
        assert!(
            !matches!(self.body, BodyKind::ByRef),
            "a by_ref route cannot be signed"
        );
        Self {
            signed: true,
            ..self
        }
    }

    /// The body kind.
    pub const fn body(self) -> BodyKind {
        self.body
    }

    /// Whether the Route carries a View envelope.
    pub const fn has_view(self) -> bool {
        self.view
    }

    /// Whether the Route carries a select nonce.
    pub const fn has_select(self) -> bool {
        self.select
    }

    /// Whether the Route's inline bodies carry an integrity tag.
    pub const fn is_signed(self) -> bool {
        self.signed
    }
}

/// A Route: a route id, a codec shape, a schema version and a Payload type.
///
/// Story 2.5 derives this; a hand implementation looks like this:
///
/// ```
/// use core::fmt;
/// use noctis_codec::{
///     CodecError, FieldReader, FieldWriter, Parts, Payload, PayloadDecode,
///     PayloadSource, Route, RouteId, RouteShape, decode_route, encode,
/// };
///
/// #[derive(Debug, PartialEq)]
/// struct BuyV1 {
///     item: u64,
///     qty: u8,
/// }
///
/// impl Payload for BuyV1 {
///     fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
///         out.field(&self.item)?;
///         out.field(&self.qty)
///     }
/// }
///
/// impl<'de> PayloadDecode<'de> for BuyV1 {
///     fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError> {
///         Ok(Self { item: f.field()?, qty: f.field()? })
///     }
/// }
///
/// struct Buy;
///
/// impl Route for Buy {
///     const ID: RouteId = RouteId::new("shop.buy");
///     const SHAPE: RouteShape = RouteShape::INLINE;
///     const SCHEMA: u8 = 0;
///     type Payload = BuyV1;
/// }
///
/// let id = encode::<Buy>(&BuyV1 { item: 255, qty: 3 }, &Parts::new())?;
/// assert_eq!(id.as_str(), "0shop.buy:47,3");
/// assert_eq!(
///     decode_route::<Buy>(&id),
///     Ok(PayloadSource::Inline(BuyV1 { item: 255, qty: 3 }))
/// );
/// # Ok::<(), noctis_codec::EncodeError>(())
/// ```
pub trait Route {
    /// The route id.
    const ID: RouteId;
    /// The codec shape.
    const SHAPE: RouteShape;
    /// The schema version, `0..=61`; `0` writes no `S` section.
    const SCHEMA: u8;
    /// The Payload carried by the Route's `custom_id`s.
    type Payload: Payload;
}

/// A Payload: an ordered list of [`Field`]s.
///
/// Fields are written in order, separated by `,`. Appending an `Option`
/// field is compatible with ids encoded before it existed (see
/// [`FieldDecode::absent`]); every other change bumps the Route's schema.
pub trait Payload {
    /// Writes every field, in order, with [`FieldWriter::field`].
    ///
    /// # Errors
    ///
    /// Returns the writer's error, for example when the id is full.
    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result;
}

/// Decoding of a [`Payload`] from an inline body.
pub trait PayloadDecode<'de>: Payload + Sized {
    /// Reads every field, in the order [`Payload::encode_fields`] writes them,
    /// with [`FieldReader::field`].
    ///
    /// # Errors
    ///
    /// Returns the reader's [`CodecError`].
    fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError>;
}

/// Writes a Payload's fields, separated by `,`.
pub struct FieldWriter<'w> {
    out: &'w mut dyn fmt::Write,
    first: bool,
}

impl<'w> FieldWriter<'w> {
    pub(crate) fn new(out: &'w mut dyn fmt::Write) -> Self {
        Self { out, first: true }
    }

    /// Writes the next field.
    ///
    /// # Errors
    ///
    /// Returns the output's error, for example when the id is full.
    pub fn field(&mut self, value: &impl Field) -> fmt::Result {
        if !core::mem::replace(&mut self.first, false) {
            self.out.write_char(',')?;
        }
        value.encode(&mut *self.out)
    }
}

impl fmt::Debug for FieldWriter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldWriter")
            .field("first", &self.first)
            .finish_non_exhaustive()
    }
}

/// Reads a Payload's fields from an inline body.
///
/// When the body has fewer fields than the Payload, a missing trailing field
/// takes its [`FieldDecode::absent`] value, or is a
/// [`CodecError::MissingField`]. An empty body is both "no field" and "one
/// empty field": the first field is decoded from `""` and falls back to its
/// absent value.
#[derive(Debug, Clone)]
pub struct FieldReader<'de> {
    rest: Option<&'de str>,
    next: usize,
}

impl<'de> FieldReader<'de> {
    pub(crate) fn new(body: &'de str) -> Self {
        Self {
            rest: Some(body),
            next: 0,
        }
    }

    /// Decodes the next field.
    ///
    /// # Errors
    ///
    /// [`CodecError::MissingField`] when the body has no more fields and the
    /// field has no absent value; [`CodecError::Field`] when it does not
    /// decode.
    pub fn field<F: FieldDecode<'de>>(&mut self) -> Result<F, CodecError> {
        let field = self.next;
        self.next = field.saturating_add(1);
        let Some(rest) = self.rest else {
            return F::absent().ok_or(CodecError::MissingField { field });
        };
        let (raw, tail) = match rest.split_once(',') {
            Some((raw, tail)) => (raw, Some(tail)),
            None => (rest, None),
        };
        self.rest = tail;
        match F::decode(raw) {
            Ok(v) => Ok(v),
            Err(error) => {
                let absent = if field == 0 && rest.is_empty() {
                    F::absent()
                } else {
                    None
                };
                absent.ok_or(CodecError::Field { field, error })
            }
        }
    }

    /// Fails when the body has fields left over.
    pub(crate) fn finish(self) -> Result<(), CodecError> {
        match self.rest {
            None => Ok(()),
            // A zero-field Payload is the empty body.
            Some("") if self.next == 0 => Ok(()),
            Some(_) => Err(CodecError::ExtraField { field: self.next }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BodyKind, FieldReader, FieldWriter, RouteId, RouteShape};
    use crate::{CodecError, FieldError};

    #[test]
    fn route_ids() {
        for id in ["a", "abcdefghijklmnopqrstuvwx", "_x", "a.b-c_9"] {
            let r = RouteId::new(id);
            assert_eq!(r.as_str(), id);
            assert_eq!(r.to_string(), id);
            assert_eq!(r.is_reserved(), id == "_x");
        }
    }

    #[test]
    #[should_panic(expected = "empty")]
    fn route_id_empty_panics() {
        let _ = RouteId::new("");
    }

    #[test]
    #[should_panic(expected = "longer than 24")]
    fn route_id_too_long_panics() {
        let _ = RouteId::new("abcdefghijklmnopqrstuvwxy");
    }

    #[test]
    #[should_panic(expected = "outside")]
    fn route_id_upper_case_panics() {
        let _ = RouteId::new("Shop");
    }

    #[test]
    #[should_panic(expected = "outside")]
    fn route_id_colon_panics() {
        let _ = RouteId::new("a:b");
    }

    #[test]
    #[should_panic(expected = "outside")]
    fn route_id_non_ascii_panics() {
        let _ = RouteId::new("é");
    }

    #[test]
    #[should_panic(expected = "cannot be signed")]
    fn by_ref_signed_panics() {
        let _ = RouteShape::BY_REF.signed();
    }

    #[test]
    fn shapes() {
        let spill = RouteShape::SPILL.signed();
        assert_eq!(spill.body(), BodyKind::Spill);
        assert!(spill.is_signed() && !spill.has_view() && !spill.has_select());
        let vs = RouteShape::INLINE.view().select();
        assert_eq!(vs.body(), BodyKind::Inline);
        assert!(vs.has_view() && vs.has_select() && !vs.is_signed());
        assert_eq!(RouteShape::BY_REF.body(), BodyKind::ByRef);
    }

    #[test]
    fn writer_separates_fields() {
        let mut out = String::new();
        let mut w = FieldWriter::new(&mut out);
        w.field(&255u64).unwrap();
        w.field(&3u8).unwrap();
        w.field(&"a,b".to_owned()).unwrap();
        assert_eq!(out, "47,3,a!1b");
    }

    #[test]
    fn reader_fills_absent_trailing_options() {
        let mut r = FieldReader::new("5");
        assert_eq!(r.field::<u8>(), Ok(5));
        assert_eq!(r.field::<Option<u8>>(), Ok(None));
        assert_eq!(r.field::<u8>(), Err(CodecError::MissingField { field: 2 }));
        assert_eq!(r.finish(), Ok(()));
    }

    #[test]
    fn reader_empty_body() {
        let mut r = FieldReader::new("");
        assert_eq!(r.field::<String>(), Ok(String::new()));
        assert_eq!(r.finish(), Ok(()));
        let mut r = FieldReader::new("");
        assert_eq!(r.field::<Option<u8>>(), Ok(None));
        assert_eq!(r.finish(), Ok(()));
        let mut r = FieldReader::new("");
        assert_eq!(
            r.field::<u8>(),
            Err(CodecError::Field {
                field: 0,
                error: FieldError::Empty
            })
        );
        assert_eq!(FieldReader::new("").finish(), Ok(()));
        assert_eq!(
            FieldReader::new("x").finish(),
            Err(CodecError::ExtraField { field: 0 })
        );
    }

    #[test]
    fn reader_rejects_trailing_separator() {
        let mut r = FieldReader::new("1,");
        assert_eq!(r.field::<u8>(), Ok(1));
        assert_eq!(r.finish(), Err(CodecError::ExtraField { field: 1 }));
    }
}
