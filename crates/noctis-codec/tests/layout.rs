//! The `custom_id` layout: encoder matrix, Route-checked decode outcomes, and
//! property tests that decoding is total and round-trips (CID-06, FR-16,
//! FR-19, FR-60).

use core::cell::RefCell;
use core::fmt;

use noctis_codec::{
    BodyKind, CodecError, DecodeError, EncodeError, Field, FieldError, FieldReader, FieldWriter,
    Flags, ForgedReason, Parts, Payload, PayloadDecode, PayloadSource, RawBody, Route, RouteId,
    RouteShape, SelectNonce, Sign, StaleReason, Tag, ViewEnvelope, decode, decode_route, encode,
};
use proptest::prelude::*;

/// Hand-implements `Payload` and `PayloadDecode` for a struct of owned fields.
macro_rules! payload {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct $name {
            $($field: $ty,)*
        }

        impl Payload for $name {
            #[allow(unused_variables, reason = "zero-field payloads")]
            fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
                $(out.field(&self.$field)?;)*
                Ok(())
            }
        }

        impl<'de> PayloadDecode<'de> for $name {
            #[allow(unused_variables, reason = "zero-field payloads")]
            fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError> {
                Ok(Self { $($field: f.field()?,)* })
            }
        }
    };
}

/// Declares a marker Route.
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

payload!(BuyV1 { item: u64, qty: u8 });
payload!(BuyV2Payload { item: u64, qty: u8, note: Option<u8>, gift: Option<bool> });
payload!(TextP { s: String });
payload!(PingP {});
payload!(OptP { v: Option<u8> });
payload!(NumP { n: u8 });
payload!(MixP { a: u8, b: i64, c: Option<u64>, d: String });

route!(Buy, "shop.buy", RouteShape::INLINE, 0, BuyV1);
route!(BuyV2, "shop.buy", RouteShape::INLINE, 0, BuyV2Payload);
route!(BuyS2, "shop.buy", RouteShape::INLINE, 2, BuyV1);
route!(Text, "t", RouteShape::INLINE, 0, TextP);
route!(TextSpill, "t", RouteShape::SPILL, 0, TextP);
route!(TextSigned, "t", RouteShape::INLINE.signed(), 0, TextP);
route!(Ping, "ping", RouteShape::INLINE, 0, PingP);
route!(Opt, "o", RouteShape::INLINE, 0, OptP);
route!(View, "v", RouteShape::INLINE.view(), 0, NumP);
route!(Sel, "s", RouteShape::INLINE.select(), 0, NumP);
route!(Sig, "sig", RouteShape::INLINE.signed(), 0, NumP);
route!(
    All,
    "all",
    RouteShape::INLINE.view().select().signed(),
    61,
    NumP
);
route!(Stored, "r", RouteShape::BY_REF, 0, BuyV1);
route!(SpillSigned, "ss", RouteShape::SPILL.signed(), 0, TextP);
route!(BadSchema, "x", RouteShape::INLINE, 62, NumP);

/// Returns `Tag::from_u64(n)` and records what it was asked to sign.
struct FakeSigner(u64, RefCell<String>);

impl FakeSigner {
    fn new(n: u64) -> Self {
        Self(n, RefCell::new(String::new()))
    }

    fn saw(&self) -> String {
        self.1.borrow().clone()
    }
}

impl Sign for FakeSigner {
    fn sign(&self, signed: &str) -> Tag {
        signed.clone_into(&mut self.1.borrow_mut());
        Tag::from_u64(self.0)
    }
}

fn enc<R: Route>(p: &R::Payload, parts: &Parts<'_>) -> Result<String, EncodeError> {
    encode::<R>(p, parts).map(|id| id.as_str().to_owned())
}

fn plain<R: Route>(p: &R::Payload) -> Result<String, EncodeError> {
    enc::<R>(p, &Parts::new())
}

fn text(s: &str) -> TextP {
    TextP { s: s.to_owned() }
}

fn x(n: usize) -> TextP {
    text(&"x".repeat(n))
}

fn buy(item: u64, qty: u8) -> BuyV1 {
    BuyV1 { item, qty }
}

#[test]
fn encode_matrix() {
    assert_eq!(plain::<Buy>(&buy(255, 3)).unwrap(), "0shop.buy:47,3");
    assert_eq!(plain::<BuyS2>(&buy(255, 3)).unwrap(), "1shop.buy:247,3");

    let env = ViewEnvelope::new('t', 1, 63, 2).unwrap();
    let parts = Parts::new().with_envelope(env);
    assert_eq!(enc::<View>(&NumP { n: 1 }, &parts).unwrap(), "2v:t11121");

    let parts = Parts::new().with_nonce(SelectNonce::new(3843).unwrap());
    assert_eq!(enc::<Sel>(&NumP { n: 1 }, &parts).unwrap(), "4s:zz1");

    let signer = FakeSigner::new(0);
    let parts = Parts::new().with_signer(&signer);
    assert_eq!(enc::<Sig>(&NumP { n: 1 }, &parts).unwrap(), "8sig:1000000");
    assert_eq!(signer.saw(), "8sig:1");

    let signer = FakeSigner::new(1);
    let parts = Parts::new()
        .with_envelope(ViewEnvelope::new('_', 61, 3843, 61).unwrap())
        .with_nonce(SelectNonce::new(0).unwrap())
        .with_signer(&signer);
    assert_eq!(
        enc::<All>(&NumP { n: 0 }, &parts).unwrap(),
        "Fall:z_zzzz000000001"
    );
    assert_eq!(signer.saw(), "Fall:z_zzzz000");

    assert_eq!(plain::<Ping>(&PingP {}).unwrap(), "0ping:");
    assert_eq!(plain::<Text>(&text("")).unwrap(), "0t:");
    assert_eq!(plain::<Text>(&text("a,b")).unwrap(), "0t:a!1b");
    assert_eq!(plain::<Text>(&text("é")).unwrap(), "0t:!u0000e9");

    assert_eq!(plain::<Text>(&x(97)).unwrap().len(), 100);
    assert_eq!(plain::<Text>(&x(98)), Err(EncodeError::TooLong));
    assert_eq!(
        plain::<TextSpill>(&x(97)).unwrap(),
        format!("0t:{}", "x".repeat(97))
    );
    assert_eq!(plain::<TextSpill>(&x(98)), Err(EncodeError::NeedsRef));

    let signer = FakeSigner::new(0);
    let parts = Parts::new().with_signer(&signer);
    assert_eq!(enc::<TextSigned>(&x(91), &parts).unwrap().len(), 100);
    assert_eq!(enc::<TextSigned>(&x(92), &parts), Err(EncodeError::TooLong));
    assert_eq!(enc::<SpillSigned>(&x(90), &parts).unwrap().len(), 100);
    assert_eq!(
        enc::<SpillSigned>(&x(91), &parts),
        Err(EncodeError::NeedsRef)
    );

    assert_eq!(plain::<Stored>(&buy(1, 1)), Err(EncodeError::NeedsRef));
    assert_eq!(
        plain::<BadSchema>(&NumP { n: 1 }),
        Err(EncodeError::InvalidSchema { schema: 62 })
    );
}

/// A Payload that ignores the writer's error and reports success.
#[derive(Debug)]
struct Swallow(String);

impl Payload for Swallow {
    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
        let _ = out.field(&self.0);
        Ok(())
    }
}

impl PayloadDecode<'_> for Swallow {
    fn decode_fields(f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
        Ok(Self(f.field()?))
    }
}

route!(SwallowInline, "w", RouteShape::INLINE, 0, Swallow);
route!(SwallowSpill, "w", RouteShape::SPILL, 0, Swallow);

#[test]
fn encode_swallowed_overflow() {
    let p = Swallow("x".repeat(98));
    assert_eq!(plain::<SwallowInline>(&p), Err(EncodeError::TooLong));
    assert_eq!(plain::<SwallowSpill>(&p), Err(EncodeError::NeedsRef));
}

/// A field whose own `encode` fails without the id overflowing.
#[derive(Debug)]
struct Failing;

impl Field for Failing {
    const MAX_LEN: Option<usize> = Some(0);

    fn encode<W: fmt::Write + ?Sized>(&self, _out: &mut W) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[derive(Debug)]
struct FailP;

impl Payload for FailP {
    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
        out.field(&Failing)
    }
}

impl PayloadDecode<'_> for FailP {
    fn decode_fields(_f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
        Ok(Self)
    }
}

route!(FailInline, "f", RouteShape::INLINE, 0, FailP);
route!(FailSpill, "f", RouteShape::SPILL, 0, FailP);

#[test]
fn encode_field_error() {
    assert_eq!(plain::<FailInline>(&FailP), Err(EncodeError::Field));
    assert_eq!(plain::<FailSpill>(&FailP), Err(EncodeError::Field));
}

#[test]
fn encode_parts_mismatch() {
    let one = NumP { n: 1 };
    let env = Parts::new().with_envelope(ViewEnvelope::new('a', 0, 0, 0).unwrap());
    let nonce = Parts::new().with_nonce(SelectNonce::new(1).unwrap());
    let signer = FakeSigner::new(0);
    let with_signer = Parts::new().with_signer(&signer);
    let b = buy(1, 1);

    assert_eq!(plain::<View>(&one), Err(EncodeError::MissingEnvelope));
    assert_eq!(enc::<Buy>(&b, &env), Err(EncodeError::UnexpectedEnvelope));
    assert_eq!(plain::<Sel>(&one), Err(EncodeError::MissingNonce));
    assert_eq!(enc::<Buy>(&b, &nonce), Err(EncodeError::UnexpectedNonce));
    assert_eq!(plain::<Sig>(&one), Err(EncodeError::MissingSigner));
    assert_eq!(
        enc::<Buy>(&b, &with_signer),
        Err(EncodeError::UnexpectedSigner)
    );
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "compared with `decode_route` results"
)]
fn inline<P>(p: P) -> Result<PayloadSource<'static, P>, DecodeError> {
    Ok(PayloadSource::Inline(p))
}

const fn malformed(e: CodecError) -> DecodeError {
    DecodeError::Malformed(e)
}

const fn stale(r: StaleReason) -> DecodeError {
    DecodeError::Stale(r)
}

const fn field(field: usize, error: FieldError) -> DecodeError {
    DecodeError::Malformed(CodecError::Field { field, error })
}

fn v2(note: Option<u8>, gift: Option<bool>) -> BuyV2Payload {
    BuyV2Payload {
        item: 255,
        qty: 3,
        note,
        gift,
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "one row per matrix line")]
fn cid_06_payload_outcomes() {
    const REF: &str = "0123456789A";
    let flags = |expected, found| stale(StaleReason::Flags { expected, found });

    assert_eq!(decode_route::<Buy>("0shop.buy:47,3"), inline(buy(255, 3)));
    assert_eq!(
        decode_route::<BuyV2>("0shop.buy:47,3"),
        inline(v2(None, None))
    );
    assert_eq!(
        decode_route::<BuyV2>("0shop.buy:47,3,15"),
        inline(v2(Some(5), None))
    );
    assert_eq!(
        decode_route::<BuyV2>("0shop.buy:47,3,15,11"),
        inline(v2(Some(5), Some(true)))
    );
    assert_eq!(
        decode_route::<Buy>("0shop.buy:47,3,15"),
        Err(malformed(CodecError::ExtraField { field: 2 }))
    );
    assert_eq!(
        decode_route::<Buy>("0shop.buy:47"),
        Err(malformed(CodecError::MissingField { field: 1 }))
    );
    assert_eq!(
        decode_route::<BuyV2>("0shop.buy:47,3,"),
        Err(field(2, FieldError::Empty))
    );
    assert_eq!(
        decode_route::<Buy>("0shop.buy:"),
        Err(field(0, FieldError::Empty))
    );
    assert_eq!(
        decode_route::<Buy>("0shop.buy:47,-"),
        Err(field(1, FieldError::InvalidChar { index: 0 }))
    );

    assert_eq!(decode_route::<Opt>("0o:"), inline(OptP { v: None }));
    assert_eq!(decode_route::<Opt>("0o:0"), inline(OptP { v: None }));
    assert_eq!(decode_route::<Opt>("0o:15"), inline(OptP { v: Some(5) }));
    assert_eq!(decode_route::<Text>("0t:"), inline(text("")));
    assert_eq!(decode_route::<Ping>("0ping:"), inline(PingP {}));
    assert_eq!(
        decode_route::<Ping>("0ping:x"),
        Err(malformed(CodecError::ExtraField { field: 0 }))
    );

    let forged = Err(DecodeError::Forged(ForgedReason::RouteMismatch));
    assert_eq!(decode_route::<Buy>("0shop.buz:47,3"), forged);
    assert_eq!(decode_route::<Buy>("1x:247,3"), forged);

    assert_eq!(
        decode_route::<BuyS2>("0shop.buy:47,3"),
        Err(stale(StaleReason::Schema {
            expected: 2,
            found: 0
        }))
    );
    let schema_2 = Err(stale(StaleReason::Schema {
        expected: 0,
        found: 2,
    }));
    assert_eq!(decode_route::<Buy>("1shop.buy:247,3"), schema_2);
    assert_eq!(decode_route::<Buy>("3shop.buy:2_000047,3"), schema_2);

    assert_eq!(
        decode_route::<Buy>("2shop.buy:_000047,3"),
        Err(flags(Flags::EMPTY, Flags::E))
    );
    assert_eq!(
        decode_route::<View>("0v:1"),
        Err(flags(Flags::E, Flags::EMPTY))
    );
    assert_eq!(
        decode_route::<Sig>("0sig:1"),
        Err(flags(Flags::T, Flags::EMPTY))
    );
    assert_eq!(
        decode_route::<Sel>("0s:1"),
        Err(flags(Flags::N, Flags::EMPTY))
    );
    assert_eq!(
        decode_route::<Buy>("4shop.buy:0047,3"),
        Err(flags(Flags::EMPTY, Flags::N))
    );

    assert_eq!(
        decode_route::<Buy>("0shop.buy:~0123456789A"),
        Err(stale(StaleReason::Body {
            expected: BodyKind::Inline
        }))
    );
    assert_eq!(
        decode_route::<Stored>("0r:47,3"),
        Err(stale(StaleReason::Body {
            expected: BodyKind::ByRef
        }))
    );
    assert_eq!(
        decode_route::<Stored>("0r:~0123456789A"),
        Ok(PayloadSource::Ref(REF))
    );
    assert_eq!(
        decode_route::<TextSpill>("0t:~0123456789A"),
        Ok(PayloadSource::Ref(REF))
    );
    assert_eq!(decode_route::<TextSpill>("0t:ab"), inline(text("ab")));

    assert_eq!(
        decode_route::<SpillSigned>("8ss:a000000"),
        inline(text("a"))
    );
    assert_eq!(
        decode_route::<SpillSigned>("0ss:~0123456789A"),
        Ok(PayloadSource::Ref(REF))
    );
    assert_eq!(
        decode_route::<SpillSigned>("0ss:a"),
        Err(flags(Flags::T, Flags::EMPTY))
    );
    assert_eq!(decode_route::<Sig>("8sig:1000000"), inline(NumP { n: 1 }));

    assert_eq!(decode_route::<Buy>(""), Err(malformed(CodecError::Empty)));
    assert_eq!(
        decode_route::<Text>("0t:a:b"),
        Err(malformed(CodecError::InvalidChar { index: 4 }))
    );
}

#[test]
fn trailing_option_compat() {
    let old = plain::<Buy>(&buy(255, 3)).unwrap();
    assert_eq!(decode_route::<BuyV2>(&old), inline(v2(None, None)));
}

/// Rebuilds the input from a [`noctis_codec::Decoded`]'s getters.
fn rebuild(d: &noctis_codec::Decoded<'_>) -> String {
    let mut out = String::new();
    let digits = b"0123456789ABCDEF";
    out.push(char::from(digits[usize::from(d.flags().bits())]));
    out.push_str(d.route());
    out.push(':');
    if d.flags().contains(Flags::S) {
        let alphabet = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        out.push(char::from(alphabet[usize::from(d.schema())]));
    }
    if let Some(e) = d.envelope() {
        out.push_str(&e.to_string());
    }
    if let Some(n) = d.nonce() {
        out.push_str(&n.to_string());
    }
    match d.body() {
        RawBody::Inline(b) => out.push_str(b),
        RawBody::Ref(r) => {
            out.push('~');
            out.push_str(r);
        }
    }
    if let Some(t) = d.tag() {
        out.push_str(t.as_str());
    }
    out
}

/// Encodes then decodes `p`; `None` when either step fails or yields a Ref.
fn rt<R>(p: &R::Payload, parts: &Parts<'_>) -> Option<R::Payload>
where
    R: Route,
    R::Payload: for<'de> PayloadDecode<'de>,
{
    let id = encode::<R>(p, parts).ok()?;
    match decode_route::<R>(&id) {
        Ok(PayloadSource::Inline(v)) => Some(v),
        _ => None,
    }
}

fn decode_total(raw: &str) {
    match decode(raw) {
        Ok(d) => {
            assert!(raw.len() <= 100);
            assert!(raw.is_ascii());
            assert_eq!(d.as_str(), raw);
            assert_eq!(rebuild(&d), raw);
            if let Some(signed) = d.signed_part() {
                assert_eq!(raw.strip_prefix(signed).map(str::len), Some(6));
            }
            let _ = d.payload::<Buy>();
            let _ = d.payload::<BuyV2>();
            let _ = d.payload::<Text>();
            let _ = d.payload::<TextSpill>();
            let _ = d.payload::<Ping>();
            let _ = d.payload::<Opt>();
            let _ = d.payload::<All>();
            let _ = d.payload::<Stored>();
            let _ = d.payload::<SpillSigned>();
        }
        Err(e) => {
            let _ = e.to_string();
        }
    }
    let _ = decode_route::<Buy>(raw);
}

/// Encodes `p` for `R` with every section its shape needs, then decodes it.
fn round_trip<R>(p: &MixP, env: ViewEnvelope, nonce: SelectNonce)
where
    R: Route<Payload = MixP>,
{
    let signer = FakeSigner::new(7);
    let mut parts = Parts::new();
    if R::SHAPE.has_view() {
        parts = parts.with_envelope(env);
    }
    if R::SHAPE.has_select() {
        parts = parts.with_nonce(nonce);
    }
    if R::SHAPE.is_signed() {
        parts = parts.with_signer(&signer);
    }
    let shape = R::SHAPE;
    let expected_len = "0mix.route:".len()
        + usize::from(R::SCHEMA > 0)
        + if shape.has_view() { 5 } else { 0 }
        + if shape.has_select() { 2 } else { 0 }
        + body_len(p)
        + if shape.is_signed() { 6 } else { 0 };
    match encode::<R>(p, &parts) {
        Ok(id) => {
            assert_eq!(id.len(), expected_len);
            assert!(id.len() <= 100);
            assert_eq!(decode_route::<R>(&id), inline(p.clone()));
            let Ok(d) = decode(&id) else {
                panic!("{id} did not decode")
            };
            assert_eq!(d.envelope(), shape.has_view().then_some(env));
            assert_eq!(d.nonce(), shape.has_select().then_some(nonce));
            if shape.is_signed() {
                assert_eq!(d.signed_part(), Some(signer.saw().as_str()));
            }
        }
        Err(e) => {
            assert_eq!(e, EncodeError::TooLong);
            assert!(expected_len > 100, "{p:?} fits in {expected_len}");
        }
    }
}

/// The inline body length of a `MixP`: its fields plus 3 separators.
fn body_len(p: &MixP) -> usize {
    let mut out = String::new();
    // Writing into a `String` cannot fail for the built-in fields.
    let _ = p.a.encode(&mut out);
    let _ = p.b.encode(&mut out);
    let _ = p.c.encode(&mut out);
    let _ = p.d.encode(&mut out);
    out.len() + 3
}

macro_rules! mix_routes {
    ($($name:ident: $shape:expr, $schema:literal;)*) => {
        $(route!($name, "mix.route", $shape, $schema, MixP);)*

        fn round_trip_all(p: &MixP, env: ViewEnvelope, nonce: SelectNonce) {
            $(round_trip::<$name>(p, env, nonce);)*
        }
    };
}

mix_routes! {
    M0: RouteShape::INLINE, 0;
    M1: RouteShape::INLINE.view(), 0;
    M2: RouteShape::INLINE.select(), 0;
    M3: RouteShape::INLINE.view().select(), 0;
    M4: RouteShape::INLINE.signed(), 0;
    M5: RouteShape::INLINE.view().signed(), 0;
    M6: RouteShape::INLINE.select().signed(), 0;
    M7: RouteShape::INLINE.view().select().signed(), 0;
    M8: RouteShape::INLINE, 61;
    M9: RouteShape::INLINE.view(), 61;
    M10: RouteShape::INLINE.select(), 61;
    M11: RouteShape::INLINE.view().select(), 61;
    M12: RouteShape::INLINE.signed(), 61;
    M13: RouteShape::INLINE.view().signed(), 61;
    M14: RouteShape::INLINE.select().signed(), 61;
    M15: RouteShape::INLINE.view().select().signed(), 61;
}

fn envelope() -> impl Strategy<Value = ViewEnvelope> {
    (
        prop_oneof![
            Just('_'),
            proptest::char::range('0', '9'),
            proptest::char::range('A', 'Z'),
            proptest::char::range('a', 'z')
        ],
        0..=61u8,
        0..=3843u16,
        0..=61u8,
    )
        .prop_filter_map("valid envelope", |(a, t, p, v)| {
            ViewEnvelope::new(a, t, p, v).ok()
        })
}

fn mix() -> impl Strategy<Value = MixP> {
    (
        any::<u8>(),
        any::<i64>(),
        any::<Option<u64>>(),
        prop_oneof![".{0,30}", ".{0,90}"],
    )
        .prop_map(|(a, b, c, d)| MixP { a, b, c, d })
}

proptest! {
    #[test]
    fn cid_06_decode_never_panics(raw in ".*") {
        decode_total(&raw);
    }

    #[test]
    fn cid_06_decode_never_panics_printable(raw in "\\PC{0,130}") {
        decode_total(&raw);
    }

    #[test]
    fn cid_06_decode_never_panics_near_valid(raw in "[0-9A-Za-z][a-z0-9._:~!,_ -]{0,110}") {
        decode_total(&raw);
    }

    #[test]
    fn encode_decode_round_trip(
        p in mix(),
        env in envelope(),
        nonce in (0..=3843u16).prop_filter_map("nonce", SelectNonce::new),
    ) {
        round_trip_all(&p, env, nonce);
    }

    #[test]
    fn fixtures_round_trip(item in any::<u64>(), qty in any::<u8>(), note in any::<Option<u8>>(), s in ".{0,8}") {
        let b = buy(item, qty);
        prop_assert_eq!(rt::<Buy>(&b, &Parts::new()), Some(b.clone()));
        prop_assert_eq!(rt::<BuyS2>(&b, &Parts::new()), Some(b.clone()));
        let o = OptP { v: note };
        prop_assert_eq!(rt::<Opt>(&o, &Parts::new()), Some(o));
        let t = text(&s);
        prop_assert_eq!(rt::<Text>(&t, &Parts::new()), Some(t.clone()));
        prop_assert_eq!(rt::<TextSpill>(&t, &Parts::new()), Some(t.clone()));
        let signer = FakeSigner::new(item);
        let parts = Parts::new().with_signer(&signer);
        prop_assert_eq!(rt::<TextSigned>(&t, &parts), Some(t.clone()));
        prop_assert_eq!(rt::<SpillSigned>(&t, &parts), Some(t));
        let n = NumP { n: qty };
        prop_assert_eq!(rt::<Sig>(&n, &parts), Some(n.clone()));
        let env = ViewEnvelope::new('_', 61, u16::from(qty), 0).unwrap();
        let view = Parts::new().with_envelope(env);
        prop_assert_eq!(rt::<View>(&n, &view), Some(n.clone()));
        let nonce = SelectNonce::new(u16::from(qty)).unwrap();
        let sel = Parts::new().with_nonce(nonce);
        prop_assert_eq!(rt::<Sel>(&n, &sel), Some(n.clone()));
        let all = Parts::new().with_envelope(env).with_nonce(nonce).with_signer(&signer);
        prop_assert_eq!(rt::<All>(&n, &all), Some(n));
        let v2 = BuyV2Payload { item, qty, note, gift: note.map(|v| v % 2 == 0) };
        prop_assert_eq!(rt::<BuyV2>(&v2, &Parts::new()), Some(v2));
        prop_assert_eq!(rt::<Ping>(&PingP {}, &Parts::new()), Some(PingP {}));
    }
}
