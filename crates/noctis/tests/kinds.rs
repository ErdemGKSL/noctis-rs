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

use noctis::__private::codec::{Parts, PayloadKey, PayloadKeys, PayloadSource, Ref, SelectNonce};
use noctis::__private::core::__testing::{call_component, call_modal, responded};
use noctis::__private::ui::__macro::ModalFromPayload;
use noctis::prelude::*;
use noctis::rules::{LimitMode, Limits, RuleId};
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{
    ActionRow, ButtonStyle, ChannelSelect, Components, MentionableSelect, Modal, ModalSubmission,
    RoleSelect, SelectOption, StringSelect, SubmitError, UiError, UserSelect, V2Message, validate,
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
#[modal("note.ref", by_ref, defaults(title = "Note"))]
struct NoteRef {
    user: Id<UserMarker>,
    #[text_input("Note")]
    note: Option<String>,
}

#[derive(Modal, Debug)]
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

/// Story 9.3 B1-B5 and S4 through the derives: one overrides struct per
/// render, the later call wins, defaults stay a const.
#[test]
fn derived_overrides_and_defaults() {
    let plain = json!(Buy { item: 1 }.build());
    assert_eq!(
        json!(
            Buy { item: 1 }
                .build()
                .overrides(ButtonOverrides::default())
        ),
        plain
    );
    let again = json!(Buy { item: 1 }.build().overrides(ButtonOverrides {
        label: Some("Again".into()),
        disabled: Some(true),
        ..Default::default()
    }));
    assert_eq!(
        (&again["label"], &again["disabled"]),
        (&Value::from("Again"), &Value::from(true))
    );
    assert_eq!(
        (&again["style"], &again["custom_id"]),
        (&plain["style"], &plain["custom_id"])
    );
    let later = json!(
        Buy { item: 1 }
            .build()
            .overrides(ButtonOverrides {
                label: Some("b".into()),
                ..Default::default()
            })
            .label("a")
    );
    assert_eq!(later["label"], "a");
    assert_eq!(json!(Buy { item: 1 }.build()), plain);

    let colours = resolved(row(Colourful.build().overrides(StringSelectOverrides {
        placeholder: Some("Pick a colour".into()),
        default_values: Some(vec!["b".into()]),
        ..Default::default()
    })));
    let select = &colours[0]["components"][0];
    assert_eq!(select["placeholder"], "Pick a colour");
    assert_eq!(select["options"][1]["default"], true);
    assert!(select["options"][0].get("default").is_none());
    let users = resolved(row(Pick.build().overrides(UserSelectOverrides {
        default_values: Some(vec![Id::new(1)]),
        ..Default::default()
    })));
    assert_eq!(
        users[0]["components"][0]["default_values"],
        serde_json::json!([{"id": "1", "type": "user"}])
    );
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
    let built = json!(Modal::from(
        BioForm {
            user,
            bio: Some("hi".into())
        }
        .build()
    ));
    let blank = json!(Modal::from(BioForm::blank(user)));
    // The struct value pre-fills the input; a blank form sends no value.
    assert_eq!(built["components"][0]["component"]["value"], "hi");
    assert!(blank["components"][0]["component"].get("value").is_none());
    assert_eq!(built["custom_id"], blank["custom_id"]);
    assert_eq!(built["custom_id"], "0bio:g");
    assert_eq!(built["title"], "Bio");
    insta::assert_json_snapshot!("modal", built);
    let _: Modal = BioForm::blank(user).into();

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
/// does not decode directly and is `Malformed` (Story 7.2 S5); a signed id
/// decodes through the verified Payload, as component dispatch does it.
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

    let built = json!(Modal::from(NoteV3 { user, note: None }.build()));
    let id = built["custom_id"].as_str().unwrap();
    let form = NoteV3::from_json(submission(id).as_bytes()).unwrap();
    assert_eq!((form.user, form.note.as_deref()), (user, Some("hey")));

    let mut modal = Modal::from(NoteRef::blank(user));
    let id = modal.custom_id_mut();
    let slot = id.resolve(&Parts::new()).unwrap().expect("a Ref slot");
    id.finish(&slot, Ref::mint().unwrap()).unwrap();
    let reason = malformed(id.get().unwrap());
    assert!(reason.contains("route `note.ref`"), "{reason}");
    assert!(reason.contains("Ref"), "{reason}");

    let key = PayloadKey::from_bytes([7; 32]);
    let mut modal = Modal::from(NoteSigned::blank(user));
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

    // Story 7.2 S5: dispatch decodes the id once with the key ring and
    // builds the form from the verified Payload (`ModalFromPayload`).
    let raw = submission(id.get().unwrap());
    let parsed = ModalSubmission::from_slice(raw.as_bytes()).unwrap();
    let decoded = noctis::__private::codec::decode(id.get().unwrap()).unwrap();
    let Ok(PayloadSource::Inline(payload)) =
        decoded.verified_payload::<NoteSigned>(&PayloadKeys::new(key))
    else {
        panic!("the tag verifies with the key");
    };
    let form = <NoteSigned as ModalFromPayload>::from_payload(payload, &parsed).unwrap();
    assert_eq!((form.user, form.note.as_deref()), (user, Some("hey")));
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
    // Story 7.1: one handler per Route id (CID-04).
    let err = App::builder().module(module).build().unwrap_err();
    assert!(
        matches!(
            err.violations(),
            [noctis::BuildViolation::DuplicateRoute {
                route: "shop.buy",
                ..
            }]
        ),
        "{err}"
    );

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
