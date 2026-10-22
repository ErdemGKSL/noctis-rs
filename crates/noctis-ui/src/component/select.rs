//! Message selects: String (type 3), User (5), Role (6), Mentionable (7) and
//! Channel (8) (CV2-09).

use std::borrow::Cow;
use std::marker::PhantomData;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};

use super::Emoji;
use crate::Text;
use crate::overrides::{
    ChannelSelectOverrides, MentionableSelectOverrides, RoleSelectOverrides, StringSelectOverrides,
    UserSelectOverrides,
};
use crate::route::{CustomId, SelectDefaults};
use crate::wire::{Ids, Wire, flag, head, opt, serialize_via_wire};

type Str = Cow<'static, str>;

/// One option of a [`StringSelect`].
///
/// ```
/// use noctis_ui::SelectOption;
///
/// let option = SelectOption::new("Small", "s").description("Fits one").default(true);
/// assert_eq!(
///     serde_json::to_value(&option).unwrap(),
///     serde_json::json!({"label": "Small", "value": "s", "description": "Fits one", "default": true})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOption {
    pub(crate) label: Cow<'static, str>,
    pub(crate) value: Cow<'static, str>,
    pub(crate) description: Option<Cow<'static, str>>,
    emoji: Option<Emoji>,
    pub(crate) default: bool,
}

impl SelectOption {
    /// An option showing `label` and sending `value`.
    pub fn new(label: impl Into<Cow<'static, str>>, value: impl Into<Cow<'static, str>>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            description: None,
            emoji: None,
            default: false,
        }
    }

    /// The text under the label.
    #[must_use]
    pub fn description(self, description: impl Into<Cow<'static, str>>) -> Self {
        Self {
            description: Some(description.into()),
            ..self
        }
    }

    /// The emoji shown before the label.
    #[must_use]
    pub fn emoji(self, emoji: Emoji) -> Self {
        Self {
            emoji: Some(emoji),
            ..self
        }
    }

    /// Whether the option is selected when the select renders.
    #[must_use]
    pub fn default(self, default: bool) -> Self {
        Self { default, ..self }
    }
}

impl SelectOption {
    /// Whether the option is selected when the select renders, given the
    /// select's `default_values` list (which replaces every option's flag).
    pub(crate) fn is_default(&self, list: Option<&[Str]>) -> bool {
        match list {
            Some(list) => list.contains(&self.value),
            None => self.default,
        }
    }

    fn write<S: Serializer>(&self, default: bool, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("label", &self.label)?;
        map.serialize_entry("value", &self.value)?;
        opt(&mut map, "description", self.description.as_deref())?;
        opt(&mut map, "emoji", self.emoji.as_ref())?;
        flag(&mut map, "default", default)?;
        map.end()
    }
}

impl Serialize for SelectOption {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.write(self.default, s)
    }
}

/// A String Select's options with the `default` flags its
/// `default_values` list decides.
struct OptionsWire<'a> {
    options: &'a [SelectOption],
    list: Option<&'a [Str]>,
}

impl Serialize for OptionsWire<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        struct One<'a>(&'a SelectOption, bool);
        impl Serialize for One<'_> {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.0.write(self.1, s)
            }
        }
        s.collect_seq(self.options.iter().map(|o| One(o, o.is_default(self.list))))
    }
}

/// Pushes `value` unless the list already holds it (the first occurrence
/// is kept).
pub(crate) fn push_unique<T: PartialEq>(list: &mut Vec<T>, value: T) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// The fields every message select shares.
#[derive(Debug, Clone)]
pub(crate) struct Common {
    pub(crate) custom_id: CustomId,
    pub(crate) placeholder: Option<Text>,
    pub(crate) min_values: Option<u8>,
    pub(crate) max_values: Option<u8>,
    pub(crate) disabled: bool,
}

impl Common {
    fn new(custom_id: CustomId) -> Self {
        Self {
            custom_id,
            placeholder: None,
            min_values: None,
            max_values: None,
            disabled: false,
        }
    }

    /// The declared defaults of a kind derive.
    fn apply(&mut self, d: &SelectDefaults) {
        self.placeholder = d.placeholder.map(Text::from);
        self.min_values = d.min_values;
        self.max_values = d.max_values;
        self.disabled = d.disabled;
    }
}

/// A pre-selected value of an auto-populated select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultValue {
    User(Id<UserMarker>),
    Role(Id<RoleMarker>),
    Channel(Id<ChannelMarker>),
}

/// A user or role pre-selected in a [`MentionableSelect`] (or a modal one).
///
/// ```
/// use noctis_ui::MentionableId;
/// use twilight_model::id::Id;
///
/// let ids = [MentionableId::User(Id::new(1)), MentionableId::Role(Id::new(2))];
/// # let _ = ids;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MentionableId {
    /// A user.
    User(Id<UserMarker>),
    /// A role.
    Role(Id<RoleMarker>),
}

impl From<MentionableId> for DefaultValue {
    fn from(id: MentionableId) -> Self {
        match id {
            MentionableId::User(id) => Self::User(id),
            MentionableId::Role(id) => Self::Role(id),
        }
    }
}

/// Replaces `list` with `values`, duplicates removed.
pub(crate) fn replace_defaults(
    list: &mut Vec<DefaultValue>,
    values: impl IntoIterator<Item = DefaultValue>,
) {
    list.clear();
    extend_defaults(list, values);
}

/// Appends `values` to `list`, duplicates removed.
pub(crate) fn extend_defaults(
    list: &mut Vec<DefaultValue>,
    values: impl IntoIterator<Item = DefaultValue>,
) {
    for v in values {
        push_unique(list, v);
    }
}

impl Serialize for DefaultValue {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (id, ty) = match self {
            Self::User(id) => (id.get(), "user"),
            Self::Role(id) => (id.get(), "role"),
            Self::Channel(id) => (id.get(), "channel"),
        };
        let mut map = s.serialize_map(Some(2))?;
        map.serialize_entry("id", &id.to_string())?;
        map.serialize_entry("type", ty)?;
        map.end()
    }
}

/// The select fields after `custom_id` and the kind's own entries, shared by
/// message and modal selects.
pub(crate) struct SelectTail<'a> {
    pub(crate) placeholder: Option<&'a str>,
    pub(crate) defaults: &'a [DefaultValue],
    pub(crate) min_values: Option<u8>,
    pub(crate) max_values: Option<u8>,
}

/// Starts a select: `type`, `id`, `custom_id`, `extra`, then `placeholder`,
/// `default_values`, `min_values` and `max_values`. The caller writes the
/// trailing flag (`disabled` or `required`) and ends the map.
pub(crate) fn select_map<S: Serializer, I: Serialize + ?Sized>(
    ty: u8,
    custom_id: &I,
    ids: Option<&Ids>,
    s: S,
    extra: impl FnOnce(&mut S::SerializeMap) -> Result<(), S::Error>,
    tail: &SelectTail<'_>,
) -> Result<S::SerializeMap, S::Error> {
    let mut map = head(s, ty, ids)?;
    map.serialize_entry("custom_id", custom_id)?;
    extra(&mut map)?;
    opt(&mut map, "placeholder", tail.placeholder)?;
    if !tail.defaults.is_empty() {
        map.serialize_entry("default_values", tail.defaults)?;
    }
    opt(&mut map, "min_values", tail.min_values.as_ref())?;
    opt(&mut map, "max_values", tail.max_values.as_ref())?;
    Ok(map)
}

/// Writes `channel_types` (as Discord's codes) when `types` is not empty.
pub(crate) fn channel_types_entry<M: SerializeMap>(
    map: &mut M,
    types: &[ChannelType],
) -> Result<(), M::Error> {
    if types.is_empty() {
        return Ok(());
    }
    let codes: Vec<u8> = types.iter().map(|&t| u8::from(t)).collect();
    map.serialize_entry("channel_types", &codes)
}

/// Writes a message select: [`select_map`], then `disabled`.
fn wire_select<S: Serializer>(
    ty: u8,
    common: &Common,
    ids: Option<&Ids>,
    s: S,
    extra: impl FnOnce(&mut S::SerializeMap) -> Result<(), S::Error>,
    defaults: &[DefaultValue],
) -> Result<S::Ok, S::Error> {
    let tail = SelectTail {
        placeholder: common.placeholder.as_ref().map(Text::as_str),
        defaults,
        min_values: common.min_values,
        max_values: common.max_values,
    };
    let mut map = select_map(ty, &common.custom_id, ids, s, extra, &tail)?;
    flag(&mut map, "disabled", common.disabled)?;
    map.end()
}

/// The shared select setters.
macro_rules! common_setters {
    ($($ty:ident),*) => {$(
        impl $ty {
            /// Applies the `defaults(..)` of a kind derive.
            pub(crate) fn with_defaults(mut self, defaults: &SelectDefaults) -> Self {
                self.common.apply(defaults);
                self
            }

            /// The text shown while nothing is selected: a literal or a
            /// locale key (≤ 150, CV2-09).
            #[must_use]
            pub fn placeholder(mut self, placeholder: impl Into<Text>) -> Self {
                self.common.placeholder = Some(placeholder.into());
                self
            }

            /// The fewest values a user must pick (Discord's default is 1).
            #[must_use]
            pub fn min_values(mut self, min: u8) -> Self {
                self.common.min_values = Some(min);
                self
            }

            /// The most values a user may pick (Discord's default is 1).
            #[must_use]
            pub fn max_values(mut self, max: u8) -> Self {
                self.common.max_values = Some(max);
                self
            }

            /// Whether the select is greyed out.
            #[must_use]
            pub fn disabled(mut self, disabled: bool) -> Self {
                self.common.disabled = disabled;
                self
            }

            /// The shared fields of an overrides struct, in its order.
            fn common_overrides(
                self,
                placeholder: Option<Text>,
                min_values: Option<u8>,
                max_values: Option<u8>,
                disabled: Option<bool>,
            ) -> Self {
                let mut this = self;
                if let Some(p) = placeholder {
                    this = this.placeholder(p);
                }
                if let Some(n) = min_values {
                    this = this.min_values(n);
                }
                if let Some(n) = max_values {
                    this = this.max_values(n);
                }
                if let Some(d) = disabled {
                    this = this.disabled(d);
                }
                this
            }
        }
    )*};
}

/// A String Select (type 3): pick from fixed options; built with
/// `R { .. }.build()` from a `#[derive(StringSelectMenu)]` struct that
/// declares `defaults(options = <Choices type>)`, or from the
/// [`StringSelectBuilder`] such a struct without declared options builds.
///
/// The setters override one field each and [`overrides`](Self::overrides)
/// several at once ([`StringSelectOverrides`]); the later call wins, and
/// the result is validated before sending (CV2-09).
///
/// Placement: alone in an Action Row ([`ActionRow::select`](crate::ActionRow::select)).
#[derive(Debug, Clone)]
pub struct StringSelect {
    pub(crate) common: Common,
    pub(crate) options: Vec<SelectOption>,
    /// `default_values`: the values of the options selected on render,
    /// replacing every option's own `default` flag.
    pub(crate) default_values: Option<Vec<Str>>,
}

impl StringSelect {
    pub(crate) fn from_id(custom_id: CustomId, options: Vec<SelectOption>) -> Self {
        Self {
            common: Common::new(custom_id),
            options,
            default_values: None,
        }
    }

    /// Replaces the options (the declared ones included); more than 25
    /// fail the runtime validator (CV2-09).
    #[must_use]
    pub fn options(mut self, options: impl IntoIterator<Item = SelectOption>) -> Self {
        self.options = options.into_iter().collect();
        self
    }

    /// The values of the options selected when the select renders,
    /// replacing the list: on the wire exactly the listed options have
    /// `default: true`, whatever their own flag and whatever the setter
    /// order. Duplicates are removed (the first is kept). A value no option
    /// has, or more values than `max_values`, fails the validator (CV2-09)
    /// in every mode.
    #[must_use]
    pub fn default_values(
        mut self,
        values: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
    ) -> Self {
        let mut list = Vec::new();
        for v in values {
            push_unique(&mut list, v.into());
        }
        self.default_values = Some(list);
        self
    }

    /// Applies every `Some` field of `overrides` exactly as its setter
    /// would, in declaration order (`placeholder`, `min_values`,
    /// `max_values`, `disabled`, `options`, `default_values`).
    #[must_use]
    pub fn overrides(self, overrides: StringSelectOverrides) -> Self {
        let StringSelectOverrides {
            placeholder,
            min_values,
            max_values,
            disabled,
            options,
            default_values,
        } = overrides;
        let mut this = self.common_overrides(placeholder, min_values, max_values, disabled);
        if let Some(options) = options {
            this = this.options(options);
        }
        if let Some(values) = default_values {
            this = this.default_values(values);
        }
        this
    }
}

/// A String Select whose struct declares no `defaults(options = ..)`: what
/// `R { .. }.build()` returns for it. It is not a component until
/// [`options`](Self::options) gives it its options, so it cannot be placed
/// or sent (CV2-09). It has no `.overrides(..)`: that comes with the
/// [`StringSelect`] `.options(..)` returns.
///
/// ```compile_fail,E0599
/// # use core::fmt;
/// # use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape, SelectRoute};
/// use noctis_ui::StringSelectOverrides;
/// use noctis_ui::__macro::{DefaultText, SelectDefaults, build_string_select_builder};
///
/// struct Pick;
/// # impl Payload for Pick {
/// #     const MAX_ENCODED_LEN: Option<usize> = Some(0);
/// #     fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result { Ok(()) }
/// # }
/// # impl Route for Pick {
/// #     const ID: RouteId = RouteId::new("pick");
/// #     const SHAPE: RouteShape = RouteShape::INLINE.select();
/// #     const SCHEMA: u8 = 0;
/// #     type Payload = Self;
/// # }
/// # impl SelectRoute for Pick {}
/// # impl noctis_codec::DeclaredRuntime for Pick {}
/// # impl noctis_ui::Handled for Pick {}
/// let defaults = SelectDefaults { placeholder: Some(DefaultText::Literal("Pick")), min_values: None, max_values: None, disabled: false };
/// let _ = build_string_select_builder(Pick, &defaults).overrides(StringSelectOverrides::default());
/// ```
///
/// ```compile_fail,E0277
/// # use core::fmt;
/// # use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape, SelectRoute};
/// use noctis_ui::ActionRow;
/// use noctis_ui::__macro::{DefaultText, SelectDefaults, build_string_select_builder};
///
/// struct Pick;
/// # impl Payload for Pick {
/// #     const MAX_ENCODED_LEN: Option<usize> = Some(0);
/// #     fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result { Ok(()) }
/// # }
/// # impl Route for Pick {
/// #     const ID: RouteId = RouteId::new("pick");
/// #     const SHAPE: RouteShape = RouteShape::INLINE.select();
/// #     const SCHEMA: u8 = 0;
/// #     type Payload = Self;
/// # }
/// # impl SelectRoute for Pick {}
/// # impl noctis_codec::DeclaredRuntime for Pick {}
/// # impl noctis_ui::Handled for Pick {}
/// let defaults = SelectDefaults { placeholder: Some(DefaultText::Literal("Pick")), min_values: None, max_values: None, disabled: false };
/// let _ = ActionRow::select(build_string_select_builder(Pick, &defaults)); // [CV2-09]
/// ```
pub struct StringSelectBuilder<R> {
    select: StringSelect,
    route: PhantomData<fn() -> R>,
}

impl<R> StringSelectBuilder<R> {
    pub(crate) fn new(select: StringSelect) -> Self {
        Self {
            select,
            route: PhantomData,
        }
    }

    /// The options; makes the builder a [`StringSelect`].
    pub fn options(self, options: impl IntoIterator<Item = SelectOption>) -> StringSelect {
        self.select.options(options)
    }

    /// The text shown while nothing is selected: a literal or a locale key.
    #[must_use]
    pub fn placeholder(self, placeholder: impl Into<Text>) -> Self {
        Self::new(self.select.placeholder(placeholder))
    }

    /// The values of the options selected when the select renders; kept
    /// when [`options`](Self::options) gives the options (see
    /// [`StringSelect::default_values`]).
    #[must_use]
    pub fn default_values(
        self,
        values: impl IntoIterator<Item = impl Into<Cow<'static, str>>>,
    ) -> Self {
        Self::new(self.select.default_values(values))
    }

    /// The fewest values a user must pick (Discord's default is 1).
    #[must_use]
    pub fn min_values(self, min: u8) -> Self {
        Self::new(self.select.min_values(min))
    }

    /// The most values a user may pick (Discord's default is 1).
    #[must_use]
    pub fn max_values(self, max: u8) -> Self {
        Self::new(self.select.max_values(max))
    }

    /// Whether the select is greyed out.
    #[must_use]
    pub fn disabled(self, disabled: bool) -> Self {
        Self::new(self.select.disabled(disabled))
    }

    /// The select once given no options; what the placement impls of the
    /// unreachable `OptionsGiven` bound fall back to.
    pub(crate) fn into_empty(self) -> StringSelect {
        self.select
    }
}

impl<R> Clone for StringSelectBuilder<R> {
    fn clone(&self) -> Self {
        Self::new(self.select.clone())
    }
}

impl<R> core::fmt::Debug for StringSelectBuilder<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("StringSelectBuilder")
            .field(&self.select)
            .finish()
    }
}

/// A User Select (type 5); built with `R { .. }.build()` from a
/// `#[derive(UserSelectMenu)]` struct.
///
/// Placement: alone in an Action Row.
#[derive(Debug, Clone)]
pub struct UserSelect {
    pub(crate) common: Common,
    defaults: Vec<DefaultValue>,
}

impl UserSelect {
    pub(crate) fn from_id(custom_id: CustomId) -> Self {
        Self {
            common: Common::new(custom_id),
            defaults: Vec::new(),
        }
    }

    /// Appends users selected when the select renders (duplicates are
    /// removed; at most `max_values`, CV2-09).
    #[must_use]
    pub fn default_users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            users.into_iter().map(DefaultValue::User),
        );
        self
    }

    /// The users selected when the select renders, replacing the list
    /// (duplicates are removed; an empty list sends no `default_values`).
    #[must_use]
    pub fn default_values(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        replace_defaults(
            &mut self.defaults,
            users.into_iter().map(DefaultValue::User),
        );
        self
    }

    /// Applies every `Some` field of `overrides` exactly as its setter
    /// would, in declaration order.
    #[must_use]
    pub fn overrides(self, overrides: UserSelectOverrides) -> Self {
        let UserSelectOverrides {
            placeholder,
            min_values,
            max_values,
            disabled,
            default_values,
        } = overrides;
        let this = self.common_overrides(placeholder, min_values, max_values, disabled);
        match default_values {
            Some(values) => this.default_values(values),
            None => this,
        }
    }
}

/// A Role Select (type 6); built with `R { .. }.build()` from a
/// `#[derive(RoleSelectMenu)]` struct.
///
/// Placement: alone in an Action Row.
#[derive(Debug, Clone)]
pub struct RoleSelect {
    pub(crate) common: Common,
    defaults: Vec<DefaultValue>,
}

impl RoleSelect {
    pub(crate) fn from_id(custom_id: CustomId) -> Self {
        Self {
            common: Common::new(custom_id),
            defaults: Vec::new(),
        }
    }

    /// Appends roles selected when the select renders (duplicates are
    /// removed; at most `max_values`, CV2-09).
    #[must_use]
    pub fn default_roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            roles.into_iter().map(DefaultValue::Role),
        );
        self
    }

    /// The roles selected when the select renders, replacing the list
    /// (duplicates are removed; an empty list sends no `default_values`).
    #[must_use]
    pub fn default_values(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        replace_defaults(
            &mut self.defaults,
            roles.into_iter().map(DefaultValue::Role),
        );
        self
    }

    /// Applies every `Some` field of `overrides` exactly as its setter
    /// would, in declaration order.
    #[must_use]
    pub fn overrides(self, overrides: RoleSelectOverrides) -> Self {
        let RoleSelectOverrides {
            placeholder,
            min_values,
            max_values,
            disabled,
            default_values,
        } = overrides;
        let this = self.common_overrides(placeholder, min_values, max_values, disabled);
        match default_values {
            Some(values) => this.default_values(values),
            None => this,
        }
    }
}

/// A Mentionable Select (type 7): users and roles; built with
/// `R { .. }.build()` from a `#[derive(MentionableSelectMenu)]` struct.
///
/// Placement: alone in an Action Row.
#[derive(Debug, Clone)]
pub struct MentionableSelect {
    pub(crate) common: Common,
    defaults: Vec<DefaultValue>,
}

impl MentionableSelect {
    pub(crate) fn from_id(custom_id: CustomId) -> Self {
        Self {
            common: Common::new(custom_id),
            defaults: Vec::new(),
        }
    }

    /// Appends users selected when the select renders, after any default
    /// already set (duplicates are removed).
    #[must_use]
    pub fn default_users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            users.into_iter().map(DefaultValue::User),
        );
        self
    }

    /// Appends roles selected when the select renders, after any default
    /// already set (duplicates are removed).
    #[must_use]
    pub fn default_roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            roles.into_iter().map(DefaultValue::Role),
        );
        self
    }

    /// The users and roles selected when the select renders, in order,
    /// replacing the list (duplicates are removed; an empty list sends no
    /// `default_values`).
    #[must_use]
    pub fn default_values(mut self, ids: impl IntoIterator<Item = MentionableId>) -> Self {
        replace_defaults(&mut self.defaults, ids.into_iter().map(DefaultValue::from));
        self
    }

    /// Applies every `Some` field of `overrides` exactly as its setter
    /// would, in declaration order.
    #[must_use]
    pub fn overrides(self, overrides: MentionableSelectOverrides) -> Self {
        let MentionableSelectOverrides {
            placeholder,
            min_values,
            max_values,
            disabled,
            default_values,
        } = overrides;
        let this = self.common_overrides(placeholder, min_values, max_values, disabled);
        match default_values {
            Some(values) => this.default_values(values),
            None => this,
        }
    }
}

/// A Channel Select (type 8); built with `R { .. }.build()` from a
/// `#[derive(ChannelSelectMenu)]` struct.
///
/// Placement: alone in an Action Row.
#[derive(Debug, Clone)]
pub struct ChannelSelect {
    pub(crate) common: Common,
    defaults: Vec<DefaultValue>,
    channel_types: Vec<ChannelType>,
}

impl ChannelSelect {
    pub(crate) fn from_id(custom_id: CustomId) -> Self {
        Self {
            common: Common::new(custom_id),
            defaults: Vec::new(),
            channel_types: Vec::new(),
        }
    }

    /// Appends channels selected when the select renders (duplicates are
    /// removed; at most `max_values`, CV2-09).
    #[must_use]
    pub fn default_channels(
        mut self,
        channels: impl IntoIterator<Item = Id<ChannelMarker>>,
    ) -> Self {
        extend_defaults(
            &mut self.defaults,
            channels.into_iter().map(DefaultValue::Channel),
        );
        self
    }

    /// The channels selected when the select renders, replacing the list
    /// (duplicates are removed; an empty list sends no `default_values`).
    #[must_use]
    pub fn default_values(mut self, channels: impl IntoIterator<Item = Id<ChannelMarker>>) -> Self {
        replace_defaults(
            &mut self.defaults,
            channels.into_iter().map(DefaultValue::Channel),
        );
        self
    }

    /// Applies every `Some` field of `overrides` exactly as its setter
    /// would, in declaration order (`channel_types` last).
    #[must_use]
    pub fn overrides(self, overrides: ChannelSelectOverrides) -> Self {
        let ChannelSelectOverrides {
            placeholder,
            min_values,
            max_values,
            disabled,
            default_values,
            channel_types,
        } = overrides;
        let mut this = self.common_overrides(placeholder, min_values, max_values, disabled);
        if let Some(values) = default_values {
            this = this.default_values(values);
        }
        if let Some(types) = channel_types {
            this = this.channel_types(types);
        }
        this
    }

    /// The channel types the select offers, replacing the declared ones;
    /// none means every type.
    #[must_use]
    pub fn channel_types(mut self, types: impl IntoIterator<Item = ChannelType>) -> Self {
        self.channel_types = types.into_iter().collect();
        self
    }
}

common_setters!(
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect
);

impl Wire for StringSelect {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let options = OptionsWire {
            options: &self.options,
            list: self.default_values.as_deref(),
        };
        wire_select(
            3,
            &self.common,
            ids,
            s,
            |m| m.serialize_entry("options", &options),
            &[],
        )
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        out.push(&mut self.common.custom_id);
    }

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        out.extend(self.common.placeholder.as_mut());
    }
}

macro_rules! wire_auto {
    ($($ty:ident = $code:literal),*) => {$(
        impl Wire for $ty {
            fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
                wire_select($code, &self.common, ids, s, |_| Ok(()), &self.defaults)
            }

            fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
                out.push(&mut self.common.custom_id);
            }

            fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
                out.extend(self.common.placeholder.as_mut());
            }
        }
    )*};
}

wire_auto!(UserSelect = 5, RoleSelect = 6, MentionableSelect = 7);

impl Wire for ChannelSelect {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let types = &self.channel_types;
        let extra = |m: &mut S::SerializeMap| channel_types_entry(m, types);
        wire_select(8, &self.common, ids, s, extra, &self.defaults)
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        out.push(&mut self.common.custom_id);
    }

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        out.extend(self.common.placeholder.as_mut());
    }
}

/// A message select inside an Action Row.
#[derive(Debug, Clone)]
pub enum SelectNode {
    String(StringSelect),
    User(UserSelect),
    Role(RoleSelect),
    Mentionable(MentionableSelect),
    Channel(ChannelSelect),
}

impl SelectNode {
    /// The shared fields.
    pub(crate) fn common(&self) -> &Common {
        match self {
            Self::String(x) => &x.common,
            Self::User(x) => &x.common,
            Self::Role(x) => &x.common,
            Self::Mentionable(x) => &x.common,
            Self::Channel(x) => &x.common,
        }
    }

    /// The shared fields and, for a String Select, the options.
    pub(crate) fn parts_mut(&mut self) -> (&mut Common, Option<&mut Vec<SelectOption>>) {
        match self {
            Self::String(x) => (&mut x.common, Some(&mut x.options)),
            Self::User(x) => (&mut x.common, None),
            Self::Role(x) => (&mut x.common, None),
            Self::Mentionable(x) => (&mut x.common, None),
            Self::Channel(x) => (&mut x.common, None),
        }
    }

    /// The number of `default_values` (0 for a String Select, whose defaults
    /// are flagged options).
    pub(crate) fn default_count(&self) -> usize {
        match self {
            Self::String(_) => 0,
            Self::User(x) => x.defaults.len(),
            Self::Role(x) => x.defaults.len(),
            Self::Mentionable(x) => x.defaults.len(),
            Self::Channel(x) => x.defaults.len(),
        }
    }
}

impl Wire for SelectNode {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::String(x) => x.wire(ids, s),
            Self::User(x) => x.wire(ids, s),
            Self::Role(x) => x.wire(ids, s),
            Self::Mentionable(x) => x.wire(ids, s),
            Self::Channel(x) => x.wire(ids, s),
        }
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        match self {
            Self::String(x) => x.custom_ids_mut(out),
            Self::User(x) => x.custom_ids_mut(out),
            Self::Role(x) => x.custom_ids_mut(out),
            Self::Mentionable(x) => x.custom_ids_mut(out),
            Self::Channel(x) => x.custom_ids_mut(out),
        }
    }

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        match self {
            Self::String(x) => x.texts_mut(out),
            Self::User(x) => x.texts_mut(out),
            Self::Role(x) => x.texts_mut(out),
            Self::Mentionable(x) => x.texts_mut(out),
            Self::Channel(x) => x.texts_mut(out),
        }
    }
}

serialize_via_wire!(
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect
);
