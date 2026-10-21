//! The Route half of the component derives, `#[derive(Payload)]` and
//! `#[derive(PayloadEnum)]`:
//! generated impls, widths and round trips (CID-01, CID-03, FR-16, AD-6).

use noctis::__private::codec::{
    self, BodyKind, BoundedStr, EncodedId, Field, FieldDecode, FieldError, Parts, PayloadSource,
    RouteId, RouteShape, check, decode_route, encode, encode_id, route_width,
};
use noctis::prelude::{Payload, PayloadEnum, Route};
use proptest::prelude::*;
use twilight_model::id::Id;

struct ItemMarker;

#[derive(::noctis::Button, Debug, PartialEq)]
#[button("shop.buy", defaults(style = Secondary, label = "Go"))]
struct BuyButton {
    item: Id<ItemMarker>,
    qty: u8,
}

impl BuyButton {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button, Debug, PartialEq)]
#[button("ping", defaults(style = Secondary, label = "Go"))]
struct Ping;

impl Ping {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button, Debug, PartialEq)]
#[button("t", defaults(style = Secondary, label = "Go"))]
struct T2 {
    a: u8,
    b: bool,
}

impl T2 {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::StringSelectMenu)]
#[string_select("a", defaults(placeholder = "Pick"))]
struct SelectR;

impl SelectR {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button)]
#[button("a", signed, defaults(style = Secondary, label = "Go"))]
struct SignedR;

impl SignedR {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button)]
#[button("a", schema = 3, defaults(style = Secondary, label = "Go"))]
struct SchemaR;

impl SchemaR {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::StringSelectMenu)]
#[string_select("a", signed, schema = 61, overflow = spill, defaults(placeholder = "Pick"))]
struct AllArgs;

impl AllArgs {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button)]
#[button("wizard.step", by_ref, defaults(style = Secondary, label = "Go"))]
struct Wizard {
    s: String,
}

impl Wizard {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button, Debug, PartialEq)]
#[button("s", overflow = spill, defaults(style = Secondary, label = "Go"))]
struct SpillS {
    s: String,
}

impl SpillS {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button)]
#[button("ss", overflow = spill, signed, defaults(style = Secondary, label = "Go"))]
struct SpillSigned {
    n: u8,
}

impl SpillSigned {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::Button)]
#[button("shop\x2Ebuy", schema = 1u8, defaults(style = Secondary, label = "Go"))]
struct Escaped;

impl Escaped {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum K {
    A,
    B,
    C,
}

#[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
enum E {
    A = 5,
    B,
}

/// A standalone derived Payload for a hand-written Route.
#[derive(Payload, Debug, PartialEq)]
struct DerivedPayload {
    k: K,
    n: Option<u16>,
}

struct HandRoute;

impl Route for HandRoute {
    const ID: RouteId = RouteId::new("hand");
    const SHAPE: RouteShape = RouteShape::INLINE;
    const SCHEMA: u8 = 0;
    type Payload = DerivedPayload;
}

#[test]
fn cid_01_buy_button() {
    assert_eq!(BuyButton::ID.as_str(), "shop.buy");
    assert_eq!(BuyButton::SHAPE, RouteShape::INLINE);
    assert_eq!(BuyButton::SCHEMA, 0);
    assert_eq!(BuyButton::MAX_ENCODED_LEN, Some(14));
    let width = route_width::<BuyButton>();
    assert_eq!(width.total(), Some(24));
    assert_eq!(
        width.to_string(),
        "shop.buy: F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)"
    );
    let buy = BuyButton {
        item: Id::new(255),
        qty: 3,
    };
    let id = encode::<BuyButton>(&buy, &Parts::new()).unwrap();
    assert_eq!(id.as_str(), "0shop.buy:47,3");
    assert_eq!(
        decode_route::<BuyButton>(&id),
        Ok(PayloadSource::Inline(buy))
    );

    assert_eq!(Ping::MAX_ENCODED_LEN, Some(0));
    assert_eq!(route_width::<Ping>().total(), Some(6));
    let id = encode::<Ping>(&Ping, &Parts::new()).unwrap();
    assert_eq!(id.as_str(), "0ping:");
    assert_eq!(decode_route::<Ping>(&id), Ok(PayloadSource::Inline(Ping)));

    assert_eq!(T2::MAX_ENCODED_LEN, Some(4));
    let id = encode::<T2>(&T2 { a: 7, b: true }, &Parts::new()).unwrap();
    assert_eq!(id.as_str(), "0t:7,1");
    assert_eq!(
        decode_route::<T2>(&id),
        Ok(PayloadSource::Inline(T2 { a: 7, b: true }))
    );
}

#[test]
fn cid_01_codec_argument_matrix() {
    // `0a:` and an empty body.
    let base = 3;
    assert_eq!(SelectR::SHAPE, RouteShape::INLINE.select());
    assert_eq!(route_width::<SelectR>().total(), Some(base + 2));
    assert_eq!(SignedR::SHAPE, RouteShape::INLINE.signed());
    assert_eq!(route_width::<SignedR>().total(), Some(base + 6));
    assert_eq!(SchemaR::SHAPE, RouteShape::INLINE);
    assert_eq!(SchemaR::SCHEMA, 3);
    assert_eq!(route_width::<SchemaR>().total(), Some(base + 1));

    assert_eq!(AllArgs::SHAPE, RouteShape::SPILL.select().signed());
    assert_eq!(AllArgs::SCHEMA, 61);

    assert_eq!(Wizard::SHAPE, RouteShape::BY_REF);
    assert_eq!(Wizard::MAX_ENCODED_LEN, None);
    let width = route_width::<Wizard>();
    assert_eq!(width.body_kind(), BodyKind::ByRef);
    assert_eq!(width.total(), Some(25));
    assert_eq!(
        width.to_string(),
        "wizard.step: F 1 + route 11 + sep 1 + S 0 + N 0 + ref 12 = 25 (limit 100)"
    );
    let _ = Wizard { s: String::new() };

    assert_eq!(Escaped::ID.as_str(), "shop.buy");
    assert_eq!(Escaped::SCHEMA, 1);
}

#[test]
fn cid_01_spill_inline_or_ref() {
    let width = route_width::<SpillS>();
    assert_eq!(width.total(), Some(100));
    assert_eq!(
        width.to_string(),
        "s: F 1 + route 1 + sep 1 + S 0 + N 0 + body unbounded + T 0 inline, else ref 12 = 100 (limit 100)"
    );
    let short = SpillS { s: "x".to_owned() };
    match encode_id::<SpillS>(&short, &Parts::new()).unwrap() {
        EncodedId::Inline(id) => assert_eq!(id.as_str(), "0s:x"),
        other @ EncodedId::RefSlot(_) => panic!("expected inline, got {other:?}"),
    }
    let long = SpillS { s: "x".repeat(98) };
    assert!(matches!(
        encode_id::<SpillS>(&long, &Parts::new()).unwrap(),
        EncodedId::RefSlot(_)
    ));
    assert_eq!(route_width::<SpillSigned>().total(), Some(16));
    let _ = SpillSigned { n: 0 };
}

#[test]
fn payload_enum_matrix() {
    assert_eq!(K::MAX_LEN, Some(1));
    for (k, s) in [(K::A, "0"), (K::B, "1"), (K::C, "2")] {
        let mut out = String::new();
        k.encode(&mut out).unwrap();
        assert_eq!(out, s);
        check::assert_worst_case(&k);
    }
    assert_eq!(K::decode("2"), Ok(K::C));
    assert_eq!(K::decode("3"), Err(FieldError::Overflow));
    assert_eq!(K::decode(""), Err(FieldError::Empty));
    assert_eq!(
        K::decode("00"),
        Err(FieldError::TooLong { max: 1, actual: 2 })
    );
    assert_eq!(K::decode("-"), Err(FieldError::InvalidChar { index: 0 }));
    assert_eq!(
        K::decode("é"),
        Err(FieldError::TooLong { max: 1, actual: 2 })
    );
    assert_eq!(K::decode("z"), Err(FieldError::Overflow));
    assert_eq!(K::absent(), None);

    let mut out = String::new();
    E::A.encode(&mut out).unwrap();
    E::B.encode(&mut out).unwrap();
    assert_eq!(out, "01");
    assert_eq!(E::decode("1"), Ok(E::B));

    let mut out = String::new();
    codec::encode_variant(61, &mut out).unwrap();
    assert_eq!(out, "z");
    assert!(codec::encode_variant(62, &mut out).is_err());
    assert_eq!(codec::decode_variant("1", 1), Err(FieldError::Overflow));
    assert_eq!(codec::decode_variant("0", 1), Ok(0));
}

// rustc reports `unnameable_test_items` on the generated width test only
// through an enclosing item's lint level, so a function-local Route needs this
// `allow` on its function (documented in the Usage Skill §5).
#[test]
#[allow(
    unnameable_test_items,
    reason = "the function-local Route's width test cannot run"
)]
fn function_local_route_compiles_clean() {
    #[derive(::noctis::Button, Debug, PartialEq)]
    #[button("local", defaults(style = Secondary, label = "Go"))]
    struct Local {
        n: u8,
    }

    impl Local {
        #[::noctis::handler]
        async fn handle(
            self,
            _ctx: ::noctis::ComponentCtx,
        ) -> ::noctis::HandlerResult<::noctis::Responded> {
            Ok(::noctis::__private::core::__testing::responded())
        }
    }
    assert_eq!(Local::ID.as_str(), "local");
    assert_eq!(Local { n: 1 }.n, 1);
}

#[test]
fn derive_payload_standalone() {
    assert_eq!(DerivedPayload::MAX_ENCODED_LEN, Some(1 + 1 + 4));
    let p = DerivedPayload {
        k: K::B,
        n: Some(62),
    };
    let id = encode::<HandRoute>(&p, &Parts::new()).unwrap();
    assert_eq!(id.as_str(), "0hand:1,110");
    assert_eq!(decode_route::<HandRoute>(&id), Ok(PayloadSource::Inline(p)));
    // A trailing `Option` may be missing from an older id.
    assert_eq!(
        decode_route::<HandRoute>("0hand:2"),
        Ok(PayloadSource::Inline(DerivedPayload { k: K::C, n: None }))
    );
}

#[derive(::noctis::Button, Debug, Clone, PartialEq)]
#[button("prop.round-trip", defaults(style = Secondary, label = "Go"))]
struct Mixed {
    a: u8,
    b: i64,
    c: Option<u64>,
    d: BoundedStr<4>,
    e: bool,
    k: K,
}

impl Mixed {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

fn k_strategy() -> impl Strategy<Value = K> {
    prop_oneof![Just(K::A), Just(K::B), Just(K::C)]
}

proptest! {
    #[test]
    fn cid_01_derived_round_trip(
        a in any::<u8>(),
        b in any::<i64>(),
        c in any::<Option<u64>>(),
        d in "[ -~]{0,4}",
        e in any::<bool>(),
        k in k_strategy(),
    ) {
        let value = Mixed { a, b, c, d: BoundedStr::new(&d).unwrap(), e, k };
        let id = encode::<Mixed>(&value, &Parts::new()).unwrap();
        prop_assert!(Some(id.len()) <= route_width::<Mixed>().total());
        prop_assert_eq!(decode_route::<Mixed>(&id), Ok(PayloadSource::Inline(value)));
    }
}
