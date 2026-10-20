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
    struct Vec;
    struct String;
    struct bool;
    struct Modal;
    struct Choices;
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
    #[route("hygiene", schema = 2, select)]
    pub(crate) struct Named {
        pub(crate) kind: Kind,
        pub(crate) n: ::core::primitive::u8,
    }

    #[derive(noctis::prelude::Route, Debug, PartialEq)]
    #[route("hygiene.unit")]
    pub(crate) struct Unit;

    #[derive(noctis::prelude::Payload, Debug, PartialEq)]
    pub(crate) struct Tuple(pub(crate) Kind, pub(crate) ::core::primitive::bool);

    impl ::noctis::__private::ui::Handled for Unit {}

    #[derive(noctis::prelude::Choices, Debug, Clone, Copy, PartialEq)]
    pub(crate) enum Pick {
        #[choice("A", description = "first")]
        A,
        #[choice("B", value = "b")]
        B,
    }

    #[derive(noctis::prelude::Modal, Debug)]
    #[modal(route = Unit, title = "Hygiene")]
    pub(crate) struct Form {
        #[text_display("Hi")]
        pub(crate) intro: (),
        #[text_input("Text")]
        pub(crate) text: ::std::option::Option<::std::string::String>,
        #[checkbox_group("Picks", min_values = 0)]
        pub(crate) picks: ::std::vec::Vec<Pick>,
        #[radio_group("Pick")]
        pub(crate) pick: Pick,
        #[checkbox("Ok")]
        pub(crate) ok: ::core::primitive::bool,
    }
}

use noctis::__private::codec::{Parts, PayloadSource, decode_route, encode};
use noctis::prelude::{Choices, ModalForm, Payload, Route};

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

#[test]
fn modal_derives_ignore_local_names() {
    assert_eq!(shadowed::Pick::ALL, &[shadowed::Pick::A, shadowed::Pick::B]);
    assert_eq!(shadowed::Pick::A.value(), "A");
    assert_eq!(shadowed::Pick::A.description(), Some("first"));
    assert_eq!(shadowed::Pick::from_value("b"), Some(shadowed::Pick::B));
    assert_eq!(shadowed::Form::modal(shadowed::Unit).len(), 5);
    let json = br#"{"type": 5, "data": {"custom_id": "0hygiene.unit:", "components": [
        {"type": 18, "component": {"type": 4, "custom_id": "text", "value": "t"}},
        {"type": 18, "component": {"type": 22, "custom_id": "picks", "values": ["b"]}},
        {"type": 18, "component": {"type": 21, "custom_id": "pick", "value": "A"}},
        {"type": 18, "component": {"type": 23, "custom_id": "ok", "value": true}}
    ]}}"#;
    let form = shadowed::Form::from_json(json).unwrap();
    assert_eq!(form.text.as_deref(), Some("t"));
    assert_eq!(form.picks, vec![shadowed::Pick::B]);
    assert_eq!(form.pick, shadowed::Pick::A);
    assert!(form.ok);
    let () = form.intro;
}
