//! `derive(Modal)` and `derive(Choices)` end to end: the built modal (insta
//! snapshot) and typed submissions decoded from raw interaction JSON
//! (FR-29, FR-64, MOD-09).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    Mentionable, ResolvedAttachment, ResolvedChannel, ResolvedRole, ResolvedUser, SubmitError,
    validate,
};
use serde_json::{Value, json};
use twilight_model::application::interaction::modal::{
    ModalInteractionComponent, ModalInteractionData,
};

#[derive(Route)]
#[route("t.modal")]
struct Submit;

impl ::noctis::__private::ui::Handled for Submit {}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Colour {
    #[choice("Red", value = "r")]
    Red,
    #[choice("Green", value = "g", description = "Like grass")]
    Green,
    #[choice("Blue", value = "b")]
    Blue,
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    #[choice("Low", value = "l")]
    Low,
    #[choice("High", value = "h")]
    High,
}

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
enum Topic {
    #[choice("Rust")]
    Rust,
    #[choice("Art", value = "art")]
    Art,
    #[choice("Music")]
    Music,
}

/// Text Display, Text Input, String Select, Radio Group, Checkbox Group.
#[derive(Modal, Debug)]
#[modal(route = Submit, title = "Tell us")]
struct TextForm {
    #[text_display("Fill this in.")]
    _intro: (),
    #[text_input("Bio", style = paragraph, max_length = 300, placeholder = "About you")]
    bio: Option<String>,
    #[string_select("Colour", placeholder = "Pick one")]
    colour: Colour,
    #[radio_group("Severity", description = "How bad?")]
    severity: Severity,
    #[checkbox_group("Topics", max_values = 2)]
    topics: Vec<Topic>,
}

/// User, Role, Mentionable and Channel Selects.
#[derive(Modal, Debug)]
#[modal(route = Submit, title = "Pick people")]
struct EntityForm {
    #[user_select("Users", max_values = 3)]
    users: Vec<ResolvedUser>,
    #[role_select("Role")]
    role: ResolvedRole,
    #[mentionable_select("Pings")]
    pings: Vec<Mentionable>,
    #[channel_select("Channel", channel_types = [GuildText, GuildAnnouncement])]
    channel: Option<ResolvedChannel>,
}

/// File Uploads, Checkbox, Text Input and a multi String Select.
#[derive(Modal, Debug)]
#[modal(route = Submit, title = "Upload")]
struct UploadForm {
    #[file_upload("Evidence", max_values = 3, file_types = ["image", ".pdf"])]
    evidence: Vec<ResolvedAttachment>,
    #[file_upload("Avatar")]
    avatar: Option<ResolvedAttachment>,
    #[checkbox("I agree")]
    agree: bool,
    #[text_input("Title", min_length = 2, max_length = 5)]
    r#title: String,
    #[string_select("Colours")]
    colours: Vec<Colour>,
}

/// One required Text Input.
#[derive(Modal, Debug)]
#[modal(route = Submit, title = "Legacy")]
struct LegacyForm {
    #[text_input("Reason")]
    reason: String,
}

/// Every field optional.
#[derive(Modal, Debug)]
#[modal(route = Submit, title = "Optional")]
struct OptionalForm {
    #[text_input("Note")]
    note: Option<String>,
    #[radio_group("Pick")]
    pick: Option<Severity>,
    #[string_select("Colour")]
    colour: Option<Colour>,
    #[file_upload("File")]
    file: Option<ResolvedAttachment>,
    #[checkbox_group("Topics", min_values = 0)]
    topics: Vec<Topic>,
}

/// The same inputs, every field required.
#[derive(Modal, Debug)]
#[modal(route = Submit, title = "Required")]
struct RequiredForm {
    #[text_input("Note")]
    note: String,
    #[radio_group("Pick")]
    pick: Severity,
    #[string_select("Colour")]
    colour: Colour,
    #[file_upload("File")]
    file: ResolvedAttachment,
    #[checkbox_group("Topics")]
    topics: Vec<Topic>,
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/modal/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn submit(components: &Value) -> Vec<u8> {
    json!({"type": 5, "data": {"custom_id": "0t.modal:", "components": components}})
        .to_string()
        .into_bytes()
}

fn label(component: Value) -> Value {
    let mut label = serde_json::Map::new();
    label.insert("type".into(), 18.into());
    label.insert("component".into(), component);
    Value::Object(label)
}

fn modal_json(modal: noctis::ui::Modal) -> Value {
    let mut modal = modal;
    validate(&mut modal, &Limits::default()).expect("a derived modal validates");
    serde_json::to_value(&modal).unwrap()
}

#[test]
fn mod_02_derived_modals_snapshot() {
    let forms = json!({
        "text": modal_json(TextForm::modal(Submit)),
        "entity": modal_json(EntityForm::modal(Submit)),
        "upload": modal_json(UploadForm::modal(Submit)),
        "optional": modal_json(OptionalForm::modal(Submit)),
    });
    assert_eq!(
        forms["text"]["components"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(
        forms["upload"]["components"][3]["component"]["custom_id"],
        "title"
    );
    insta::assert_json_snapshot!("form", forms);
}

#[test]
fn mod_07_choices_derive() {
    use noctis::ui::Choices as _;
    assert_eq!(Colour::ALL, &[Colour::Red, Colour::Green, Colour::Blue]);
    assert_eq!(Colour::Green.label(), "Green");
    assert_eq!(Colour::Green.value(), "g");
    assert_eq!(Colour::Green.description(), Some("Like grass"));
    assert_eq!(Colour::from_value("b"), Some(Colour::Blue));
    assert_eq!(Colour::from_value("Blue"), None);
    assert_eq!(Topic::Rust.value(), "Rust");
}

#[test]
fn mod_09_decode_text_and_choices() {
    let form = TextForm::from_json(&fixture("text_and_choices")).unwrap();
    assert_eq!(form.bio.as_deref(), Some("Hello 😀"));
    assert_eq!(form.colour, Colour::Green);
    assert_eq!(form.severity, Severity::High);
    assert_eq!(form.topics, vec![Topic::Rust, Topic::Art]);
}

#[test]
fn mod_09_decode_entity_selects() {
    let form = EntityForm::from_json(&fixture("entity_selects")).unwrap();
    assert_eq!(form.users.len(), 2);
    assert_eq!(form.users[0].user.name, "ada");
    assert_eq!(
        form.users[0]
            .member
            .as_ref()
            .and_then(|m| m.nick.as_deref()),
        Some("ada-nick")
    );
    assert_eq!(form.users[1].user.name, "bob");
    assert!(form.users[1].member.is_none());
    assert_eq!(form.role.role.name, "Mods");
    assert!(matches!(&form.pings[0], Mentionable::User(u) if u.user.name == "bob"));
    assert!(matches!(&form.pings[1], Mentionable::Role(r) if r.role.name == "Mods"));
    assert_eq!(
        form.channel.map(|c| c.channel.name).as_deref(),
        Some("general")
    );
}

#[test]
fn mod_09_decode_uploads_and_checks() {
    let form = UploadForm::from_json(&fixture("uploads_and_checks")).unwrap();
    let names: Vec<&str> = form
        .evidence
        .iter()
        .map(ResolvedAttachment::filename)
        .collect();
    assert_eq!(names, ["one.png", "two.pdf"]);
    assert_eq!(form.evidence[1].content_type(), Some("application/pdf"));
    assert_eq!(form.avatar.as_ref().map(ResolvedAttachment::size), Some(7));
    assert!(form.agree);
    assert_eq!(form.r#title, "Bug");
    assert_eq!(form.colours, vec![Colour::Red, Colour::Blue]);
}

#[test]
fn mod_09_twilight_drops_what_noctis_decodes() {
    // FR-64: twilight-model 0.17.1 turns types 21-23 into `Unknown`.
    let twilight = |name: &str| {
        let raw: Value = serde_json::from_slice(&fixture(name)).unwrap();
        serde_json::from_value::<ModalInteractionData>(raw["data"].clone())
    };
    let data = twilight("text_and_choices").unwrap();
    let unknown: Vec<u8> = data
        .components
        .iter()
        .filter_map(|c| match c {
            ModalInteractionComponent::Label(l) => match *l.component {
                ModalInteractionComponent::Unknown(k) => Some(k),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(unknown, [21, 22]);
    // A Checkbox's boolean `value` makes twilight reject the whole payload.
    assert!(twilight("uploads_and_checks").is_err());
    let text = TextForm::from_json(&fixture("text_and_choices")).unwrap();
    assert_eq!(text.severity, Severity::High);
    assert_eq!(text.topics, vec![Topic::Rust, Topic::Art]);
    assert!(
        UploadForm::from_json(&fixture("uploads_and_checks"))
            .unwrap()
            .agree
    );
}

#[test]
fn mod_09_legacy_action_row_text_input() {
    let form = LegacyForm::from_json(&fixture("legacy_row")).unwrap();
    assert_eq!(form.reason, "legacy row");
}

#[test]
fn mod_09_optional_empties_are_none() {
    let form = OptionalForm::from_json(&fixture("optional_empty")).unwrap();
    assert_eq!(form.note, None);
    assert_eq!(form.pick, None);
    assert_eq!(form.colour, None);
    assert!(form.file.is_none());
    assert!(form.topics.is_empty());
    let absent = OptionalForm::from_json(&submit(&json!([]))).unwrap();
    assert_eq!(
        (absent.note, absent.pick, absent.colour),
        (None, None, None)
    );
    assert!(absent.topics.is_empty());
}

#[test]
fn mod_09_required_empties_and_absent_are_missing() {
    let missing = |key: &str| SubmitError::Missing { key: key.into() };
    let empty = fixture("optional_empty");
    let mut raw: Value = serde_json::from_slice(&empty).unwrap();
    let components = raw["data"]["components"].as_array_mut().unwrap();
    let decode = |cs: &[Value]| RequiredForm::from_json(&submit(&json!(cs)));
    assert_eq!(decode(&components[..1]).unwrap_err(), missing("note"));
    let ok_note = label(json!({"type": 4, "custom_id": "note", "value": "x"}));
    components[0] = ok_note;
    assert_eq!(decode(&components[..2]).unwrap_err(), missing("pick"));
    components[1] = label(json!({"type": 21, "custom_id": "pick", "value": "l"}));
    assert_eq!(decode(&components[..3]).unwrap_err(), missing("colour"));
    components[2] = label(json!({"type": 3, "custom_id": "colour", "values": ["r"]}));
    assert_eq!(decode(&components[..4]).unwrap_err(), missing("file"));
    assert_eq!(decode(&components[..3]).unwrap_err(), missing("file"));
    let agree = |cs: &Value| UploadForm::from_json(&submit(cs)).unwrap_err();
    assert_eq!(agree(&json!([])), missing("evidence"));
    let only_agree = json!([label(
        json!({"type": 23, "custom_id": "agree", "value": true})
    )]);
    let mut uploads: Value = serde_json::from_slice(&fixture("uploads_and_checks")).unwrap();
    uploads["data"]["components"][2] = json!({"type": 10});
    assert_eq!(
        UploadForm::from_json(uploads.to_string().as_bytes()).unwrap_err(),
        missing("agree")
    );
    assert_eq!(agree(&only_agree), missing("evidence"));

    let mut full: Value = serde_json::from_slice(&fixture("uploads_and_checks")).unwrap();
    full["data"]["components"] = json!([
        label(json!({"type": 4, "custom_id": "note", "value": "n"})),
        label(json!({"type": 21, "custom_id": "pick", "value": "l"})),
        label(json!({"type": 3, "custom_id": "colour", "values": ["g"]})),
        label(json!({"type": 19, "custom_id": "file", "values": ["1000000000000000031"]})),
        label(json!({"type": 22, "custom_id": "topics", "values": ["Music"]}))
    ]);
    let form = RequiredForm::from_json(full.to_string().as_bytes()).unwrap();
    assert_eq!(
        (
            form.note.as_str(),
            form.pick,
            form.colour,
            form.file.filename()
        ),
        ("n", Severity::Low, Colour::Green, "one.png")
    );
    assert_eq!(form.topics, vec![Topic::Music]);
    let mut no_topics = full.clone();
    no_topics["data"]["components"][4] = json!({"type": 10});
    assert_eq!(
        RequiredForm::from_json(no_topics.to_string().as_bytes()).unwrap_err(),
        missing("topics")
    );
}

#[test]
fn mod_09_wrong_type() {
    let err = OptionalForm::from_json(&submit(&json!([label(
        json!({"type": 3, "custom_id": "note", "values": []})
    )])))
    .unwrap_err();
    assert_eq!(
        err,
        SubmitError::WrongType {
            key: "note".into(),
            expected: 4,
            found: 3
        }
    );
    let err = LegacyForm::from_json(&submit(&json!([label(
        json!({"type": 99, "custom_id": "reason"})
    )])))
    .unwrap_err();
    assert!(matches!(
        err,
        SubmitError::WrongType {
            expected: 4,
            found: 99,
            ..
        }
    ));
}

#[test]
fn mod_09_ill_typed_values_are_invalid() {
    for component in [
        json!({"type": 4, "custom_id": "reason", "value": 5}),
        json!({"type": 3, "custom_id": "colour", "values": "x"}),
        json!({"type": 23, "custom_id": "agree", "value": "yes"}),
        json!({"type": 5, "custom_id": "users", "values": ["abc"]}),
        json!({"type": 5, "custom_id": "users", "values": ["0"]}),
    ] {
        let err = LegacyForm::from_json(&submit(&json!([label(component.clone())]))).unwrap_err();
        assert!(
            matches!(err, SubmitError::Invalid { .. }),
            "{component}: {err:?}"
        );
    }
}

#[test]
fn mod_09_unknown_choice_is_invalid() {
    let err = OptionalForm::from_json(&submit(&json!([label(
        json!({"type": 21, "custom_id": "pick", "value": "zzz"})
    )])))
    .unwrap_err();
    assert!(matches!(err, SubmitError::Invalid { key, .. } if key == "pick"));
}

#[test]
fn mod_09_unresolved_ids() {
    let mut raw: Value = serde_json::from_slice(&fixture("entity_selects")).unwrap();
    raw["data"]["resolved"]["users"]
        .as_object_mut()
        .unwrap()
        .remove("1000000000000000002");
    let err = EntityForm::from_json(raw.to_string().as_bytes()).unwrap_err();
    assert_eq!(
        err,
        SubmitError::Unresolved {
            key: "users".into(),
            id: 1_000_000_000_000_000_002
        }
    );

    let mut raw: Value = serde_json::from_slice(&fixture("entity_selects")).unwrap();
    raw["data"]["components"][2]["component"]["values"] = json!(["1000000000000000099"]);
    let err = EntityForm::from_json(raw.to_string().as_bytes()).unwrap_err();
    assert!(
        matches!(err, SubmitError::Unresolved { key, id: 1_000_000_000_000_000_099 } if key == "pings")
    );

    let mut raw: Value = serde_json::from_slice(&fixture("uploads_and_checks")).unwrap();
    raw["data"]["resolved"]["attachments"]
        .as_object_mut()
        .unwrap()
        .remove("1000000000000000033");
    let err = UploadForm::from_json(raw.to_string().as_bytes()).unwrap_err();
    assert!(matches!(err, SubmitError::Unresolved { key, .. } if key == "avatar"));
}

#[test]
fn mod_09_counts_and_lengths_out_of_bounds() {
    let base: Value = serde_json::from_slice(&fixture("uploads_and_checks")).unwrap();
    let with = |i: usize, component: Value| {
        let mut raw = base.clone();
        raw["data"]["components"][i]["component"] = component;
        UploadForm::from_json(raw.to_string().as_bytes())
    };
    let invalid =
        |r: Result<UploadForm, SubmitError>| matches!(r, Err(SubmitError::Invalid { .. }));
    // A single upload with two values.
    assert!(invalid(with(
        1,
        json!({"type": 19, "custom_id": "avatar", "values": ["1000000000000000031", "1000000000000000033"]})
    )));
    // Over max_values = 3.
    let four = json!([
        "1000000000000000031",
        "1000000000000000032",
        "1000000000000000033",
        "1000000000000000031"
    ]);
    assert!(invalid(with(
        0,
        json!({"type": 19, "custom_id": "evidence", "values": four})
    )));
    // Under the required minimum of 1.
    assert!(invalid(with(
        0,
        json!({"type": 19, "custom_id": "evidence", "values": []})
    )));
    // Text over max_length 5 (by discord_len: 3 emoji are 6) and under min_length 2.
    assert!(invalid(with(
        3,
        json!({"type": 4, "custom_id": "title", "value": "😀😀😀"})
    )));
    assert!(invalid(with(
        3,
        json!({"type": 4, "custom_id": "title", "value": "a"})
    )));
    assert!(with(3, json!({"type": 4, "custom_id": "title", "value": "😀😀"})).is_ok());
    // A String Select over its option count.
    assert!(invalid(with(
        4,
        json!({"type": 3, "custom_id": "colours", "values": ["r", "g", "b", "r"]})
    )));
}

#[test]
fn mod_09_unknown_components_are_kept() {
    let bytes = submit(&json!([
        label(json!({"type": 4, "custom_id": "reason", "value": "v"})),
        {"type": 99, "custom_id": "x", "foo": 1},
        label(json!({"type": 4, "custom_id": "x2", "value": "unclaimed"}))
    ]));
    let form = LegacyForm::from_json(&bytes).unwrap();
    assert_eq!(form.reason, "v");
    let submission = noctis::ui::ModalSubmission::from_slice(&bytes).unwrap();
    assert_eq!(
        submission.unknown(),
        &[json!({"type": 99, "custom_id": "x", "foo": 1})][..]
    );
    assert_eq!(
        submission.get("x2"),
        Some(&noctis::ui::SubmittedValue::TextInput("unclaimed".into()))
    );
}

#[test]
fn mod_09_duplicate_key() {
    let err = LegacyForm::from_json(&submit(&json!([
        label(json!({"type": 4, "custom_id": "reason", "value": "a"})),
        label(json!({"type": 4, "custom_id": "reason", "value": "b"}))
    ])))
    .unwrap_err();
    assert_eq!(
        err,
        SubmitError::Duplicate {
            key: "reason".into()
        }
    );
}

#[test]
fn mod_09_not_a_modal_submit_or_malformed() {
    let err = LegacyForm::from_json(br#"{"type": 3, "data": {}}"#).unwrap_err();
    assert_eq!(err, SubmitError::NotModalSubmit { kind: 3 });
    let malformed = |bytes: &[u8]| {
        matches!(
            LegacyForm::from_json(bytes),
            Err(SubmitError::Malformed { .. })
        )
    };
    assert!(malformed(b"not json"));
    assert!(malformed(b""));
    assert!(malformed(br#"{"type": 5}"#));
    assert!(malformed(
        br#"{"type": 5, "data": {"custom_id": "m", "components": 1}}"#
    ));
    assert!(malformed(&submit(&json!([label(
        json!({"type": 4, "value": "x"})
    )]))));
    let deep = format!(
        r#"{{"type": 5, "data": {{"custom_id": "m", "components": [{}{{"type": 4, "custom_id": "reason", "value": "v"}}{}]}}}}"#,
        r#"{"type": 18, "component": "#.repeat(200),
        "}".repeat(200)
    );
    assert!(malformed(deep.as_bytes()));
}

#[test]
fn mod_09_per_component_resolved_joins() {
    let mut raw: Value = serde_json::from_slice(&fixture("entity_selects")).unwrap();
    let resolved = raw["data"]["resolved"].take();
    raw["data"].as_object_mut().unwrap().remove("resolved");
    for i in 0..4 {
        raw["data"]["components"][i]["component"]["resolved"] = resolved.clone();
    }
    let form = EntityForm::from_json(raw.to_string().as_bytes()).unwrap();
    assert_eq!(form.users.len(), 2);
    assert_eq!(form.role.role.name, "Mods");
}

#[test]
fn mod_07_checkbox_group_min_values_zero() {
    let json = modal_json(OptionalForm::modal(Submit));
    let group = &json["components"][4]["component"];
    assert_eq!(group["min_values"], 0);
    assert_eq!(group["required"], false);
    let form = OptionalForm::from_json(&submit(&json!([label(
        json!({"type": 22, "custom_id": "topics", "values": []})
    )])))
    .unwrap();
    assert_eq!(form.topics, vec![]);
}

#[test]
fn cid_01_raw_ident_key_and_field_order() {
    let json = modal_json(UploadForm::modal(Submit));
    let keys: Vec<&str> = json["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["component"]["custom_id"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["evidence", "avatar", "agree", "title", "colours"]);
    assert_eq!(json["custom_id"], "0t.modal:");
}
