//! Support items for the `noctis` derives (the component and modal kind
//! derives, `#[derive(Payload)]`, `#[derive(PayloadEnum)]`): the compile-time length
//! proof (CID-01), `PayloadEnum` digits and the Route runtime declaration.
//!
//! Every item is `#[doc(hidden)]`: macro output names them through
//! `::noctis::__private::codec`, and they are not part of the stable API.

use core::fmt;

use crate::FieldError;
use crate::base62::{ALPHABET, digit_value};
use crate::layout::{
    FMT_LEN, MAX_ID_LEN, NONCE_LEN, REF_LEN, REF_MARKER_LEN, SCHEMA_LEN, SEP_LEN, TAG_LEN,
};
use crate::route::{BodyKind, Payload, Route, RouteId};

/// Width of a Ref body: `~` and the Ref.
const REF_BODY_LEN: usize = REF_MARKER_LEN + REF_LEN;

/// The worst-case body width of a list of fields: the sum of `widths` plus
/// one `,` between each two, saturating; `None` when any width is `None`.
///
/// ```
/// use noctis_codec::sum_fields;
///
/// assert_eq!(sum_fields(&[]), Some(0));
/// assert_eq!(sum_fields(&[Some(11), Some(2)]), Some(14));
/// assert_eq!(sum_fields(&[Some(1), None]), None);
/// ```
#[doc(hidden)]
pub const fn sum_fields(widths: &[Option<usize>]) -> Option<usize> {
    let mut total: usize = 0;
    let mut i = 0;
    while i < widths.len() {
        match widths[i] {
            Some(w) => total = total.saturating_add(w),
            None => return None,
        }
        i += 1;
    }
    Some(total.saturating_add(widths.len().saturating_sub(1)))
}

/// The worst-case width of a Route's `custom_id`, section by section.
///
/// `Display` prints the breakdown the generated `noctis_route_width` test
/// shows, for example
/// `shop.buy: F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteWidth {
    route: RouteId,
    body_kind: BodyKind,
    schema: usize,
    nonce: usize,
    body: Option<usize>,
    tag: usize,
}

/// The [`RouteWidth`] of Route `R`.
#[doc(hidden)]
pub const fn route_width<R: Route>() -> RouteWidth {
    let shape = R::SHAPE;
    RouteWidth {
        route: R::ID,
        body_kind: shape.body(),
        schema: if R::SCHEMA == 0 { 0 } else { SCHEMA_LEN },
        nonce: if shape.has_select() { NONCE_LEN } else { 0 },
        body: R::Payload::MAX_ENCODED_LEN,
        tag: if shape.is_signed() && !matches!(shape.body(), BodyKind::ByRef) {
            TAG_LEN
        } else {
            0
        },
    }
}

impl RouteWidth {
    /// The route id.
    pub const fn route(&self) -> RouteId {
        self.route
    }

    /// How the body is stored.
    pub const fn body_kind(&self) -> BodyKind {
        self.body_kind
    }

    /// Everything before the body: `F`, the route, `:`, `S` and `N`.
    pub const fn prefix(&self) -> usize {
        FMT_LEN + self.route.as_str().len() + SEP_LEN + self.schema + self.nonce
    }

    /// Width of `S`: 1, or 0 for schema 0.
    pub const fn schema(&self) -> usize {
        self.schema
    }

    /// Width of the select nonce `N`: 2 or 0.
    pub const fn nonce(&self) -> usize {
        self.nonce
    }

    /// The Payload's worst-case inline body (`MAX_ENCODED_LEN`), `None` when
    /// unbounded.
    pub const fn body(&self) -> Option<usize> {
        self.body
    }

    /// Width of the integrity tag `T` on inline bodies: 6 or 0.
    pub const fn tag(&self) -> usize {
        self.tag
    }

    /// The longest id the Route can produce, `None` when an inline Route has
    /// an unbounded body. A `spill` id is at most 100: it is inline only when
    /// it fits.
    pub const fn total(&self) -> Option<usize> {
        let prefix = self.prefix();
        match self.body_kind {
            BodyKind::Inline => match self.body {
                Some(body) => Some(prefix.saturating_add(body).saturating_add(self.tag)),
                None => None,
            },
            BodyKind::ByRef => Some(prefix + REF_BODY_LEN),
            BodyKind::Spill => {
                let inline = match self.body {
                    Some(body) => body.saturating_add(self.tag),
                    None => usize::MAX,
                };
                let body = if inline > REF_BODY_LEN {
                    inline
                } else {
                    REF_BODY_LEN
                };
                let total = prefix.saturating_add(body);
                Some(if total < MAX_ID_LEN {
                    total
                } else {
                    MAX_ID_LEN
                })
            }
        }
    }

    /// Whether every id of the Route fits 100 characters (CID-01).
    pub const fn fits(&self) -> bool {
        match self.body_kind {
            BodyKind::Inline => matches!(self.total(), Some(total) if total <= MAX_ID_LEN),
            BodyKind::ByRef | BodyKind::Spill => self.prefix() + REF_BODY_LEN <= MAX_ID_LEN,
        }
    }
}

impl fmt::Display for RouteWidth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: F {FMT_LEN} + route {} + sep {SEP_LEN} + S {} + N {} + ",
            self.route,
            self.route.as_str().len(),
            self.schema,
            self.nonce,
        )?;
        let body = Body(self.body);
        match self.body_kind {
            BodyKind::Inline => write!(f, "body {body} + T {}", self.tag)?,
            BodyKind::ByRef => write!(f, "ref {REF_BODY_LEN}")?,
            BodyKind::Spill => write!(
                f,
                "body {body} + T {} inline, else ref {REF_BODY_LEN}",
                self.tag
            )?,
        }
        match self.total() {
            Some(total) => write!(f, " = {total} (limit {MAX_ID_LEN})"),
            None => write!(f, " = unbounded (limit {MAX_ID_LEN})"),
        }
    }
}

/// A body width, printed as its number or `unbounded`.
struct Body(Option<usize>);

impl fmt::Display for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(n) => write!(f, "{n}"),
            None => f.write_str("unbounded"),
        }
    }
}

/// Capacity of the panic message buffer of [`assert_fits`].
const MSG_CAP: usize = 512;

/// A fixed byte buffer that const code appends text and numbers to.
struct Msg {
    buf: [u8; MSG_CAP],
    len: usize,
}

impl Msg {
    const fn new() -> Self {
        Self {
            buf: [0; MSG_CAP],
            len: 0,
        }
    }

    const fn push(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() && self.len < MSG_CAP {
            self.buf[self.len] = bytes[i];
            self.len += 1;
            i += 1;
        }
    }

    const fn push_usize(&mut self, mut n: usize) {
        let mut digits = [0u8; 20];
        let mut start = digits.len();
        loop {
            start -= 1;
            // `n % 10 < 10` always fits `u8`.
            #[allow(clippy::cast_possible_truncation, reason = "n % 10 < 10")]
            let d = (n % 10) as u8;
            digits[start] = b'0' + d;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        while start < digits.len() && self.len < MSG_CAP {
            self.buf[self.len] = digits[start];
            self.len += 1;
            start += 1;
        }
    }

    const fn as_str(&self) -> &str {
        let (used, _) = self.buf.split_at(self.len);
        match core::str::from_utf8(used) {
            Ok(text) => text,
            // Only ASCII and whole route ids are pushed, and the buffer is
            // large enough for both messages.
            Err(_) => "[CID-01] route can exceed 100 characters",
        }
    }
}

/// Fails const evaluation when Route `R` can produce an id longer than 100
/// characters (CID-01); the kind derives emit
/// `const _: () = assert_fits::<R>();`.
///
/// # Panics
///
/// When `route_width::<R>().fits()` is false, with a message that names the
/// route id and the worst case.
#[doc(hidden)]
pub const fn assert_fits<R: Route>() {
    let width = route_width::<R>();
    if width.fits() {
        return;
    }
    let mut msg = Msg::new();
    msg.push("[CID-01] route `");
    msg.push(width.route.as_str());
    match width.total() {
        None => {
            msg.push("` has unbounded fields\n  = help: bound them (`BoundedStr<N>`), or add `by_ref` or `overflow = spill` to the kind attribute");
        }
        Some(total) => {
            msg.push("` can exceed 100 characters: worst case ");
            msg.push_usize(total);
            msg.push("\n  = help: shorten the route id or the fields (`PayloadEnum`, smaller integers), or add `by_ref` or `overflow = spill`; run the generated noctis_route_width test for the breakdown");
        }
    }
    panic!("{}", msg.as_str());
}

/// Writes `PayloadEnum` variant `index` as one base62 digit.
///
/// # Errors
///
/// `fmt::Error` when `index >= 62`, or the output's error.
#[doc(hidden)]
pub fn encode_variant<W: fmt::Write + ?Sized>(index: u8, out: &mut W) -> fmt::Result {
    let Some(&b) = ALPHABET.get(usize::from(index)) else {
        return Err(fmt::Error);
    };
    out.write_char(char::from(b))
}

/// Decodes a `PayloadEnum` variant index of an enum with `count` variants.
///
/// # Errors
///
/// In order: [`FieldError::Empty`], [`FieldError::TooLong`] for more than one
/// byte, [`FieldError::InvalidChar`] for a non-base62 byte, and
/// [`FieldError::Overflow`] for an index `>= count`.
#[doc(hidden)]
pub fn decode_variant(raw: &str, count: u8) -> Result<u8, FieldError> {
    let &[b] = raw.as_bytes() else {
        return Err(if raw.is_empty() {
            FieldError::Empty
        } else {
            FieldError::TooLong {
                max: 1,
                actual: raw.len(),
            }
        });
    };
    let Some(index) = digit_value(b) else {
        return Err(FieldError::InvalidChar { index: 0 });
    };
    if index < count {
        Ok(index)
    } else {
        Err(FieldError::Overflow)
    }
}

/// The runtime arguments a Route declares (TTL, one-shot, owner,
/// permissions, guard, modal). Empty in this release: `#[route]` accepts
/// codec arguments only, and later releases add fields.
#[doc(hidden)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRuntime {}

impl RouteRuntime {
    /// No runtime argument.
    pub const EMPTY: Self = Self {};
}

/// The [`RouteRuntime`] a derived Route declares.
#[doc(hidden)]
pub trait DeclaredRuntime {
    /// The declaration.
    const RUNTIME: RouteRuntime;
}

#[cfg(test)]
mod tests {
    use core::fmt;

    use super::{assert_fits, decode_variant, encode_variant, route_width, sum_fields};
    use crate::{
        BodyKind, BoundedStr, CodecError, Field, FieldError, FieldReader, FieldWriter, Payload,
        PayloadDecode, Route, RouteId, RouteShape,
    };

    /// A Payload with a fixed `MAX_ENCODED_LEN`.
    struct Width<const W: usize>;
    /// A Payload with an unbounded body.
    struct Unbounded;

    impl<const W: usize> Payload for Width<W> {
        const MAX_ENCODED_LEN: Option<usize> = Some(W);
        fn encode_fields(&self, _out: &mut FieldWriter<'_>) -> fmt::Result {
            Ok(())
        }
    }

    impl<const W: usize> PayloadDecode<'_> for Width<W> {
        fn decode_fields(_f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
            Ok(Self)
        }
    }

    impl Payload for Unbounded {
        const MAX_ENCODED_LEN: Option<usize> = None;
        fn encode_fields(&self, _out: &mut FieldWriter<'_>) -> fmt::Result {
            Ok(())
        }
    }

    impl PayloadDecode<'_> for Unbounded {
        fn decode_fields(_f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
            Ok(Self)
        }
    }

    macro_rules! route {
        ($name:ident, $id:literal, $shape:expr, $schema:literal, $payload:ty) => {
            struct $name;
            impl Route for $name {
                const ID: RouteId = RouteId::new($id);
                const SHAPE: RouteShape = $shape;
                const SCHEMA: u8 = $schema;
                type Payload = $payload;
            }
        };
    }

    route!(Buy, "shop.buy", RouteShape::INLINE, 0, Width<14>);
    route!(Ping, "ping", RouteShape::INLINE, 0, Width<0>);
    route!(Select, "a", RouteShape::INLINE.select(), 0, Width<0>);
    route!(Signed, "a", RouteShape::INLINE.signed(), 0, Width<0>);
    route!(Schema, "a", RouteShape::INLINE, 3, Width<0>);
    route!(Wizard, "wizard.step", RouteShape::BY_REF, 0, Unbounded);
    route!(
        Long,
        "abcdefghijklmnopqrstuvwx",
        RouteShape::BY_REF.select(),
        61,
        Unbounded
    );
    route!(SpillS, "s", RouteShape::SPILL, 0, Unbounded);
    route!(SpillSigned, "ss", RouteShape::SPILL.signed(), 0, Width<2>);
    route!(Exact, "a", RouteShape::INLINE, 0, Width<97>);
    route!(
        ExactAll,
        "r",
        RouteShape::INLINE.select().signed(),
        61,
        Width<88>
    );
    route!(Over, "a", RouteShape::INLINE, 0, Width<98>);
    route!(Report, "report.reason", RouteShape::INLINE, 0, Unbounded);

    #[test]
    fn sum_fields_matrix() {
        assert_eq!(sum_fields(&[]), Some(0));
        assert_eq!(sum_fields(&[Some(1)]), Some(1));
        assert_eq!(sum_fields(&[Some(11), Some(2)]), Some(14));
        assert_eq!(sum_fields(&[Some(1); 3]), Some(5));
        assert_eq!(sum_fields(&[Some(1), None]), None);
        assert_eq!(sum_fields(&[None]), None);
        assert_eq!(sum_fields(&[Some(usize::MAX), Some(1)]), Some(usize::MAX));
        // The widths of `{t: BoundedStr<47>, n: u8}`.
        assert_eq!(
            sum_fields(&[<BoundedStr<47> as Field>::MAX_LEN, <u8 as Field>::MAX_LEN]),
            Some(97)
        );
    }

    #[test]
    fn cid_01_route_width_matrix() {
        let w = route_width::<Buy>();
        assert_eq!(w.route().as_str(), "shop.buy");
        assert_eq!(w.body_kind(), BodyKind::Inline);
        assert_eq!(w.prefix(), 10);
        assert_eq!((w.schema(), w.nonce(), w.tag()), (0, 0, 0));
        assert_eq!(w.body(), Some(14));
        assert_eq!(w.total(), Some(24));
        assert!(w.fits());
        assert_eq!(
            w.to_string(),
            "shop.buy: F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)"
        );

        assert_eq!(route_width::<Ping>().total(), Some(6));
        let base = 3;
        assert_eq!(route_width::<Select>().total(), Some(base + 2));
        assert_eq!(route_width::<Select>().nonce(), 2);
        assert_eq!(route_width::<Signed>().total(), Some(base + 6));
        assert_eq!(route_width::<Signed>().tag(), 6);
        assert_eq!(route_width::<Schema>().total(), Some(base + 1));
        assert_eq!(route_width::<Schema>().schema(), 1);

        let w = route_width::<Wizard>();
        assert_eq!(w.body_kind(), BodyKind::ByRef);
        assert_eq!(w.body(), None);
        assert_eq!(w.total(), Some(25));
        assert!(w.fits());
        assert_eq!(
            w.to_string(),
            "wizard.step: F 1 + route 11 + sep 1 + S 0 + N 0 + ref 12 = 25 (limit 100)"
        );

        assert_eq!(route_width::<Long>().total(), Some(41));
        assert_eq!(route_width::<Long>().total(), Some(crate::MAX_REF_ID_LEN));
        assert!(route_width::<Long>().fits());

        let w = route_width::<SpillS>();
        assert_eq!(w.total(), Some(100));
        assert!(w.fits());
        assert_eq!(
            w.to_string(),
            "s: F 1 + route 1 + sep 1 + S 0 + N 0 + body unbounded + T 0 inline, else ref 12 = 100 (limit 100)"
        );
        assert_eq!(route_width::<SpillSigned>().total(), Some(16));

        assert_eq!(route_width::<Exact>().total(), Some(100));
        assert!(route_width::<Exact>().fits());
        assert_eq!(route_width::<ExactAll>().total(), Some(100));
        assert!(route_width::<ExactAll>().fits());

        assert_eq!(route_width::<Over>().total(), Some(101));
        assert!(!route_width::<Over>().fits());
        let w = route_width::<Report>();
        assert_eq!(w.total(), None);
        assert!(!w.fits());
        assert_eq!(
            w.to_string(),
            "report.reason: F 1 + route 13 + sep 1 + S 0 + N 0 + body unbounded + T 0 = unbounded (limit 100)"
        );
    }

    #[test]
    fn cid_01_assert_fits_passes() {
        assert_fits::<Buy>();
        assert_fits::<Exact>();
        assert_fits::<ExactAll>();
        assert_fits::<Wizard>();
        assert_fits::<SpillS>();
    }

    #[test]
    #[should_panic(
        expected = "[CID-01] route `a` can exceed 100 characters: worst case 101\n  = help: shorten the route id or the fields (`PayloadEnum`, smaller integers), or add `by_ref` or `overflow = spill`; run the generated noctis_route_width test for the breakdown"
    )]
    fn cid_01_assert_fits_panics() {
        assert_fits::<Over>();
    }

    #[test]
    #[should_panic(
        expected = "[CID-01] route `report.reason` has unbounded fields\n  = help: bound them (`BoundedStr<N>`), or add `by_ref` or `overflow = spill` to the kind attribute"
    )]
    fn cid_01_assert_fits_panics_unbounded() {
        assert_fits::<Report>();
    }

    #[test]
    fn variant_digits() {
        let mut s = String::new();
        encode_variant(0, &mut s).unwrap();
        encode_variant(10, &mut s).unwrap();
        encode_variant(61, &mut s).unwrap();
        assert_eq!(s, "0Az");
        assert_eq!(encode_variant(62, &mut s), Err(fmt::Error));
        assert_eq!(encode_variant(u8::MAX, &mut s), Err(fmt::Error));

        assert_eq!(decode_variant("2", 3), Ok(2));
        assert_eq!(decode_variant("3", 3), Err(FieldError::Overflow));
        assert_eq!(decode_variant("", 3), Err(FieldError::Empty));
        assert_eq!(
            decode_variant("00", 3),
            Err(FieldError::TooLong { max: 1, actual: 2 })
        );
        assert_eq!(
            decode_variant("-", 3),
            Err(FieldError::InvalidChar { index: 0 })
        );
        assert_eq!(
            decode_variant("é", 3),
            Err(FieldError::TooLong { max: 1, actual: 2 })
        );
        assert_eq!(decode_variant("z", 3), Err(FieldError::Overflow));
        assert_eq!(decode_variant("z", 62), Ok(61));
        assert_eq!(decode_variant("1", 1), Err(FieldError::Overflow));
        assert_eq!(decode_variant("0", 1), Ok(0));
    }
}
