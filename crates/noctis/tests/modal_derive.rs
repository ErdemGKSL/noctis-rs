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

#[derive(::noctis::Button)]
#[button("t.modal", defaults(style = Secondary, label = "Go"))]
struct Submit;

impl Submit {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ComponentCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

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
#[modal("t.modal", defaults(title = "Tell us"))]
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

impl TextForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

/// User, Role, Mentionable and Channel Selects.
#[derive(Modal, Debug)]
#[modal("t.modal", defaults(title = "Pick people"))]
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

impl EntityForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

/// File Uploads, Checkbox, Text Input and a multi String Select.
#[derive(Modal, Debug)]
#[modal("t.modal", defaults(title = "Upload"))]
struct UploadForm {
    #[file_upload("Evidence", max_values = 3, file_types = ["image", ".pdf"])]
    evidence: Vec<ResolvedAttachment>,
    #[file_upload("Avatar")]
    avatar: Option<ResolvedAttachment>,
    #[checkbox("I agree")]
    agree: bool,
    #[text_input("Title", min_length = 2, max_length = 5)]
    r#type: String,
    #[string_select("Colours")]
    colours: Vec<Colour>,
}

impl UploadForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

/// One required Text Input.
#[derive(Modal, Debug)]
#[modal("t.modal", defaults(title = "Legacy"))]
struct LegacyForm {
    #[text_input("Reason")]
    reason: String,
}

impl LegacyForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

/// Every field optional.
#[derive(Modal, Debug)]
#[modal("t.modal", defaults(title = "Optional"))]
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

impl OptionalForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

/// The same inputs, every field required.
#[derive(Modal, Debug)]
#[modal("t.modal", defaults(title = "Required"))]
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

impl RequiredForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
}

/// A File Upload `Vec` without `max_values`: Discord's default of 10.
#[derive(Modal, Debug)]
#[modal("t.modal", defaults(title = "Files"))]
struct FilesForm {
    #[file_upload("Files")]
    files: Vec<ResolvedAttachment>,
}

impl FilesForm {
    #[::noctis::handler]
    async fn handle(
        self,
        _ctx: ::noctis::ModalSubmitCtx,
    ) -> ::noctis::HandlerResult<::noctis::Responded> {
        Ok(::noctis::__private::core::__testing::responded())
    }
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

fn modal_json(modal: impl Into<Modal>) -> Value {
    let mut modal = modal.into();
    validate(&mut modal, &Limits::default()).expect("a derived modal validates");
    serde_json::to_value(&modal).unwrap()
}

#[test]
fn mod_02_derived_modals_snapshot() {
    let forms = json!({
        "text": modal_json(TextForm::blank()),
        "entity": modal_json(EntityForm::blank()),
        "upload": modal_json(UploadForm::blank()),
        "optional": modal_json(OptionalForm::blank()),
    });
    assert_eq!(
        forms["text"]["components"].as_array().map(Vec::len),
        Some(5)
    );
    assert_eq!(
        forms["upload"]["components"][3]["component"]["custom_id"],
        "type"
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
    assert_eq!(form.r#type, "Bug");
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
        json!({"type": 4, "custom_id": "type", "value": "😀😀😀"})
    )));
    assert!(invalid(with(
        3,
        json!({"type": 4, "custom_id": "type", "value": "a"})
    )));
    assert!(with(3, json!({"type": 4, "custom_id": "type", "value": "😀😀"})).is_ok());
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
    let json = modal_json(OptionalForm::blank());
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
    let json = modal_json(UploadForm::blank());
    let keys: Vec<&str> = json["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["component"]["custom_id"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["evidence", "avatar", "agree", "type", "colours"]);
    assert_eq!(json["custom_id"], "0t.modal:");
}

#[test]
fn mod_09_file_upload_default_max_values_is_ten() {
    let built = modal_json(FilesForm::blank());
    assert_eq!(built["components"][0]["component"]["max_values"], 10);

    let files = |n: u64| {
        let ids: Vec<String> = (1..=n)
            .map(|i| format!("{}", 1_000_000_000_000_000_100 + i))
            .collect();
        let attachments: serde_json::Map<String, Value> = ids
            .iter()
            .map(|id| {
                let url = format!("https://cdn.discordapp.com/ephemeral-attachments/1/{id}/f.png");
                (
                    id.clone(),
                    json!({"id": id, "filename": "f.png", "size": 1, "url": url, "proxy_url": url}),
                )
            })
            .collect();
        json!({"type": 5, "data": {
            "custom_id": "0t.modal:",
            "components": [label(json!({"type": 19, "custom_id": "files", "values": ids}))],
            "resolved": {"attachments": attachments}
        }})
        .to_string()
        .into_bytes()
    };
    let form = FilesForm::from_json(&files(10)).unwrap();
    assert_eq!(form.files.len(), 10);
    let err = FilesForm::from_json(&files(11)).unwrap_err();
    assert!(
        matches!(&err, SubmitError::Invalid { key, .. } if key == "files"),
        "{err:?}"
    );
}

// Story 9.3: pre-fill, `blank`, and per-render modal overrides.

use noctis::rules::{LimitMode, MessageKey, RuleId};
use noctis::ui::{InputOverrides, Modal, ModalBuilder, UiError};

/// Validates `modal` in `mode` and returns its JSON, or the violated rules.
fn checked(modal: impl Into<Modal>, mode: LimitMode) -> Result<Value, Vec<RuleId>> {
    let mut modal = modal.into();
    let mut limits = Limits::default();
    limits.mode = mode;
    match validate(&mut modal, &limits) {
        Ok(_) => Ok(serde_json::to_value(&modal).unwrap()),
        Err(UiError::Limits { violations }) => Err(violations.iter().map(|v| v.rule).collect()),
        Err(other) => panic!("{other}"),
    }
}

/// The inner component of the `i`-th Label.
fn input(json: &Value, i: usize) -> &Value {
    &json["components"][i]["component"]
}

fn defaults(component: &Value) -> Vec<&str> {
    component["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["default"] == json!(true))
        .filter_map(|o| o["value"].as_str())
        .collect()
}

fn text_form() -> TextForm {
    TextForm {
        _intro: (),
        bio: Some("hi".into()),
        colour: Colour::Green,
        severity: Severity::High,
        topics: vec![Topic::Rust, Topic::Music],
    }
}

/// M1, M3, M4: text, radio, string select and checkbox group pre-fill;
/// the pre-filled `TextForm` snapshot.
#[test]
fn m1_m4_prefill_text_and_choices() {
    let json = checked(text_form().build(), LimitMode::Strict).unwrap();
    assert_eq!(
        json["components"][0]["type"], 10,
        "the Text Display is unchanged"
    );
    assert_eq!(input(&json, 1)["value"], "hi");
    assert_eq!(defaults(input(&json, 2)), ["g"]);
    assert_eq!(defaults(input(&json, 3)), ["h"]);
    assert_eq!(defaults(input(&json, 4)), ["Rust", "Music"]);
    insta::assert_json_snapshot!("prefill_text", json);
    // The decode is unchanged (M19): the custom_id is the blank one.
    let blank = checked(TextForm::blank(), LimitMode::Strict).unwrap();
    assert_eq!(json["custom_id"], blank["custom_id"]);
}

/// M2, M3: empty values send nothing.
#[test]
fn m2_m3_empty_values_send_nothing() {
    let none = checked(
        OptionalForm {
            note: None,
            pick: None,
            colour: None,
            file: None,
            topics: vec![],
        },
        LimitMode::Strict,
    )
    .unwrap();
    assert!(input(&none, 0).get("value").is_none());
    for i in 1..5 {
        assert!(defaults(input(&none, i)).is_empty(), "{i}");
    }
    let empty = checked(
        OptionalForm {
            note: Some(String::new()),
            ..OptionalForm::blank()
        },
        LimitMode::Strict,
    )
    .unwrap();
    assert!(input(&empty, 0).get("value").is_none());
    let legacy = checked(
        LegacyForm {
            reason: String::new(),
        }
        .build(),
        LimitMode::Strict,
    )
    .unwrap();
    assert!(input(&legacy, 0).get("value").is_none());
    assert_eq!(
        none,
        checked(OptionalForm::blank(), LimitMode::Strict).unwrap()
    );
}

/// M3, M6: a `true` Checkbox starts checked, File Uploads get no
/// defaults; the pre-filled `UploadForm` snapshot.
#[test]
fn m3_m6_checkbox_and_uploads() {
    let form = UploadForm::from_json(&fixture("uploads_and_checks")).unwrap();
    assert!(form.agree);
    let json = checked(form, LimitMode::Strict).unwrap();
    for i in 0..2 {
        let upload = input(&json, i);
        assert_eq!(upload["type"], 19);
        assert!(upload.get("default_values").is_none() && upload.get("value").is_none());
    }
    assert_eq!(input(&json, 2)["default"], true);
    assert_eq!(input(&json, 3)["value"], "Bug");
    insta::assert_json_snapshot!("prefill_upload", json);
    let unchecked = checked(UploadForm::blank(), LimitMode::Strict).unwrap();
    assert!(input(&unchecked, 2).get("default").is_none());
}

/// M5: entity selects pre-fill their ids with their kinds; the
/// pre-filled `EntityForm` snapshot.
#[test]
fn m5_prefill_entities() {
    let form = EntityForm::from_json(&fixture("entity_selects")).unwrap();
    let json = checked(form, LimitMode::Strict).unwrap();
    assert_eq!(
        input(&json, 0)["default_values"],
        json!([
            {"id": "1000000000000000001", "type": "user"},
            {"id": "1000000000000000002", "type": "user"}
        ])
    );
    assert_eq!(
        input(&json, 1)["default_values"],
        json!([{"id": "1000000000000000010", "type": "role"}])
    );
    assert_eq!(
        input(&json, 2)["default_values"],
        json!([
            {"id": "1000000000000000002", "type": "user"},
            {"id": "1000000000000000010", "type": "role"}
        ])
    );
    assert_eq!(
        input(&json, 3)["default_values"],
        json!([{"id": "1000000000000000020", "type": "channel"}])
    );
    insta::assert_json_snapshot!("prefill_entity", json);
}

/// M7, M8: `blank` is `Self` when every input has an empty value, else a
/// `ModalBuilder`.
#[test]
fn m7_m8_blank_kinds() {
    let blank: OptionalForm = OptionalForm::blank();
    assert!(blank.note.is_none() && blank.topics.is_empty());
    let upload: UploadForm = UploadForm::blank();
    assert!(!upload.agree && upload.r#type.is_empty() && upload.evidence.is_empty());
    let _: LegacyForm = LegacyForm::blank();
    let _: ModalBuilder<EntityForm> = EntityForm::blank();
    let _: ModalBuilder<TextForm> = TextForm::blank();
    let _: ModalBuilder<RequiredForm> = RequiredForm::blank();
    let note = OptionalForm {
        note: Some("n".into()),
        ..OptionalForm::blank()
    };
    let json = checked(note, LimitMode::Strict).unwrap();
    assert_eq!(input(&json, 0)["value"], "n");
}

/// M9: the title override and its MOD-01 bound.
#[test]
fn m9_title_override() {
    let json = checked(TextForm::blank().title("x"), LimitMode::Strict).unwrap();
    assert_eq!(json["title"], "x");
    assert!(checked(TextForm::blank().title("t".repeat(45)), LimitMode::Strict).is_ok());
    assert_eq!(
        checked(TextForm::blank().title("t".repeat(46)), LimitMode::Strict).unwrap_err(),
        [RuleId::MOD_01]
    );
    let lenient = checked(TextForm::blank().title("t".repeat(46)), LimitMode::Lenient).unwrap();
    assert_eq!(
        lenient["title"].as_str().map(|t| t.chars().count()),
        Some(45)
    );
    // An unresolved key serializes as the key; core resolves it before sending.
    let key = Modal::from(TextForm::blank().title(MessageKey::new("form.title")));
    assert_eq!(serde_json::to_value(&key).unwrap()["title"], "form.title");
}

/// M10: `.value(X::FIELD, v)` replaces one pre-fill; the later call wins.
#[test]
fn m10_value_handles() {
    let json = checked(
        text_form().build().value(TextForm::BIO, Some("new".into())),
        LimitMode::Strict,
    )
    .unwrap();
    assert_eq!(input(&json, 1)["value"], "new");
    let json = checked(
        TextForm::blank()
            .value(TextForm::BIO, Some("one".into()))
            .value(TextForm::TOPICS, vec![Topic::Art])
            .value(TextForm::BIO, Some("two".into())),
        LimitMode::Strict,
    )
    .unwrap();
    assert_eq!(input(&json, 1)["value"], "two");
    assert_eq!(defaults(input(&json, 4)), ["art"]);
    let cleared = checked(
        text_form().build().value(TextForm::BIO, None),
        LimitMode::Strict,
    )
    .unwrap();
    assert!(input(&cleared, 1).get("value").is_none());
}

/// M11, M12: input overrides change the Label only and merge field by
/// field; the title override composes with `.title`.
#[test]
fn m11_m12_input_overrides() {
    let json = checked(
        text_form().build().overrides(TextFormOverrides {
            bio: InputOverrides {
                label: Some("About".into()),
                ..Default::default()
            },
            ..Default::default()
        }),
        LimitMode::Strict,
    )
    .unwrap();
    assert_eq!(json["components"][1]["label"], "About");
    assert_eq!(input(&json, 1)["value"], "hi");
    assert_eq!(json["components"][2]["label"], "Colour");
    let merged = checked(
        TextForm::blank()
            .overrides(TextFormOverrides {
                bio: InputOverrides {
                    label: Some("a".into()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .overrides(TextFormOverrides {
                bio: InputOverrides {
                    description: Some("d".into()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .title("a")
            .overrides(TextFormOverrides {
                title: Some("b".into()),
                ..Default::default()
            }),
        LimitMode::Strict,
    )
    .unwrap();
    assert_eq!(merged["components"][1]["label"], "a");
    assert_eq!(merged["components"][1]["description"], "d");
    assert_eq!(merged["title"], "b");
}

fn bio(o: InputOverrides) -> TextFormOverrides {
    TextFormOverrides {
        bio: o,
        ..Default::default()
    }
}

/// Builds the overrides of one bound case from a text length.
type MakeOverrides = fn(usize) -> TextFormOverrides;

/// M13: override bounds; a placeholder on a radio is ignored.
#[test]
fn m13_override_bounds() {
    let with = |o: TextFormOverrides, mode| checked(TextForm::blank().overrides(o), mode);
    let cases: [(MakeOverrides, usize, RuleId); 4] = [
        (
            |n| {
                bio(InputOverrides {
                    label: Some("l".repeat(n).into()),
                    ..Default::default()
                })
            },
            45,
            RuleId::MOD_04,
        ),
        (
            |n| {
                bio(InputOverrides {
                    description: Some("d".repeat(n).into()),
                    ..Default::default()
                })
            },
            100,
            RuleId::MOD_04,
        ),
        (
            |n| {
                bio(InputOverrides {
                    placeholder: Some("p".repeat(n).into()),
                    ..Default::default()
                })
            },
            100,
            RuleId::MOD_05,
        ),
        (
            |n| TextFormOverrides {
                colour: InputOverrides {
                    placeholder: Some("p".repeat(n).into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            150,
            RuleId::CV2_09,
        ),
    ];
    for (make, max, rule) in cases {
        assert!(
            with(make(max), LimitMode::Strict).is_ok(),
            "{rule} at {max}"
        );
        assert_eq!(with(make(max + 1), LimitMode::Strict).unwrap_err(), [rule]);
        assert!(
            with(make(max + 1), LimitMode::Lenient).is_ok(),
            "{rule} truncates"
        );
    }
    let radio = with(
        TextFormOverrides {
            severity: InputOverrides {
                placeholder: Some("p".into()),
                ..Default::default()
            },
            ..Default::default()
        },
        LimitMode::Strict,
    )
    .unwrap();
    assert!(input(&radio, 3).get("placeholder").is_none());
}

/// M14, M15: a value over 4000, or over the input's `max_length`, is
/// MOD-05 in both modes.
#[test]
fn m14_m15_value_bounds() {
    for mode in [LimitMode::Strict, LimitMode::Lenient] {
        let legacy = |n: usize| {
            checked(
                LegacyForm {
                    reason: "r".repeat(n),
                },
                mode,
            )
        };
        assert!(legacy(4000).is_ok());
        assert_eq!(legacy(4001).unwrap_err(), [RuleId::MOD_05]);
        let bio =
            |text: String| checked(text_form().build().value(TextForm::BIO, Some(text)), mode);
        assert!(bio("b".repeat(300)).is_ok());
        assert!(bio("ü".repeat(300)).is_ok());
        assert_eq!(bio("b".repeat(301)).unwrap_err(), [RuleId::MOD_05]);
    }
}

/// M16: more Checkbox Group defaults than `max_values` is MOD-07.
#[test]
fn m16_too_many_checkbox_defaults() {
    for mode in [LimitMode::Strict, LimitMode::Lenient] {
        let three = TextForm {
            topics: vec![Topic::Rust, Topic::Art, Topic::Music],
            ..text_form()
        };
        assert_eq!(checked(three, mode).unwrap_err(), [RuleId::MOD_07]);
    }
}
