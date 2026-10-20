//! Message selects: String (type 3), User (5), Role (6), Mentionable (7) and
//! Channel (8) (CV2-09).

use std::borrow::Cow;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};

use super::Emoji;
use crate::route::CustomId;
use crate::wire::{Ids, Wire, flag, head, opt, serialize_via_wire};

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

impl Serialize for SelectOption {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        map.serialize_entry("label", &self.label)?;
        map.serialize_entry("value", &self.value)?;
        opt(&mut map, "description", self.description.as_deref())?;
        opt(&mut map, "emoji", self.emoji.as_ref())?;
        flag(&mut map, "default", self.default)?;
        map.end()
    }
}

/// The fields every message select shares.
#[derive(Debug, Clone)]
pub(crate) struct Common {
    pub(crate) custom_id: CustomId,
    pub(crate) placeholder: Option<Cow<'static, str>>,
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
}

/// A pre-selected value of an auto-populated select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefaultValue {
    User(Id<UserMarker>),
    Role(Id<RoleMarker>),
    Channel(Id<ChannelMarker>),
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
        placeholder: common.placeholder.as_deref(),
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
            /// The text shown while nothing is selected.
            #[must_use]
            pub fn placeholder(mut self, placeholder: impl Into<Cow<'static, str>>) -> Self {
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
        }
    )*};
}

/// A String Select (type 3): pick from fixed options; built with
/// [`RouteComponents::string_select`](crate::RouteComponents::string_select).
///
/// Placement: alone in an Action Row ([`ActionRow::select`](crate::ActionRow::select)).
#[derive(Debug, Clone)]
pub struct StringSelect {
    pub(crate) common: Common,
    pub(crate) options: Vec<SelectOption>,
}

impl StringSelect {
    pub(crate) fn from_id(custom_id: CustomId, options: Vec<SelectOption>) -> Self {
        Self {
            common: Common::new(custom_id),
            options,
        }
    }
}

/// A User Select (type 5); built with
/// [`RouteComponents::user_select`](crate::RouteComponents::user_select).
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

    /// Appends users selected when the select renders.
    #[must_use]
    pub fn default_users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        self.defaults
            .extend(users.into_iter().map(DefaultValue::User));
        self
    }
}

/// A Role Select (type 6); built with
/// [`RouteComponents::role_select`](crate::RouteComponents::role_select).
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

    /// Appends roles selected when the select renders.
    #[must_use]
    pub fn default_roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        self.defaults
            .extend(roles.into_iter().map(DefaultValue::Role));
        self
    }
}

/// A Mentionable Select (type 7): users and roles; built with
/// [`RouteComponents::mentionable_select`](crate::RouteComponents::mentionable_select).
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
    /// already set.
    #[must_use]
    pub fn default_users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        self.defaults
            .extend(users.into_iter().map(DefaultValue::User));
        self
    }

    /// Appends roles selected when the select renders, after any default
    /// already set.
    #[must_use]
    pub fn default_roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        self.defaults
            .extend(roles.into_iter().map(DefaultValue::Role));
        self
    }
}

/// A Channel Select (type 8); built with
/// [`RouteComponents::channel_select`](crate::RouteComponents::channel_select).
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

    /// Appends channels selected when the select renders.
    #[must_use]
    pub fn default_channels(
        mut self,
        channels: impl IntoIterator<Item = Id<ChannelMarker>>,
    ) -> Self {
        self.defaults
            .extend(channels.into_iter().map(DefaultValue::Channel));
        self
    }

    /// Appends channel types the select offers; none means every type.
    #[must_use]
    pub fn channel_types(mut self, types: impl IntoIterator<Item = ChannelType>) -> Self {
        self.channel_types.extend(types);
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
        let options = &self.options;
        wire_select(
            3,
            &self.common,
            ids,
            s,
            |m| m.serialize_entry("options", options),
            &[],
        )
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        out.push(&mut self.common.custom_id);
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
}

serialize_via_wire!(
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect
);
