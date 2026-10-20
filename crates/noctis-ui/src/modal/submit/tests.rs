#![allow(clippy::unwrap_used, reason = "tests")]

use serde_json::{Value, json};

use super::*;

fn submit(components: &Value) -> Vec<u8> {
    json!({"type": 5, "data": {"custom_id": "0m:", "components": components}})
        .to_string()
        .into_bytes()
}

fn parse(components: &Value) -> Result<ModalSubmission, SubmitError> {
    ModalSubmission::from_slice(&submit(components))
}

fn malformed(bytes: &[u8]) -> bool {
    matches!(
        ModalSubmission::from_slice(bytes),
        Err(SubmitError::Malformed { .. })
    )
}

fn user(id: &str) -> Value {
    json!({"id": id, "username": "u", "discriminator": "0", "avatar": null})
}

#[test]
fn mod_09_label_wrapped_values_of_every_type() {
    let s = parse(&json!([
        {"type": 10, "id": 1, "content": "intro"},
        {"type": 18, "component": {"type": 4, "custom_id": "t", "value": "hi"}},
        {"type": 18, "component": {"type": 3, "custom_id": "s", "values": ["a", "b"]}},
        {"type": 18, "component": {"type": 5, "custom_id": "u", "values": ["11"]}},
        {"type": 18, "component": {"type": 6, "custom_id": "r", "values": ["12"]}},
        {"type": 18, "component": {"type": 7, "custom_id": "m", "values": ["13"]}},
        {"type": 18, "component": {"type": 8, "custom_id": "c", "values": ["14"]}},
        {"type": 18, "component": {"type": 19, "custom_id": "f", "values": ["15"]}},
        {"type": 18, "component": {"type": 21, "custom_id": "rg", "value": "x"}},
        {"type": 18, "component": {"type": 22, "custom_id": "cg", "values": ["y"]}},
        {"type": 18, "component": {"type": 23, "custom_id": "cb", "value": false}}
    ]))
    .unwrap();
    assert_eq!(s.custom_id(), "0m:");
    assert_eq!(s.get("t"), Some(&SubmittedValue::TextInput("hi".into())));
    assert_eq!(
        s.get("s"),
        Some(&SubmittedValue::StringSelect(vec!["a".into(), "b".into()]))
    );
    assert_eq!(
        s.get("u"),
        Some(&SubmittedValue::UserSelect(vec![Id::new(11)]))
    );
    assert_eq!(
        s.get("r"),
        Some(&SubmittedValue::RoleSelect(vec![Id::new(12)]))
    );
    assert_eq!(
        s.get("m"),
        Some(&SubmittedValue::MentionableSelect(vec![Id::new(13)]))
    );
    assert_eq!(
        s.get("c"),
        Some(&SubmittedValue::ChannelSelect(vec![Id::new(14)]))
    );
    assert_eq!(
        s.get("f"),
        Some(&SubmittedValue::FileUpload(vec![Id::new(15)]))
    );
    assert_eq!(
        s.get("rg"),
        Some(&SubmittedValue::RadioGroup(Some("x".into())))
    );
    assert_eq!(
        s.get("cg"),
        Some(&SubmittedValue::CheckboxGroup(vec!["y".into()]))
    );
    assert_eq!(s.get("cb"), Some(&SubmittedValue::Checkbox(false)));
    assert!(s.unknown().is_empty());
}

#[test]
fn mod_09_legacy_action_row_text_input() {
    let s = parse(&json!([
        {"type": 1, "components": [{"type": 4, "custom_id": "bio", "value": "x"}]}
    ]))
    .unwrap();
    assert_eq!(s.get("bio"), Some(&SubmittedValue::TextInput("x".into())));
}

#[test]
fn mod_09_optional_empties() {
    let s = parse(&json!([
        {"type": 18, "component": {"type": 4, "custom_id": "t", "value": ""}},
        {"type": 18, "component": {"type": 21, "custom_id": "r", "value": null}},
        {"type": 18, "component": {"type": 21, "custom_id": "r2"}},
        {"type": 18, "component": {"type": 3, "custom_id": "s", "values": []}}
    ]))
    .unwrap();
    assert_eq!(s.get("t"), Some(&SubmittedValue::TextInput(String::new())));
    assert_eq!(s.get("r"), Some(&SubmittedValue::RadioGroup(None)));
    assert_eq!(s.get("r2"), Some(&SubmittedValue::RadioGroup(None)));
    assert_eq!(s.get("s"), Some(&SubmittedValue::StringSelect(vec![])));
}

#[test]
fn mod_09_ill_typed_values_are_invalid() {
    for component in [
        json!({"type": 4, "custom_id": "k", "value": 5}),
        json!({"type": 4, "custom_id": "k"}),
        json!({"type": 3, "custom_id": "k", "values": "x"}),
        json!({"type": 3, "custom_id": "k", "values": [1]}),
        json!({"type": 23, "custom_id": "k", "value": "yes"}),
        json!({"type": 21, "custom_id": "k", "value": 1}),
        json!({"type": 5, "custom_id": "k", "values": ["abc"]}),
        json!({"type": 5, "custom_id": "k", "values": ["0"]}),
        json!({"type": 19, "custom_id": "k", "values": ["-1"]}),
    ] {
        let err = parse(&json!([{"type": 18, "component": component}])).unwrap_err();
        assert!(
            matches!(&err, SubmitError::Invalid { key, .. } if key == "k"),
            "{component}: {err:?}"
        );
    }
}

#[test]
fn mod_09_unknown_components_are_kept_verbatim() {
    let odd = json!({"type": 99, "custom_id": "x", "foo": 1});
    let s = parse(&json!([
        {"type": 18, "component": odd},
        "text",
        {"no_type": true},
        {"type": 18, "component": {"type": 4, "custom_id": "x2", "value": "v"}}
    ]))
    .unwrap();
    assert_eq!(
        s.unknown(),
        &[odd, json!("text"), json!({"no_type": true})][..]
    );
    assert_eq!(s.unknown_type_of("x"), Some(99));
    assert_eq!(s.get("x2"), Some(&SubmittedValue::TextInput("v".into())));
}

#[test]
fn mod_09_duplicate_key() {
    let err = parse(&json!([
        {"type": 18, "component": {"type": 4, "custom_id": "bio", "value": "a"}},
        {"type": 18, "component": {"type": 4, "custom_id": "bio", "value": "b"}}
    ]))
    .unwrap_err();
    assert_eq!(err, SubmitError::Duplicate { key: "bio".into() });
}

#[test]
fn mod_09_not_a_modal_submit() {
    let err = ModalSubmission::from_slice(br#"{"type": 3, "data": {}}"#).unwrap_err();
    assert_eq!(err, SubmitError::NotModalSubmit { kind: 3 });
}

#[test]
fn mod_09_malformed_payloads_never_panic() {
    assert!(malformed(b"not json"));
    assert!(malformed(b""));
    assert!(malformed(b"[]"));
    assert!(malformed(br#"{"type": "5"}"#));
    assert!(malformed(br#"{"type": 5}"#));
    assert!(malformed(
        br#"{"type": 5, "data": {"custom_id": "m", "components": {}}}"#
    ));
    assert!(malformed(br#"{"type": 5, "data": {"components": []}}"#));
    assert!(malformed(&submit(&json!([
        {"type": 18, "component": {"type": 4, "value": "x"}}
    ]))));
    assert!(malformed(&submit(&json!([{"type": 18}]))));
    assert!(malformed(&submit(&json!([{"type": 1, "components": 3}]))));
    assert!(malformed(
        br#"{"type": 5, "data": {"custom_id": "m", "components": [], "resolved": {"users": 1}}}"#
    ));
    let deep = format!(
        r#"{{"type": 5, "data": {{"custom_id": "m", "components": [{}{{"type": 4, "custom_id": "k", "value": "v"}}{}]}}}}"#,
        r#"{"type": 18, "component": "#.repeat(200),
        "}".repeat(200)
    );
    assert!(malformed(deep.as_bytes()));
}

#[test]
fn mod_09_per_component_resolved_joins_data_resolved() {
    let bytes = json!({"type": 5, "data": {
        "custom_id": "m",
        "resolved": {"users": {"11": user("11")}},
        "components": [{"type": 18, "component": {
            "type": 5, "custom_id": "u", "values": ["11", "12"],
            "resolved": {"users": {"12": user("12")}}
        }}]
    }})
    .to_string();
    let s = ModalSubmission::from_slice(bytes.as_bytes()).unwrap();
    assert_eq!(s.resolved().users.len(), 2);
    assert!(s.resolved().users.contains_key(&Id::new(12)));
}
