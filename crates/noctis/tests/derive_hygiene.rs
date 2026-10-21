//! The derives and the method attributes expand to absolute paths only, so
//! local items named like std or codec items do not change their meaning
//! (AD-2; Story 6.11 Matrix 55).

#[allow(dead_code, non_camel_case_types, reason = "shadowing items only")]
mod shadowed {
    struct Option;
    struct Some;
    struct None;
    struct Ok;
    struct Err;
    type Result = ();
    struct Box;
    struct Route;
    struct Payload;
    struct Field;
    struct FieldDecode;
    struct usize;
    struct u8;
    struct u64;
    struct str;
    struct Vec;
    struct String;
    struct bool;
    struct Modal;
    struct Choices;
    struct Module;
    mod noctis_codec {}
    mod std {}
    mod core {}
    // An imported `test` attribute (like `use tokio::test;`) must not
    // replace the generated width test's `#[test]`.
    #[allow(unused_imports, reason = "shadowing the test attribute only")]
    use ::std::prelude::v1::derive as test;

    use ::noctis::events::{EventCtx, MessageCreate};
    use ::noctis::ui::{LegacyMessage, ResolvedMessage, ResolvedUser};
    use ::noctis::{
        AutocompleteCtx, CommandCtx, CommandResponder, ComponentCtx, HandlerResult, Manual,
        ModalSubmitCtx, Partial, Responded, Suggestions,
    };

    #[expect(
        clippy::unnecessary_wraps,
        reason = "every handler returns it as its result"
    )]
    fn responded() -> HandlerResult<Responded> {
        ::core::result::Result::Ok(::noctis::__private::core::__testing::responded())
    }

    #[derive(noctis::prelude::PayloadEnum, Debug, PartialEq)]
    pub(crate) enum Kind {
        A,
        B,
    }

    #[derive(noctis::prelude::StringSelectMenu, Debug, PartialEq)]
    #[string_select("hygiene", schema = 2, defaults(placeholder = "Pick", min_values = 0))]
    pub(crate) struct Named {
        pub(crate) kind: Kind,
        pub(crate) n: ::core::primitive::u8,
    }

    impl Named {
        #[noctis::prelude::handler]
        async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
            responded()
        }
    }

    #[derive(noctis::prelude::Button, Debug, PartialEq)]
    #[button("hygiene.unit", defaults(style = Primary, emoji = "<:pt:123>"))]
    pub(crate) struct Unit;

    impl Unit {
        #[noctis::prelude::handler]
        async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
            responded()
        }
    }

    #[derive(noctis::prelude::Payload, Debug, PartialEq)]
    pub(crate) struct Tuple(pub(crate) Kind, pub(crate) ::core::primitive::bool);

    #[derive(noctis::prelude::Choices, Debug, Clone, Copy, PartialEq)]
    pub(crate) enum Pick {
        #[choice("A", description = "first")]
        A,
        #[choice("B", value = "b")]
        B,
    }

    #[derive(noctis::prelude::Modal, Debug)]
    #[modal("hygiene.form", defaults(title = "Hygiene"))]
    pub(crate) struct Form {
        pub(crate) id: ::core::primitive::u64,
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

    impl Form {
        #[noctis::prelude::handler]
        async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
            responded()
        }
    }

    #[derive(noctis::prelude::SlashCommand)]
    #[command("hygiene (a|b)", description = "d", response = ephemeral, auto_defer = false, nsfw = false)]
    pub(crate) struct Command {
        #[option(description = "d", autocomplete)]
        pub(crate) query: ::std::string::String,
        #[option(description = "d")]
        pub(crate) count: ::std::option::Option<::core::primitive::i64>,
    }

    impl Command {
        #[noctis::prelude::handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            _path: CommandPath,
            r: CommandResponder<Manual>,
        ) -> HandlerResult<LegacyMessage> {
            let _ = r;
            ::core::result::Result::Ok(LegacyMessage::new("ok"))
        }

        #[noctis::prelude::autocomplete(query)]
        async fn query(
            _ctx: AutocompleteCtx,
            typed: &::core::primitive::str,
            _filled: Partial<Self>,
        ) -> HandlerResult<Suggestions<::std::string::String>> {
            let mut out = Suggestions::new();
            out.push(::std::borrow::ToOwned::to_owned(typed), "typed");
            ::core::result::Result::Ok(out)
        }
    }

    #[derive(noctis::prelude::UserCommand)]
    #[user_command("Hygiene")]
    pub(crate) struct Menu;

    impl Menu {
        #[noctis::prelude::handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            _target: ResolvedUser,
        ) -> HandlerResult<LegacyMessage> {
            ::core::result::Result::Ok(LegacyMessage::new("ok"))
        }
    }

    #[derive(noctis::prelude::MessageCommand)]
    #[message_command("Hygiene")]
    pub(crate) struct Quote;

    impl Quote {
        #[noctis::prelude::handler]
        async fn handle(
            self,
            _ctx: CommandCtx,
            _target: ResolvedMessage,
        ) -> HandlerResult<LegacyMessage> {
            ::core::result::Result::Ok(LegacyMessage::new("ok"))
        }
    }

    #[derive(noctis::prelude::EventHandler)]
    #[event(ordered)]
    pub(crate) struct Event;

    impl Event {
        #[noctis::prelude::handler]
        async fn handle(self, _ctx: EventCtx, _event: MessageCreate) -> HandlerResult<()> {
            ::core::result::Result::Ok(())
        }
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
    let json = serde_json::to_value(shadowed::Unit.build()).unwrap();
    assert_eq!(json["emoji"]["id"], "123");
}

#[test]
fn kind_derives_register_in_a_module() {
    let module = noctis::Module::new("hygiene")
        .add::<shadowed::Named>()
        .add::<shadowed::Unit>()
        .add::<shadowed::Form>()
        .add::<shadowed::Command>()
        .add::<shadowed::Menu>()
        .add::<shadowed::Quote>()
        .add::<shadowed::Event>();
    assert!(format!("{module:?}").contains("hygiene"));
    assert_eq!(
        shadowed::Command::descriptor().name(),
        "derive_hygiene::shadowed::Command"
    );
}

#[test]
fn modal_derives_ignore_local_names() {
    assert_eq!(shadowed::Pick::ALL, &[shadowed::Pick::A, shadowed::Pick::B]);
    assert_eq!(shadowed::Pick::A.value(), "A");
    assert_eq!(shadowed::Pick::A.description(), Some("first"));
    assert_eq!(shadowed::Pick::from_value("b"), Some(shadowed::Pick::B));
    assert_eq!(shadowed::Form::blank(7).len(), 5);
    let json = br#"{"type": 5, "data": {"custom_id": "0hygiene.form:7", "components": [
        {"type": 18, "component": {"type": 4, "custom_id": "text", "value": "t"}},
        {"type": 18, "component": {"type": 22, "custom_id": "picks", "values": ["b"]}},
        {"type": 18, "component": {"type": 21, "custom_id": "pick", "value": "A"}},
        {"type": 18, "component": {"type": 23, "custom_id": "ok", "value": true}}
    ]}}"#;
    let form = shadowed::Form::from_json(json).unwrap();
    assert_eq!(form.id, 7);
    assert_eq!(form.text.as_deref(), Some("t"));
    assert_eq!(form.picks, vec![shadowed::Pick::B]);
    assert_eq!(form.pick, shadowed::Pick::A);
    assert!(form.ok);
    let () = form.intro;
}
