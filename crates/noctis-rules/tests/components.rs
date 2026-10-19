//! The Story 1.6 acceptance criteria through the public API: each rule's limit
//! passes and limit + 1 fails (CV2-03..15, MOD-01..07, MSG-01, MSG-04, MSG-06,
//! ATT-01..03, ATT-05, ATT-06, WH-04, INT-07), and lowered overrides apply.

use noctis_rules::attachment::{
    AttachmentDecl, validate_attachment_description, validate_attachment_ref, validate_attachments,
};
use noctis_rules::command::{Choice, ChoiceValue, OptionType, validate_autocomplete};
use noctis_rules::component::{
    Accessory, Button, ButtonStyle, Component, Container, Media, Section, Select, SelectKind,
    SelectOption, validate_accent_color, validate_button_label, validate_components,
    validate_legacy_components, validate_spacing,
};
use noctis_rules::message::{
    AllowedMentions, MentionIds, validate_content, validate_mentions, validate_nonce,
    validate_sticker_ids, validate_webhook_name,
};
use noctis_rules::modal::{
    Checkbox, CheckboxGroup, FileUpload, Label, LabelChild, Modal, ModalComponent, ModalOption,
    RadioGroup, TextInput, TextInputStyle, validate_modal, validate_modal_title,
};
use noctis_rules::{Limits, RuleId, Violation};

const GO: Button<'static> = Button::new(ButtonStyle::Primary).label("Go");
const B: Component<'static> = Component::Button(GO);

fn defaults() -> Limits {
    Limits::default()
}

fn lowered(f: impl FnOnce(&mut Limits)) -> Limits {
    let mut l = Limits::default();
    f(&mut l);
    l
}

fn rules_of(result: Result<(), Vec<Violation>>) -> Vec<RuleId> {
    result
        .err()
        .unwrap_or_default()
        .iter()
        .map(|v| v.rule)
        .collect()
}

fn tree(components: &[Component<'_>]) -> Vec<RuleId> {
    rules_of(validate_components(components, &defaults()))
}

fn ids(errs: &[Violation]) -> Vec<RuleId> {
    errs.iter().map(|v| v.rule).collect()
}

fn labelled(s: &str) -> Component<'_> {
    Component::Button(Button::new(ButtonStyle::Primary).label(s))
}

fn described(d: &str) -> [Media<'_>; 1] {
    [Media::new("u").description(d)]
}

fn in_row(c: Component<'_>) -> Vec<RuleId> {
    tree(&[Component::Row(&[c])])
}

#[test]
fn cv2_03_forty_components() {
    let texts = vec![Component::Text("a"); 41];
    assert_eq!(tree(&texts[..40]), []);
    assert_eq!(tree(&texts), [RuleId::CV2_03]);
    let errs = validate_components(&texts, &defaults()).unwrap_err();
    assert_eq!(
        errs[0].to_string().lines().next(),
        Some("[CV2-03] components: limit at most 40, actual 41")
    );
}

#[test]
fn cv2_04_text_budget() {
    let (a4000, a4001) = ("a".repeat(4000), "a".repeat(4001));
    assert_eq!(tree(&[Component::Text(&a4000)]), []);
    assert_eq!(tree(&[Component::Text(&a4001)]), [RuleId::CV2_04]);
}

#[test]
fn cv2_05_container_in_container() {
    let inner_children = [Component::Text("a")];
    let inner = [Component::Container(Container::new(&inner_children))];
    assert_eq!(
        tree(&[Component::Container(Container::new(&inner))]),
        [RuleId::CV2_05]
    );
}

#[test]
fn cv2_06_row_buttons_and_selects() {
    let u = Component::Select(Select::new(SelectKind::User));
    assert_eq!(tree(&[Component::Row(&[B; 5])]), []);
    assert_eq!(tree(&[Component::Row(&[B; 6])]), [RuleId::CV2_06]);
    assert_eq!(tree(&[Component::Row(&[u])]), []);
    assert_eq!(tree(&[Component::Row(&[u, u])]), [RuleId::CV2_06]);
}

#[test]
fn cv2_07_section_texts() {
    let section = |texts: &[&str]| {
        tree(&[Component::Section(Section::new(
            texts,
            Accessory::Button(GO),
        ))])
    };
    assert_eq!(section(&["a", "b", "c"]), []);
    assert_eq!(section(&["a", "b", "c", "d"]), [RuleId::CV2_07]);
}

#[test]
fn cv2_08_labels_urls_and_premium() {
    let (a80, a81) = ("a".repeat(80), "a".repeat(81));
    assert_eq!(in_row(labelled(&a80)), []);
    assert_eq!(in_row(labelled(&a81)), [RuleId::CV2_08]);
    let (u512, u513) = ("a".repeat(512), "a".repeat(513));
    assert_eq!(
        in_row(Component::Button(Button::new(ButtonStyle::Link(&u512)))),
        []
    );
    assert_eq!(
        in_row(Component::Button(Button::new(ButtonStyle::Link(&u513)))),
        [RuleId::CV2_08]
    );
    assert_eq!(
        in_row(Component::Button(
            Button::new(ButtonStyle::Premium).label("x")
        )),
        [RuleId::CV2_08]
    );
    assert!(validate_button_label(&a80, &defaults()).is_empty());
    assert_eq!(
        ids(&validate_button_label(&a81, &defaults())),
        [RuleId::CV2_08]
    );
}

#[test]
fn cv2_09_selects() {
    let user = Select::new(SelectKind::User);
    let (p150, p151) = ("a".repeat(150), "a".repeat(151));
    assert_eq!(in_row(Component::Select(user.placeholder(&p150))), []);
    assert_eq!(
        in_row(Component::Select(user.placeholder(&p151))),
        [RuleId::CV2_09]
    );
    let options = [SelectOption::new("a", "a"); 26];
    let string = |n: usize| Component::Select(Select::new(SelectKind::String(&options[..n])));
    assert_eq!(in_row(string(25)), []);
    assert_eq!(in_row(string(26)), [RuleId::CV2_09]);
    assert_eq!(
        in_row(Component::Select(user.min_values(1).max_values(25))),
        []
    );
    assert_eq!(
        in_row(Component::Select(user.min_values(3).max_values(2))),
        [RuleId::CV2_09]
    );
    let ranged = user.min_values(1).max_values(3);
    assert_eq!(in_row(Component::Select(ranged.default_values(3))), []);
    assert_eq!(
        in_row(Component::Select(ranged.default_values(4))),
        [RuleId::CV2_09]
    );
}

#[test]
fn cv2_10_cv2_11_media() {
    let items = [Media::new("https://a/x.png"); 11];
    assert_eq!(tree(&[Component::Gallery(&items[..10])]), []);
    assert_eq!(tree(&[Component::Gallery(&items)]), [RuleId::CV2_10]);
    let (d1024, d1025) = ("a".repeat(1024), "a".repeat(1025));
    assert_eq!(tree(&[Component::Gallery(&described(&d1024))]), []);
    assert_eq!(
        tree(&[Component::Gallery(&described(&d1025))]),
        [RuleId::CV2_10]
    );
    let thumbnail = |d: &str| {
        tree(&[Component::Section(Section::new(
            &["a"],
            Accessory::Thumbnail(Media::new("u").description(d)),
        ))])
    };
    assert_eq!(thumbnail(&d1024), []);
    assert_eq!(thumbnail(&d1025), [RuleId::CV2_11]);
}

#[test]
fn cv2_13_colour_and_spacing() {
    let l = defaults();
    assert!(validate_accent_color(0x00FF_FFFF, &l).is_empty());
    assert_eq!(
        ids(&validate_accent_color(0x0100_0000, &l)),
        [RuleId::CV2_13]
    );
    assert!(validate_spacing(2, &l).is_empty());
    assert_eq!(ids(&validate_spacing(3, &l)), [RuleId::CV2_13]);
}

#[test]
fn cv2_15_legacy_rows() {
    let row = Component::Row(&[B]);
    let legacy = |n: usize| rules_of(validate_legacy_components(&vec![row; n], &defaults()));
    assert_eq!(legacy(5), []);
    assert_eq!(legacy(6), [RuleId::CV2_15]);
}

fn label(child: LabelChild<'_>) -> Vec<RuleId> {
    let components = [ModalComponent::Label(Label::new("L", child))];
    rules_of(validate_modal(&Modal::new("T", &components), &defaults()))
}

#[test]
fn mod_01_mod_02_title_and_count() {
    let text = [ModalComponent::Text("x"); 6];
    let (t45, t46) = ("a".repeat(45), "a".repeat(46));
    let modal = |title: &str, n: usize| {
        rules_of(validate_modal(&Modal::new(title, &text[..n]), &defaults()))
    };
    assert_eq!(modal(&t45, 5), []);
    assert_eq!(modal(&t46, 5), [RuleId::MOD_01]);
    assert_eq!(modal("T", 6), [RuleId::MOD_02]);
    assert!(validate_modal_title(&t45, &defaults()).is_empty());
    assert_eq!(
        ids(&validate_modal_title(&t46, &defaults())),
        [RuleId::MOD_01]
    );
}

#[test]
fn mod_03_mod_04_children_and_labels() {
    let components = [ModalComponent::Message(B)];
    assert_eq!(
        rules_of(validate_modal(&Modal::new("T", &components), &defaults())),
        [RuleId::MOD_03]
    );
    let (l45, l46) = ("a".repeat(45), "a".repeat(46));
    let one = |text: &str| {
        let components = [ModalComponent::Label(Label::new(
            text,
            LabelChild::Checkbox(Checkbox::new()),
        ))];
        rules_of(validate_modal(&Modal::new("T", &components), &defaults()))
    };
    assert_eq!(one(&l45), []);
    assert_eq!(one(&l46), [RuleId::MOD_04]);
}

#[test]
fn mod_05_text_input() {
    let short = TextInput::new(TextInputStyle::Short);
    assert_eq!(label(LabelChild::TextInput(short.max_length(4000))), []);
    assert_eq!(
        label(LabelChild::TextInput(short.max_length(4001))),
        [RuleId::MOD_05]
    );
}

#[test]
fn mod_06_file_upload() {
    assert_eq!(
        label(LabelChild::FileUpload(FileUpload::new().max_values(10))),
        []
    );
    assert_eq!(
        label(LabelChild::FileUpload(FileUpload::new().max_values(11))),
        [RuleId::MOD_06]
    );
}

#[test]
fn mod_07_groups_and_checkbox() {
    let options = [ModalOption::new("a", "a"); 11];
    assert_eq!(
        label(LabelChild::RadioGroup(RadioGroup::new(&options[..10]))),
        []
    );
    assert_eq!(
        label(LabelChild::RadioGroup(RadioGroup::new(&options))),
        [RuleId::MOD_07]
    );
    assert_eq!(
        label(LabelChild::CheckboxGroup(CheckboxGroup::new(
            &options[..10]
        ))),
        []
    );
    assert_eq!(
        label(LabelChild::CheckboxGroup(CheckboxGroup::new(&options))),
        [RuleId::MOD_07]
    );
    assert_eq!(
        label(LabelChild::Checkbox(Checkbox::new().required(true))),
        [RuleId::MOD_07]
    );
}

#[test]
fn msg_01_msg_04_msg_06_message_fields() {
    let l = defaults();
    assert!(validate_content(&"a".repeat(2000), &l).is_empty());
    assert_eq!(
        ids(&validate_content(&"a".repeat(2001), &l)),
        [RuleId::MSG_01]
    );
    let users: Vec<u64> = (0..101).collect();
    let ok = AllowedMentions::new().users(MentionIds::Only(&users[..100]));
    let over = AllowedMentions::new().users(MentionIds::Only(&users));
    assert!(validate_mentions(&ok, &l).is_empty());
    assert_eq!(ids(&validate_mentions(&over, &l)), [RuleId::MSG_04]);
    assert!(validate_nonce(&"a".repeat(25), &l).is_empty());
    assert_eq!(ids(&validate_nonce(&"a".repeat(26), &l)), [RuleId::MSG_06]);
    assert!(validate_sticker_ids(3, &l).is_empty());
    assert_eq!(ids(&validate_sticker_ids(4, &l)), [RuleId::MSG_06]);
}

#[test]
fn wh_04_webhook_name() {
    let l = defaults();
    assert!(validate_webhook_name(&"a".repeat(80), &l).is_empty());
    assert_eq!(
        ids(&validate_webhook_name(&"a".repeat(81), &l)),
        [RuleId::WH_04]
    );
}

#[test]
fn att_01_att_02_att_03_att_06_attachments() {
    let l = defaults();
    let check = |files: &[AttachmentDecl<'_>], json: usize| {
        rules_of(validate_attachments(files, json, None, &l))
    };
    assert_eq!(check(&[AttachmentDecl::new("a.png", 20_971_520)], 0), []);
    assert_eq!(
        check(&[AttachmentDecl::new("a.png", 20_971_521)], 0),
        [RuleId::ATT_01]
    );
    let half = [AttachmentDecl::new("a.png", 13_107_200); 2];
    assert_eq!(check(&half, 0), []);
    assert_eq!(check(&half, 1), [RuleId::ATT_02]);
    let small = [AttachmentDecl::new("a.png", 1); 11];
    assert_eq!(check(&small[..10], 0), []);
    assert_eq!(check(&small, 0), [RuleId::ATT_03]);
    assert!(validate_attachment_description(&"a".repeat(1024), &l).is_empty());
    assert_eq!(
        ids(&validate_attachment_description(&"a".repeat(1025), &l)),
        [RuleId::ATT_06]
    );
    let over = "a".repeat(1025);
    assert_eq!(
        check(&[AttachmentDecl::new("a.png", 1).description(&over)], 0),
        [RuleId::ATT_06]
    );
}

#[test]
fn att_05_attachment_refs() {
    let l = defaults();
    let files = [AttachmentDecl::new("a.png", 1)];
    assert!(validate_attachment_ref("attachment://a.png", &files, &l).is_empty());
    assert_eq!(
        ids(&validate_attachment_ref("attachment://b.png", &files, &l)),
        [RuleId::ATT_05]
    );
}

#[test]
fn int_07_autocomplete_choices() {
    let l = defaults();
    let choices = vec![Choice::new("a", ChoiceValue::Str("a")); 26];
    assert!(validate_autocomplete(&choices[..25], OptionType::String, &l).is_empty());
    let errs = validate_autocomplete(&choices, OptionType::String, &l);
    assert_eq!(ids(&errs), [RuleId::INT_07]);
    assert_eq!(
        errs[0].help.as_deref(),
        Some("Noctis sends the first 25 choices and logs this as a warning")
    );
}

#[test]
fn lowered_override_components() {
    let l = lowered(|l| l.components = 10);
    let texts = vec![Component::Text("a"); 11];
    assert_eq!(rules_of(validate_components(&texts, &defaults())), []);
    assert_eq!(rules_of(validate_components(&texts, &l)), [RuleId::CV2_03]);
}

#[test]
fn lowered_override_button_label_len() {
    let l = lowered(|l| l.button_label_len = 5);
    assert!(validate_button_label("abcdef", &defaults()).is_empty());
    assert_eq!(ids(&validate_button_label("abcdef", &l)), [RuleId::CV2_08]);
    let row = [Component::Button(
        Button::new(ButtonStyle::Primary).label("abcdef"),
    )];
    assert_eq!(
        rules_of(validate_components(&[Component::Row(&row)], &l)),
        [RuleId::CV2_08]
    );
}

#[test]
fn lowered_override_modal_title_len() {
    let l = lowered(|l| l.modal_title_len = 10);
    let title = "a".repeat(11);
    assert!(validate_modal_title(&title, &defaults()).is_empty());
    assert_eq!(ids(&validate_modal_title(&title, &l)), [RuleId::MOD_01]);
    let text = [ModalComponent::Text("x")];
    assert_eq!(
        rules_of(validate_modal(&Modal::new(&title, &text), &l)),
        [RuleId::MOD_01]
    );
}

#[test]
fn lowered_override_attachment_bytes() {
    let l = lowered(|l| l.attachment_bytes = 1000);
    let files = [AttachmentDecl::new("a.png", 1001)];
    assert_eq!(
        rules_of(validate_attachments(&files, 0, None, &defaults())),
        []
    );
    assert_eq!(
        rules_of(validate_attachments(&files, 0, None, &l)),
        [RuleId::ATT_01]
    );
}

#[test]
fn lowered_override_webhook_name_len() {
    let l = lowered(|l| l.webhook_name_len = 10);
    let name = "a".repeat(11);
    assert!(validate_webhook_name(&name, &defaults()).is_empty());
    assert_eq!(ids(&validate_webhook_name(&name, &l)), [RuleId::WH_04]);
}
