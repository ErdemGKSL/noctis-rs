//! Modal boundary passes (MOD-*, CID-01, CV2-09): runtime-built and derived
//! modals exactly at a limit compile and validate through
//! `noctis::ui::validate`.

use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    CheckboxGroup, FileUpload, Modal, ModalInput, ModalLabel, ModalOption, RadioGroup, TextInput,
    TextInputStyle, validate,
};

#[derive(Route)]
#[route("b.form")]
struct Form;

impl ::noctis::__private::ui::Handled for Form {}

fn a(n: usize) -> String {
    "a".repeat(n)
}

fn short(key: impl Into<String>) -> TextInput {
    TextInput::new(key.into(), TextInputStyle::Short)
}

fn one(input: impl ModalInput) -> Modal {
    Form.modal("T").push(ModalLabel::new("L", input))
}

fn opts(n: usize) -> Vec<ModalOption> {
    (0..n)
        .map(|i| ModalOption::new(format!("o{i}"), format!("v{i}")))
        .collect()
}

fn passes(mut modal: Modal) -> usize {
    match validate(&mut modal, &Limits::default()) {
        Ok(report) => report.components,
        Err(e) => panic!("expected a pass, got {e}"),
    }
}

#[test]
fn mod_02_exactly_five_top_level() {
    let modal = (0..5).fold(Form.modal("T"), |m, i| {
        m.push(ModalLabel::new("L", short(format!("k{i}"))))
    });
    assert_eq!(passes(modal), 10);
}

#[test]
fn mod_01_title_45() {
    passes(Form.modal(a(45)).push(ModalLabel::new("L", short("k"))));
}

#[test]
fn mod_04_label_45_description_100() {
    let label = ModalLabel::new(a(45), short("k")).description(a(100));
    passes(Form.modal("T").push(label));
}

#[test]
fn mod_05_text_input_bounds() {
    passes(one(short("k")
        .min_length(0)
        .max_length(4000)
        .value(a(4000))
        .placeholder(a(100))));
}

#[test]
fn mod_06_file_upload_ten_values_ten_types() {
    let types = ["image", "video", "audio"]
        .into_iter()
        .map(String::from)
        .chain((0..7).map(|i| format!(".e{i}")));
    passes(one(FileUpload::new("f").max_values(10).file_types(types)));
}

#[test]
fn mod_07_radio_two_and_ten_options() {
    passes(one(RadioGroup::new("r", opts(2))));
    passes(one(RadioGroup::new("r", opts(10))));
}

#[test]
fn mod_07_checkbox_group_one_and_ten_options() {
    passes(one(CheckboxGroup::new("g", opts(1))));
    passes(one(CheckboxGroup::new("g", opts(10))));
}

#[test]
fn mod_07_option_label_and_value_100() {
    let options = [ModalOption::new(a(100), a(100)), ModalOption::new("b", "b")];
    passes(one(RadioGroup::new("r", options)));
}

#[test]
fn cid_01_key_100() {
    passes(one(short(a(100))));
}

// derive(Modal) / derive(Choices) boundary passes: literals exactly at a
// limit compile, and the built modal validates.

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Two {
    #[choice("V0")]
    V0,
    #[choice("V1")]
    V1,
}

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Ten {
    #[choice("V0")]
    V0,
    #[choice("V1")]
    V1,
    #[choice("V2")]
    V2,
    #[choice("V3")]
    V3,
    #[choice("V4")]
    V4,
    #[choice("V5")]
    V5,
    #[choice("V6")]
    V6,
    #[choice("V7")]
    V7,
    #[choice("V8")]
    V8,
    #[choice("V9")]
    V9,
}

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum One {
    #[choice("V0")]
    V0,
}

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum TwentyFive {
    #[choice("V0")]
    V0,
    #[choice("V1")]
    V1,
    #[choice("V2")]
    V2,
    #[choice("V3")]
    V3,
    #[choice("V4")]
    V4,
    #[choice("V5")]
    V5,
    #[choice("V6")]
    V6,
    #[choice("V7")]
    V7,
    #[choice("V8")]
    V8,
    #[choice("V9")]
    V9,
    #[choice("V10")]
    V10,
    #[choice("V11")]
    V11,
    #[choice("V12")]
    V12,
    #[choice("V13")]
    V13,
    #[choice("V14")]
    V14,
    #[choice("V15")]
    V15,
    #[choice("V16")]
    V16,
    #[choice("V17")]
    V17,
    #[choice("V18")]
    V18,
    #[choice("V19")]
    V19,
    #[choice("V20")]
    V20,
    #[choice("V21")]
    V21,
    #[choice("V22")]
    V22,
    #[choice("V23")]
    V23,
    #[choice("V24")]
    V24,
}

#[derive(Choices, Clone, Copy, Debug, PartialEq)]
enum Long {
    #[choice(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        value = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        description = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )]
    A,
    #[choice("B")]
    B,
}

#[derive(Modal)]
#[allow(dead_code, reason = "boundary passes only build the modal")]
#[modal(route = Form, title = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
struct FiveFields {
    #[text_display(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )]
    intro: (),
    #[text_input(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        description = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        min_length = 0,
        max_length = 4000,
        placeholder = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    )]
    text: String,
    #[file_upload("Files", max_values = 10, file_types = ["image", "video", "audio", ".a", ".b", ".c", ".d", ".e", ".f", ".g"])]
    files: Vec<noctis::ui::ResolvedAttachment>,
    #[radio_group("Two")]
    two: Two,
    #[radio_group("Ten")]
    ten: Ten,
}

#[derive(Modal)]
#[allow(dead_code, reason = "boundary passes only build the modal")]
#[modal(route = Form, title = "T")]
struct Groups {
    #[checkbox_group("One")]
    one: Vec<One>,
    #[checkbox_group("Ten", max_values = 10)]
    ten: Vec<Ten>,
    #[string_select(
        "Many",
        placeholder = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        max_values = 25
    )]
    many: Vec<TwentyFive>,
    #[radio_group("Long")]
    long: Long,
    #[checkbox("Key")]
    kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk:
        bool,
}

#[test]
fn mod_02_derive_five_fields_at_every_limit() {
    assert_eq!(passes(FiveFields::modal(Form)), 9);
}

#[test]
fn mod_01_derive_title_45_and_mod_04_label_45() {
    assert_eq!(FiveFields::modal(Form).title().chars().count(), 45);
}

#[test]
fn mod_05_derive_text_input_bounds_and_mod_06_file_upload() {
    let json = serde_json::to_value(FiveFields::modal(Form)).unwrap();
    assert_eq!(json["components"][1]["component"]["max_length"], 4000);
    assert_eq!(
        json["components"][2]["component"]["file_types"]
            .as_array()
            .map(Vec::len),
        Some(10)
    );
}

#[test]
fn mod_07_derive_radio_2_10_checkbox_1_10_and_choice_text_100() {
    passes(Groups::modal(Form));
    let json = serde_json::to_value(Groups::modal(Form)).unwrap();
    assert_eq!(
        json["components"][0]["component"]["options"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        json["components"][1]["component"]["options"]
            .as_array()
            .map(Vec::len),
        Some(10)
    );
    assert_eq!(
        json["components"][3]["component"]["options"][0]["value"]
            .as_str()
            .map(str::len),
        Some(100)
    );
}

#[test]
fn cv2_09_derive_string_select_25_options_placeholder_150() {
    let json = serde_json::to_value(Groups::modal(Form)).unwrap();
    assert_eq!(
        json["components"][2]["component"]["options"]
            .as_array()
            .map(Vec::len),
        Some(25)
    );
}

#[test]
fn cid_01_derive_key_100() {
    let json = serde_json::to_value(Groups::modal(Form)).unwrap();
    assert_eq!(
        json["components"][4]["component"]["custom_id"]
            .as_str()
            .map(str::len),
        Some(100)
    );
}
