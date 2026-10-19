//! The derives expand to absolute paths only, so local items named like std
//! or codec items do not change their meaning (AD-2).

#[allow(dead_code, non_camel_case_types, reason = "shadowing items only")]
mod shadowed {
    struct Option;
    struct Some;
    struct None;
    struct Ok;
    struct Err;
    type Result = ();
    struct Route;
    struct Payload;
    struct Field;
    struct FieldDecode;
    struct usize;
    struct u8;
    struct str;
    mod noctis_codec {}
    mod std {}
    mod core {}
    // An imported `test` attribute (like `use tokio::test;`) must not
    // replace the generated width test's `#[test]`.
    #[allow(unused_imports, reason = "shadowing the test attribute only")]
    use ::std::prelude::v1::derive as test;

    #[derive(noctis::prelude::PayloadEnum, Debug, PartialEq)]
    pub(crate) enum Kind {
        A,
        B,
    }

    #[derive(noctis::prelude::Route, Debug, PartialEq)]
    #[route("hygiene", schema = 2, view)]
    pub(crate) struct Named {
        pub(crate) kind: Kind,
        pub(crate) n: ::core::primitive::u8,
    }

    #[derive(noctis::prelude::Route, Debug, PartialEq)]
    #[route("hygiene.unit")]
    pub(crate) struct Unit;

    #[derive(noctis::prelude::Payload, Debug, PartialEq)]
    pub(crate) struct Tuple(pub(crate) Kind, pub(crate) ::core::primitive::bool);
}

use noctis::__private::codec::{Parts, PayloadSource, decode_route, encode};
use noctis::prelude::{Payload, Route};

#[test]
fn derives_ignore_local_names() {
    assert_eq!(shadowed::Named::ID.as_str(), "hygiene");
    assert_eq!(shadowed::Named::SCHEMA, 2);
    assert_eq!(shadowed::Named::MAX_ENCODED_LEN, Some(4));
    assert_eq!(shadowed::Tuple::MAX_ENCODED_LEN, Some(3));
    let id = encode::<shadowed::Unit>(&shadowed::Unit, &Parts::new()).unwrap();
    assert_eq!(
        decode_route::<shadowed::Unit>(&id),
        Ok(PayloadSource::Inline(shadowed::Unit))
    );
}
