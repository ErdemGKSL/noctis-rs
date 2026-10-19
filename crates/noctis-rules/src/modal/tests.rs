use super::*;
use crate::component::{Button, ButtonStyle, Container, SelectKind, check_select};

fn check(modal: &Modal<'_>) -> Vec<Violation> {
    check_with(modal, &Limits::default())
}

fn check_with(modal: &Modal<'_>, limits: &Limits) -> Vec<Violation> {
    match validate_modal(modal, limits) {
        Ok(()) => Vec::new(),
        Err(errs) => {
            assert!(!errs.is_empty());
            errs
        }
    }
}

/// One Label "L" with `child` in a modal titled "T".
fn child_with(child: LabelChild<'_>, limits: &Limits) -> Vec<Violation> {
    let components = [ModalComponent::Label(Label::new("L", child))];
    check_with(&Modal::new("T", &components), limits)
}

fn child(child: LabelChild<'_>) -> Vec<Violation> {
    child_with(child, &Limits::default())
}

fn rules(errs: &[Violation]) -> Vec<RuleId> {
    errs.iter().map(|v| v.rule).collect()
}

fn v(rule: RuleId, path: &str, limit: Bound, actual: Actual) -> Violation {
    Violation::new(rule, path, limit, actual)
}

fn btw(min: usize, max: usize) -> Bound {
    Bound::Between { min, max }
}

fn cnt(n: usize) -> Actual {
    Actual::Count(n)
}

fn txt(s: &str) -> Actual {
    Actual::Text(s.to_owned())
}

fn lowered(f: impl FnOnce(&mut Limits)) -> Limits {
    let mut l = Limits::default();
    f(&mut l);
    l
}

fn opt<'a>(label: &'a str, value: &'a str) -> ModalOption<'a> {
    ModalOption::new(label, value)
}

fn input(t: TextInput<'_>) -> Vec<Violation> {
    child(LabelChild::TextInput(t))
}

fn upload(f: FileUpload<'_>) -> Vec<Violation> {
    child(LabelChild::FileUpload(f))
}

fn group(g: CheckboxGroup<'_>) -> Vec<Violation> {
    child(LabelChild::CheckboxGroup(g))
}

fn radio(options: &[ModalOption<'_>]) -> Vec<Violation> {
    child(LabelChild::RadioGroup(RadioGroup::new(options)))
}

const SHORT: TextInput<'static> = TextInput::new(TextInputStyle::Short);

#[test]
fn mod_02_ac_valid_modal() {
    let options = [opt("Low", "low"), opt("High", "high")];
    let file_types = ["image", ".pdf"];
    let components = [
        ModalComponent::Text("Pick"),
        ModalComponent::Label(Label::new(
            "Reason",
            LabelChild::TextInput(
                TextInput::new(TextInputStyle::Paragraph)
                    .min_length(10)
                    .max_length(500),
            ),
        )),
        ModalComponent::Label(Label::new(
            "Channel",
            LabelChild::Select(ModalSelect::new(Select::new(SelectKind::Channel))),
        )),
        ModalComponent::Label(Label::new(
            "Proof",
            LabelChild::FileUpload(
                FileUpload::new()
                    .min_values(0)
                    .required(false)
                    .file_types(&file_types),
            ),
        )),
        ModalComponent::Label(Label::new(
            "Severity",
            LabelChild::RadioGroup(RadioGroup::new(&options)),
        )),
    ];
    assert_eq!(
        validate_modal(&Modal::new("Report", &components), &Limits::default()),
        Ok(())
    );
}

#[test]
fn mod_01_title() {
    let text = [ModalComponent::Text("x")];
    let title = |t: &str, l: &Limits| check_with(&Modal::new(t, &text), l);
    let l = Limits::default();
    let err = |max, n| vec![v(RuleId::MOD_01, "title", btw(1, max), cnt(n))];
    assert_eq!(title("", &l), err(45, 0));
    assert_eq!(title(&"a".repeat(45), &l), []);
    assert_eq!(title(&"a".repeat(46), &l), err(45, 46));
    assert_eq!(title(&"😀".repeat(22), &l), []);
    assert_eq!(title(&"😀".repeat(23), &l), err(45, 46));
    let low = lowered(|l| l.modal_title_len = 10);
    assert_eq!(title(&"a".repeat(11), &low), err(10, 11));
}

#[test]
fn mod_02_component_count() {
    let texts = [ModalComponent::Text("x"); 6];
    let err = |max, n| vec![v(RuleId::MOD_02, "components", btw(1, max), cnt(n))];
    assert_eq!(check(&Modal::new("T", &[])), err(5, 0));
    assert_eq!(check(&Modal::new("T", &texts[..1])), []);
    assert_eq!(check(&Modal::new("T", &texts[..5])), []);
    assert_eq!(check(&Modal::new("T", &texts)), err(5, 6));
    let low = lowered(|l| l.modal_components = 2);
    assert_eq!(check_with(&Modal::new("T", &texts[..3]), &low), err(2, 3));
}

#[test]
fn mod_03_allowed_children() {
    let top = Bound::Describe("only Labels or Text Displays at the top level of a modal");
    let label = "a".repeat(81);
    let button = Component::Button(Button::new(ButtonStyle::Primary).label(&label));
    let message = [ModalComponent::Message(button)];
    assert_eq!(
        check(&Modal::new("T", &message)),
        [v(RuleId::MOD_03, "button[0]", top.clone(), txt("button"))]
    );
    let inner = [Component::Text("a")];
    let message = [ModalComponent::Message(Component::Container(
        Container::new(&inner),
    ))];
    assert_eq!(
        check(&Modal::new("T", &message)),
        [v(RuleId::MOD_03, "container[0]", top, txt("container"))]
    );
    let message = [ModalComponent::Message(Component::Text("x"))];
    assert_eq!(check(&Modal::new("T", &message)), []);
    let disabled = ModalSelect::new(Select::new(SelectKind::User).disabled(true));
    assert_eq!(
        child(LabelChild::Select(disabled)),
        [v(
            RuleId::MOD_03,
            "label[0].select.disabled",
            Bound::Describe("no disabled in a modal"),
            txt("true")
        )]
    );
}

#[test]
fn mod_04_label() {
    let one = |label: Label<'_>, l: &Limits| {
        check_with(&Modal::new("T", &[ModalComponent::Label(label)]), l)
    };
    let l = Limits::default();
    let cb = LabelChild::Checkbox(Checkbox::new());
    let text = |n, max| v(RuleId::MOD_04, "label[0].label", btw(1, max), cnt(n));
    assert_eq!(one(Label::new("", cb), &l), [text(0, 45)]);
    assert_eq!(one(Label::new(&"a".repeat(45), cb), &l), []);
    assert_eq!(one(Label::new(&"a".repeat(46), cb), &l), [text(46, 45)]);
    let (a100, a101) = ("a".repeat(100), "a".repeat(101));
    assert_eq!(one(Label::new("L", cb).description(&a100), &l), []);
    assert_eq!(
        one(Label::new("L", cb).description(&a101), &l),
        [v(
            RuleId::MOD_04,
            "label[0].description",
            Bound::AtMost(100),
            cnt(101)
        )]
    );
    assert_eq!(one(Label::new("L", cb).description(""), &l), []);
    let low = lowered(|l| l.label_len = 3);
    assert_eq!(one(Label::new("abcd", cb), &low), [text(4, 3)]);
}

#[test]
fn mod_05_text_input() {
    let at = |f: &str| format!("label[0].text_input.{f}");
    let min = |n| v(RuleId::MOD_05, &at("min_length"), btw(0, 4000), cnt(n));
    let max = |n| v(RuleId::MOD_05, &at("max_length"), btw(1, 4000), cnt(n));
    assert_eq!(input(SHORT.min_length(0)), []);
    assert_eq!(input(SHORT.min_length(4000)), []);
    assert_eq!(input(SHORT.min_length(4001)), [min(4001)]);
    assert_eq!(input(SHORT.max_length(1)), []);
    assert_eq!(input(SHORT.max_length(4000)), []);
    assert_eq!(input(SHORT.max_length(0)), [max(0)]);
    assert_eq!(input(SHORT.max_length(4001)), [max(4001)]);
    assert_eq!(input(SHORT.min_length(5).max_length(5)), []);
    assert_eq!(
        input(SHORT.min_length(10).max_length(5)),
        [v(
            RuleId::MOD_05,
            "label[0].text_input",
            Bound::Describe("min_length ≤ max_length"),
            txt("10 > 5")
        )]
    );
    assert_eq!(input(SHORT.min_length(4001).max_length(5)), [min(4001)]);
    assert_eq!(input(SHORT.min_length(10)), []);
    let (a4000, a4001) = ("a".repeat(4000), "a".repeat(4001));
    assert_eq!(input(SHORT.value(&a4000)), []);
    assert_eq!(
        input(SHORT.value(&a4001)),
        [v(
            RuleId::MOD_05,
            &at("value"),
            Bound::AtMost(4000),
            cnt(4001)
        )]
    );
    let (a100, a101) = ("a".repeat(100), "a".repeat(101));
    assert_eq!(input(SHORT.placeholder(&a100)), []);
    assert_eq!(
        input(SHORT.placeholder(&a101)),
        [v(
            RuleId::MOD_05,
            &at("placeholder"),
            Bound::AtMost(100),
            cnt(101)
        )]
    );
    let low = lowered(|l| l.text_input_len = 10);
    assert_eq!(
        child_with(LabelChild::TextInput(SHORT.max_length(11)), &low),
        [v(RuleId::MOD_05, &at("max_length"), btw(1, 10), cnt(11))]
    );
}

#[test]
fn mod_06_file_upload() {
    let at = |f: &str| format!("label[0].file_upload.{f}");
    let required = v(
        RuleId::MOD_06,
        &at("min_values"),
        Bound::Describe("min_values ≥ 1 when required"),
        cnt(0),
    );
    assert_eq!(upload(FileUpload::new().min_values(0).required(false)), []);
    assert_eq!(upload(FileUpload::new().min_values(0)), [required]);
    assert_eq!(upload(FileUpload::new().min_values(10).max_values(10)), []);
    assert_eq!(
        upload(FileUpload::new().min_values(11)),
        [v(RuleId::MOD_06, &at("min_values"), btw(0, 10), cnt(11))]
    );
    assert_eq!(
        upload(FileUpload::new().max_values(0)),
        [v(RuleId::MOD_06, &at("max_values"), btw(1, 10), cnt(0))]
    );
    assert_eq!(
        upload(FileUpload::new().max_values(11)),
        [v(RuleId::MOD_06, &at("max_values"), btw(1, 10), cnt(11))]
    );
    assert_eq!(
        upload(FileUpload::new().min_values(3)),
        [v(
            RuleId::MOD_06,
            "label[0].file_upload",
            Bound::Describe("min_values ≤ max_values"),
            txt("3 > 1")
        )]
    );
    let types = [".png"; 11];
    assert_eq!(upload(FileUpload::new().file_types(&types[..10])), []);
    assert_eq!(
        upload(FileUpload::new().file_types(&types)),
        [v(
            RuleId::MOD_06,
            &at("file_types"),
            Bound::AtMost(10),
            cnt(11)
        )]
    );
    for bad in ["pdf", ".", ".tar.gz", "Image"] {
        assert_eq!(
            upload(FileUpload::new().file_types(&[bad])),
            [v(
                RuleId::MOD_06,
                &at("file_types[0]"),
                Bound::Describe(FILE_TYPE),
                txt(bad)
            )],
            "{bad}"
        );
    }
    assert_eq!(upload(FileUpload::new().file_types(&[".PNG", "video"])), []);
    let low = lowered(|l| l.file_upload_values = 2);
    assert_eq!(
        child_with(
            LabelChild::FileUpload(FileUpload::new().max_values(3)),
            &low
        ),
        [v(RuleId::MOD_06, &at("max_values"), btw(1, 2), cnt(3))]
    );
}

#[test]
fn mod_07_radio_group() {
    let opts: Vec<ModalOption<'_>> = (0..11).map(|_| opt("a", "a")).collect();
    let count = |n, max| {
        v(
            RuleId::MOD_07,
            "label[0].radio_group.options",
            btw(2, max),
            cnt(n),
        )
    };
    assert_eq!(radio(&opts[..1]), [count(1, 10)]);
    assert_eq!(radio(&opts[..2]), []);
    assert_eq!(radio(&opts[..10]), []);
    assert_eq!(radio(&opts), [count(11, 10)]);
    let field = |f: &str, bound, n| {
        v(
            RuleId::MOD_07,
            &format!("label[0].radio_group.options[0].{f}"),
            bound,
            cnt(n),
        )
    };
    let (a100, a101) = ("a".repeat(100), "a".repeat(101));
    let b = opt("b", "b");
    assert_eq!(radio(&[opt("", "a"), b]), [field("label", btw(1, 100), 0)]);
    assert_eq!(radio(&[opt(&a100, "a"), b]), []);
    assert_eq!(
        radio(&[opt(&a101, "a"), b]),
        [field("label", btw(1, 100), 101)]
    );
    assert_eq!(radio(&[opt("a", ""), b]), [field("value", btw(1, 100), 0)]);
    assert_eq!(
        radio(&[opt("a", &a101), b]),
        [field("value", btw(1, 100), 101)]
    );
    assert_eq!(
        radio(&[opt("a", "a").description(&a101), b]),
        [field("description", Bound::AtMost(100), 101)]
    );
    let low = lowered(|l| l.radio_options = 3);
    assert_eq!(
        child_with(LabelChild::RadioGroup(RadioGroup::new(&opts[..4])), &low),
        [count(4, 3)]
    );
    let low = lowered(|l| l.modal_option_text_len = 5);
    assert_eq!(
        child_with(
            LabelChild::RadioGroup(RadioGroup::new(&[opt("abcdef", "a"), b])),
            &low
        ),
        [field("label", btw(1, 5), 6)]
    );
}

#[test]
fn mod_07_checkbox_group() {
    let opts: Vec<ModalOption<'_>> = (0..11).map(|_| opt("a", "a")).collect();
    let at = |f: &str| format!("label[0].checkbox_group.{f}");
    let g = |n: usize| CheckboxGroup::new(&opts[..n]);
    let count = |n| v(RuleId::MOD_07, &at("options"), btw(1, 10), cnt(n));
    assert_eq!(group(g(0)), [count(0)]);
    assert_eq!(group(g(1)), []);
    assert_eq!(group(g(10)), []);
    assert_eq!(group(CheckboxGroup::new(&opts)), [count(11)]);
    assert_eq!(group(g(3).min_values(0).required(false)), []);
    assert_eq!(
        group(g(3).min_values(0)),
        [v(
            RuleId::MOD_07,
            &at("min_values"),
            Bound::Describe("min_values ≥ 1 when required"),
            cnt(0)
        )]
    );
    assert_eq!(
        group(g(3).min_values(11)),
        [v(RuleId::MOD_07, &at("min_values"), btw(0, 10), cnt(11))]
    );
    assert_eq!(
        group(g(3).max_values(0)),
        [v(RuleId::MOD_07, &at("max_values"), btw(1, 10), cnt(0))]
    );
    assert_eq!(
        group(g(3).max_values(11)),
        [v(RuleId::MOD_07, &at("max_values"), btw(1, 10), cnt(11))]
    );
    let order = |s: &str| {
        v(
            RuleId::MOD_07,
            "label[0].checkbox_group",
            Bound::Describe("min_values ≤ max_values"),
            txt(s),
        )
    };
    assert_eq!(group(g(2).min_values(3)), [order("3 > 2")]);
    assert_eq!(group(g(3).min_values(2).max_values(1)), [order("2 > 1")]);
    let low = lowered(|l| l.checkbox_group_values = 3);
    assert_eq!(
        child_with(LabelChild::CheckboxGroup(g(3).max_values(4)), &low),
        [v(RuleId::MOD_07, &at("max_values"), btw(1, 3), cnt(4))]
    );
}

#[test]
fn mod_07_checkbox() {
    assert_eq!(child(LabelChild::Checkbox(Checkbox::new())), []);
    assert_eq!(
        child(LabelChild::Checkbox(Checkbox::new().required(true))),
        [v(
            RuleId::MOD_07,
            "label[0].checkbox.required",
            Bound::Describe("a Checkbox is never required"),
            txt("true")
        )]
    );
}

#[test]
fn cv2_09_modal_select() {
    let a151 = "a".repeat(151);
    let s = Select::new(SelectKind::User)
        .placeholder(&a151)
        .min_values(3)
        .max_values(2);
    let errs = child(LabelChild::Select(ModalSelect::new(s)));
    let mut expected = Vec::new();
    check_select(&s, "label[0].select", &Limits::default(), &mut expected);
    assert_eq!(errs, expected);
    assert_eq!(rules(&errs), [RuleId::CV2_09, RuleId::CV2_09]);
    assert_eq!(errs[0].path, "label[0].select.placeholder");
    let zero = Select::new(SelectKind::User).min_values(0);
    assert_eq!(
        child(LabelChild::Select(ModalSelect::new(zero))),
        [v(
            RuleId::CV2_09,
            "label[0].select.min_values",
            Bound::Describe("min_values ≥ 1 when required"),
            cnt(0)
        )]
    );
    assert_eq!(
        child(LabelChild::Select(ModalSelect::new(zero).required(false))),
        []
    );
}

#[test]
fn mod_01_mod_02_mod_03_mod_04_mod_07_all_at_once() {
    let title = "a".repeat(46);
    let button = Component::Button(Button::new(ButtonStyle::Primary).label("Go"));
    let components = [
        ModalComponent::Message(button),
        ModalComponent::Label(Label::new(
            "",
            LabelChild::Checkbox(Checkbox::new().required(true)),
        )),
        ModalComponent::Text("x"),
        ModalComponent::Text("x"),
        ModalComponent::Text("x"),
        ModalComponent::Text("x"),
    ];
    assert_eq!(
        rules(&check(&Modal::new(&title, &components))),
        [
            RuleId::MOD_01,
            RuleId::MOD_02,
            RuleId::MOD_03,
            RuleId::MOD_04,
            RuleId::MOD_07
        ]
    );
}

#[test]
fn mod_01_mod_04_mod_05_granular() {
    let l = Limits::default();
    assert_eq!(
        validate_modal_title("", &l),
        [v(RuleId::MOD_01, "", btw(1, 45), cnt(0))]
    );
    assert!(validate_modal_title(&"a".repeat(45), &l).is_empty());
    assert_eq!(
        validate_modal_title(&"a".repeat(46), &l),
        [v(RuleId::MOD_01, "", btw(1, 45), cnt(46))]
    );
    assert_eq!(
        validate_label_text("", &l),
        [v(RuleId::MOD_04, "", btw(1, 45), cnt(0))]
    );
    assert!(validate_label_text(&"a".repeat(45), &l).is_empty());
    assert_eq!(
        validate_label_text(&"a".repeat(46), &l),
        [v(RuleId::MOD_04, "", btw(1, 45), cnt(46))]
    );
    assert!(validate_label_description("", &l).is_empty());
    assert!(validate_label_description(&"a".repeat(100), &l).is_empty());
    assert_eq!(
        validate_label_description(&"a".repeat(101), &l),
        [v(RuleId::MOD_04, "", Bound::AtMost(100), cnt(101))]
    );
    assert_eq!(
        validate_text_input(&SHORT.min_length(4001).max_length(0), &l),
        [
            v(RuleId::MOD_05, "min_length", btw(0, 4000), cnt(4001)),
            v(RuleId::MOD_05, "max_length", btw(1, 4000), cnt(0)),
        ]
    );
    assert_eq!(
        validate_text_input(&SHORT.min_length(10).max_length(5), &l),
        [v(
            RuleId::MOD_05,
            "",
            Bound::Describe("min_length ≤ max_length"),
            txt("10 > 5")
        )]
    );
    let a101 = "a".repeat(101);
    let a4001 = "a".repeat(4001);
    let errs = validate_text_input(&SHORT.value(&a4001).placeholder(&a101), &l);
    let paths: Vec<&str> = errs.iter().map(|v| v.path.as_str()).collect();
    assert_eq!(paths, ["value", "placeholder"]);
    assert!(validate_text_input_value("", &l).is_empty());
    assert!(validate_text_input_value(&"a".repeat(4000), &l).is_empty());
    assert_eq!(
        validate_text_input_value(&a4001, &l),
        [v(RuleId::MOD_05, "", Bound::AtMost(4000), cnt(4001))]
    );
}

#[test]
fn mod_05_text_input_style_values() {
    assert_eq!(TextInputStyle::Short.value(), 1);
    assert_eq!(TextInputStyle::Paragraph.value(), 2);
    assert!(TextInput::new(TextInputStyle::Short).required);
    assert!(!Checkbox::new().required);
    assert_eq!(FileUpload::default(), FileUpload::new());
}
