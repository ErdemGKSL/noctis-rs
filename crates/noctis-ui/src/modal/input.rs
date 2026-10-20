//! Modal inputs other than selects: Text Input (type 4), File Upload (19),
//! Radio Group (21), Checkbox Group (22) and Checkbox (23), plus their
//! [`ModalOption`]s (MOD-05, MOD-06, MOD-07).

use std::borrow::Cow;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

pub use noctis_rules::modal::TextInputStyle;

use crate::wire::{Ids, Wire, flag, head, opt, serialize_via_wire};

type Text = Cow<'static, str>;

/// Writes `"required": false` only when `required` is `false` (Discord's
/// default is `true`).
pub(crate) fn required_entry<M: SerializeMap>(map: &mut M, required: bool) -> Result<(), M::Error> {
    if required {
        Ok(())
    } else {
        map.serialize_entry("required", &false)
    }
}

/// A Text Input (type 4): free text inside a [`ModalLabel`](crate::ModalLabel).
///
/// Placement: only as a modal Label's input ([`ModalInput`](crate::ModalInput)).
/// It never carries a `label` and is never wrapped in an Action Row; the
/// Label holds the text. `key` is its `custom_id`, the name of the
/// submitted field. Limits (MOD-05): `min_length` 0-4000, `max_length`
/// 1-4000, `value` ≤ 4000 (never truncated), `placeholder` ≤ 100.
///
/// ```
/// use noctis_ui::{TextInput, TextInputStyle};
///
/// let input = TextInput::new("reason", TextInputStyle::Paragraph)
///     .max_length(500)
///     .required(false)
///     .value("draft")
///     .placeholder("What happened?");
/// assert_eq!(
///     serde_json::to_value(&input).unwrap(),
///     serde_json::json!({
///         "type": 4, "custom_id": "reason", "style": 2, "max_length": 500,
///         "required": false, "value": "draft", "placeholder": "What happened?"
///     })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct TextInput {
    pub(crate) key: Text,
    pub(crate) style: TextInputStyle,
    pub(crate) min_length: Option<u16>,
    pub(crate) max_length: Option<u16>,
    pub(crate) required: bool,
    pub(crate) value: Option<Text>,
    pub(crate) placeholder: Option<Text>,
}

impl TextInput {
    /// A required Text Input whose submitted field is named `key`.
    pub fn new(key: impl Into<Text>, style: TextInputStyle) -> Self {
        Self {
            key: key.into(),
            style,
            min_length: None,
            max_length: None,
            required: true,
            value: None,
            placeholder: None,
        }
    }

    /// The fewest characters accepted (0-4000).
    #[must_use]
    pub fn min_length(mut self, length: u16) -> Self {
        self.min_length = Some(length);
        self
    }

    /// The most characters accepted (1-4000).
    #[must_use]
    pub fn max_length(mut self, length: u16) -> Self {
        self.max_length = Some(length);
        self
    }

    /// Whether the user must fill it in (default `true`).
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// The pre-filled text (≤ 4000, FR-63).
    #[must_use]
    pub fn value(mut self, value: impl Into<Text>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// The text shown while it is empty (≤ 100).
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Text>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
}

impl Wire for TextInput {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 4, ids)?;
        map.serialize_entry("custom_id", &self.key)?;
        map.serialize_entry("style", &self.style.value())?;
        opt(&mut map, "min_length", self.min_length.as_ref())?;
        opt(&mut map, "max_length", self.max_length.as_ref())?;
        required_entry(&mut map, self.required)?;
        opt(&mut map, "value", self.value.as_deref())?;
        opt(&mut map, "placeholder", self.placeholder.as_deref())?;
        map.end()
    }
}

/// A File Upload (type 19): the user attaches files.
///
/// Placement: only as a modal Label's input. Limits (MOD-06): `min_values`
/// 0-10 (0 only when not required), `max_values` 1-10, at most 10
/// `file_types`, each `image`, `video`, `audio` or `.ext`. Uploaded files are
/// ephemeral CDN URLs (MOD-09).
///
/// ```
/// use noctis_ui::FileUpload;
///
/// let upload = FileUpload::new("files").required(false).min_values(0).max_values(3).file_types(["image", ".pdf"]);
/// assert_eq!(
///     serde_json::to_value(&upload).unwrap(),
///     serde_json::json!({
///         "type": 19, "custom_id": "files", "min_values": 0, "max_values": 3,
///         "required": false, "file_types": ["image", ".pdf"]
///     })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct FileUpload {
    pub(crate) key: Text,
    pub(crate) min_values: Option<u8>,
    pub(crate) max_values: Option<u8>,
    pub(crate) required: bool,
    pub(crate) file_types: Vec<Text>,
}

impl FileUpload {
    /// A required File Upload whose submitted field is named `key`.
    pub fn new(key: impl Into<Text>) -> Self {
        Self {
            key: key.into(),
            min_values: None,
            max_values: None,
            required: true,
            file_types: Vec::new(),
        }
    }

    /// The fewest files a user must attach (0-10; Discord's default is 1).
    #[must_use]
    pub fn min_values(mut self, min: u8) -> Self {
        self.min_values = Some(min);
        self
    }

    /// The most files a user may attach (1-10; Discord's default is 1).
    #[must_use]
    pub fn max_values(mut self, max: u8) -> Self {
        self.max_values = Some(max);
        self
    }

    /// Whether an upload is required (default `true`).
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Appends accepted file types: `image`, `video`, `audio` or `.ext`
    /// (at most 10 in all); none means any file.
    #[must_use]
    pub fn file_types(mut self, types: impl IntoIterator<Item = impl Into<Text>>) -> Self {
        self.file_types.extend(types.into_iter().map(Into::into));
        self
    }
}

impl Wire for FileUpload {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 19, ids)?;
        map.serialize_entry("custom_id", &self.key)?;
        opt(&mut map, "min_values", self.min_values.as_ref())?;
        opt(&mut map, "max_values", self.max_values.as_ref())?;
        required_entry(&mut map, self.required)?;
        if !self.file_types.is_empty() {
            map.serialize_entry("file_types", &self.file_types)?;
        }
        map.end()
    }
}

/// One option of a [`RadioGroup`] or [`CheckboxGroup`].
///
/// Limits (MOD-07): `label` and `value` 1-100, `description` ≤ 100.
///
/// ```
/// use noctis_ui::ModalOption;
///
/// let option = ModalOption::new("High", "h").description("Urgent").default(true);
/// assert_eq!(
///     serde_json::to_value(&option).unwrap(),
///     serde_json::json!({"value": "h", "label": "High", "description": "Urgent", "default": true})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalOption {
    pub(crate) label: Text,
    pub(crate) value: Text,
    pub(crate) description: Option<Text>,
    pub(crate) default: bool,
}

impl ModalOption {
    /// An option showing `label` and submitting `value`.
    pub fn new(label: impl Into<Text>, value: impl Into<Text>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            description: None,
            default: false,
        }
    }

    /// The text under the label (≤ 100).
    #[must_use]
    pub fn description(mut self, description: impl Into<Text>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Whether the option starts selected.
    #[must_use]
    pub fn default(mut self, default: bool) -> Self {
        self.default = default;
        self
    }
}

impl Serialize for ModalOption {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("value", &self.value)?;
        map.serialize_entry("label", &self.label)?;
        opt(&mut map, "description", self.description.as_deref())?;
        flag(&mut map, "default", self.default)?;
        map.end()
    }
}

/// A Radio Group (type 21): pick one of 2-10 options.
///
/// Placement: only as a modal Label's input. Limits (MOD-07): 2-10
/// [`ModalOption`]s. The constructor takes any count; the runtime validator
/// rejects a wrong one.
///
/// ```
/// use noctis_ui::{ModalOption, RadioGroup};
///
/// let group = RadioGroup::new("sev", [ModalOption::new("Low", "l"), ModalOption::new("High", "h")]).required(false);
/// assert_eq!(
///     serde_json::to_value(&group).unwrap(),
///     serde_json::json!({
///         "type": 21, "custom_id": "sev",
///         "options": [{"value": "l", "label": "Low"}, {"value": "h", "label": "High"}],
///         "required": false
///     })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct RadioGroup {
    pub(crate) key: Text,
    pub(crate) options: Vec<ModalOption>,
    pub(crate) required: bool,
}

impl RadioGroup {
    /// A required Radio Group whose submitted field is named `key`.
    pub fn new(key: impl Into<Text>, options: impl IntoIterator<Item = ModalOption>) -> Self {
        Self {
            key: key.into(),
            options: options.into_iter().collect(),
            required: true,
        }
    }

    /// Whether a choice is required (default `true`).
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl Wire for RadioGroup {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 21, ids)?;
        map.serialize_entry("custom_id", &self.key)?;
        map.serialize_entry("options", &self.options)?;
        required_entry(&mut map, self.required)?;
        map.end()
    }
}

/// A Checkbox Group (type 22): pick several of 1-10 options.
///
/// Placement: only as a modal Label's input. Limits (MOD-07): 1-10
/// [`ModalOption`]s, `min_values` 0-10 (0 only when not required),
/// `max_values` 1-10.
///
/// ```
/// use noctis_ui::{CheckboxGroup, ModalOption};
///
/// let group = CheckboxGroup::new("tags", [ModalOption::new("Bug", "bug")]).min_values(1).max_values(1);
/// assert_eq!(
///     serde_json::to_value(&group).unwrap(),
///     serde_json::json!({
///         "type": 22, "custom_id": "tags", "options": [{"value": "bug", "label": "Bug"}],
///         "min_values": 1, "max_values": 1
///     })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct CheckboxGroup {
    pub(crate) key: Text,
    pub(crate) options: Vec<ModalOption>,
    pub(crate) min_values: Option<u8>,
    pub(crate) max_values: Option<u8>,
    pub(crate) required: bool,
}

impl CheckboxGroup {
    /// A required Checkbox Group whose submitted field is named `key`.
    pub fn new(key: impl Into<Text>, options: impl IntoIterator<Item = ModalOption>) -> Self {
        Self {
            key: key.into(),
            options: options.into_iter().collect(),
            min_values: None,
            max_values: None,
            required: true,
        }
    }

    /// The fewest options a user must check (0-10; Discord's default is 1).
    #[must_use]
    pub fn min_values(mut self, min: u8) -> Self {
        self.min_values = Some(min);
        self
    }

    /// The most options a user may check (1-10; Discord's default is the
    /// option count).
    #[must_use]
    pub fn max_values(mut self, max: u8) -> Self {
        self.max_values = Some(max);
        self
    }

    /// Whether a choice is required (default `true`).
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

impl Wire for CheckboxGroup {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 22, ids)?;
        map.serialize_entry("custom_id", &self.key)?;
        map.serialize_entry("options", &self.options)?;
        opt(&mut map, "min_values", self.min_values.as_ref())?;
        opt(&mut map, "max_values", self.max_values.as_ref())?;
        required_entry(&mut map, self.required)?;
        map.end()
    }
}

/// A single Checkbox (type 23): a yes/no answer.
///
/// Placement: only as a modal Label's input. It is never required (MOD-07),
/// so it has no `required` setter; for "must accept", use a one-option
/// [`CheckboxGroup`] with `min_values(1)`.
///
/// ```
/// use noctis_ui::Checkbox;
///
/// assert_eq!(
///     serde_json::to_value(Checkbox::new("c").default(true)).unwrap(),
///     serde_json::json!({"type": 23, "custom_id": "c", "default": true})
/// );
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::Checkbox;
///
/// let _ = Checkbox::new("c").required(true); // [MOD-07]
/// ```
#[derive(Debug, Clone)]
pub struct Checkbox {
    pub(crate) key: Text,
    pub(crate) default: bool,
}

impl Checkbox {
    /// An unchecked Checkbox whose submitted field is named `key`.
    pub fn new(key: impl Into<Text>) -> Self {
        Self {
            key: key.into(),
            default: false,
        }
    }

    /// Whether it starts checked.
    #[must_use]
    pub fn default(mut self, default: bool) -> Self {
        self.default = default;
        self
    }
}

impl Wire for Checkbox {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 23, ids)?;
        map.serialize_entry("custom_id", &self.key)?;
        flag(&mut map, "default", self.default)?;
        map.end()
    }
}

serialize_via_wire!(TextInput, FileUpload, RadioGroup, CheckboxGroup, Checkbox);
