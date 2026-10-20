//! Lenient mode: truncating over-long display text with `…` before the
//! checks run.
//!
//! Only these fields are touched, and only when `Limits::truncates` allows
//! their rule: Text Display content (CV2-04 total), button labels (CV2-08),
//! select placeholders and String Select option labels and descriptions
//! (CV2-09), gallery item alt text (CV2-10), Thumbnail alt text (CV2-11),
//! attachment alt text (ATT-06) and legacy content (MSG-01). In a modal:
//! the title (MOD-01), Label text and description (MOD-04), Text Input
//! placeholders (MOD-05), modal select placeholders and String Select option
//! labels and descriptions (CV2-09), and Radio and Checkbox Group option
//! labels and descriptions (MOD-07); never a Text Input `value`, an option
//! value, a key or a count.

use std::borrow::Cow;

use noctis_rules::len::{discord_len, truncate};
use noctis_rules::{Actual, Bound, Limits, RuleId, Violation};

use super::LimitWarning;
use super::walk::child_kind;
use crate::component::{
    AccessoryNode, ActionRow, Button, ButtonNode, ChildNode, ContainerComponent, RowNode,
    SelectNode, TopNode,
};
use crate::modal::{ModalInputNode, ModalTopNode};
use crate::{LegacyMessage, Modal, ModalOption, SelectOption, V2Message};

type Text = Cow<'static, str>;

/// A truncatable field with its rule, limit and path.
struct Field<'m> {
    rule: RuleId,
    max: usize,
    path: String,
    text: &'m mut Text,
}

/// The truncatable fields of a tree, in pre-order, and its Text Displays.
#[derive(Default)]
struct Sites<'m> {
    fields: Vec<Field<'m>>,
    texts: Vec<(String, &'m mut Text)>,
}

impl<'m> Sites<'m> {
    fn field(&mut self, rule: RuleId, max: usize, path: String, text: &'m mut Text) {
        self.fields.push(Field {
            rule,
            max,
            path,
            text,
        });
    }
}

/// Truncates the display text of a V2 message: per-field in pre-order, then
/// the CV2-04 total, then attachment alt text.
pub(super) fn fit_v2(msg: &mut V2Message, limits: &Limits, warnings: &mut Vec<LimitWarning>) {
    let mut sites = Sites::default();
    for (i, item) in msg.components.items.iter_mut().enumerate() {
        match &mut item.0 {
            TopNode::Container(c) => {
                for (j, child) in c.children.iter_mut().enumerate() {
                    let path = format!("container[{i}].{}[{j}]", child_kind(&child.0));
                    child_sites(child, path, limits, &mut sites);
                }
            }
            TopNode::Child(child) => {
                let path = format!("{}[{i}]", child_kind(&child.0));
                child_sites(child, path, limits, &mut sites);
            }
        }
    }
    fit_fields(sites.fields, limits, warnings);
    fit_texts(sites.texts, limits, warnings);
    for (k, attachment) in msg.attachments.iter_mut().enumerate() {
        if let Some(alt) = &mut attachment.alt
            && let Some(v) = fit(
                RuleId::ATT_06,
                limits.attachment_description_len,
                format!("attachments[{k}].description"),
                alt,
                limits,
            )
        {
            let note = format!("file `{}`", attachment.name());
            warnings.push(LimitWarning::Truncated(v.with_note(note)));
        }
    }
}

/// Truncates the display text of a legacy message: per-field in row order,
/// then the content.
pub(super) fn fit_legacy(
    msg: &mut LegacyMessage,
    limits: &Limits,
    warnings: &mut Vec<LimitWarning>,
) {
    let mut sites = Sites::default();
    for (i, row) in msg.rows.iter_mut().enumerate() {
        row_sites(row, &format!("row[{i}]"), limits, &mut sites);
    }
    fit_fields(sites.fields, limits, warnings);
    if let Some(v) = fit(
        RuleId::MSG_01,
        limits.content_len,
        "content".to_owned(),
        &mut msg.content,
        limits,
    ) {
        warnings.push(LimitWarning::Truncated(v));
    }
}

/// Truncates `text` to `max` when it is longer and `rule` may truncate,
/// returning the violation it avoided.
fn fit(
    rule: RuleId,
    max: usize,
    path: String,
    text: &mut Text,
    limits: &Limits,
) -> Option<Violation> {
    let len = discord_len(text);
    if len <= max || !limits.truncates(rule) {
        return None;
    }
    *text = Cow::Owned(truncate(text, max).into_owned());
    Some(Violation::new(
        rule,
        path,
        Bound::AtMost(max),
        Actual::Count(len),
    ))
}

fn fit_fields(fields: Vec<Field<'_>>, limits: &Limits, warnings: &mut Vec<LimitWarning>) {
    for f in fields {
        if let Some(v) = fit(f.rule, f.max, f.path, f.text, limits) {
            warnings.push(LimitWarning::Truncated(v));
        }
    }
}

/// CV2-04 in Lenient mode.
///
/// With `n` texts and the limit `L`, text `j` may keep
/// `L − used − (n − j − 1)` units, leaving one unit for each later text. The
/// first text over its allowance is truncated to it and every later one to
/// one unit (`…`). This fits whenever `L ≥ n`; otherwise nothing changes and
/// the check fails.
fn fit_texts(texts: Vec<(String, &mut Text)>, limits: &Limits, warnings: &mut Vec<LimitWarning>) {
    let max = limits.text_total_len;
    let n = texts.len();
    let total = texts
        .iter()
        .fold(0_usize, |acc, (_, t)| acc.saturating_add(discord_len(t)));
    if total <= max || max < n || !limits.truncates(RuleId::CV2_04) {
        return;
    }
    let mut used = 0_usize;
    let mut cut = false;
    let mut warning = Violation::new(
        RuleId::CV2_04,
        "components",
        Bound::AtMost(max),
        Actual::Count(total),
    );
    for (j, (path, text)) in texts.into_iter().enumerate() {
        let len = discord_len(text);
        let allowance = if cut {
            1
        } else {
            max.saturating_sub(used).saturating_sub(n - j - 1)
        };
        if !cut && len <= allowance {
            used += len;
            continue;
        }
        cut = true;
        if len > allowance {
            *text = Cow::Owned(truncate(text, allowance).into_owned());
            warning = warning.with_note(format!("truncated {path}"));
        }
    }
    warnings.push(LimitWarning::Truncated(warning));
}

fn child_sites<'m>(
    child: &'m mut ContainerComponent,
    path: String,
    limits: &Limits,
    sites: &mut Sites<'m>,
) {
    match &mut child.0 {
        ChildNode::Section(s) => {
            for (k, t) in s.texts.iter_mut().enumerate() {
                sites
                    .texts
                    .push((format!("{path}.text[{k}]"), &mut t.content));
            }
            let accessory = format!("{path}.accessory");
            match &mut s.accessory {
                AccessoryNode::Button(b) => button_sites(b, &accessory, limits, sites),
                AccessoryNode::Thumbnail(t) => {
                    if let Some(alt) = &mut t.alt {
                        sites.field(
                            RuleId::CV2_11,
                            limits.thumbnail_description_len,
                            format!("{accessory}.description"),
                            alt,
                        );
                    }
                }
            }
        }
        ChildNode::Text(t) => sites.texts.push((path, &mut t.content)),
        ChildNode::Gallery(g) => {
            for (k, item) in g.items.iter_mut().enumerate() {
                if let Some(alt) = &mut item.alt {
                    sites.field(
                        RuleId::CV2_10,
                        limits.gallery_description_len,
                        format!("{path}.item[{k}].description"),
                        alt,
                    );
                }
            }
        }
        ChildNode::Row(row) => row_sites(row, &path, limits, sites),
        ChildNode::File(_) | ChildNode::Separator(_) => {}
    }
}

fn row_sites<'m>(row: &'m mut ActionRow, path: &str, limits: &Limits, sites: &mut Sites<'m>) {
    match &mut row.0 {
        RowNode::Buttons(buttons) => {
            for (j, b) in buttons.iter_mut().enumerate() {
                button_sites(b, &format!("{path}.button[{j}]"), limits, sites);
            }
        }
        RowNode::Select(s) => select_sites(s, &format!("{path}.select[0]"), limits, sites),
    }
}

fn button_sites<'m>(button: &'m mut Button, path: &str, limits: &Limits, sites: &mut Sites<'m>) {
    let label = match &mut button.0 {
        ButtonNode::Route(d) => &mut d.label,
        ButtonNode::Link(l) => &mut l.label,
        ButtonNode::Premium(_) => return,
    };
    if let Some(label) = label {
        sites.field(
            RuleId::CV2_08,
            limits.button_label_len,
            format!("{path}.label"),
            label,
        );
    }
}

fn select_sites<'m>(
    select: &'m mut SelectNode,
    path: &str,
    limits: &Limits,
    sites: &mut Sites<'m>,
) {
    let (common, options) = select.parts_mut();
    select_text_sites(common.placeholder.as_mut(), options, path, limits, sites);
}

/// A select's placeholder and String Select option labels and descriptions
/// (CV2-09), for message and modal selects alike.
fn select_text_sites<'m>(
    placeholder: Option<&'m mut Text>,
    options: Option<&'m mut Vec<SelectOption>>,
    path: &str,
    limits: &Limits,
    sites: &mut Sites<'m>,
) {
    if let Some(p) = placeholder {
        sites.field(
            RuleId::CV2_09,
            limits.select_placeholder_len,
            format!("{path}.placeholder"),
            p,
        );
    }
    for (k, option) in options.into_iter().flatten().enumerate() {
        let option_path = format!("{path}.options[{k}]");
        sites.field(
            RuleId::CV2_09,
            limits.select_option_label_len,
            format!("{option_path}.label"),
            &mut option.label,
        );
        if let Some(d) = &mut option.description {
            sites.field(
                RuleId::CV2_09,
                limits.select_option_description_len,
                format!("{option_path}.description"),
                d,
            );
        }
    }
}

/// Truncates the display text of a modal, in pre-order: the title, then per
/// Label its text, description and input text.
pub(super) fn fit_modal(modal: &mut Modal, limits: &Limits, warnings: &mut Vec<LimitWarning>) {
    let mut sites = Sites::default();
    sites.field(
        RuleId::MOD_01,
        limits.modal_title_len,
        "title".to_owned(),
        &mut modal.title,
    );
    for (i, component) in modal.components.iter_mut().enumerate() {
        let ModalTopNode::Label(label) = &mut component.0 else {
            continue;
        };
        let path = format!("label[{i}]");
        sites.field(
            RuleId::MOD_04,
            limits.label_len,
            format!("{path}.label"),
            &mut label.label,
        );
        if let Some(d) = &mut label.description {
            sites.field(
                RuleId::MOD_04,
                limits.label_description_len,
                format!("{path}.description"),
                d,
            );
        }
        let child = format!("{path}.{}", label.input.kind());
        match &mut label.input {
            ModalInputNode::TextInput(t) => {
                if let Some(p) = &mut t.placeholder {
                    sites.field(
                        RuleId::MOD_05,
                        limits.text_input_placeholder_len,
                        format!("{child}.placeholder"),
                        p,
                    );
                }
            }
            ModalInputNode::Select(s) => {
                let (common, options) = s.parts_mut();
                select_text_sites(
                    common.placeholder.as_mut(),
                    options,
                    &child,
                    limits,
                    &mut sites,
                );
            }
            ModalInputNode::RadioGroup(r) => {
                modal_option_sites(&mut r.options, &child, limits, &mut sites);
            }
            ModalInputNode::CheckboxGroup(g) => {
                modal_option_sites(&mut g.options, &child, limits, &mut sites);
            }
            ModalInputNode::FileUpload(_) | ModalInputNode::Checkbox(_) => {}
        }
    }
    fit_fields(sites.fields, limits, warnings);
}

/// Radio and Checkbox Group option labels and descriptions (MOD-07).
fn modal_option_sites<'m>(
    options: &'m mut [ModalOption],
    path: &str,
    limits: &Limits,
    sites: &mut Sites<'m>,
) {
    let max = limits.modal_option_text_len;
    for (k, option) in options.iter_mut().enumerate() {
        let option_path = format!("{path}.options[{k}]");
        sites.field(
            RuleId::MOD_07,
            max,
            format!("{option_path}.label"),
            &mut option.label,
        );
        if let Some(d) = &mut option.description {
            sites.field(RuleId::MOD_07, max, format!("{option_path}.description"), d);
        }
    }
}
