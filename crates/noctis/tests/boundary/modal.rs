//! Modal boundary passes (MOD-*, CID-01): runtime-built modals exactly at a
//! limit validate through `noctis::ui::validate`.

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
