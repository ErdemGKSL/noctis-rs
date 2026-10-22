//! Modals: [`validate_modal`] (MOD-01..07, CV2-09 for modal selects) and
//! granular functions for single literals.
//!
//! `derive(Modal)` and the runtime validator describe a modal as a [`Modal`]
//! and call these functions. The descriptors carry no `custom_id`: the
//! modal's own is a Route's (MOD-08, checked as CID-01 by `derive(Modal)`),
//! and each input's key is checked by [`validate_input_custom_id`] (CID-01).
//!
//! # Paths
//!
//! `title`, `components`, then per top-level component `label[i]`, `text[i]`,
//! or `kind[i]` of a [`ModalComponent::Message`] component
//! ([`Component::kind`]). A Label's child is `label[i].text_input`,
//! `.select`, `.file_upload`, `.radio_group`, `.checkbox_group` or
//! `.checkbox`; fields append `.label`, `.description`, `.min_length`,
//! `.max_length`, `.value`, `.placeholder`, `.min_values`, `.max_values`,
//! `.disabled`, `.required`, `.file_types[k]`, `.options[k].label`, … .
//!
//! # Effective defaults
//!
//! Discord applies `min_values` 1 and `max_values` 1 to selects and File
//! Uploads, and `min_values` 1 and `max_values` = option count to Checkbox
//! Groups. These drive the `min ≤ max` checks, which run only when both
//! effective values are in range.
//!
//! [`ModalComponent::Message`] carries a message component, which a modal
//! rejects (MOD-03); a Label child can never be a button:
//!
//! ```compile_fail
//! use noctis_rules::modal::LabelChild;
//!
//! let _ = LabelChild::Button; // [MOD-03]
//! ```
//!
//! The functions only report: they never truncate.

use crate::check::{at_most, between, finish, join, text_at_most, text_between};
use crate::command::{FILE_TYPE, is_file_type};
use crate::component::{Component, Select, check_select};
use crate::len::discord_len;
use crate::{Actual, Bound, Limits, RuleId, Violation};

const REQUIRED_MIN: &str = "min_values ≥ 1 when required";

/// A modal: title and top-level components (MOD-01, MOD-02).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modal<'a> {
    /// The title (1-45).
    pub title: &'a str,
    /// The top-level components (1-5).
    pub components: &'a [ModalComponent<'a>],
}

impl<'a> Modal<'a> {
    /// A modal with its title and components.
    pub const fn new(title: &'a str, components: &'a [ModalComponent<'a>]) -> Self {
        Self { title, components }
    }
}

/// A top-level modal component.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModalComponent<'a> {
    /// A Label wrapping one input.
    Label(Label<'a>),
    /// A Text Display.
    Text(&'a str),
    /// A message component; only [`Component::Text`] is allowed (MOD-03).
    Message(Component<'a>),
}

/// A Label: text, optional description and one input (MOD-04).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Label<'a> {
    /// The label text (1-45).
    pub label: &'a str,
    /// The description (≤ 100).
    pub description: Option<&'a str>,
    /// The wrapped input.
    pub child: LabelChild<'a>,
}

impl<'a> Label<'a> {
    /// A Label without description.
    pub const fn new(label: &'a str, child: LabelChild<'a>) -> Self {
        Self {
            label,
            description: None,
            child,
        }
    }

    /// Sets the description.
    #[must_use]
    pub const fn description(self, description: &'a str) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }
}

/// The input inside a [`Label`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LabelChild<'a> {
    /// A Text Input (MOD-05).
    TextInput(TextInput<'a>),
    /// A select (CV2-09, MOD-03).
    Select(ModalSelect<'a>),
    /// A File Upload (MOD-06).
    FileUpload(FileUpload<'a>),
    /// A Radio Group (MOD-07).
    RadioGroup(RadioGroup<'a>),
    /// A Checkbox Group (MOD-07).
    CheckboxGroup(CheckboxGroup<'a>),
    /// A Checkbox (MOD-07).
    Checkbox(Checkbox),
}

/// A Text Input style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextInputStyle {
    /// One line (1).
    Short,
    /// Several lines (2).
    Paragraph,
}

impl TextInputStyle {
    /// The Discord style value: 1 or 2.
    pub const fn value(self) -> u8 {
        match self {
            Self::Short => 1,
            Self::Paragraph => 2,
        }
    }
}

/// A Text Input (MOD-05).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextInput<'a> {
    /// The style.
    pub style: TextInputStyle,
    /// `min_length` (0-4000).
    pub min_length: Option<usize>,
    /// `max_length` (1-4000).
    pub max_length: Option<usize>,
    /// Whether the input is required (default `true`).
    pub required: bool,
    /// The pre-filled value (≤ 4000).
    pub value: Option<&'a str>,
    /// The placeholder (≤ 100).
    pub placeholder: Option<&'a str>,
}

impl<'a> TextInput<'a> {
    /// A required Text Input with everything else unset.
    pub const fn new(style: TextInputStyle) -> Self {
        Self {
            style,
            min_length: None,
            max_length: None,
            required: true,
            value: None,
            placeholder: None,
        }
    }

    /// Sets `min_length`.
    #[must_use]
    pub const fn min_length(self, length: usize) -> Self {
        Self {
            min_length: Some(length),
            ..self
        }
    }

    /// Sets `max_length`.
    #[must_use]
    pub const fn max_length(self, length: usize) -> Self {
        Self {
            max_length: Some(length),
            ..self
        }
    }

    /// Sets whether the input is required.
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }

    /// Sets the pre-filled value.
    #[must_use]
    pub const fn value(self, value: &'a str) -> Self {
        Self {
            value: Some(value),
            ..self
        }
    }

    /// Sets the placeholder.
    #[must_use]
    pub const fn placeholder(self, placeholder: &'a str) -> Self {
        Self {
            placeholder: Some(placeholder),
            ..self
        }
    }
}

/// A select inside a modal: the select and whether it is required.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalSelect<'a> {
    /// The select (its `disabled` must stay `false`, MOD-03).
    pub select: Select<'a>,
    /// Whether a value is required (default `true`).
    pub required: bool,
}

impl<'a> ModalSelect<'a> {
    /// A required modal select.
    pub const fn new(select: Select<'a>) -> Self {
        Self {
            select,
            required: true,
        }
    }

    /// Sets whether a value is required.
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }
}

/// A File Upload (MOD-06).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FileUpload<'a> {
    /// `min_values` (0-10, default 1).
    pub min_values: Option<usize>,
    /// `max_values` (1-10, default 1).
    pub max_values: Option<usize>,
    /// Whether an upload is required (default `true`).
    pub required: bool,
    /// The upload filter, as OPT-13 (empty means unset).
    pub file_types: &'a [&'a str],
}

impl<'a> FileUpload<'a> {
    /// A required File Upload with everything else unset.
    pub const fn new() -> Self {
        Self {
            min_values: None,
            max_values: None,
            required: true,
            file_types: &[],
        }
    }

    /// Sets `min_values`.
    #[must_use]
    pub const fn min_values(self, min: usize) -> Self {
        Self {
            min_values: Some(min),
            ..self
        }
    }

    /// Sets `max_values`.
    #[must_use]
    pub const fn max_values(self, max: usize) -> Self {
        Self {
            max_values: Some(max),
            ..self
        }
    }

    /// Sets whether an upload is required.
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }

    /// Sets the upload filter.
    #[must_use]
    pub const fn file_types(self, file_types: &'a [&'a str]) -> Self {
        Self { file_types, ..self }
    }
}

impl Default for FileUpload<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A Radio Group or Checkbox Group option (MOD-07).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalOption<'a> {
    /// The label (1 to `modal_option_text_len`).
    pub label: &'a str,
    /// The value (1 to `modal_option_text_len`).
    pub value: &'a str,
    /// The description (≤ `modal_option_text_len`).
    pub description: Option<&'a str>,
    /// Whether the option is selected by default.
    pub default: bool,
}

impl<'a> ModalOption<'a> {
    /// An option without description, not a default.
    pub const fn new(label: &'a str, value: &'a str) -> Self {
        Self {
            label,
            value,
            description: None,
            default: false,
        }
    }

    /// Sets the description.
    #[must_use]
    pub const fn description(self, description: &'a str) -> Self {
        Self {
            description: Some(description),
            ..self
        }
    }

    /// Sets whether the option is selected by default.
    #[must_use]
    pub const fn default(self, default: bool) -> Self {
        Self { default, ..self }
    }
}

/// A Radio Group: 2-10 options (MOD-07).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadioGroup<'a> {
    /// The options.
    pub options: &'a [ModalOption<'a>],
    /// Whether a choice is required (default `true`).
    pub required: bool,
}

impl<'a> RadioGroup<'a> {
    /// A required Radio Group.
    pub const fn new(options: &'a [ModalOption<'a>]) -> Self {
        Self {
            options,
            required: true,
        }
    }

    /// Sets whether a choice is required.
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }
}

/// A Checkbox Group: 1-10 options, `min_values` / `max_values` (MOD-07).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CheckboxGroup<'a> {
    /// The options.
    pub options: &'a [ModalOption<'a>],
    /// `min_values` (0-10, default 1).
    pub min_values: Option<usize>,
    /// `max_values` (1-10, default the option count).
    pub max_values: Option<usize>,
    /// Whether a choice is required (default `true`).
    pub required: bool,
}

impl<'a> CheckboxGroup<'a> {
    /// A required Checkbox Group with `min_values` / `max_values` unset.
    pub const fn new(options: &'a [ModalOption<'a>]) -> Self {
        Self {
            options,
            min_values: None,
            max_values: None,
            required: true,
        }
    }

    /// Sets `min_values`.
    #[must_use]
    pub const fn min_values(self, min: usize) -> Self {
        Self {
            min_values: Some(min),
            ..self
        }
    }

    /// Sets `max_values`.
    #[must_use]
    pub const fn max_values(self, max: usize) -> Self {
        Self {
            max_values: Some(max),
            ..self
        }
    }

    /// Sets whether a choice is required.
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }
}

/// A single Checkbox; never required (MOD-07).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Checkbox {
    /// Whether it starts checked.
    pub default: bool,
    /// Must stay `false` (MOD-07).
    pub required: bool,
}

impl Checkbox {
    /// An unchecked, optional Checkbox.
    pub const fn new() -> Self {
        Self {
            default: false,
            required: false,
        }
    }

    /// Sets whether it starts checked.
    #[must_use]
    pub const fn default(self, default: bool) -> Self {
        Self { default, ..self }
    }

    /// Sets `required` (any `true` is a MOD-07 violation).
    #[must_use]
    pub const fn required(self, required: bool) -> Self {
        Self { required, ..self }
    }
}

/// Checks a whole modal.
///
/// The order is MOD-01, MOD-02, then per top-level component in input order:
/// MOD-03 for a [`ModalComponent::Message`] (not walked; a message
/// [`Component::Text`] is allowed like [`ModalComponent::Text`]); for a Label
/// its text, its description, then the child. `TextInput`: `min_length`,
/// `max_length`, order, `value` (≤ `text_input_len`, then ≤ its own
/// `max_length`), `placeholder`. `Select`: MOD-03 `disabled`, the
/// CV2-09 checks, then required with `min_values` 0. `FileUpload`: `min_values`,
/// `max_values`, order, required with `min_values` 0, `file_types` count, each
/// entry. `RadioGroup`: options count, per option label, value,
/// description, then at most one default (`default_values`).
/// `CheckboxGroup`: options count, `min_values`, `max_values`, order,
/// required with `min_values` 0, per option, then (with at least one
/// default and an ordered range) the default count within the effective
/// `[min_values, max_values]` (`default_values`). `Checkbox`: `required`.
///
/// # Errors
///
/// Returns every violation found; never an empty vector.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::modal::{Label, LabelChild, Modal, ModalComponent, TextInput, TextInputStyle, validate_modal};
///
/// let input = TextInput::new(TextInputStyle::Short).min_length(10).max_length(5);
/// let components = [ModalComponent::Label(Label::new("Reason", LabelChild::TextInput(input)))];
/// let errs = validate_modal(&Modal::new("Report", &components), &Limits::default()).unwrap_err();
/// assert_eq!(errs[0].rule, RuleId::MOD_05);
/// assert_eq!(errs[0].path, "label[0].text_input");
/// ```
pub fn validate_modal(modal: &Modal<'_>, limits: &Limits) -> Result<(), Vec<Violation>> {
    let mut out = Vec::new();
    text_between(
        RuleId::MOD_01,
        "title",
        1,
        limits.modal_title_len,
        modal.title,
        &mut out,
    );
    between(
        RuleId::MOD_02,
        "components",
        1,
        limits.modal_components,
        modal.components.len(),
        &mut out,
    );
    for (i, component) in modal.components.iter().enumerate() {
        match component {
            ModalComponent::Text(_) | ModalComponent::Message(Component::Text(_)) => {}
            ModalComponent::Message(c) => out.push(Violation::new(
                RuleId::MOD_03,
                format!("{}[{i}]", c.kind()),
                Bound::Describe("only Labels or Text Displays at the top level of a modal"),
                Actual::Text(c.kind().to_owned()),
            )),
            ModalComponent::Label(label) => {
                check_label(label, &format!("label[{i}]"), limits, &mut out);
            }
        }
    }
    finish(out)
}

fn check_label(label: &Label<'_>, path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    text_between(
        RuleId::MOD_04,
        join(path, "label"),
        1,
        limits.label_len,
        label.label,
        out,
    );
    if let Some(d) = label.description {
        text_at_most(
            RuleId::MOD_04,
            join(path, "description"),
            limits.label_description_len,
            d,
            out,
        );
    }
    match &label.child {
        LabelChild::TextInput(t) => check_text_input(t, &join(path, "text_input"), limits, out),
        LabelChild::Select(s) => check_modal_select(s, &join(path, "select"), limits, out),
        LabelChild::FileUpload(f) => check_file_upload(f, &join(path, "file_upload"), limits, out),
        LabelChild::RadioGroup(r) => {
            let p = join(path, "radio_group");
            between(
                RuleId::MOD_07,
                join(&p, "options"),
                2,
                limits.radio_options,
                r.options.len(),
                out,
            );
            check_options(r.options, &p, limits, out);
            let defaults = r.options.iter().filter(|o| o.default).count();
            at_most(RuleId::MOD_07, join(&p, "default_values"), 1, defaults, out);
        }
        LabelChild::CheckboxGroup(g) => {
            check_checkbox_group(g, &join(path, "checkbox_group"), limits, out);
        }
        LabelChild::Checkbox(c) => {
            if c.required {
                out.push(Violation::new(
                    RuleId::MOD_07,
                    format!("{path}.checkbox.required"),
                    Bound::Describe("a Checkbox is never required"),
                    Actual::Text("true".to_owned()),
                ));
            }
        }
    }
}

fn ordering(rule: RuleId, path: &str, what: &'static str, min: usize, max: usize) -> Violation {
    Violation::new(
        rule,
        path,
        Bound::Describe(what),
        Actual::Text(format!("{min} > {max}")),
    )
}

/// MOD-05: `min_length`, `max_length`, order, `value`, `placeholder`.
fn check_text_input(input: &TextInput<'_>, path: &str, limits: &Limits, out: &mut Vec<Violation>) {
    let max_len = limits.text_input_len;
    if let Some(min) = input.min_length {
        between(
            RuleId::MOD_05,
            join(path, "min_length"),
            0,
            max_len,
            min,
            out,
        );
    }
    if let Some(max) = input.max_length {
        between(
            RuleId::MOD_05,
            join(path, "max_length"),
            1,
            max_len,
            max,
            out,
        );
    }
    let min = input.min_length.filter(|&m| m <= max_len);
    let max = input.max_length.filter(|m| (1..=max_len).contains(m));
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        out.push(ordering(
            RuleId::MOD_05,
            path,
            "min_length ≤ max_length",
            min,
            max,
        ));
    }
    if let Some(v) = input.value {
        let len = discord_len(v);
        let value_path = join(path, "value");
        if len > max_len {
            text_at_most(RuleId::MOD_05, value_path, max_len, v, out);
        } else if let Some(max) = max
            && len > max
        {
            // A pre-fill longer than the input's own `max_length`; a value
            // shorter than `min_length` is not checked.
            at_most(RuleId::MOD_05, value_path, max, len, out);
        }
    }
    if let Some(p) = input.placeholder {
        text_at_most(
            RuleId::MOD_05,
            join(path, "placeholder"),
            limits.text_input_placeholder_len,
            p,
            out,
        );
    }
}

/// MOD-03 `disabled`, CV2-09, then CV2-09 required with `min_values` 0.
fn check_modal_select(
    select: &ModalSelect<'_>,
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    if select.select.disabled {
        out.push(Violation::new(
            RuleId::MOD_03,
            join(path, "disabled"),
            Bound::Describe("no disabled in a modal"),
            Actual::Text("true".to_owned()),
        ));
    }
    check_select(&select.select, path, limits, out);
    check_required_min(
        RuleId::CV2_09,
        select.required,
        select.select.min_values,
        path,
        out,
    );
}

fn check_required_min(
    rule: RuleId,
    required: bool,
    min: Option<usize>,
    path: &str,
    out: &mut Vec<Violation>,
) {
    if required && min == Some(0) {
        out.push(Violation::new(
            rule,
            join(path, "min_values"),
            Bound::Describe(REQUIRED_MIN),
            Actual::Count(0),
        ));
    }
}

/// `min_values` (0..=`limit`), `max_values` (1..=`limit`), then the order of the
/// effective values (defaults `default_min` / `default_max`) when both are in
/// range. The effective `(min, max)` when both are in range and ordered.
fn check_value_range(
    rule: RuleId,
    (min, max): (Option<usize>, Option<usize>),
    (default_min, default_max): (usize, usize),
    limit: usize,
    path: &str,
    out: &mut Vec<Violation>,
) -> Option<(usize, usize)> {
    if let Some(min) = min {
        between(rule, join(path, "min_values"), 0, limit, min, out);
    }
    if let Some(max) = max {
        between(rule, join(path, "max_values"), 1, limit, max, out);
    }
    let min = min.unwrap_or(default_min);
    let max = max.unwrap_or(default_max);
    if min <= limit && (1..=limit).contains(&max) {
        if min > max {
            out.push(ordering(rule, path, "min_values ≤ max_values", min, max));
            return None;
        }
        return Some((min, max));
    }
    None
}

/// MOD-06: `min_values`, `max_values`, order, required, `file_types`.
fn check_file_upload(
    upload: &FileUpload<'_>,
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let _ = check_value_range(
        RuleId::MOD_06,
        (upload.min_values, upload.max_values),
        (1, 1),
        limits.file_upload_values,
        path,
        out,
    );
    check_required_min(
        RuleId::MOD_06,
        upload.required,
        upload.min_values,
        path,
        out,
    );
    let types_path = join(path, "file_types");
    at_most(
        RuleId::MOD_06,
        types_path.as_str(),
        limits.file_upload_file_types,
        upload.file_types.len(),
        out,
    );
    for (k, entry) in upload.file_types.iter().enumerate() {
        if !is_file_type(entry) {
            out.push(Violation::new(
                RuleId::MOD_06,
                format!("{types_path}[{k}]"),
                Bound::Describe(FILE_TYPE),
                Actual::Text((*entry).to_owned()),
            ));
        }
    }
}

/// MOD-07: options count, `min_values`, `max_values`, order, required, options.
fn check_checkbox_group(
    group: &CheckboxGroup<'_>,
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    between(
        RuleId::MOD_07,
        join(path, "options"),
        1,
        limits.checkbox_group_options,
        group.options.len(),
        out,
    );
    let ordered = check_value_range(
        RuleId::MOD_07,
        (group.min_values, group.max_values),
        (1, group.options.len()),
        limits.checkbox_group_values,
        path,
        out,
    );
    check_required_min(RuleId::MOD_07, group.required, group.min_values, path, out);
    check_options(group.options, path, limits, out);
    let defaults = group.options.iter().filter(|o| o.default).count();
    if let Some((min, max)) = ordered
        && defaults > 0
    {
        between(
            RuleId::MOD_07,
            join(path, "default_values"),
            min,
            max,
            defaults,
            out,
        );
    }
}

/// MOD-07 per option: label, value, description.
fn check_options(
    options: &[ModalOption<'_>],
    path: &str,
    limits: &Limits,
    out: &mut Vec<Violation>,
) {
    let max = limits.modal_option_text_len;
    for (k, option) in options.iter().enumerate() {
        let p = format!("{path}.options[{k}]");
        text_between(RuleId::MOD_07, join(&p, "label"), 1, max, option.label, out);
        text_between(RuleId::MOD_07, join(&p, "value"), 1, max, option.value, out);
        if let Some(d) = option.description {
            text_at_most(RuleId::MOD_07, join(&p, "description"), max, d, out);
        }
    }
}

/// MOD-01: a modal title of 1 to `modal_title_len` characters. The path is
/// empty.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::modal::validate_modal_title;
///
/// assert!(validate_modal_title("Report", &Limits::default()).is_empty());
/// assert_eq!(validate_modal_title("", &Limits::default())[0].rule, RuleId::MOD_01);
/// ```
pub fn validate_modal_title(title: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_between(
        RuleId::MOD_01,
        "",
        1,
        limits.modal_title_len,
        title,
        &mut out,
    );
    out
}

/// MOD-04: a Label text of 1 to `label_len` characters. The path is empty.
pub fn validate_label_text(label: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_between(RuleId::MOD_04, "", 1, limits.label_len, label, &mut out);
    out
}

/// MOD-04: a Label description of at most `label_description_len` characters
/// (empty is allowed). The path is empty.
pub fn validate_label_description(description: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(
        RuleId::MOD_04,
        "",
        limits.label_description_len,
        description,
        &mut out,
    );
    out
}

/// MOD-05: one Text Input on its own, in the [`validate_modal`] order; paths
/// are `min_length`, `max_length`, `value`, `placeholder`, and `""` for the
/// `min_length ≤ max_length` order.
pub fn validate_text_input(input: &TextInput<'_>, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    check_text_input(input, "", limits, &mut out);
    out
}

/// MOD-05: a Text Input pre-fill value of at most `text_input_len` characters.
/// The path is empty.
pub fn validate_text_input_value(value: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_at_most(RuleId::MOD_05, "", limits.text_input_len, value, &mut out);
    out
}

/// CID-01: a modal input's `custom_id` (its key) of 1 to `custom_id_len`
/// characters, measured with `discord_len`. The path is empty.
///
/// ```
/// use noctis_rules::{Limits, RuleId};
/// use noctis_rules::modal::validate_input_custom_id;
///
/// assert!(validate_input_custom_id("reason", &Limits::default()).is_empty());
/// assert_eq!(validate_input_custom_id("", &Limits::default())[0].rule, RuleId::CID_01);
/// assert_eq!(validate_input_custom_id(&"a".repeat(101), &Limits::default())[0].path, "");
/// ```
pub fn validate_input_custom_id(id: &str, limits: &Limits) -> Vec<Violation> {
    let mut out = Vec::new();
    text_between(RuleId::CID_01, "", 1, limits.custom_id_len, id, &mut out);
    out
}

#[cfg(test)]
mod tests;
