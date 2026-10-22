//! Per-render modal building: [`ModalBuilder`] (what `#[derive(Modal)]`'s
//! `X { .. }.build()` returns), the typed [`InputField`] handles of
//! `.value(..)`, [`InputOverrides`] and the hidden [`ModalRender`] trait the
//! derive implements (FR-59, FR-63; maintainer decisions §7 and §9).

use core::fmt;
use core::marker::PhantomData;

use super::{Modal, ModalInputNode, ModalLabel};
use crate::route::{ComponentRoute, CustomId, ModalDefaults};
use crate::{MentionableId, Text};

/// What `#[derive(Modal)]` implements so a [`ModalBuilder`] can render the
/// modal: its generated pre-fill and overrides types and the function that
/// pushes its inputs. Not public API.
#[doc(hidden)]
pub trait ModalRender:
    ComponentRoute<Defaults = ModalDefaults, Payload: Send + Sync + 'static>
{
    /// One `Option<Prefill>` per input field (`None`: no pre-fill).
    type Prefill: Default + Clone + fmt::Debug + Send + Sync + 'static;
    /// The generated `<X>Overrides`.
    type Overrides: Default + Clone + fmt::Debug + Send + Sync + 'static;

    /// The `title` field of the overrides.
    fn title_mut(overrides: &mut Self::Overrides) -> &mut Option<Text>;

    /// Merges `from` into `into` field by field: each `Some` replaces, an
    /// [`InputOverrides`] merges its own fields.
    fn merge(into: &mut Self::Overrides, from: Self::Overrides);

    /// Pushes the inputs onto `modal` (its `custom_id` and title set), each
    /// pre-filled from `prefill` and with its overrides applied.
    fn render(modal: Modal, prefill: &Self::Prefill, overrides: &Self::Overrides) -> Modal;
}

/// The modal of one render of a `#[derive(Modal)]` struct `X`: what
/// `X { .. }.build()` returns (and `X::blank(..)` for a form whose inputs
/// have no empty value).
///
/// It holds the struct's Payload (its `custom_id`), the pre-fill of every
/// input and `X`'s generated `<X>Overrides`, and renders them when it
/// converts into a [`Modal`] (`Modal::from(b)`, or any `impl Into<Modal>`
/// such as `show_modal`). Pre-fill comes from the struct value: a non-empty
/// Text Input value becomes its `value`, a chosen option is marked
/// `default`, picked users, roles and channels become `default_values`, a
/// `true` Checkbox starts checked; `None`, `""`, `vec![]` and `false` send
/// nothing. Pre-fill never changes the `custom_id`, the decode or the
/// declared input specs.
///
/// - [`title`](Self::title) replaces the declared title for this render
///   (MOD-01, ≤ 45);
/// - [`value`](Self::value) replaces one input's pre-fill through its
///   typed handle `X::FIELD`;
/// - [`overrides`](Self::overrides) applies an `<X>Overrides` (the title
///   and, per Label input, an [`InputOverrides`]); presentation only,
///   never values.
///
/// Later calls win. Everything is validated with the modal before it is
/// sent: a value longer than 4000 or than the input's `max_length`
/// (MOD-05), or more defaults than `max_values` (CV2-09, MOD-07), is
/// `ResponderError::Invalid` with no request, in every Limit mode.
#[must_use = "a ModalBuilder does nothing until it is converted into a Modal (e.g. by show_modal)"]
pub struct ModalBuilder<X: ModalRender> {
    custom_id: CustomId,
    prefill: X::Prefill,
    overrides: X::Overrides,
}

impl<X: ModalRender> ModalBuilder<X> {
    /// The builder of `payload` with `prefill`; what the derive calls.
    #[doc(hidden)]
    pub fn __new(payload: X::Payload, prefill: X::Prefill) -> Self {
        Self {
            custom_id: CustomId::new::<X>(payload),
            prefill,
            overrides: X::Overrides::default(),
        }
    }

    /// The title of this render (a literal or a locale key, ≤ 45, MOD-01),
    /// replacing the declared one; the same field as `<X>Overrides::title`.
    pub fn title(mut self, title: impl Into<Text>) -> Self {
        *X::title_mut(&mut self.overrides) = Some(title.into());
        self
    }

    /// Replaces the pre-fill of one input: `field` is the input's typed
    /// handle, the associated const `X::<FIELD>` the derive generates, and
    /// `value` has the field's type (`BioForm::BIO` takes an
    /// `Option<String>`).
    pub fn value<T>(mut self, field: InputField<X, T>, value: T) -> Self {
        (field.set)(&mut self.prefill, value);
        self
    }

    /// Applies `overrides`: each `Some` replaces what an earlier call set,
    /// and each [`InputOverrides`] merges field by field.
    pub fn overrides(mut self, overrides: X::Overrides) -> Self {
        X::merge(&mut self.overrides, overrides);
        self
    }
}

impl<X: ModalRender> From<ModalBuilder<X>> for Modal {
    fn from(b: ModalBuilder<X>) -> Self {
        let mut overrides = b.overrides;
        let title = X::title_mut(&mut overrides)
            .take()
            .unwrap_or_else(|| Text::from(X::DEFAULTS.title));
        X::render(Modal::from_id(b.custom_id, title), &b.prefill, &overrides)
    }
}

impl<X: ModalRender> Clone for ModalBuilder<X> {
    fn clone(&self) -> Self {
        Self {
            custom_id: self.custom_id.clone(),
            prefill: self.prefill.clone(),
            overrides: self.overrides.clone(),
        }
    }
}

impl<X: ModalRender> fmt::Debug for ModalBuilder<X> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModalBuilder")
            .field("custom_id", &self.custom_id)
            .field("prefill", &self.prefill)
            .field("overrides", &self.overrides)
            .finish()
    }
}

/// The typed handle of one input field of the modal `X`, whose values are
/// `T`: the associated const `X::<FIELD>` (`BioForm::BIO`) that
/// [`ModalBuilder::value`] takes. Every input field has one except
/// `#[text_display]` and `#[file_upload]` fields.
pub struct InputField<X: ModalRender, T> {
    set: fn(&mut X::Prefill, T),
    form: PhantomData<fn() -> X>,
}

impl<X: ModalRender, T> InputField<X, T> {
    /// The handle that stores a value with `set`; what the derive calls.
    #[doc(hidden)]
    pub const fn __new(set: fn(&mut X::Prefill, T)) -> Self {
        Self {
            set,
            form: PhantomData,
        }
    }
}

impl<X: ModalRender, T> Clone for InputField<X, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<X: ModalRender, T> Copy for InputField<X, T> {}

impl<X: ModalRender, T> fmt::Debug for InputField<X, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputField")
            .field("form", &core::any::type_name::<X>())
            .finish_non_exhaustive()
    }
}

/// The presentation overrides of one Label input in a generated
/// `<X>Overrides`: its Label text (≤ 45) and description (≤ 100, MOD-04)
/// and its placeholder (Text Input ≤ 100, MOD-05; select ≤ 150, CV2-09).
/// Each takes a literal or a locale key.
///
/// `placeholder` is ignored for a File Upload, Radio Group, Checkbox Group
/// or Checkbox, which have none. Overrides never change a value.
#[derive(Debug, Clone, Default)]
pub struct InputOverrides {
    /// The Label text.
    pub label: Option<Text>,
    /// The Label description.
    pub description: Option<Text>,
    /// The input's placeholder.
    pub placeholder: Option<Text>,
}

impl InputOverrides {
    /// Merges `from` into `self`: each `Some` replaces. Not public API.
    #[doc(hidden)]
    pub fn __merge(&mut self, from: Self) {
        let Self {
            label,
            description,
            placeholder,
        } = from;
        if label.is_some() {
            self.label = label;
        }
        if description.is_some() {
            self.description = description;
        }
        if placeholder.is_some() {
            self.placeholder = placeholder;
        }
    }
}

/// Applies `o` to a derived Label; what the derive's render calls.
#[doc(hidden)]
pub fn input_label(mut label: ModalLabel, o: &InputOverrides) -> ModalLabel {
    if let Some(text) = &o.label {
        label.label = text.clone();
    }
    if let Some(text) = &o.description {
        label.description = Some(text.clone());
    }
    if let Some(text) = &o.placeholder {
        match &mut label.input {
            ModalInputNode::TextInput(t) => t.placeholder = Some(text.clone()),
            ModalInputNode::Select(s) => {
                let (common, _) = s.parts_mut();
                common.placeholder = Some(text.clone());
            }
            ModalInputNode::FileUpload(_)
            | ModalInputNode::RadioGroup(_)
            | ModalInputNode::CheckboxGroup(_)
            | ModalInputNode::Checkbox(_) => {}
        }
    }
    label
}

/// The pre-fill of one input, from its field value. Not public API.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefill(pub(crate) PrefillKind);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrefillKind {
    /// A Text Input value; `None` for `None` or `""`.
    Text(Option<String>),
    /// The `Choices::value()`s of the chosen options.
    Options(Vec<&'static str>),
    /// Users.
    Users(Vec<twilight_model::id::Id<twilight_model::id::marker::UserMarker>>),
    /// Roles.
    Roles(Vec<twilight_model::id::Id<twilight_model::id::marker::RoleMarker>>),
    /// Users and roles.
    Mentionables(Vec<MentionableId>),
    /// Channels.
    Channels(Vec<twilight_model::id::Id<twilight_model::id::marker::ChannelMarker>>),
    /// A Checkbox.
    Checkbox(bool),
}

impl Prefill {
    pub(crate) fn text(&self) -> Option<&str> {
        match &self.0 {
            PrefillKind::Text(t) => t.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn options(&self) -> &[&'static str] {
        match &self.0 {
            PrefillKind::Options(v) => v,
            _ => &[],
        }
    }
}
