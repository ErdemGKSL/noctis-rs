//! Fixtures shared by the layout and Ref integration tests.
#![allow(dead_code, reason = "each test file uses a subset of the fixtures")]

use core::cell::RefCell;

use noctis_codec::{RouteShape, Sign, Tag, Verify};

/// Hand-implements `Payload` and `PayloadDecode` for a struct of owned fields.
macro_rules! payload {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub(crate) struct $name {
            $(pub(crate) $field: $ty,)*
        }

        impl ::noctis_codec::Payload for $name {
            const MAX_ENCODED_LEN: Option<usize> = ::noctis_codec::sum_fields(&[
                $(<$ty as ::noctis_codec::Field>::MAX_LEN,)*
            ]);

            #[allow(unused_variables, reason = "zero-field payloads")]
            fn encode_fields(
                &self,
                out: &mut ::noctis_codec::FieldWriter<'_>,
            ) -> ::core::fmt::Result {
                $(out.field(&self.$field)?;)*
                Ok(())
            }
        }

        impl<'de> ::noctis_codec::PayloadDecode<'de> for $name {
            #[allow(unused_variables, reason = "zero-field payloads")]
            fn decode_fields(
                f: &mut ::noctis_codec::FieldReader<'de>,
            ) -> Result<Self, ::noctis_codec::CodecError> {
                Ok(Self { $($field: f.field()?,)* })
            }
        }
    };
}

/// Declares a marker Route.
macro_rules! route {
    ($name:ident, $id:literal, $shape:expr, $schema:literal, $payload:ty) => {
        pub(crate) struct $name;

        impl ::noctis_codec::Route for $name {
            const ID: ::noctis_codec::RouteId = ::noctis_codec::RouteId::new($id);
            const SHAPE: ::noctis_codec::RouteShape = $shape;
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
route!(Sel, "s", RouteShape::INLINE.select(), 0, NumP);
route!(Sig, "sig", RouteShape::INLINE.signed(), 0, NumP);
route!(All, "all", RouteShape::INLINE.select().signed(), 61, NumP);
route!(Stored, "r", RouteShape::BY_REF, 0, BuyV1);
route!(SpillSigned, "ss", RouteShape::SPILL.signed(), 0, TextP);
route!(BadSchema, "x", RouteShape::INLINE, 62, NumP);
route!(StoredV2, "r", RouteShape::BY_REF, 0, BuyV2Payload);
route!(StoredS2, "r", RouteShape::BY_REF, 2, BuyV1);
route!(StoredText, "rt", RouteShape::BY_REF, 0, TextP);
route!(
    StoredAll,
    "aaaaaaaaaaaaaaaaaaaaaaaa",
    RouteShape::BY_REF.select(),
    61,
    NumP
);

/// Returns `Tag::from_u64(n)` and records what it was asked to sign.
pub(crate) struct FakeSigner(u64, RefCell<String>);

impl FakeSigner {
    pub(crate) fn new(n: u64) -> Self {
        Self(n, RefCell::new(String::new()))
    }

    pub(crate) fn saw(&self) -> String {
        self.1.borrow().clone()
    }
}

impl Sign for FakeSigner {
    fn sign(&self, signed: &str) -> Tag {
        signed.clone_into(&mut self.1.borrow_mut());
        Tag::from_u64(self.0)
    }
}

impl Verify for FakeSigner {
    fn verify(&self, _signed: &str, tag: Tag) -> bool {
        tag.as_str() == Tag::from_u64(self.0).as_str()
    }
}
pub(crate) fn text(s: &str) -> TextP {
    TextP { s: s.to_owned() }
}

pub(crate) fn x(n: usize) -> TextP {
    text(&"x".repeat(n))
}

pub(crate) fn buy(item: u64, qty: u8) -> BuyV1 {
    BuyV1 { item, qty }
}
