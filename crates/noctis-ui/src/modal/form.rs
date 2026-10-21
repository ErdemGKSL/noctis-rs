//! Typed modal forms: [`ModalForm`] (implemented by `#[derive(Modal)]`) and
//! [`Choices`] (implemented by `#[derive(Choices)]`), plus the hidden per-kind
//! field traits and builders the derive expands to (FR-29, FR-64, MOD-09).

use noctis_rules::Limits;
use noctis_rules::len::discord_len;
use twilight_model::channel::ChannelType;

use super::resolved::{
    Mentionable, ResolvedAttachment, ResolvedChannel, ResolvedRole, ResolvedUser,
};
use super::submit::{ModalSubmission, SubmitError, SubmittedValue};
use super::{
    Checkbox, CheckboxGroup, FileUpload, ModalChannelSelect, ModalMentionableSelect, ModalOption,
    ModalRoleSelect, ModalStringSelect, ModalUserSelect, RadioGroup, TextInput, TextInputStyle,
};
use crate::{CheckboxOptions, RadioOptions, SelectOption, StringSelectOptions, TextDisplay};

/// A modal declared as a struct: the Payload fields travel in the
/// `custom_id`, the input fields are decoded from the submission.
/// Implemented by `#[derive(Modal)]` (`noctis::prelude::Modal`), which checks
/// every literal text and count at `cargo check` and also generates the
/// inherent `X { .. }.build()` and `X::blank(..)` that build the modal.
///
/// ```
/// use noctis_ui::{ModalForm, ModalSubmission, SubmitError, SubmittedValue};
///
/// struct Bio(String);
///
/// impl ModalForm for Bio {
///     fn from_submission(s: &ModalSubmission) -> Result<Self, SubmitError> {
///         match s.get("bio") {
///             Some(SubmittedValue::TextInput(t)) => Ok(Self(t.clone())),
///             _ => Err(SubmitError::Missing { key: "bio".into() }),
///         }
///     }
/// }
///
/// let json = br#"{"type":5,"data":{"custom_id":"0form.save:","components":[
///     {"type":18,"component":{"type":4,"custom_id":"bio","value":"hi"}}]}}"#;
/// assert_eq!(Bio::from_json(json)?.0, "hi");
/// # Ok::<(), SubmitError>(())
/// ```
pub trait ModalForm: Sized {
    /// Decodes the form from a parsed submission: the derive decodes the
    /// Payload fields from [`ModalSubmission::custom_id`] (any decode error
    /// is [`SubmitError::Malformed`]), then the inputs.
    ///
    /// # Errors
    ///
    /// A [`SubmitError`] naming the first field that is missing, of
    /// another type, invalid or unresolved, or a malformed `custom_id`.
    fn from_submission(submission: &ModalSubmission) -> Result<Self, SubmitError>;

    /// Decodes the form from the raw interaction JSON.
    ///
    /// # Errors
    ///
    /// Any [`SubmitError`] of [`ModalSubmission::from_slice`] or
    /// [`Self::from_submission`]; never a panic.
    fn from_json(bytes: &[u8]) -> Result<Self, SubmitError> {
        Self::from_submission(&ModalSubmission::from_slice(bytes)?)
    }
}

/// Decodes the Payload of the modal Route `R` from the submission's
/// `custom_id`; what the `from_submission` of `#[derive(Modal)]` calls first.
///
/// # Errors
///
/// [`SubmitError::Malformed`] naming the Route for any decode error. A
/// modal declared `by_ref`, `overflow = spill` (once its Payload overflows)
/// or `signed` decodes only through component dispatch (Epic 7), which
/// resolves the Ref and verifies the tag: decoded directly, its Payload is
/// stored by Ref or its tag never verifies, so this returns `Malformed`
/// until then.
#[doc(hidden)]
pub fn decode_modal_payload<R>(submission: &ModalSubmission) -> Result<R::Payload, SubmitError>
where
    R: noctis_codec::Route,
    R::Payload: for<'a> noctis_codec::PayloadDecode<'a>,
{
    let malformed = |reason: String| SubmitError::Malformed {
        reason: format!("custom_id of route `{}`: {reason}", R::ID),
    };
    match noctis_codec::decode_route::<R>(submission.custom_id()) {
        Ok(noctis_codec::PayloadSource::Inline(payload)) => Ok(payload),
        Ok(noctis_codec::PayloadSource::Ref(_)) => Err(malformed(
            "the Payload is stored by Ref; core resolves it before decoding".to_owned(),
        )),
        Err(e) => Err(malformed(e.to_string())),
    }
}

/// A closed set of options for a modal Radio Group, Checkbox Group or String
/// Select. Implemented by `#[derive(Choices)]` (`noctis::prelude::Choices`)
/// on a unit enum, which checks every label, value and description at
/// `cargo check` (MOD-07).
///
/// ```
/// use noctis_ui::{Arity, Choices};
///
/// #[derive(Debug, Clone, Copy, PartialEq)]
/// enum Size { S, L }
///
/// impl Choices for Size {
///     type Count = Arity<2>;
///     const ALL: &'static [Self] = &[Self::S, Self::L];
///     fn label(&self) -> &'static str { match self { Self::S => "Small", Self::L => "Large" } }
///     fn value(&self) -> &'static str { match self { Self::S => "s", Self::L => "l" } }
///     fn description(&self) -> Option<&'static str> { None }
///     fn from_value(value: &str) -> Option<Self> { Self::ALL.iter().copied().find(|c| c.value() == value) }
/// }
///
/// assert_eq!(Size::from_value("l"), Some(Size::L));
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a Choices enum",
    label = "not a Choices enum",
    note = "derive it with `#[derive(Choices)]` on a unit enum"
)]
pub trait Choices: Sized + 'static {
    /// The type-level count; always `Arity<N>` with `N` the variant count.
    type Count;
    /// Every option, in declaration order.
    const ALL: &'static [Self];
    /// The text shown for the option.
    fn label(&self) -> &'static str;
    /// The submitted value.
    fn value(&self) -> &'static str;
    /// The text shown under the label.
    fn description(&self) -> Option<&'static str>;
    /// The option submitted as `value`, if any.
    fn from_value(value: &str) -> Option<Self>;
}

/// The declared bounds and placeholder of a derived input.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spec {
    /// `min_length` (Text Input) or `min_values`.
    pub min: Option<u16>,
    /// `max_length` (Text Input) or `max_values`.
    pub max: Option<u16>,
    /// The placeholder.
    pub placeholder: Option<&'static str>,
}

fn to_u8(n: u16) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// A Radio Group submits at most one value.
fn single(_: usize) -> usize {
    1
}

fn len_u8(n: usize) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

mod sealed {
    pub trait Sealed {}
}

/// How a field's Rust type shapes the submitted items: one (`T`), at most
/// one (`Option<T>`) or many (`Vec<T>`).
trait Shape<I>: Sized {
    /// Whether the field is a `Vec`.
    const MULTI: bool;
    /// Whether the built input is required, given the declared min.
    fn required(min: Option<u16>) -> bool;
    /// The field from the items (`None` when the input is absent).
    fn shape(
        key: &str,
        items: Option<Vec<I>>,
        min: Option<u16>,
        max: usize,
    ) -> Result<Self, SubmitError>;
}

fn at_most_one<I>(key: &str, items: Option<Vec<I>>) -> Result<Option<I>, SubmitError> {
    let mut items = items.unwrap_or_default();
    match items.len() {
        0 | 1 => Ok(items.pop()),
        n => Err(SubmitError::invalid(
            key,
            format!("expected at most one value, got {n}"),
        )),
    }
}

impl<I> Shape<I> for I {
    const MULTI: bool = false;

    fn required(_: Option<u16>) -> bool {
        true
    }

    fn shape(
        key: &str,
        items: Option<Vec<I>>,
        _: Option<u16>,
        _: usize,
    ) -> Result<Self, SubmitError> {
        at_most_one(key, items)?.ok_or_else(|| SubmitError::missing(key))
    }
}

impl<I> Shape<I> for Option<I> {
    const MULTI: bool = false;

    fn required(_: Option<u16>) -> bool {
        false
    }

    fn shape(
        key: &str,
        items: Option<Vec<I>>,
        _: Option<u16>,
        _: usize,
    ) -> Result<Self, SubmitError> {
        at_most_one(key, items)
    }
}

impl<I> Shape<I> for Vec<I> {
    const MULTI: bool = true;

    fn required(min: Option<u16>) -> bool {
        min != Some(0)
    }

    fn shape(
        key: &str,
        items: Option<Vec<I>>,
        min: Option<u16>,
        max: usize,
    ) -> Result<Self, SubmitError> {
        let Some(items) = items else {
            return if <Self as Shape<I>>::required(min) {
                Err(SubmitError::missing(key))
            } else {
                Ok(Vec::new())
            };
        };
        let min = min.map_or(1, usize::from);
        if items.len() < min || items.len() > max {
            return Err(SubmitError::invalid(
                key,
                format!("expected {min} to {max} values, got {}", items.len()),
            ));
        }
        Ok(items)
    }
}

/// The value under `key` when it has type `expected`, read by `extract`.
fn lookup<'s, T>(
    submission: &'s ModalSubmission,
    key: &str,
    expected: u64,
    extract: impl FnOnce(&'s SubmittedValue) -> Option<T>,
) -> Result<Option<T>, SubmitError> {
    let wrong = |found| SubmitError::WrongType {
        key: key.to_owned(),
        expected,
        found,
    };
    match submission.get(key) {
        Some(v) => match extract(v) {
            Some(t) if v.component_type() == expected => Ok(Some(t)),
            _ => Err(wrong(v.component_type())),
        },
        None => match submission.unknown_type_of(key) {
            Some(found) => Err(wrong(found)),
            None => Ok(None),
        },
    }
}

fn choice<C: Choices>(key: &str, value: &str) -> Result<C, SubmitError> {
    C::from_value(value)
        .ok_or_else(|| SubmitError::invalid(key, format!("`{value}` is not one of the choices")))
}

fn choices<C: Choices>(key: &str, values: &[String]) -> Result<Vec<C>, SubmitError> {
    values.iter().map(|v| choice(key, v)).collect()
}

fn select_items<C: Choices>(s: &ModalSubmission, key: &str) -> Result<Option<Vec<C>>, SubmitError> {
    let values = lookup(s, key, 3, |v| match v {
        SubmittedValue::StringSelect(x) => Some(x),
        _ => None,
    })?;
    values.map(|v| choices(key, v)).transpose()
}

fn radio_items<C: Choices>(s: &ModalSubmission, key: &str) -> Result<Option<Vec<C>>, SubmitError> {
    let value = lookup(s, key, 21, |v| match v {
        SubmittedValue::RadioGroup(x) => Some(x),
        _ => None,
    })?;
    value
        .map(|v| v.iter().map(|x| choice(key, x)).collect())
        .transpose()
}

fn checkbox_group_items<C: Choices>(
    s: &ModalSubmission,
    key: &str,
) -> Result<Option<Vec<C>>, SubmitError> {
    let values = lookup(s, key, 22, |v| match v {
        SubmittedValue::CheckboxGroup(x) => Some(x),
        _ => None,
    })?;
    values.map(|v| choices(key, v)).transpose()
}

fn unresolved(key: &str, id: u64) -> SubmitError {
    SubmitError::Unresolved {
        key: key.to_owned(),
        id,
    }
}

fn user(s: &ModalSubmission, key: &str, id: u64) -> Result<Option<ResolvedUser>, SubmitError> {
    let Some(id) = twilight_model::id::Id::new_checked(id) else {
        return Err(unresolved(key, id));
    };
    Ok(ResolvedUser::from_resolved(s.resolved(), id))
}

fn role(s: &ModalSubmission, id: u64) -> Option<ResolvedRole> {
    let id = twilight_model::id::Id::new_checked(id)?;
    ResolvedRole::from_resolved(s.resolved(), id)
}

fn user_items(s: &ModalSubmission, key: &str) -> Result<Option<Vec<ResolvedUser>>, SubmitError> {
    let ids = lookup(s, key, 5, |v| match v {
        SubmittedValue::UserSelect(x) => Some(x),
        _ => None,
    })?;
    ids.map(|ids| {
        ids.iter()
            .map(|id| user(s, key, id.get())?.ok_or_else(|| unresolved(key, id.get())))
            .collect()
    })
    .transpose()
}

fn role_items(s: &ModalSubmission, key: &str) -> Result<Option<Vec<ResolvedRole>>, SubmitError> {
    let ids = lookup(s, key, 6, |v| match v {
        SubmittedValue::RoleSelect(x) => Some(x),
        _ => None,
    })?;
    ids.map(|ids| {
        ids.iter()
            .map(|id| role(s, id.get()).ok_or_else(|| unresolved(key, id.get())))
            .collect()
    })
    .transpose()
}

fn mentionable_items(
    s: &ModalSubmission,
    key: &str,
) -> Result<Option<Vec<Mentionable>>, SubmitError> {
    let ids = lookup(s, key, 7, |v| match v {
        SubmittedValue::MentionableSelect(x) => Some(x),
        _ => None,
    })?;
    ids.map(|ids| {
        ids.iter()
            .map(|id| {
                let id = id.get();
                if let Some(u) = user(s, key, id)? {
                    return Ok(Mentionable::User(u));
                }
                role(s, id)
                    .map(Mentionable::Role)
                    .ok_or_else(|| unresolved(key, id))
            })
            .collect()
    })
    .transpose()
}

fn channel_items(
    s: &ModalSubmission,
    key: &str,
) -> Result<Option<Vec<ResolvedChannel>>, SubmitError> {
    let ids = lookup(s, key, 8, |v| match v {
        SubmittedValue::ChannelSelect(x) => Some(x),
        _ => None,
    })?;
    ids.map(|ids| {
        ids.iter()
            .map(|id| {
                ResolvedChannel::from_resolved(s.resolved(), *id)
                    .ok_or_else(|| unresolved(key, id.get()))
            })
            .collect()
    })
    .transpose()
}

fn attachment_items(
    s: &ModalSubmission,
    key: &str,
) -> Result<Option<Vec<ResolvedAttachment>>, SubmitError> {
    let ids = lookup(s, key, 19, |v| match v {
        SubmittedValue::FileUpload(x) => Some(x),
        _ => None,
    })?;
    ids.map(|ids| {
        ids.iter()
            .map(|id| {
                ResolvedAttachment::from_resolved(s.resolved(), *id)
                    .ok_or_else(|| unresolved(key, id.get()))
            })
            .collect()
    })
    .transpose()
}

/// The default `max_values` of a modal entity select (Discord's maximum).
const SELECT_MAX: usize = 25;
/// The default `max_values` of a File Upload (Discord's maximum).
const UPLOAD_MAX: usize = 10;

/// A `#[text_input]` field: `String` or `Option<String>`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a `#[text_input]` field",
    label = "not a Text Input field type",
    note = "a `#[text_input]` field is `String` (required) or `Option<String>` (optional)"
)]
pub trait TextInputField: sealed::Sealed + Sized {
    /// Whether the input is required.
    const REQUIRED: bool;
    /// Decodes the field.
    ///
    /// # Errors
    ///
    /// See [`ModalForm::from_submission`].
    fn decode(submission: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError>;
}

fn text(s: &ModalSubmission, key: &str, spec: &Spec) -> Result<Option<String>, SubmitError> {
    let Some(value) = lookup(s, key, 4, |v| match v {
        SubmittedValue::TextInput(x) => Some(x),
        _ => None,
    })?
    else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let limit = Limits::default().text_input_len;
    let min = spec.min.map_or(0, usize::from);
    let max = spec.max.map_or(limit, |m| usize::from(m).min(limit));
    let len = discord_len(value);
    if len < min || len > max {
        return Err(SubmitError::invalid(
            key,
            format!("length {len} is outside {min}..={max}"),
        ));
    }
    Ok(Some(value.clone()))
}

impl sealed::Sealed for String {}
impl sealed::Sealed for Option<String> {}

impl TextInputField for String {
    const REQUIRED: bool = true;

    fn decode(s: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError> {
        text(s, key, spec)?.ok_or_else(|| SubmitError::missing(key))
    }
}

impl TextInputField for Option<String> {
    const REQUIRED: bool = false;

    fn decode(s: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError> {
        text(s, key, spec)
    }
}

macro_rules! choice_kind {
    (
        $(#[$meta:meta])*
        $trait:ident, $items:ident, default_max: $max:expr, shapes: [$($shape:ty),+]
    ) => {
        $(#[$meta])*
        #[doc(hidden)]
        pub trait $trait: sealed::Sealed + Sized {
            /// The option count, `<Choice as Choices>::Count`.
            type Count;
            /// The option set.
            type Choice: Choices;
            /// Whether the field is a `Vec`.
            const MULTI: bool;
            /// Whether the input is required, given the declared min.
            fn required(spec: &Spec) -> bool;
            /// Decodes the field.
            ///
            /// # Errors
            ///
            /// See [`ModalForm::from_submission`].
            fn decode(submission: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError>;
        }

        $(
            impl<C: Choices> $trait for $shape {
                type Count = C::Count;
                type Choice = C;
                const MULTI: bool = <$shape as Shape<C>>::MULTI;

                fn required(spec: &Spec) -> bool {
                    <$shape as Shape<C>>::required(spec.min)
                }

                fn decode(s: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError> {
                    let max = spec.max.map_or_else(|| $max(C::ALL.len()), usize::from);
                    <$shape as Shape<C>>::shape(key, $items::<C>(s, key)?, spec.min, max)
                }
            }
        )+
    };
}

impl<C: Choices> sealed::Sealed for C {}
impl<C: Choices> sealed::Sealed for Option<C> {}
impl<C: Choices> sealed::Sealed for Vec<C> {}

choice_kind!(
    /// A `#[string_select]` field: `C`, `Option<C>` or `Vec<C>` with `C: Choices`.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[string_select]` field",
        label = "not a String Select field type",
        note = "a `#[string_select]` field is `C`, `Option<C>` or `Vec<C>` where `C` is a `#[derive(Choices)]` enum"
    )]
    StringSelectField, select_items, default_max: core::convert::identity, shapes: [C, Option<C>, Vec<C>]
);

choice_kind!(
    /// A `#[radio_group]` field: `C` or `Option<C>` with `C: Choices`.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[radio_group]` field",
        label = "not a Radio Group field type",
        note = "a `#[radio_group]` field is `C` or `Option<C>` where `C` is a `#[derive(Choices)]` enum"
    )]
    RadioGroupField, radio_items, default_max: single, shapes: [C, Option<C>]
);

choice_kind!(
    /// A `#[checkbox_group]` field: `Vec<C>` with `C: Choices`.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[checkbox_group]` field",
        label = "not a Checkbox Group field type",
        note = "a `#[checkbox_group]` field is `Vec<C>` where `C` is a `#[derive(Choices)]` enum"
    )]
    CheckboxGroupField, checkbox_group_items, default_max: core::convert::identity, shapes: [Vec<C>]
);

macro_rules! entity_kind {
    (
        $(#[$meta:meta])*
        $trait:ident, $item:ty, $items:ident, default_max: $max:expr
    ) => {
        $(#[$meta])*
        #[doc(hidden)]
        pub trait $trait: sealed::Sealed + Sized {
            /// Whether the field is a `Vec`.
            const MULTI: bool;
            /// The built `max_values` when a `Vec` field declares none.
            const DEFAULT_MAX: usize = $max;
            /// Whether the input is required, given the declared min.
            fn required(spec: &Spec) -> bool;
            /// Decodes the field.
            ///
            /// # Errors
            ///
            /// See [`ModalForm::from_submission`].
            fn decode(submission: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError>;
        }

        impl sealed::Sealed for $item {}
        impl sealed::Sealed for Option<$item> {}
        impl sealed::Sealed for Vec<$item> {}

        entity_kind!(@impl $trait, $item, $items, $item);
        entity_kind!(@impl $trait, $item, $items, Option<$item>);
        entity_kind!(@impl $trait, $item, $items, Vec<$item>);
    };
    (@impl $trait:ident, $item:ty, $items:ident, $shape:ty) => {
        impl $trait for $shape {
            const MULTI: bool = <$shape as Shape<$item>>::MULTI;

            fn required(spec: &Spec) -> bool {
                <$shape as Shape<$item>>::required(spec.min)
            }

            fn decode(s: &ModalSubmission, key: &str, spec: &Spec) -> Result<Self, SubmitError> {
                let max = spec.max.map_or(Self::DEFAULT_MAX, usize::from);
                <$shape as Shape<$item>>::shape(key, $items(s, key)?, spec.min, max)
            }
        }
    };
}

entity_kind!(
    /// A `#[user_select]` field.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[user_select]` field",
        label = "not a User Select field type",
        note = "a `#[user_select]` field is `ResolvedUser`, `Option<ResolvedUser>` or `Vec<ResolvedUser>`"
    )]
    UserSelectField, ResolvedUser, user_items, default_max: SELECT_MAX
);

entity_kind!(
    /// A `#[role_select]` field.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[role_select]` field",
        label = "not a Role Select field type",
        note = "a `#[role_select]` field is `ResolvedRole`, `Option<ResolvedRole>` or `Vec<ResolvedRole>`"
    )]
    RoleSelectField, ResolvedRole, role_items, default_max: SELECT_MAX
);

entity_kind!(
    /// A `#[mentionable_select]` field.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[mentionable_select]` field",
        label = "not a Mentionable Select field type",
        note = "a `#[mentionable_select]` field is `Mentionable`, `Option<Mentionable>` or `Vec<Mentionable>`"
    )]
    MentionableSelectField, Mentionable, mentionable_items, default_max: SELECT_MAX
);

entity_kind!(
    /// A `#[channel_select]` field.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[channel_select]` field",
        label = "not a Channel Select field type",
        note = "a `#[channel_select]` field is `ResolvedChannel`, `Option<ResolvedChannel>` or `Vec<ResolvedChannel>`"
    )]
    ChannelSelectField, ResolvedChannel, channel_items, default_max: SELECT_MAX
);

entity_kind!(
    /// A `#[file_upload]` field.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot be a `#[file_upload]` field",
        label = "not a File Upload field type",
        note = "a `#[file_upload]` field is `ResolvedAttachment`, `Option<ResolvedAttachment>` or `Vec<ResolvedAttachment>`"
    )]
    FileUploadField, ResolvedAttachment, attachment_items, default_max: UPLOAD_MAX
);

/// A `#[checkbox]` field: `bool`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a `#[checkbox]` field",
    label = "not a Checkbox field type",
    note = "a `#[checkbox]` field is `bool`"
)]
pub trait CheckboxField: sealed::Sealed + Sized {
    /// Decodes the field.
    ///
    /// # Errors
    ///
    /// See [`ModalForm::from_submission`].
    fn decode(submission: &ModalSubmission, key: &str) -> Result<Self, SubmitError>;
}

impl sealed::Sealed for bool {}

impl CheckboxField for bool {
    fn decode(s: &ModalSubmission, key: &str) -> Result<Self, SubmitError> {
        lookup(s, key, 23, |v| match v {
            SubmittedValue::Checkbox(x) => Some(*x),
            _ => None,
        })?
        .ok_or_else(|| SubmitError::missing(key))
    }
}

/// A `#[text_display]` field: `()`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a `#[text_display]` field",
    label = "not a Text Display field type",
    note = "a `#[text_display]` field is `()`: it shows text and submits nothing"
)]
pub trait TextDisplayField: sealed::Sealed + Sized {
    /// The field value.
    fn decode() -> Self;
}

impl sealed::Sealed for () {}

impl TextDisplayField for () {
    fn decode() -> Self {}
}

// The builders `derive(Modal)` expands to; each applies the field type's
// required flag and default `max_values`.

/// A Text Input for a `T` field.
#[doc(hidden)]
pub fn text_input<T: TextInputField>(
    key: &'static str,
    style: TextInputStyle,
    spec: Spec,
) -> TextInput {
    let mut x = TextInput::new(key, style);
    if let Some(n) = spec.min {
        x = x.min_length(n);
    }
    if let Some(n) = spec.max {
        x = x.max_length(n);
    }
    if !T::REQUIRED {
        x = x.required(false);
    }
    if let Some(p) = spec.placeholder {
        x = x.placeholder(p);
    }
    x
}

macro_rules! values {
    ($x:ident, $spec:ident, $required:expr, $default_max:expr) => {{
        if let Some(p) = $spec.placeholder {
            $x = $x.placeholder(p);
        }
        if let Some(n) = $spec.min {
            $x = $x.min_values(to_u8(n));
        }
        match ($spec.max, $default_max) {
            (Some(n), _) => $x = $x.max_values(to_u8(n)),
            (None, Some(n)) => $x = $x.max_values(len_u8(n)),
            (None, None) => {}
        }
        if !$required {
            $x = $x.required(false);
        }
        $x
    }};
}

/// A String Select of `T::Choice`'s options for a `T` field.
#[doc(hidden)]
pub fn string_select<T>(key: &'static str, spec: Spec) -> ModalStringSelect
where
    T: StringSelectField,
    T::Count: StringSelectOptions,
{
    let options = T::Choice::ALL.iter().map(|c| {
        let o = SelectOption::new(c.label(), c.value());
        match c.description() {
            Some(d) => o.description(d),
            None => o,
        }
    });
    let mut x = ModalStringSelect::new(key, options);
    let default_max = T::MULTI.then_some(T::Choice::ALL.len());
    values!(x, spec, T::required(&spec), default_max)
}

/// A User Select for a `T` field.
#[doc(hidden)]
pub fn user_select<T: UserSelectField>(key: &'static str, spec: Spec) -> ModalUserSelect {
    let mut x = ModalUserSelect::new(key);
    values!(
        x,
        spec,
        T::required(&spec),
        T::MULTI.then_some(T::DEFAULT_MAX)
    )
}

/// A Role Select for a `T` field.
#[doc(hidden)]
pub fn role_select<T: RoleSelectField>(key: &'static str, spec: Spec) -> ModalRoleSelect {
    let mut x = ModalRoleSelect::new(key);
    values!(
        x,
        spec,
        T::required(&spec),
        T::MULTI.then_some(T::DEFAULT_MAX)
    )
}

/// A Mentionable Select for a `T` field.
#[doc(hidden)]
pub fn mentionable_select<T: MentionableSelectField>(
    key: &'static str,
    spec: Spec,
) -> ModalMentionableSelect {
    let mut x = ModalMentionableSelect::new(key);
    values!(
        x,
        spec,
        T::required(&spec),
        T::MULTI.then_some(T::DEFAULT_MAX)
    )
}

/// A Channel Select for a `T` field.
#[doc(hidden)]
pub fn channel_select<T: ChannelSelectField>(
    key: &'static str,
    spec: Spec,
    channel_types: &[ChannelType],
) -> ModalChannelSelect {
    let mut x = ModalChannelSelect::new(key);
    if !channel_types.is_empty() {
        x = x.channel_types(channel_types.iter().copied());
    }
    values!(
        x,
        spec,
        T::required(&spec),
        T::MULTI.then_some(T::DEFAULT_MAX)
    )
}

/// A File Upload for a `T` field.
#[doc(hidden)]
pub fn file_upload<T: FileUploadField>(
    key: &'static str,
    spec: Spec,
    file_types: &[&'static str],
) -> FileUpload {
    let mut x = FileUpload::new(key);
    if let Some(n) = spec.min {
        x = x.min_values(to_u8(n));
    }
    match spec.max {
        Some(n) => x = x.max_values(to_u8(n)),
        None if T::MULTI => x = x.max_values(len_u8(T::DEFAULT_MAX)),
        None => {}
    }
    if !T::required(&spec) {
        x = x.required(false);
    }
    if !file_types.is_empty() {
        x = x.file_types(file_types.iter().copied());
    }
    x
}

fn modal_options<C: Choices>() -> impl Iterator<Item = ModalOption> {
    C::ALL.iter().map(|c| {
        let o = ModalOption::new(c.label(), c.value());
        match c.description() {
            Some(d) => o.description(d),
            None => o,
        }
    })
}

/// A Radio Group of `T::Choice`'s options for a `T` field.
#[doc(hidden)]
pub fn radio_group<T>(key: &'static str) -> RadioGroup
where
    T: RadioGroupField,
    T::Count: RadioOptions,
{
    let x = RadioGroup::new(key, modal_options::<T::Choice>());
    if T::required(&Spec::default()) {
        x
    } else {
        x.required(false)
    }
}

/// A Checkbox Group of `T::Choice`'s options for a `T` field.
#[doc(hidden)]
pub fn checkbox_group<T>(key: &'static str, spec: Spec) -> CheckboxGroup
where
    T: CheckboxGroupField,
    T::Count: CheckboxOptions,
{
    let mut x = CheckboxGroup::new(key, modal_options::<T::Choice>());
    if let Some(n) = spec.min {
        x = x.min_values(to_u8(n));
    }
    if let Some(n) = spec.max {
        x = x.max_values(to_u8(n));
    }
    if !T::required(&spec) {
        x = x.required(false);
    }
    x
}

/// A Checkbox for a `T` field.
#[doc(hidden)]
pub fn checkbox<T: CheckboxField>(key: &'static str) -> Checkbox {
    Checkbox::new(key)
}

/// A Text Display for a `T` field.
#[doc(hidden)]
pub fn text_display<T: TextDisplayField>(content: &'static str) -> TextDisplay {
    TextDisplay::new(content)
}
