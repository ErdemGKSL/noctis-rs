//! Messages against Discord JSON: V2 and legacy bodies (insta snapshots),
//! per-endpoint flags, mentions and `custom_id` resolution (FR-23, CV2-01,
//! CV2-02, CV2-15, MSG-03, MSG-05, MSG-07, INT-09).

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use core::fmt;

use noctis_codec::{
    FieldWriter, Parts, Payload, Route, RouteId, RouteShape, SelectNonce, SelectRoute,
};
use noctis_ui::__macro::{ComponentValue, SelectDefaults, build_string_select_builder};
use noctis_ui::{
    ActionRow, Attachment, AttachmentSet, CallbackFlags, Components, CreateFlags, EditFlags,
    Handled, LegacyMessage, LinkButton, Mentions, MessageBody, MessageFormat, Roles, SelectOption,
    StringSelect, TextDisplay, UiError, Users, V2Message,
};
use serde_json::{Value, json};
use twilight_model::id::Id;

/// Select defaults that send nothing (as before Story 6.11's derives).
const NO_DEFAULTS: SelectDefaults = SelectDefaults {
    placeholder: None,
    min_values: None,
    max_values: None,
    disabled: false,
};

/// A String Select of `route` with `options` and no declared defaults.
fn string_select<R: ComponentValue + SelectRoute>(
    route: R,
    options: impl IntoIterator<Item = SelectOption>,
) -> StringSelect {
    build_string_select_builder(route, &NO_DEFAULTS).options(options)
}

/// A handled `select` Route with no field: its id waits for a nonce.
#[derive(Debug, Clone)]
struct Pick;

impl Payload for Pick {
    const MAX_ENCODED_LEN: Option<usize> = Some(0);

    fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl Route for Pick {
    const ID: RouteId = RouteId::new("pick");
    const SHAPE: RouteShape = RouteShape::INLINE.select();
    const SCHEMA: u8 = 0;
    type Payload = Self;
}

impl noctis_codec::DeclaredRuntime for Pick {}
impl Handled for Pick {}
impl SelectRoute for Pick {}

fn c1() -> Components {
    Components::new().push(TextDisplay::new("hi"))
}

fn c1_json() -> Value {
    json!([{"type": 10, "id": 1, "content": "hi"}])
}

fn row() -> ActionRow {
    ActionRow::buttons([LinkButton::new("https://a.b").label("A")])
}

fn row_json() -> Value {
    json!({"type": 1, "components": [{"type": 2, "style": 5, "label": "A", "url": "https://a.b"}]})
}

fn to_json(body: &MessageBody<'_>) -> Value {
    serde_json::to_value(body).expect("serializes")
}

fn pick_row() -> ActionRow {
    ActionRow::select(string_select(Pick, [SelectOption::new("A", "a")]))
}

const fn assert_send_sync<T: Send + Sync>() {}
const _: () = assert_send_sync::<V2Message>();
const _: () = assert_send_sync::<LegacyMessage>();
const _: () = assert_send_sync::<MessageBody<'static>>();
const _: () = assert_send_sync::<Mentions>();
const _: () = assert_send_sync::<CreateFlags>();

// Flags.

#[test]
fn flags_empty() {
    assert_eq!(CreateFlags::empty().bits(), 0);
    assert!(CreateFlags::default().is_empty());
    assert_eq!(EditFlags::empty(), EditFlags::default());
    assert!(EditFlags::empty().is_empty());
    assert_eq!(CallbackFlags::default().bits(), 0);
    assert!(CallbackFlags::empty().is_empty());
}

#[test]
fn msg_07_flag_bits() {
    assert_eq!(CreateFlags::SUPPRESS_EMBEDS.bits(), 4);
    assert_eq!(CreateFlags::SUPPRESS_NOTIFICATIONS.bits(), 4096);
    assert_eq!(EditFlags::SUPPRESS_EMBEDS.bits(), 4);
    assert_eq!(CallbackFlags::SUPPRESS_EMBEDS.bits(), 4);
    assert_eq!(CallbackFlags::EPHEMERAL.bits(), 64);
    assert_eq!(CallbackFlags::SUPPRESS_NOTIFICATIONS.bits(), 4096);
}

#[test]
fn flags_union() {
    let (a, b) = (CallbackFlags::EPHEMERAL, CallbackFlags::SUPPRESS_EMBEDS);
    assert_eq!((a | b).bits(), 68);
    assert_eq!(a.union(b), a | b);
    assert_eq!(a | a, a);
    let mut c = a;
    c |= b;
    assert_eq!(c, a | b);
    assert!(c.contains(a));
    assert!(c.contains(a | b));
    assert!(CallbackFlags::empty().contains(CallbackFlags::empty()));
    assert!(!a.contains(b));
    assert!(!c.is_empty());
}

#[test]
fn flags_const() {
    const F: CreateFlags = CreateFlags::SUPPRESS_EMBEDS.union(CreateFlags::SUPPRESS_NOTIFICATIONS);
    assert_eq!(F.bits(), 4100);
}

#[test]
fn format_from_flags() {
    assert_eq!(MessageFormat::from_flags(0), MessageFormat::Legacy);
    assert_eq!(MessageFormat::from_flags(4), MessageFormat::Legacy);
    assert_eq!(MessageFormat::from_flags(32768), MessageFormat::V2);
    assert_eq!(MessageFormat::from_flags(32768 | 64), MessageFormat::V2);
    assert_eq!(MessageFormat::from_flags(u64::MAX), MessageFormat::V2);
}

// V2 bodies.

#[test]
fn cv2_01_v2_bodies_always_set_flag() {
    let msg = V2Message::new(c1());
    let create = msg.create_body(CreateFlags::empty());
    insta::assert_json_snapshot!("v2_create", create);
    assert_eq!(
        to_json(&create),
        json!({"flags": 32768, "components": c1_json(), "allowed_mentions": {"parse": []}})
    );
    assert_eq!(
        to_json(&msg.create_body(CreateFlags::SUPPRESS_NOTIFICATIONS))["flags"],
        36864
    );
    let callback = msg.callback_body(CallbackFlags::EPHEMERAL);
    insta::assert_json_snapshot!("v2_callback", callback);
    assert_eq!(to_json(&callback)["flags"], 32832);
    let all = CallbackFlags::SUPPRESS_EMBEDS
        | CallbackFlags::EPHEMERAL
        | CallbackFlags::SUPPRESS_NOTIFICATIONS;
    assert_eq!(to_json(&msg.callback_body(all))["flags"], 36932);
}

#[test]
fn v2_edit_of_v2_has_no_clears() {
    let msg = V2Message::new(c1());
    let body = msg.edit_body(MessageFormat::V2, EditFlags::empty());
    insta::assert_json_snapshot!("v2_edit_v2", body);
    assert_eq!(
        to_json(&body),
        json!({"flags": 32768, "components": c1_json(), "allowed_mentions": {"parse": []}})
    );
    let suppressed = to_json(&msg.edit_body(MessageFormat::V2, EditFlags::SUPPRESS_EMBEDS));
    assert_eq!(suppressed["flags"], 32772);
    for key in ["content", "embeds", "poll", "sticker_ids"] {
        assert!(suppressed.get(key).is_none(), "{key}");
    }
}

#[test]
fn cv2_02_legacy_to_v2_edit_clears_fields() {
    let msg = V2Message::new(c1());
    let body = msg.edit_body(MessageFormat::Legacy, EditFlags::empty());
    insta::assert_json_snapshot!("v2_edit_legacy", body);
    let json = serde_json::to_string(&body).expect("serializes");
    assert_eq!(
        json,
        r#"{"flags":32768,"content":null,"embeds":[],"poll":null,"sticker_ids":[],"components":[{"type":10,"id":1,"content":"hi"}],"allowed_mentions":{"parse":[]}}"#
    );
}

#[test]
fn cv2_02_v2_bodies_have_no_legacy_fields() {
    let msg = V2Message::new(c1());
    for body in [
        msg.create_body(CreateFlags::SUPPRESS_EMBEDS),
        msg.callback_body(CallbackFlags::EPHEMERAL),
        msg.edit_body(MessageFormat::V2, EditFlags::empty()),
    ] {
        let json = to_json(&body);
        for key in [
            "content",
            "embeds",
            "poll",
            "sticker_ids",
            "shared_client_theme",
            "attachments",
            "files",
        ] {
            assert!(json.get(key).is_none(), "{key}");
        }
        assert!(json["flags"].as_u64().expect("flags") & 32768 != 0);
        assert!(json.get("allowed_mentions").is_some());
    }
}

#[test]
fn v2_key_order() {
    let msg = V2Message::new(c1());
    let json = serde_json::to_string(&msg.create_body(CreateFlags::empty())).expect("serializes");
    assert_eq!(
        json,
        r#"{"flags":32768,"components":[{"type":10,"id":1,"content":"hi"}],"allowed_mentions":{"parse":[]}}"#
    );
}

#[test]
fn v2_empty_body() {
    let msg = V2Message::new(Components::new());
    assert_eq!(
        to_json(&msg.create_body(CreateFlags::empty()))["components"],
        json!([])
    );
}

#[test]
fn v2_from_components() {
    let a = V2Message::from(c1());
    let b = V2Message::new(c1());
    assert_eq!(
        to_json(&a.create_body(CreateFlags::empty())),
        to_json(&b.create_body(CreateFlags::empty()))
    );
}

#[test]
fn v2_getters() {
    let msg = V2Message::new(c1());
    assert_eq!(msg.components().len(), 1);
    assert!(msg.attachment_set().is_empty());
    assert_eq!(msg.allowed_mentions(), &Mentions::default());
    let mut set = AttachmentSet::new();
    let _ = set.add(Attachment::new(&b"png"[..], "png").expect("valid ext"));
    let msg = msg.attachments(set);
    assert_eq!(msg.attachment_set().len(), 1);
    let json = to_json(&msg.create_body(CreateFlags::empty()));
    assert!(json.get("attachments").is_none());
}

#[test]
fn v2_mentions_on_body() {
    let m = Mentions::none().users(Users::only([Id::new(1)]));
    let msg = V2Message::new(c1()).mentions(m.clone());
    assert_eq!(msg.allowed_mentions(), &m);
    assert_eq!(
        to_json(&msg.create_body(CreateFlags::empty()))["allowed_mentions"],
        serde_json::to_value(&m).expect("serializes")
    );
}

// Unresolved ids.

#[test]
fn unresolved_ids_fail_until_resolved() {
    let nonce = SelectNonce::new(1).expect("in range");
    let mut v2 = V2Message::new(Components::new().push(pick_row()));
    let err = serde_json::to_value(v2.create_body(CreateFlags::empty()))
        .expect_err("unresolved")
        .to_string();
    assert!(err.contains("unresolved"), "{err}");
    let mut ids = 0;
    for id in v2.custom_ids_mut() {
        assert_eq!(id.resolve(&Parts::new().with_nonce(nonce)), Ok(None));
        ids += 1;
    }
    assert_eq!(ids, 1);
    assert!(serde_json::to_value(v2.create_body(CreateFlags::empty())).is_ok());

    let mut legacy = LegacyMessage::new("x").rows([row(), pick_row()]);
    let err = serde_json::to_value(legacy.create_body(CreateFlags::empty()))
        .expect_err("unresolved")
        .to_string();
    assert!(err.contains("unresolved"), "{err}");
    let mut ids = 0;
    for id in legacy.custom_ids_mut() {
        assert_eq!(id.route(), RouteId::new("pick"));
        assert_eq!(id.resolve(&Parts::new().with_nonce(nonce)), Ok(None));
        ids += 1;
    }
    assert_eq!(ids, 1);
    assert!(serde_json::to_value(legacy.create_body(CreateFlags::empty())).is_ok());
}

#[test]
fn custom_ids_mut_pre_order() {
    let mut components = Components::new().push(pick_row()).push(pick_row());
    let expected = components.custom_ids_mut().count();
    let mut msg = V2Message::new(components);
    assert_eq!(msg.custom_ids_mut().count(), expected);
    assert_eq!(expected, 2);
}

// Mentions.

fn mentions_json(m: &Mentions) -> Value {
    serde_json::to_value(m).expect("serializes")
}

#[test]
fn msg_05_default_parses_nothing() {
    assert_eq!(Mentions::default(), Mentions::none());
    assert_eq!(mentions_json(&Mentions::default()), json!({"parse": []}));
    let legacy = LegacyMessage::new("x");
    assert_eq!(
        to_json(&legacy.create_body(CreateFlags::empty()))["allowed_mentions"],
        json!({"parse": []})
    );
}

#[test]
fn msg_03_mentions_users_and_roles() {
    assert_eq!(
        mentions_json(&Mentions::none().users(Users::All)),
        json!({"parse": ["users"]})
    );
    let only = Users::only([Id::new(1), Id::new(2)]);
    assert_eq!(only, Users::Only(vec![Id::new(1), Id::new(2)]));
    assert_eq!(
        mentions_json(&Mentions::none().users(only)),
        json!({"parse": [], "users": ["1", "2"]})
    );
    assert_eq!(
        mentions_json(&Mentions::none().users(Users::only([Id::new(2), Id::new(2)]))),
        json!({"parse": [], "users": ["2", "2"]})
    );
    assert_eq!(
        mentions_json(
            &Mentions::none()
                .users(Users::Only(vec![]))
                .roles(Roles::Only(vec![]))
        ),
        json!({"parse": []})
    );
    assert_eq!(
        mentions_json(&Mentions::none().roles(Roles::All)),
        json!({"parse": ["roles"]})
    );
    assert_eq!(
        mentions_json(&Mentions::none().roles(Roles::only([Id::new(5)]))),
        json!({"parse": [], "roles": ["5"]})
    );
    assert_eq!(
        mentions_json(
            &Mentions::none()
                .users(Users::All)
                .roles(Roles::only([Id::new(5)]))
                .everyone(true)
        ),
        json!({"parse": ["users", "everyone"], "roles": ["5"]})
    );
    assert_eq!(
        serde_json::to_string(
            &Mentions::none()
                .everyone(true)
                .roles(Roles::All)
                .users(Users::All)
        )
        .expect("serializes"),
        r#"{"parse":["users","roles","everyone"]}"#
    );
}

#[test]
fn mentions_everyone_and_replied_user() {
    let m = Mentions::none().everyone(true).replied_user(true);
    assert!(m.pings_everyone() && m.pings_replied_user());
    assert_eq!(
        mentions_json(&m),
        json!({"parse": ["everyone"], "replied_user": true})
    );
    assert_eq!(
        mentions_json(&Mentions::none().everyone(false).replied_user(false)),
        json!({"parse": []})
    );
    let m = Mentions::none().users(Users::All).roles(Roles::All);
    assert_eq!(m.user_mentions(), &Users::All);
    assert_eq!(m.role_mentions(), &Roles::All);
}

#[test]
fn mentions_101_ids_serialize() {
    let m = Mentions::none().users(Users::only((1..=101).map(Id::new)));
    assert_eq!(
        mentions_json(&m)["users"]
            .as_array()
            .expect("users array")
            .len(),
        101
    );
}

// Legacy bodies.

#[test]
fn legacy_create() {
    let msg = LegacyMessage::new("héllo 😀");
    let body = msg.create_body(CreateFlags::empty());
    insta::assert_json_snapshot!("legacy_create", body);
    assert_eq!(
        serde_json::to_string(&body).expect("serializes"),
        r#"{"content":"héllo 😀","components":[],"allowed_mentions":{"parse":[]}}"#
    );
}

#[test]
fn legacy_flags() {
    let msg = LegacyMessage::new("x");
    assert_eq!(
        to_json(&msg.create_body(CreateFlags::SUPPRESS_EMBEDS))["flags"],
        4
    );
    assert_eq!(
        to_json(&msg.callback_body(CallbackFlags::EPHEMERAL))["flags"],
        64
    );
    assert!(
        to_json(&msg.callback_body(CallbackFlags::empty()))
            .get("flags")
            .is_none()
    );
    for body in [
        msg.create_body(CreateFlags::SUPPRESS_EMBEDS | CreateFlags::SUPPRESS_NOTIFICATIONS),
        msg.callback_body(CallbackFlags::EPHEMERAL),
    ] {
        assert_eq!(to_json(&body)["flags"].as_u64().expect("flags") & 32768, 0);
    }
}

#[test]
fn legacy_rows() {
    let msg = LegacyMessage::new("x").rows([row(), row()]);
    assert_eq!(msg.row_count(), 2);
    assert_eq!(
        to_json(&msg.create_body(CreateFlags::empty()))["components"],
        json!([row_json(), row_json()])
    );
    let msg = msg.rows([row()]);
    assert_eq!(msg.row_count(), 1);
}

#[test]
fn cv2_15_zero_and_five_rows() {
    let none = LegacyMessage::new("x").rows([]);
    assert_eq!(
        to_json(&none.create_body(CreateFlags::empty()))["components"],
        json!([])
    );
    let five = LegacyMessage::new("x").rows([row(), row(), row(), row(), row()]);
    assert_eq!(five.row_count(), 5);
    assert_eq!(
        to_json(&five.create_body(CreateFlags::empty()))["components"],
        json!([row_json(), row_json(), row_json(), row_json(), row_json()])
    );
}

#[test]
fn legacy_edit_of_legacy() {
    let msg = LegacyMessage::new("x").rows([row()]);
    let body = msg
        .edit_body(MessageFormat::Legacy, EditFlags::empty())
        .expect("legacy edit");
    insta::assert_json_snapshot!("legacy_edit", body);
    assert_eq!(
        serde_json::to_string(&body).expect("serializes"),
        r#"{"flags":0,"content":"x","components":[{"type":1,"components":[{"type":2,"style":5,"label":"A","url":"https://a.b"}]}],"allowed_mentions":{"parse":[]}}"#
    );
    let suppressed = msg
        .edit_body(MessageFormat::Legacy, EditFlags::SUPPRESS_EMBEDS)
        .expect("legacy edit");
    assert_eq!(to_json(&suppressed)["flags"], 4);
}

#[test]
fn cv2_01_legacy_edit_of_v2_is_an_error() {
    let msg = LegacyMessage::new("x");
    for flags in [EditFlags::empty(), EditFlags::SUPPRESS_EMBEDS] {
        let err = msg
            .edit_body(MessageFormat::V2, flags)
            .expect_err("V2 to legacy");
        assert_eq!(err, UiError::V2ToLegacy);
        let text = err.to_string();
        assert!(
            text.contains("Components V2") && text.contains("CV2-01"),
            "{text}"
        );
    }
}

#[test]
fn legacy_content_verbatim() {
    let long = "a".repeat(2001);
    for content in [String::new(), long] {
        let msg = LegacyMessage::new(content.clone());
        assert_eq!(msg.content_text(), content);
        assert_eq!(
            to_json(&msg.create_body(CreateFlags::empty()))["content"],
            json!(content)
        );
    }
}

#[test]
fn legacy_getters() {
    let m = Mentions::none().replied_user(true);
    let msg = LegacyMessage::new("hi").mentions(m.clone());
    assert_eq!(msg.content_text(), "hi");
    assert_eq!(msg.allowed_mentions(), &m);
    assert_eq!(msg.row_count(), 0);
    assert_eq!(
        to_json(&msg.create_body(CreateFlags::empty()))["allowed_mentions"],
        json!({"parse": [], "replied_user": true})
    );
}
