//! The component and modal kind derives through the facade: `.build()` from
//! the declared `defaults(..)`, the override setters, registration with
//! `Module::add::<T>()`, the stored descriptors and the test seams (Story
//! 6.11, Matrix 13, 29-32, 47-49, 51, 52).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests panic with the failing case"
)]

use std::sync::Mutex;

use noctis::__private::codec::{Parts, PayloadKey, Ref, SelectNonce};
use noctis::__private::core::__testing::{call_component, call_modal, responded};
use noctis::prelude::*;
use noctis::rules::{LimitMode, Limits, RuleId};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{
    ActionRow, ButtonStyle, ChannelSelect, Components, MentionableSelect, Modal, RoleSelect,
    SelectOption, StringSelect, SubmitError, UiError, UserSelect, V2Message, validate,
};
use noctis::{ComponentKind, ModalHandler};
use serde_json::Value;

/// What the component and modal handlers saw.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(Button, Debug, PartialEq)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        SEEN.lock()
            .unwrap()
            .push(format!("buy {} in {}", self.item, ctx.module()));
        Ok(responded())
    }
}

#[derive(Button)]
#[button("shop.next", defaults(style = Secondary, emoji = "➡️"))]
struct Next;

#[derive(Button)]
#[button("shop.both", defaults(style = Success, label = "Go", emoji = "<:pt:123>", disabled = true))]
struct Both;

#[derive(Button)]
#[button("shop.anim", defaults(style = Danger, emoji = "<a:pt:123>"))]
struct Animated;

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Colour {
    #[choice("Red", value = "r", description = "Warm")]
    Red,
    #[choice("Blue", value = "b")]
    Blue,
}

#[derive(StringSelectMenu)]
#[string_select("pick.colour", defaults(placeholder = "Colour", options = Colour))]
struct Colourful;

#[derive(StringSelectMenu)]
#[string_select(
    "ticket.kind",
    defaults(placeholder = "Pick", min_values = 0, max_values = 25)
)]
struct TicketKind;

#[derive(UserSelectMenu)]
#[user_select("pick.who", defaults(placeholder = "Pick"))]
struct Pick;

#[derive(RoleSelectMenu)]
#[role_select("pick.role", defaults(placeholder = "Role", disabled = true))]
struct Role;

#[derive(MentionableSelectMenu)]
#[mentionable_select("pick.any", defaults(placeholder = "Anyone"))]
struct Anyone;

#[derive(ChannelSelectMenu)]
#[channel_select("pick.chan", defaults(placeholder = "Channel", channel_types = [GuildText, GuildVoice]))]
struct Chan;

macro_rules! stub_handlers {
    ($($ty:ty),*) => {$(
        impl $ty {
            #[handler]
            async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
                Ok(responded())
            }
        }
    )*};
}

stub_handlers!(
    Next, Both, Animated, Colourful, TicketKind, Pick, Role, Anyone, Chan
);

#[derive(Modal, Debug)]
#[modal("bio", defaults(title = "Bio"))]
struct BioForm {
    user: Id<UserMarker>,
    #[text_input("Bio", style = paragraph, max_length = 300)]
    bio: Option<String>,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        SEEN.lock()
            .unwrap()
            .push(format!("bio of {}: {:?}", self.user, self.bio));
        Ok(responded())
    }
}

#[derive(Modal, Debug)]
#[modal("note.v3", schema = 3, defaults(title = "Note"))]
struct NoteV3 {
    user: Id<UserMarker>,
    #[text_input("Note")]
    note: Option<String>,
}

#[derive(Modal, Debug)]
#[expect(
    dead_code,
    reason = "a Ref modal never decodes outside component dispatch, so its fields are never read"
)]
#[modal("note.ref", by_ref, defaults(title = "Note"))]
struct NoteRef {
    user: Id<UserMarker>,
    #[text_input("Note")]
    note: Option<String>,
}

#[derive(Modal, Debug)]
#[expect(
    dead_code,
    reason = "a signed modal never decodes outside component dispatch, so its fields are never read"
)]
#[modal("note.sig", signed, defaults(title = "Note"))]
struct NoteSigned {
    user: Id<UserMarker>,
    #[text_input("Note")]
    note: Option<String>,
}

impl NoteV3 {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }
}

impl NoteRef {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }
}

impl NoteSigned {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }
}

/// The JSON of a component.
macro_rules! json {
    ($value:expr $(,)?) => {
        serde_json::to_value(&$value).unwrap()
    };
}

/// Resolves every unresolved `custom_id` (the select nonce) as core would.
fn resolved(component: impl Into<Components>) -> Value {
    let mut body = component.into();
    let nonce = SelectNonce::new(1).unwrap();
    for id in body.custom_ids_mut() {
        if id.get().is_none() {
            id.resolve(&Parts::new().with_nonce(nonce)).unwrap();
        }
    }
    json!(body)
}

fn row(select: impl noctis::ui::RowSelect) -> Components {
    Components::new().push(ActionRow::select(select))
}

/// Matrix 29.
#[test]
fn button_defaults() {
    assert_eq!(
        json!(Buy { item: 255 }.build()),
        serde_json::json!({"type": 2, "style": 1, "label": "Buy", "custom_id": "0shop.buy:47"})
    );
    insta::assert_json_snapshot!(
        "buttons",
        [
            json!(Next.build()),
            json!(Both.build()),
            json!(Animated.build()),
        ]
    );
}

/// Matrix 30.
#[test]
fn select_defaults() {
    let strings: StringSelect = Colourful.build();
    let users: UserSelect = Pick.build();
    let roles: RoleSelect = Role.build();
    let anyone: MentionableSelect = Anyone.build();
    let channels: ChannelSelect = Chan.build();
    let ticket = TicketKind
        .build()
        .options([SelectOption::new("Billing", "billing")]);
    insta::assert_json_snapshot!(
        "selects",
        [
            resolved(row(strings)),
            resolved(row(ticket)),
            resolved(row(users)),
            resolved(row(roles)),
            resolved(row(anyone)),
            resolved(row(channels)),
        ]
    );
    let users = resolved(row(Pick.build()));
    let select = &users[0]["components"][0];
    assert_eq!(select["placeholder"], "Pick");
    for absent in ["min_values", "max_values", "disabled"] {
        assert!(
            select.get(absent).is_none(),
            "{absent} is not sent: {select}"
        );
    }
}

/// Matrix 48.
#[test]
fn overrides_replace_one_field() {
    let button = json!(
        Buy { item: 1 }
            .build()
            .label("x")
            .style(ButtonStyle::Danger)
            .disabled(true),
    );
    assert_eq!(
        (&button["label"], &button["style"], &button["disabled"]),
        (&Value::from("x"), &Value::from(4), &Value::from(true))
    );
    assert_eq!(button["custom_id"], "0shop.buy:1");
    let pick = resolved(row(Pick.build().placeholder("p").max_values(3)));
    let select = &pick[0]["components"][0];
    assert_eq!(
        (&select["placeholder"], &select["max_values"]),
        (&Value::from("p"), &Value::from(3))
    );
    let colours = resolved(row(Colourful
        .build()
        .options([SelectOption::new("Green", "g")])));
    let options = colours[0]["components"][0]["options"].as_array().unwrap();
    assert_eq!(options.len(), 1);
    assert_eq!(colours[0]["components"][0]["placeholder"], "Colour");
}

/// Matrix 47 (runtime half): options given empty at runtime fail the
/// validator with CV2-09; a count is never truncated, so Lenient mode fails
/// too.
#[test]
fn empty_runtime_options_are_cv2_09() {
    let mut msg = V2Message::new(row(TicketKind.build().options(Vec::new())));
    let Err(UiError::Limits { violations }) = validate(&mut msg, &Limits::default()) else {
        panic!("an empty String Select is invalid");
    };
    assert_eq!(violations[0].rule, RuleId::CV2_09);
    let mut lenient = Limits::default();
    lenient.mode = LimitMode::Lenient;
    let mut msg = V2Message::new(row(TicketKind.build().options(Vec::new())));
    let Err(UiError::Limits { violations }) = validate(&mut msg, &lenient) else {
        panic!("counts fail in Lenient mode too");
    };
    assert_eq!(violations[0].rule, RuleId::CV2_09);
}

/// Matrix 49: a select's id waits for core's nonce.
#[test]
fn select_id_is_unresolved_until_core_resolves_it() {
    let err = serde_json::to_value(Pick.build()).unwrap_err();
    assert!(
        err.to_string()
            .contains("custom_id of route `pick.who` is unresolved"),
        "{err}"
    );
}

/// Matrix 31 and 32.
#[test]
fn modal_build_blank_and_submission() {
    let user = Id::new(42);
    let built = json!(
        BioForm {
            user,
            bio: Some("hi".into())
        }
        .build()
    );
    let blank = json!(BioForm::blank(user));
    assert_eq!(built, blank, "pre-fill arrives with Story 9.4");
    assert_eq!(built["custom_id"], "0bio:g");
    assert_eq!(built["title"], "Bio");
    insta::assert_json_snapshot!("modal", built);
    let _: Modal = BioForm::blank(user);

    let submission = |custom_id: &str| {
        format!(
            r#"{{"type": 5, "data": {{"custom_id": "{custom_id}", "components": [
                {{"type": 18, "component": {{"type": 4, "custom_id": "bio", "value": "hello"}}}}
            ]}}}}"#
        )
    };
    let form = BioForm::from_json(submission("0bio:g").as_bytes()).unwrap();
    assert_eq!((form.user, form.bio.as_deref()), (user, Some("hello")));
    for other in ["0shop.buy:1", "!!"] {
        let Err(SubmitError::Malformed { reason }) =
            BioForm::from_json(submission(other).as_bytes())
        else {
            panic!("{other} is not a `bio` custom_id");
        };
        assert!(reason.contains("route `bio`"), "{reason}");
    }
}

/// Codec arguments on a modal: `schema` round-trips; a Ref or a signed id
/// does not decode outside component dispatch (Epic 7) and is `Malformed`.
#[test]
fn modal_codec_arguments() {
    let user = Id::new(42);
    let submission = |custom_id: &str| {
        format!(
            r#"{{"type": 5, "data": {{"custom_id": "{custom_id}", "components": [
                {{"type": 18, "component": {{"type": 4, "custom_id": "note", "value": "hey"}}}}
            ]}}}}"#
        )
    };
    let malformed = |custom_id: &str| -> String {
        match NoteRef::from_json(submission(custom_id).as_bytes()) {
            Err(SubmitError::Malformed { reason }) => reason,
            other => panic!("{custom_id}: {other:?}"),
        }
    };

    let built = json!(NoteV3 { user, note: None }.build());
    let id = built["custom_id"].as_str().unwrap();
    let form = NoteV3::from_json(submission(id).as_bytes()).unwrap();
    assert_eq!((form.user, form.note.as_deref()), (user, Some("hey")));

    let mut modal = NoteRef::blank(user);
    let id = modal.custom_id_mut();
    let slot = id.resolve(&Parts::new()).unwrap().expect("a Ref slot");
    id.finish(&slot, Ref::mint().unwrap()).unwrap();
    let reason = malformed(id.get().unwrap());
    assert!(reason.contains("route `note.ref`"), "{reason}");
    assert!(reason.contains("Ref"), "{reason}");

    let key = PayloadKey::from_bytes([7; 32]);
    let mut modal = NoteSigned::blank(user);
    let id = modal.custom_id_mut();
    assert!(
        id.resolve(&Parts::new().with_signer(&key))
            .unwrap()
            .is_none()
    );
    let Err(SubmitError::Malformed { reason }) =
        NoteSigned::from_json(submission(id.get().unwrap()).as_bytes())
    else {
        panic!("a signed modal does not decode directly");
    };
    assert!(reason.contains("route `note.sig`"), "{reason}");
}

/// Matrix 13, 51 and 52: registration stores the descriptors; the seams run
/// the handlers with a struct value.
#[tokio::test]
async fn descriptors_and_handlers() {
    let buy = Buy::descriptor();
    assert_eq!(
        (buy.route(), buy.kind()),
        ("shop.buy", ComponentKind::Button)
    );
    assert_eq!(buy.name(), "kinds::Buy");
    assert_eq!(Chan::descriptor().kind(), ComponentKind::ChannelSelect);
    assert_eq!(Colourful::descriptor().kind(), ComponentKind::StringSelect);
    let bio: ModalHandler = BioForm::descriptor();
    assert_eq!(bio.route(), "bio");
    let module = Module::new("shop")
        .add::<Buy>()
        .add::<Buy>()
        .add::<Pick>()
        .add::<BioForm>();
    let debug = format!("{module:?}");
    assert!(debug.contains("shop.buy"), "{debug}");
    // `App::build` does not read component descriptors yet (Story 7.1).
    assert!(App::builder().module(module).build().is_ok());

    let _ = call_component(&buy, Buy { item: 7 }).await.unwrap();
    let _ = call_modal(
        &bio,
        BioForm {
            user: Id::new(9),
            bio: None,
        },
    )
    .await
    .unwrap();
    let seen = SEEN.lock().unwrap().clone();
    assert!(seen.iter().any(|s| s.starts_with("buy 7")), "{seen:?}");
    assert!(seen.contains(&"bio of 9: None".to_owned()), "{seen:?}");
    // A value of another struct is an error, not a panic.
    assert!(call_component(&buy, Next).await.is_err());
}
