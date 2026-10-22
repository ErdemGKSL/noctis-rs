//! Modal selects: String (type 3), User (5), Role (6), Mentionable (7) and
//! Channel (8) inside a modal Label (CV2-09, MOD-03).
//!
//! They are separate types from the message selects (AD-12): they take a
//! string key instead of a Route, have `required`, and have no `disabled`.

use std::borrow::Cow;

use serde::Serializer;
use serde::ser::SerializeMap;
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};

use super::input::required_entry;
use crate::component::{
    DefaultValue, SelectTail, channel_types_entry, extend_defaults, select_map,
};
use crate::wire::{Ids, Wire, serialize_via_wire};
use crate::{MentionableId, SelectOption, Text};

type Str = Cow<'static, str>;

/// The fields every modal select shares.
#[derive(Debug, Clone)]
pub(crate) struct ModalCommon {
    pub(crate) key: Str,
    pub(crate) placeholder: Option<Text>,
    pub(crate) min_values: Option<u8>,
    pub(crate) max_values: Option<u8>,
    pub(crate) required: bool,
}

impl ModalCommon {
    fn new(key: Str) -> Self {
        Self {
            key,
            placeholder: None,
            min_values: None,
            max_values: None,
            required: true,
        }
    }
}

/// Writes a modal select: the message select fields, then `required: false`
/// when not required; never `disabled`.
fn wire_modal_select<S: Serializer>(
    ty: u8,
    common: &ModalCommon,
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
    let mut map = select_map(ty, &common.key, ids, s, extra, &tail)?;
    required_entry(&mut map, common.required)?;
    map.end()
}

macro_rules! modal_common_setters {
    ($($ty:ident),*) => {$(
        impl $ty {
            /// The text shown while nothing is selected: a literal or a
            /// locale key (≤ 150).
            #[must_use]
            pub fn placeholder(mut self, placeholder: impl Into<Text>) -> Self {
                self.common.placeholder = Some(placeholder.into());
                self
            }

            /// The fewest values a user must pick (0-25; Discord's default
            /// is 1; 0 only when not required).
            #[must_use]
            pub fn min_values(mut self, min: u8) -> Self {
                self.common.min_values = Some(min);
                self
            }

            /// The most values a user may pick (1-25; Discord's default is 1).
            #[must_use]
            pub fn max_values(mut self, max: u8) -> Self {
                self.common.max_values = Some(max);
                self
            }

            /// Whether a value is required (default `true`).
            #[must_use]
            pub fn required(mut self, required: bool) -> Self {
                self.common.required = required;
                self
            }
        }
    )*};
}

/// A String Select (type 3) inside a modal Label: pick from fixed options.
///
/// Placement: only as a modal Label's input ([`ModalInput`](crate::ModalInput)).
/// Limits (CV2-09): 1-25 [`SelectOption`]s, `placeholder` ≤ 150,
/// `min_values` 0-25 (0 only when not required), `max_values` 1-25. It has
/// no `disabled` (MOD-03).
///
/// ```
/// use noctis_ui::{ModalStringSelect, SelectOption};
///
/// let select = ModalStringSelect::new("size", [SelectOption::new("Small", "s")]).required(false);
/// assert_eq!(
///     serde_json::to_value(&select).unwrap(),
///     serde_json::json!({
///         "type": 3, "custom_id": "size", "options": [{"label": "Small", "value": "s"}],
///         "required": false
///     })
/// );
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::ModalStringSelect;
///
/// let _ = ModalStringSelect::new("s", []).disabled(true); // [MOD-03]
/// ```
#[derive(Debug, Clone)]
pub struct ModalStringSelect {
    pub(crate) common: ModalCommon,
    pub(crate) options: Vec<SelectOption>,
}

impl ModalStringSelect {
    /// A required String Select whose submitted field is named `key`.
    pub fn new(key: impl Into<Str>, options: impl IntoIterator<Item = SelectOption>) -> Self {
        Self {
            common: ModalCommon::new(key.into()),
            options: options.into_iter().collect(),
        }
    }
}

/// A User Select (type 5) inside a modal Label.
///
/// Placement: only as a modal Label's input. Limits as
/// [`ModalStringSelect`] without options; no `disabled` (MOD-03).
///
/// ```
/// use noctis_ui::ModalUserSelect;
/// use twilight_model::id::Id;
///
/// let select = ModalUserSelect::new("who").max_values(2).default_users([Id::new(7)]);
/// assert_eq!(
///     serde_json::to_value(&select).unwrap(),
///     serde_json::json!({
///         "type": 5, "custom_id": "who",
///         "default_values": [{"id": "7", "type": "user"}], "max_values": 2
///     })
/// );
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::ModalUserSelect;
///
/// let _ = ModalUserSelect::new("u").disabled(true); // [MOD-03]
/// ```
#[derive(Debug, Clone)]
pub struct ModalUserSelect {
    pub(crate) common: ModalCommon,
    pub(crate) defaults: Vec<DefaultValue>,
}

impl ModalUserSelect {
    /// A required User Select whose submitted field is named `key`.
    pub fn new(key: impl Into<Str>) -> Self {
        Self {
            common: ModalCommon::new(key.into()),
            defaults: Vec::new(),
        }
    }

    /// Appends users selected when the modal opens.
    #[must_use]
    pub fn default_users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            users.into_iter().map(DefaultValue::User),
        );
        self
    }
}

/// A Role Select (type 6) inside a modal Label.
///
/// Placement: only as a modal Label's input. Limits as
/// [`ModalStringSelect`] without options; no `disabled` (MOD-03).
///
/// ```
/// use noctis_ui::ModalRoleSelect;
/// use twilight_model::id::Id;
///
/// let select = ModalRoleSelect::new("role").default_roles([Id::new(9)]);
/// assert_eq!(
///     serde_json::to_value(&select).unwrap(),
///     serde_json::json!({"type": 6, "custom_id": "role", "default_values": [{"id": "9", "type": "role"}]})
/// );
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::ModalRoleSelect;
///
/// let _ = ModalRoleSelect::new("r").disabled(true); // [MOD-03]
/// ```
#[derive(Debug, Clone)]
pub struct ModalRoleSelect {
    pub(crate) common: ModalCommon,
    pub(crate) defaults: Vec<DefaultValue>,
}

impl ModalRoleSelect {
    /// A required Role Select whose submitted field is named `key`.
    pub fn new(key: impl Into<Str>) -> Self {
        Self {
            common: ModalCommon::new(key.into()),
            defaults: Vec::new(),
        }
    }

    /// Appends roles selected when the modal opens.
    #[must_use]
    pub fn default_roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            roles.into_iter().map(DefaultValue::Role),
        );
        self
    }
}

/// A Mentionable Select (type 7) inside a modal Label: users and roles.
///
/// Placement: only as a modal Label's input. Limits as
/// [`ModalStringSelect`] without options; no `disabled` (MOD-03).
///
/// ```
/// use noctis_ui::ModalMentionableSelect;
///
/// let select = ModalMentionableSelect::new("target").placeholder("Who?");
/// assert_eq!(
///     serde_json::to_value(&select).unwrap(),
///     serde_json::json!({"type": 7, "custom_id": "target", "placeholder": "Who?"})
/// );
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::ModalMentionableSelect;
///
/// let _ = ModalMentionableSelect::new("m").disabled(true); // [MOD-03]
/// ```
#[derive(Debug, Clone)]
pub struct ModalMentionableSelect {
    pub(crate) common: ModalCommon,
    pub(crate) defaults: Vec<DefaultValue>,
}

impl ModalMentionableSelect {
    /// A required Mentionable Select whose submitted field is named `key`.
    pub fn new(key: impl Into<Str>) -> Self {
        Self {
            common: ModalCommon::new(key.into()),
            defaults: Vec::new(),
        }
    }

    /// Appends users selected when the modal opens, after any default
    /// already set.
    #[must_use]
    pub fn default_users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            users.into_iter().map(DefaultValue::User),
        );
        self
    }

    /// Appends roles selected when the modal opens, after any default
    /// already set.
    #[must_use]
    pub fn default_roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        extend_defaults(
            &mut self.defaults,
            roles.into_iter().map(DefaultValue::Role),
        );
        self
    }

    /// Appends users and roles selected when the modal opens, in order.
    #[must_use]
    pub fn default_mentionables(mut self, ids: impl IntoIterator<Item = MentionableId>) -> Self {
        extend_defaults(&mut self.defaults, ids.into_iter().map(DefaultValue::from));
        self
    }
}

/// A Channel Select (type 8) inside a modal Label.
///
/// Placement: only as a modal Label's input. Limits as
/// [`ModalStringSelect`] without options; no `disabled` (MOD-03).
///
/// ```
/// use noctis_ui::ModalChannelSelect;
/// use twilight_model::channel::ChannelType;
///
/// let select = ModalChannelSelect::new("where").channel_types([ChannelType::GuildText]);
/// assert_eq!(
///     serde_json::to_value(&select).unwrap(),
///     serde_json::json!({"type": 8, "custom_id": "where", "channel_types": [0]})
/// );
/// ```
///
/// ```compile_fail,E0599
/// use noctis_ui::ModalChannelSelect;
///
/// let _ = ModalChannelSelect::new("c").disabled(true); // [MOD-03]
/// ```
#[derive(Debug, Clone)]
pub struct ModalChannelSelect {
    pub(crate) common: ModalCommon,
    pub(crate) defaults: Vec<DefaultValue>,
    pub(crate) channel_types: Vec<ChannelType>,
}

impl ModalChannelSelect {
    /// A required Channel Select whose submitted field is named `key`.
    pub fn new(key: impl Into<Str>) -> Self {
        Self {
            common: ModalCommon::new(key.into()),
            defaults: Vec::new(),
            channel_types: Vec::new(),
        }
    }

    /// Appends channels selected when the modal opens.
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

    /// Appends channel types the select offers; none means every type.
    #[must_use]
    pub fn channel_types(mut self, types: impl IntoIterator<Item = ChannelType>) -> Self {
        self.channel_types.extend(types);
        self
    }
}

modal_common_setters!(
    ModalStringSelect,
    ModalUserSelect,
    ModalRoleSelect,
    ModalMentionableSelect,
    ModalChannelSelect
);

impl Wire for ModalStringSelect {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let options = &self.options;
        wire_modal_select(
            3,
            &self.common,
            ids,
            s,
            |m| m.serialize_entry("options", options),
            &[],
        )
    }
}

macro_rules! wire_modal_auto {
    ($($ty:ident = $code:literal),*) => {$(
        impl Wire for $ty {
            fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
                wire_modal_select($code, &self.common, ids, s, |_| Ok(()), &self.defaults)
            }
        }
    )*};
}

wire_modal_auto!(
    ModalUserSelect = 5,
    ModalRoleSelect = 6,
    ModalMentionableSelect = 7
);

impl Wire for ModalChannelSelect {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let types = &self.channel_types;
        let extra = |m: &mut S::SerializeMap| channel_types_entry(m, types);
        wire_modal_select(8, &self.common, ids, s, extra, &self.defaults)
    }
}

/// A modal select inside a Label.
#[derive(Debug, Clone)]
pub enum ModalSelectNode {
    String(ModalStringSelect),
    User(ModalUserSelect),
    Role(ModalRoleSelect),
    Mentionable(ModalMentionableSelect),
    Channel(ModalChannelSelect),
}

impl ModalSelectNode {
    /// The shared fields.
    pub(crate) fn common(&self) -> &ModalCommon {
        match self {
            Self::String(x) => &x.common,
            Self::User(x) => &x.common,
            Self::Role(x) => &x.common,
            Self::Mentionable(x) => &x.common,
            Self::Channel(x) => &x.common,
        }
    }

    /// The shared fields and, for a String Select, the options.
    pub(crate) fn parts_mut(&mut self) -> (&mut ModalCommon, Option<&mut Vec<SelectOption>>) {
        match self {
            Self::String(x) => (&mut x.common, Some(&mut x.options)),
            Self::User(x) => (&mut x.common, None),
            Self::Role(x) => (&mut x.common, None),
            Self::Mentionable(x) => (&mut x.common, None),
            Self::Channel(x) => (&mut x.common, None),
        }
    }

    /// The number of `default_values` (0 for a String Select).
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

impl Wire for ModalSelectNode {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::String(x) => x.wire(ids, s),
            Self::User(x) => x.wire(ids, s),
            Self::Role(x) => x.wire(ids, s),
            Self::Mentionable(x) => x.wire(ids, s),
            Self::Channel(x) => x.wire(ids, s),
        }
    }
}

serialize_via_wire!(
    ModalStringSelect,
    ModalUserSelect,
    ModalRoleSelect,
    ModalMentionableSelect,
    ModalChannelSelect
);
