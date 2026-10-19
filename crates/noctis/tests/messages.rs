//! Messages built through the facade only: a V2 create body around a
//! derived Route, a legacy message at the 5-row boundary, and a
//! legacy-to-V2 edit (FR-23, CV2-01, CV2-02, CV2-15, MSG-05).

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Components, Container, CreateFlags, EditFlags, LegacyMessage, LinkButton,
    MessageFormat, TextDisplay, UiError, V2Message,
};
use serde_json::json;

#[derive(Route)]
#[route("m.ok")]
struct Confirm;

impl ::noctis::__private::ui::Handled for Confirm {}

fn body() -> Components {
    Components::new().push(
        Container::new()
            .push(TextDisplay::new("Done"))
            .push(ActionRow::buttons([Confirm.button().label("OK")])),
    )
}

#[test]
fn v2_create_body() {
    let msg = V2Message::new(body());
    let json = serde_json::to_value(msg.create_body(CreateFlags::empty())).expect("serializes");
    assert_eq!(
        json,
        json!({
            "flags": 32768,
            "components": [{
                "type": 17,
                "id": 1,
                "components": [
                    {"type": 10, "id": 2, "content": "Done"},
                    {"type": 1, "id": 3, "components": [
                        {"type": 2, "id": 4, "style": 2, "label": "OK", "custom_id": "0m.ok:"}
                    ]}
                ]
            }],
            "allowed_mentions": {"parse": []}
        })
    );
}

#[test]
fn cv2_15_legacy_five_rows() {
    let row = || ActionRow::buttons([LinkButton::new("https://a.b").label("A")]);
    let msg = LegacyMessage::new("x").rows([row(), row(), row(), row(), row()]);
    assert_eq!(msg.row_count(), 5);
    let json = serde_json::to_value(msg.create_body(CreateFlags::empty())).expect("serializes");
    assert_eq!(json["components"].as_array().map(Vec::len), Some(5));
    assert_eq!(
        msg.edit_body(MessageFormat::V2, EditFlags::empty())
            .expect_err("V2 to legacy"),
        UiError::V2ToLegacy
    );
}

#[test]
fn cv2_02_legacy_to_v2_edit() {
    let msg = V2Message::new(body());
    let json = serde_json::to_string(&msg.edit_body(MessageFormat::Legacy, EditFlags::empty()))
        .expect("serializes");
    assert!(
        json.starts_with(
            r#"{"flags":32768,"content":null,"embeds":[],"poll":null,"sticker_ids":[],"components":["#
        ),
        "{json}"
    );
    assert!(
        json.ends_with(r#""allowed_mentions":{"parse":[]}}"#),
        "{json}"
    );
}
