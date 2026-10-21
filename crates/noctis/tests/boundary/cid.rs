//! `custom_id` boundary passes (CID-*): Routes whose worst case is exactly
//! the limit compile and encode at the limit.

use noctis::__private::codec::{
    BoundedStr, EncodedId, Field, FieldDecode, MAX_REF_ID_LEN, Parts, PayloadKey, PayloadSource,
    SelectNonce, decode_route, decode_route_verified, encode, encode_id, finish, route_width,
};
use noctis::prelude::PayloadEnum;
use noctis::rules::Limits;
use noctis::ui::{V2Message, validate};

#[derive(::noctis::Button, Debug, PartialEq)]
#[button("a", defaults(style = Secondary, label = "Go"))]
struct Exact {
    t: BoundedStr<47>,
    n: u8,
}

impl Exact {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::StringSelectMenu, Debug, PartialEq)]
#[string_select("r", schema = 61, signed, defaults(placeholder = "Pick"))]
struct ExactAll {
    t: BoundedStr<42>,
    n: u16,
}

impl ExactAll {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[derive(::noctis::StringSelectMenu, Debug, PartialEq)]
#[string_select(
    "abcdefghijklmnopqrstuvwx",
    by_ref,
    schema = 61,
    defaults(placeholder = "Pick")
)]
struct LongRef {
    s: String,
}

impl LongRef {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[test]
fn cid_01_exactly_100_inline() {
    assert_eq!(route_width::<Exact>().total(), Some(100));
    let worst = Exact {
        t: BoundedStr::new(&"!".repeat(47)).unwrap(),
        n: u8::MAX,
    };
    let id = encode::<Exact>(&worst, &Parts::new()).unwrap();
    assert_eq!(id.len(), 100);
    assert_eq!(decode_route::<Exact>(&id), Ok(PayloadSource::Inline(worst)));
}

#[test]
fn cid_01_exactly_100_all_sections() {
    assert_eq!(route_width::<ExactAll>().total(), Some(100));
    let key = PayloadKey::from_bytes([7; 32]);
    let parts = Parts::new()
        .with_nonce(SelectNonce::new(3843).unwrap())
        .with_signer(&key);
    let worst = ExactAll {
        t: BoundedStr::new(&"!".repeat(42)).unwrap(),
        n: u16::MAX,
    };
    let id = encode::<ExactAll>(&worst, &parts).unwrap();
    assert_eq!(id.len(), 100);
    assert_eq!(
        decode_route_verified::<ExactAll>(&id, &key),
        Ok(PayloadSource::Inline(worst))
    );
}

#[test]
fn cid_01_by_ref_24_char_id_41() {
    assert_eq!(route_width::<LongRef>().total(), Some(41));
    assert_eq!(MAX_REF_ID_LEN, 41);
    let parts = Parts::new().with_nonce(SelectNonce::new(0).unwrap());
    let value = LongRef { s: "x".repeat(500) };
    let EncodedId::RefSlot(slot) = encode_id::<LongRef>(&value, &parts).unwrap() else {
        panic!("a by_ref Route always yields a Ref slot");
    };
    let id = finish(&slot, noctis::__private::codec::Ref::mint().unwrap());
    assert_eq!(id.len(), 41);
    assert!(id.starts_with('5'), "flags S and N: {id}");
}

macro_rules! sixty_two {
    ($($v:ident)*) => {
        #[derive(PayloadEnum, Debug, Clone, Copy, PartialEq, Eq)]
        enum Wide { $($v),* }
    };
}

sixty_two! {
    V0 V1 V2 V3 V4 V5 V6 V7 V8 V9 V10 V11 V12 V13 V14 V15 V16 V17 V18 V19 V20
    V21 V22 V23 V24 V25 V26 V27 V28 V29 V30 V31 V32 V33 V34 V35 V36 V37 V38 V39
    V40 V41 V42 V43 V44 V45 V46 V47 V48 V49 V50 V51 V52 V53 V54 V55 V56 V57 V58
    V59 V60 V61
}

#[test]
fn payload_enum_62_variants() {
    let mut out = String::new();
    Wide::V61.encode(&mut out).unwrap();
    assert_eq!(out, "z");
    assert_eq!(Wide::decode("z"), Ok(Wide::V61));
    assert_eq!(Wide::decode("0"), Ok(Wide::V0));
}

#[derive(::noctis::Button)]
#[button("b.cid", defaults(style = Secondary, label = "Go"))]
struct Press {
    n: u8,
}

impl Press {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

#[test]
fn cid_02_components_distinct_literal_payloads() {
    let body = noctis::components! {
        row [ Press { n: 1 }.build(), Press { n: 2 }.build() ],
        section [ text("a"); accessory = Press { n: 3 }.build() ],
    };
    let mut msg = V2Message::new(body);
    validate(&mut msg, &Limits::default()).expect("distinct custom_ids");
}
