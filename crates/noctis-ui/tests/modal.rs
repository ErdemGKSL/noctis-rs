//! The modal model and its runtime validation: every row of the Story 4.5
//! I/O matrix (FR-29, FR-63, FR-78; MOD-01..08, CV2-09, CID-01, CID-02),
//! Strict and Lenient.

#![allow(
    clippy::expect_used,
    reason = "test helpers panic with a message naming the broken invariant"
)]

use core::fmt;

use noctis_codec::{FieldWriter, Parts, Payload, Route, RouteId, RouteShape, SelectNonce};
use noctis_rules::len::discord_len;
use noctis_rules::{Actual, Bound, LimitMode, Limits, RuleId, Violation};
use noctis_ui::__macro::{ComponentValue, build_modal};
use noctis_ui::{
    Checkbox, CheckboxGroup, CostReport, FileUpload, Handled, LimitWarning, Modal,
    ModalChannelSelect, ModalInput, ModalLabel, ModalMentionableSelect, ModalOption,
    ModalRoleSelect, ModalStringSelect, ModalUserSelect, RadioGroup, SelectOption, TextDisplay,
    TextInput, TextInputStyle, UiError, cost, validate,
};
use serde_json::Value;
use twilight_model::id::Id;

/// What `#[derive(Modal)]`'s `.build()` expands to, before the inputs.
fn modal<R: ComponentValue>(route: R, title: impl Into<std::borrow::Cow<'static, str>>) -> Modal {
    build_modal::<R>(route, title)
}

/// A hand-written handled Route without fields whose Payload is itself.
macro_rules! route {
    ($name:ident = $id:literal, $shape:expr) => {
        #[derive(Debug, Clone)]
        struct $name;

        impl Payload for $name {
            const MAX_ENCODED_LEN: Option<usize> = Some(0);

            fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result {
                Ok(())
            }
        }

        impl Route for $name {
            const ID: RouteId = RouteId::new($id);
            const SHAPE: RouteShape = $shape;
            const SCHEMA: u8 = 0;
            type Payload = Self;
        }

        impl Handled for $name {}
    };
}

route!(Save = "form.save", RouteShape::INLINE);
route!(Picked = "form.pick", RouteShape::INLINE.select());

// Helpers.

fn strict() -> Limits {
    Limits::default()
}

fn lenient() -> Limits {
    let mut l = Limits::default();
    l.mode = LimitMode::Lenient;
    l
}

fn short(key: impl Into<String>) -> TextInput {
    TextInput::new(key.into(), TextInputStyle::Short)
}

fn label(input: impl ModalInput) -> ModalLabel {
    ModalLabel::new("L", input)
}

/// A modal titled "T" with one Label "L" around `input`.
fn one(input: impl ModalInput) -> Modal {
    modal(Save, "T").push(label(input))
}

/// `n` Labels, each a Text Input keyed `k<i>`.
fn labels(n: usize) -> Modal {
    (0..n).fold(modal(Save, "T"), |m, i| {
        m.push(label(short(format!("k{i}"))))
    })
}

fn opts(n: usize) -> Vec<ModalOption> {
    (0..n)
        .map(|i| ModalOption::new(format!("o{i}"), format!("v{i}")))
        .collect()
}

fn select_opts(n: usize) -> Vec<SelectOption> {
    (0..n)
        .map(|i| SelectOption::new(format!("o{i}"), format!("v{i}")))
        .collect()
}

fn a(n: usize) -> String {
    "a".repeat(n)
}

fn passes_with(mut m: Modal, limits: &Limits) -> CostReport {
    match validate(&mut m, limits) {
        Ok(report) => report,
        Err(e) => panic!("expected a pass, got {e}"),
    }
}

fn passes(m: Modal) -> CostReport {
    passes_with(m, &strict())
}

fn errs_with(mut m: Modal, limits: &Limits) -> Vec<Violation> {
    match validate(&mut m, limits) {
        Err(UiError::Limits { violations }) => violations,
        other => panic!("expected a limits error, got {other:?}"),
    }
}

fn errs(m: Modal) -> Vec<Violation> {
    errs_with(m, &strict())
}

/// Asserts `m` fails with `rule` at `path`.
#[track_caller]
fn fails_at(m: Modal, rule: RuleId, path: &str) -> Vec<Violation> {
    let errs = errs(m);
    assert!(
        errs.iter().any(|v| v.rule == rule && v.path == path),
        "expected {rule} at {path}, got {errs:?}"
    );
    errs
}

fn rules(errs: &[Violation]) -> Vec<RuleId> {
    errs.iter().map(|v| v.rule).collect()
}

fn json(m: &Modal) -> Value {
    serde_json::to_value(m).expect("serializes")
}

/// Every JSON object of `value`, in pre-order.
fn objects(value: &Value, out: &mut Vec<serde_json::Map<String, Value>>) {
    match value {
        Value::Object(map) => {
            out.push(map.clone());
            map.values().for_each(|v| objects(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| objects(v, out)),
        _ => {}
    }
}

const REPORT: &str = "Tell us what happened.";

fn full() -> Modal {
    modal(Save, "Report")
        .push(TextDisplay::new(REPORT))
        .push(
            ModalLabel::new(
                "Reason",
                TextInput::new("reason", TextInputStyle::Paragraph).max_length(500),
            )
            .description("Be specific"),
        )
        .push(ModalLabel::new(
            "Category",
            ModalStringSelect::new("cat", select_opts(2)).placeholder("Pick one"),
        ))
        .push(ModalLabel::new(
            "Severity",
            RadioGroup::new(
                "sev",
                [ModalOption::new("Low", "l"), ModalOption::new("High", "h")],
            ),
        ))
        .push(ModalLabel::new("I agree", Checkbox::new("ok")))
}

// Row 1.

#[test]
fn mod_03_full_modal_snapshot() {
    let modal = full();
    let value = json(&modal);
    insta::assert_json_snapshot!("full", value);
    let mut all = Vec::new();
    objects(&value, &mut all);
    assert!(all.iter().all(|o| o.get("type") != Some(&Value::from(1))));
    let inputs: Vec<_> = all
        .iter()
        .filter(|o| o.get("type") == Some(&Value::from(4)))
        .collect();
    assert_eq!(inputs.len(), 1);
    assert!(!inputs[0].contains_key("label"));
    assert!(all.iter().all(|o| !o.contains_key("id")));
    assert!(all.iter().all(|o| !o.contains_key("disabled")));
    assert_eq!(passes(modal).components, 9);
}

// Rows 2-4.

#[test]
fn mod_02_five_top_level() {
    assert_eq!(passes(labels(5)).components, 10);
}

#[test]
fn mod_02_six_top_level() {
    let errs = fails_at(labels(6), RuleId::MOD_02, "components");
    assert_eq!(errs[0].limit, Bound::Between { min: 1, max: 5 });
    assert_eq!(errs[0].actual, Actual::Count(6));
}

#[test]
fn mod_02_empty_modal() {
    let errs = fails_at(modal(Save, "T"), RuleId::MOD_02, "components");
    assert_eq!(errs[0].actual, Actual::Count(0));
}

// Rows 5-6.

#[test]
fn mod_01_title_bounds() {
    let titled = |t: String| modal(Save, t).push(label(short("k")));
    passes(titled(a(45)));
    fails_at(titled(a(46)), RuleId::MOD_01, "title");
    fails_at(titled(String::new()), RuleId::MOD_01, "title");
}

#[test]
fn mod_01_title_unicode() {
    let titled = |t: String| modal(Save, t).push(label(short("k")));
    passes(titled("é".repeat(45)));
    let errs = fails_at(titled("😀".repeat(23)), RuleId::MOD_01, "title");
    assert_eq!(errs[0].actual, Actual::Count(46));
}

// Rows 7-8.

#[test]
fn mod_04_label_text_bounds() {
    let with = |t: String| modal(Save, "T").push(ModalLabel::new(t, short("k")));
    passes(with(a(45)));
    fails_at(with(a(46)), RuleId::MOD_04, "label[0].label");
    fails_at(with(String::new()), RuleId::MOD_04, "label[0].label");
}

#[test]
fn mod_04_label_description_bounds() {
    let with = |d: String| modal(Save, "T").push(label(short("k")).description(d));
    passes(with(a(100)));
    fails_at(with(a(101)), RuleId::MOD_04, "label[0].description");
    passes(with(String::new()));
}

// Rows 9-13.

#[test]
fn mod_05_min_length_bounds() {
    passes(one(short("k").min_length(0)));
    passes(one(short("k").min_length(4000)));
    fails_at(
        one(short("k").min_length(4001)),
        RuleId::MOD_05,
        "label[0].text_input.min_length",
    );
}

#[test]
fn mod_05_max_length_bounds() {
    let path = "label[0].text_input.max_length";
    fails_at(one(short("k").max_length(0)), RuleId::MOD_05, path);
    passes(one(short("k").max_length(1)));
    passes(one(short("k").max_length(4000)));
    fails_at(one(short("k").max_length(4001)), RuleId::MOD_05, path);
}

#[test]
fn mod_05_min_over_max() {
    fails_at(
        one(short("k").min_length(10).max_length(5)),
        RuleId::MOD_05,
        "label[0].text_input",
    );
}

#[test]
fn mod_05_prefill_value_never_truncated() {
    passes(one(short("k").value(a(4000))));
    let path = "label[0].text_input.value";
    fails_at(one(short("k").value(a(4001))), RuleId::MOD_05, path);
    let lenient_errs = errs_with(one(short("k").value(a(4001))), &lenient());
    assert_eq!(rules(&lenient_errs), [RuleId::MOD_05]);
    assert_eq!(lenient_errs[0].path, path);
}

#[test]
fn mod_05_placeholder_bounds() {
    passes(one(short("k").placeholder(a(100))));
    fails_at(
        one(short("k").placeholder(a(101))),
        RuleId::MOD_05,
        "label[0].text_input.placeholder",
    );
}

// Rows 14-17.

#[test]
fn mod_06_min_zero_required() {
    fails_at(
        one(FileUpload::new("f").min_values(0)),
        RuleId::MOD_06,
        "label[0].file_upload.min_values",
    );
}

#[test]
fn mod_06_values_bounds() {
    let up = || FileUpload::new("f").required(false);
    passes(one(up().min_values(0)));
    passes(one(up().max_values(10)));
    let path = "label[0].file_upload.max_values";
    fails_at(one(up().max_values(11)), RuleId::MOD_06, path);
    fails_at(one(up().max_values(0)), RuleId::MOD_06, path);
}

#[test]
fn mod_06_min_over_max() {
    fails_at(
        one(FileUpload::new("f").min_values(3).max_values(2)),
        RuleId::MOD_06,
        "label[0].file_upload",
    );
}

#[test]
fn mod_06_file_types() {
    let types = |n: usize| (0..n).map(|i| format!(".e{i}"));
    passes(one(FileUpload::new("f").file_types(
        ["image", "video", "audio"]
            .into_iter()
            .map(String::from)
            .chain(types(7)),
    )));
    fails_at(
        one(FileUpload::new("f").file_types(types(11))),
        RuleId::MOD_06,
        "label[0].file_upload.file_types",
    );
    fails_at(
        one(FileUpload::new("f").file_types(["exe"])),
        RuleId::MOD_06,
        "label[0].file_upload.file_types[0]",
    );
}

// Rows 18-22.

#[test]
fn mod_07_radio_option_counts() {
    let path = "label[0].radio_group.options";
    for n in [0, 1, 11] {
        fails_at(one(RadioGroup::new("r", opts(n))), RuleId::MOD_07, path);
    }
    passes(one(RadioGroup::new("r", opts(2))));
    passes(one(RadioGroup::new("r", opts(10))));
}

#[test]
fn mod_07_checkbox_group_option_counts() {
    let path = "label[0].checkbox_group.options";
    fails_at(one(CheckboxGroup::new("g", opts(0))), RuleId::MOD_07, path);
    passes(one(CheckboxGroup::new("g", opts(1))));
    passes(one(CheckboxGroup::new("g", opts(10))));
    fails_at(one(CheckboxGroup::new("g", opts(11))), RuleId::MOD_07, path);
}

#[test]
fn mod_07_checkbox_group_min_zero_required() {
    fails_at(
        one(CheckboxGroup::new("g", opts(3)).min_values(0)),
        RuleId::MOD_07,
        "label[0].checkbox_group.min_values",
    );
}

#[test]
fn mod_07_checkbox_group_values() {
    passes(one(CheckboxGroup::new("g", opts(3))
        .required(false)
        .min_values(0)));
    fails_at(
        one(CheckboxGroup::new("g", opts(3)).max_values(11)),
        RuleId::MOD_07,
        "label[0].checkbox_group.max_values",
    );
    fails_at(
        one(CheckboxGroup::new("g", opts(3)).min_values(4)),
        RuleId::MOD_07,
        "label[0].checkbox_group",
    );
}

#[test]
fn mod_07_option_text_bounds() {
    let radio = |o: ModalOption| one(RadioGroup::new("r", [o, ModalOption::new("b", "b")]));
    let base = "label[0].radio_group.options[0]";
    passes(radio(ModalOption::new(a(100), a(100))));
    fails_at(
        radio(ModalOption::new(a(101), "v")),
        RuleId::MOD_07,
        &format!("{base}.label"),
    );
    fails_at(
        radio(ModalOption::new("l", a(101))),
        RuleId::MOD_07,
        &format!("{base}.value"),
    );
    fails_at(
        radio(ModalOption::new("", "v")),
        RuleId::MOD_07,
        &format!("{base}.label"),
    );
    fails_at(
        radio(ModalOption::new("l", "")),
        RuleId::MOD_07,
        &format!("{base}.value"),
    );
    fails_at(
        radio(ModalOption::new("l", "v").description(a(101))),
        RuleId::MOD_07,
        &format!("{base}.description"),
    );
}

// Rows 23-24.

#[test]
fn cv2_09_modal_select_min_zero_required() {
    fails_at(
        one(ModalUserSelect::new("u").min_values(0)),
        RuleId::CV2_09,
        "label[0].select.min_values",
    );
    passes(one(ModalUserSelect::new("u").required(false).min_values(0)));
}

#[test]
fn cv2_09_modal_string_select_bounds() {
    let path = "label[0].select.options";
    fails_at(
        one(ModalStringSelect::new("s", select_opts(0))),
        RuleId::CV2_09,
        path,
    );
    passes(one(ModalStringSelect::new("s", select_opts(25))));
    fails_at(
        one(ModalStringSelect::new("s", select_opts(26))),
        RuleId::CV2_09,
        path,
    );
    passes(one(
        ModalStringSelect::new("s", select_opts(1)).placeholder(a(150))
    ));
    fails_at(
        one(ModalStringSelect::new("s", select_opts(1)).placeholder(a(151))),
        RuleId::CV2_09,
        "label[0].select.placeholder",
    );
}

#[test]
fn cv2_09_modal_select_default_values() {
    let path = "label[0].select.default_values";
    let users = || ModalUserSelect::new("u").default_users([Id::new(1), Id::new(2), Id::new(3)]);
    fails_at(one(users()), RuleId::CV2_09, path);
    passes(one(users().max_values(3)));
    let channels =
        || ModalChannelSelect::new("c").default_channels([Id::new(1), Id::new(2), Id::new(3)]);
    fails_at(one(channels()), RuleId::CV2_09, path);
    passes(one(channels().max_values(3)));
    passes(
        modal(Save, "T")
            .push(label(ModalRoleSelect::new("r")))
            .push(label(ModalMentionableSelect::new("m"))),
    );
}

// Rows 25-27.

#[test]
fn cid_01_key_length() {
    let path = "label[0].text_input.custom_id";
    passes(one(short("k")));
    passes(one(short(a(100))));
    let errs = fails_at(one(short(a(101))), RuleId::CID_01, path);
    assert_eq!(errs[0].actual, Actual::Count(101));
    fails_at(one(short("")), RuleId::CID_01, path);
    passes(one(short("😀".repeat(50))));
    let errs = fails_at(one(short("😀".repeat(51))), RuleId::CID_01, path);
    assert_eq!(errs[0].actual, Actual::Count(102));
}

#[test]
fn cid_02_duplicate_keys() {
    let m = modal(Save, "T")
        .push(label(short("a")))
        .push(label(Checkbox::new("a")));
    let errs = errs(m);
    assert_eq!(rules(&errs), [RuleId::CID_02]);
    assert_eq!(errs[0].path, "label[1].checkbox.custom_id");
    assert_eq!(
        errs[0].notes,
        ["first used at label[0].text_input.custom_id"]
    );
}

#[test]
fn cid_02_duplicate_select_keys() {
    let m = modal(Save, "T")
        .push(label(short("a")))
        .push(label(ModalStringSelect::new("a", select_opts(1))))
        .push(label(ModalUserSelect::new("a")));
    let errs = errs(m);
    assert_eq!(rules(&errs), [RuleId::CID_02, RuleId::CID_02]);
    assert_eq!(errs[0].path, "label[1].select.custom_id");
    assert_eq!(errs[1].path, "label[2].select.custom_id");
    for v in &errs {
        assert_eq!(v.notes, ["first used at label[0].text_input.custom_id"]);
    }
}

#[test]
fn cid_02_keys_after_text_display() {
    let m = modal(Save, "T")
        .push(TextDisplay::new("intro"))
        .push(label(short("a")))
        .push(label(Checkbox::new("a")));
    let errs = errs(m);
    assert_eq!(rules(&errs), [RuleId::CID_02]);
    assert_eq!(errs[0].path, "label[2].checkbox.custom_id");
    assert_eq!(
        errs[0].notes,
        ["first used at label[1].text_input.custom_id"]
    );
}

#[test]
fn cv2_04_modal_text_display_total() {
    passes(
        modal(Save, "T")
            .push(TextDisplay::new(a(2000)))
            .push(TextDisplay::new(a(2000))),
    );
    let errs = fails_at(
        modal(Save, "T")
            .push(TextDisplay::new(a(2000)))
            .push(TextDisplay::new(a(2001))),
        RuleId::CV2_04,
        "components",
    );
    assert_eq!(rules(&errs), [RuleId::CV2_04]);
    assert_eq!(errs[0].actual, Actual::Count(4001));
}

#[test]
fn cv2_04_modal_lenient_truncates_text_displays() {
    let mut m = modal(Save, "T")
        .push(TextDisplay::new(a(3000)))
        .push(label(short("k")))
        .push(TextDisplay::new(a(3000)));
    let report = validate(&mut m, &lenient()).expect("lenient fits");
    let cut: Vec<(RuleId, &str)> = report
        .warnings
        .iter()
        .map(|w| (w.rule().expect("a truncation"), w.path()))
        .collect();
    assert_eq!(cut, [(RuleId::CV2_04, "components")]);
    assert_eq!(report.text_chars, 4000);
    let value = json(&m);
    let last = value["components"][2]["content"]
        .as_str()
        .expect("a string");
    assert!(last.ends_with('…'), "{last}");
}

#[test]
fn mod_07_checkbox_json() {
    let m = one(Checkbox::new("c").default(true));
    assert_eq!(
        json(&m)["components"][0]["component"],
        serde_json::json!({"type": 23, "custom_id": "c", "default": true})
    );
    passes(m);
}

// Rows 28-30.

#[test]
fn mod_01_lenient_truncates() {
    let mut m = modal(Save, a(50)).push(ModalLabel::new(
        a(50),
        RadioGroup::new(
            "r",
            [ModalOption::new(a(101), "v"), ModalOption::new("b", "b")],
        ),
    ));
    let report = validate(&mut m, &lenient()).expect("lenient fits");
    let cut: Vec<(RuleId, &str)> = report
        .warnings
        .iter()
        .map(|w| (w.rule().expect("a truncation"), w.path()))
        .collect();
    assert_eq!(
        cut,
        [
            (RuleId::MOD_01, "title"),
            (RuleId::MOD_04, "label[0].label"),
            (RuleId::MOD_07, "label[0].radio_group.options[0].label"),
        ]
    );
    assert!(
        report
            .warnings
            .iter()
            .all(|w| matches!(w, LimitWarning::Truncated(_)))
    );
    let value = json(&m);
    let texts = [
        (value["title"].as_str(), 45),
        (value["components"][0]["label"].as_str(), 45),
        (
            value["components"][0]["component"]["options"][0]["label"].as_str(),
            100,
        ),
    ];
    for (text, max) in texts {
        let text = text.expect("a string");
        assert!(text.ends_with('…'), "{text}");
        assert_eq!(discord_len(text), max);
    }
}

#[test]
fn mod_04_lenient_truncates_every_site() {
    let mut m = modal(Save, "T")
        .push(ModalLabel::new("L", short("t").placeholder(a(101))).description(a(101)))
        .push(label(
            ModalStringSelect::new("s", [SelectOption::new(a(101), "v").description(a(101))])
                .placeholder(a(151)),
        ))
        .push(label(CheckboxGroup::new(
            "g",
            [ModalOption::new(a(101), "v").description(a(101))],
        )));
    let report = validate(&mut m, &lenient()).expect("lenient fits");
    let cut: Vec<(RuleId, &str)> = report
        .warnings
        .iter()
        .map(|w| (w.rule().expect("a truncation"), w.path()))
        .collect();
    assert_eq!(
        cut,
        [
            (RuleId::MOD_04, "label[0].description"),
            (RuleId::MOD_05, "label[0].text_input.placeholder"),
            (RuleId::CV2_09, "label[1].select.placeholder"),
            (RuleId::CV2_09, "label[1].select.options[0].label"),
            (RuleId::CV2_09, "label[1].select.options[0].description"),
            (RuleId::MOD_07, "label[2].checkbox_group.options[0].label"),
            (
                RuleId::MOD_07,
                "label[2].checkbox_group.options[0].description"
            ),
        ]
    );
    let value = json(&m);
    let c = &value["components"];
    let texts = [
        (c[0]["description"].as_str(), 100),
        (c[0]["component"]["placeholder"].as_str(), 100),
        (c[1]["component"]["placeholder"].as_str(), 150),
        (c[1]["component"]["options"][0]["label"].as_str(), 100),
        (c[1]["component"]["options"][0]["description"].as_str(), 100),
        (c[2]["component"]["options"][0]["label"].as_str(), 100),
        (c[2]["component"]["options"][0]["description"].as_str(), 100),
    ];
    for (text, max) in texts {
        let text = text.expect("a string");
        assert!(text.ends_with('…'), "{text}");
        assert_eq!(discord_len(text), max);
    }
}

#[test]
fn mod_04_lenient_path_after_text_display() {
    let mut m = modal(Save, "T")
        .push(TextDisplay::new("intro"))
        .push(ModalLabel::new(a(50), short("k")));
    let report = validate(&mut m, &lenient()).expect("lenient fits");
    let cut: Vec<(RuleId, &str)> = report
        .warnings
        .iter()
        .map(|w| (w.rule().expect("a truncation"), w.path()))
        .collect();
    assert_eq!(cut, [(RuleId::MOD_04, "label[1].label")]);
}

#[test]
fn mod_02_lenient_structural_still_fails() {
    let l = lenient();
    let errs = errs_with(modal(Save, "").push(label(short("k"))), &l);
    assert_eq!(rules(&errs), [RuleId::MOD_01]);
    let errs = errs_with(labels(6), &l);
    assert_eq!(rules(&errs), [RuleId::MOD_02]);
    let errs = errs_with(one(short(a(101))), &l);
    assert_eq!(rules(&errs), [RuleId::CID_01]);
}

#[test]
fn mod_01_violation_order() {
    let m = modal(Save, a(46))
        .push(label(short("")))
        .push(label(short("a")))
        .push(label(short("a")))
        .push(ModalLabel::new(a(46), short("b")))
        .push(label(short("b")))
        .push(label(short("c")));
    let errs = errs(m);
    let got: Vec<(RuleId, &str)> = errs.iter().map(|v| (v.rule, v.path.as_str())).collect();
    assert_eq!(
        got,
        [
            (RuleId::MOD_01, "title"),
            (RuleId::MOD_02, "components"),
            (RuleId::MOD_04, "label[3].label"),
            (RuleId::CID_01, "label[0].text_input.custom_id"),
            (RuleId::CID_02, "label[2].text_input.custom_id"),
            (RuleId::CID_02, "label[4].text_input.custom_id"),
        ]
    );
}

// Rows 31-34.

#[test]
fn mod_08_unresolved_modal_id() {
    let mut m = modal(Picked, "T").push(label(short("k")));
    let err = serde_json::to_value(&m).expect_err("unresolved");
    assert!(err.to_string().contains("form.pick"), "{err}");
    let nonce = SelectNonce::new(1).expect("in range");
    assert_eq!(
        m.custom_id_mut().resolve(&Parts::new().with_nonce(nonce)),
        Ok(None)
    );
    let value = json(&m);
    assert!(
        value["custom_id"]
            .as_str()
            .is_some_and(|id| id.contains("form.pick"))
    );
}

#[test]
fn mod_08_custom_id_is_the_route_encoding() {
    assert_eq!(json(&one(short("k")))["custom_id"], "0form.save:");
}

#[test]
fn mod_02_cost() {
    let m = full();
    let before = json(&m);
    let report = cost(&m, &strict());
    assert_eq!(report.components, 9);
    assert_eq!(report.text_chars, discord_len(REPORT));
    assert!(report.warnings.is_empty());
    assert_eq!(json(&m), before);
}

#[test]
fn mod_01_strict_never_mutates() {
    let mut m = modal(Save, a(50)).push(label(short("k")));
    assert!(validate(&mut m, &strict()).is_err());
    assert_eq!(m.title(), a(50));
}

#[test]
fn mod_05_required_false_emitted() {
    let input = |i: TextInput| json(&one(i))["components"][0]["component"].clone();
    assert_eq!(input(short("k").required(false))["required"], false);
    assert!(input(short("k")).get("required").is_none());
    let radio = |r: RadioGroup| json(&one(r))["components"][0]["component"].clone();
    assert_eq!(
        radio(RadioGroup::new("r", opts(2)).required(false))["required"],
        false
    );
    assert!(
        radio(RadioGroup::new("r", opts(2)))
            .get("required")
            .is_none()
    );
}
