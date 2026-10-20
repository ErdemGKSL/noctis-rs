//! The modal model: [`Modal`], its top-level [`ModalComponent`]s and the
//! [`ModalLabel`] that wraps one input (FR-29, FR-63; AD-12; MOD-01..08).

mod input;
mod select;

use std::borrow::Cow;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

pub use input::{
    Checkbox, CheckboxGroup, FileUpload, ModalOption, RadioGroup, TextInput, TextInputStyle,
};
pub(crate) use select::ModalSelectNode;
pub use select::{
    ModalChannelSelect, ModalMentionableSelect, ModalRoleSelect, ModalStringSelect, ModalUserSelect,
};

use crate::TextDisplay;
use crate::placement::{ModalInput, ModalTopLevel};
use crate::route::CustomId;
use crate::wire::{Ids, Wire, WiredSeq, head, opt, serialize_via_wire};

type Text = Cow<'static, str>;

/// A modal (the `data` of interaction callback type 9): a title and 1-5
/// top-level components, built only from a Route value with
/// [`RouteComponents::modal`](crate::RouteComponents::modal).
///
/// Its `custom_id` is the Route's encoding (MOD-08); the submission is
/// dispatched to that Route's handler. The top level takes a [`ModalLabel`]
/// or a [`TextDisplay`] ([`ModalTopLevel`], MOD-02, MOD-03). It serializes as
/// `{"custom_id", "title", "components"}` with no numeric ids and never an
/// Action Row; a `custom_id` still unresolved is a serde error.
/// [`validate`](crate::validate) and [`cost`](crate::cost) take a modal.
///
/// ```
/// # use core::fmt;
/// # use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape};
/// use noctis_ui::{ModalLabel, RouteComponents, TextDisplay, TextInput, TextInputStyle};
///
/// struct Save;
/// # impl Payload for Save {
/// #     const MAX_ENCODED_LEN: Option<usize> = Some(0);
/// #     fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result { Ok(()) }
/// # }
/// # impl Route for Save {
/// #     const ID: RouteId = RouteId::new("form.save");
/// #     const SHAPE: RouteShape = RouteShape::INLINE;
/// #     const SCHEMA: u8 = 0;
/// #     type Payload = Self;
/// # }
/// # impl noctis_ui::Handled for Save {}
///
/// let modal = Save
///     .modal("Feedback")
///     .push(TextDisplay::new("Thanks!"))
///     .push(ModalLabel::new("Text", TextInput::new("text", TextInputStyle::Short)));
/// assert_eq!((modal.title(), modal.len()), ("Feedback", 2));
/// assert_eq!(
///     serde_json::to_value(&modal).unwrap(),
///     serde_json::json!({
///         "custom_id": "0form.save:",
///         "title": "Feedback",
///         "components": [
///             {"type": 10, "content": "Thanks!"},
///             {"type": 18, "label": "Text", "component": {"type": 4, "custom_id": "text", "style": 1}}
///         ]
///     })
/// );
/// ```
///
/// A modal has no other constructor:
///
/// ```compile_fail,E0599
/// use noctis_ui::Modal;
///
/// let _ = Modal::new("id", "t"); // [MOD-08]
/// ```
#[derive(Debug, Clone)]
pub struct Modal {
    pub(crate) custom_id: CustomId,
    pub(crate) title: Text,
    pub(crate) components: Vec<ModalComponent>,
}

impl Modal {
    pub(crate) fn from_id(custom_id: CustomId, title: Text) -> Self {
        Self {
            custom_id,
            title,
            components: Vec::new(),
        }
    }

    /// Appends a top-level component: a [`ModalLabel`] or a [`TextDisplay`].
    #[must_use]
    pub fn push<C: ModalTopLevel>(mut self, component: C) -> Self {
        self.components.push(component.into_modal_top());
        self
    }

    /// The title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The number of top-level components.
    pub fn len(&self) -> usize {
        self.components.len()
    }

    /// Whether there is no component.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    /// The modal's Route `custom_id`; core resolves it before sending
    /// (AD-18).
    #[doc(hidden)]
    pub fn custom_id_mut(&mut self) -> &mut CustomId {
        &mut self.custom_id
    }
}

impl Serialize for Modal {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(3))?;
        map.serialize_entry("custom_id", &self.custom_id)?;
        map.serialize_entry("title", &self.title)?;
        map.serialize_entry("components", &WiredSeq(&self.components, None))?;
        map.end()
    }
}

/// Any top-level modal component: a [`ModalLabel`] or a [`TextDisplay`],
/// for lists built at runtime.
///
/// ```
/// use noctis_ui::{Checkbox, ModalComponent, ModalLabel, TextDisplay};
///
/// let parts: Vec<ModalComponent> = vec![
///     TextDisplay::new("Read me").into(),
///     ModalLabel::new("Agree", Checkbox::new("ok")).into(),
/// ];
/// assert_eq!(serde_json::to_value(&parts[0]).unwrap()["type"], 10);
/// ```
#[derive(Debug, Clone)]
pub struct ModalComponent(pub(crate) ModalTopNode);

#[derive(Debug, Clone)]
pub(crate) enum ModalTopNode {
    Label(ModalLabel),
    Text(TextDisplay),
}

impl From<ModalLabel> for ModalComponent {
    fn from(x: ModalLabel) -> Self {
        Self(ModalTopNode::Label(x))
    }
}

impl From<TextDisplay> for ModalComponent {
    fn from(x: TextDisplay) -> Self {
        Self(ModalTopNode::Text(x))
    }
}

impl Wire for ModalComponent {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ModalTopNode::Label(x) => x.wire(ids, s),
            ModalTopNode::Text(x) => x.wire(ids, s),
        }
    }
}

/// A Label (type 18): text, an optional description and exactly one input.
///
/// This is Discord's Label component; [`Label`](crate::Label) is the checked
/// button-label text. Placement: the top level of a [`Modal`]. Its input is
/// a [`TextInput`], a modal select ([`ModalStringSelect`],
/// [`ModalUserSelect`], [`ModalRoleSelect`], [`ModalMentionableSelect`],
/// [`ModalChannelSelect`]), a [`FileUpload`], a [`RadioGroup`], a
/// [`CheckboxGroup`] or a [`Checkbox`] ([`ModalInput`], MOD-03). Limits
/// (MOD-04): `label` 1-45, `description` ≤ 100. A Label and its input count
/// 2 components.
///
/// ```
/// use noctis_ui::{ModalLabel, TextInput, TextInputStyle};
///
/// let label = ModalLabel::new("Reason", TextInput::new("reason", TextInputStyle::Short)).description("Be brief");
/// assert_eq!(
///     serde_json::to_value(&label).unwrap(),
///     serde_json::json!({
///         "type": 18, "label": "Reason", "description": "Be brief",
///         "component": {"type": 4, "custom_id": "reason", "style": 1}
///     })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ModalLabel {
    pub(crate) label: Text,
    pub(crate) description: Option<Text>,
    pub(crate) input: ModalInputNode,
}

impl ModalLabel {
    /// A Label showing `label` above `input`.
    pub fn new(label: impl Into<Text>, input: impl ModalInput) -> Self {
        Self {
            label: label.into(),
            description: None,
            input: input.into_modal_input(),
        }
    }

    /// The text shown with the label (≤ 100).
    #[must_use]
    pub fn description(mut self, description: impl Into<Text>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl Wire for ModalLabel {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 18, ids)?;
        map.serialize_entry("label", &self.label)?;
        opt(&mut map, "description", self.description.as_deref())?;
        map.serialize_entry("component", &crate::wire::Wired(&self.input, ids))?;
        map.end()
    }
}

/// The input inside a [`ModalLabel`].
#[derive(Debug, Clone)]
pub enum ModalInputNode {
    TextInput(TextInput),
    Select(ModalSelectNode),
    FileUpload(FileUpload),
    RadioGroup(RadioGroup),
    CheckboxGroup(CheckboxGroup),
    Checkbox(Checkbox),
}

impl ModalInputNode {
    /// The `custom_id` key.
    pub(crate) fn key(&self) -> &str {
        match self {
            Self::TextInput(x) => &x.key,
            Self::Select(x) => &x.common().key,
            Self::FileUpload(x) => &x.key,
            Self::RadioGroup(x) => &x.key,
            Self::CheckboxGroup(x) => &x.key,
            Self::Checkbox(x) => &x.key,
        }
    }

    /// The path segment `noctis_rules::modal` names this input by.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::TextInput(_) => "text_input",
            Self::Select(_) => "select",
            Self::FileUpload(_) => "file_upload",
            Self::RadioGroup(_) => "radio_group",
            Self::CheckboxGroup(_) => "checkbox_group",
            Self::Checkbox(_) => "checkbox",
        }
    }
}

impl Wire for ModalInputNode {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::TextInput(x) => x.wire(ids, s),
            Self::Select(x) => x.wire(ids, s),
            Self::FileUpload(x) => x.wire(ids, s),
            Self::RadioGroup(x) => x.wire(ids, s),
            Self::CheckboxGroup(x) => x.wire(ids, s),
            Self::Checkbox(x) => x.wire(ids, s),
        }
    }
}

serialize_via_wire!(ModalComponent, ModalLabel);
