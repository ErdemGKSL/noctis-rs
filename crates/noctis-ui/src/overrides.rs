//! Multi-field overrides for one render of a Route component (AD-19,
//! FR-59): [`ButtonOverrides`], [`StringSelectOverrides`],
//! [`UserSelectOverrides`], [`RoleSelectOverrides`],
//! [`MentionableSelectOverrides`] and [`ChannelSelectOverrides`].
//!
//! Each is plain data with every field an `Option` and a `Default`, so a
//! struct literal names only what it changes and ends with
//! `..Default::default()`. `.overrides(o)` applies each `Some` field exactly
//! as its setter would, in declaration order; setters and `.overrides`
//! compose and the later call wins. Overrides change one render only: the
//! declared `defaults(..)` are a const. The structs are deliberately not
//! `#[non_exhaustive]`, so the struct-update literal compiles outside this
//! crate; adding a field is a breaking change, accepted before 1.0.
//!
//! ```
//! # use core::fmt;
//! # use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape};
//! use noctis_ui::{ButtonOverrides, ButtonStyle};
//! use noctis_ui::__macro::{ButtonDefaults, DefaultText, build_button};
//!
//! # #[derive(Clone)]
//! struct Buy;
//! # impl Payload for Buy {
//! #     const MAX_ENCODED_LEN: Option<usize> = Some(0);
//! #     fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result { Ok(()) }
//! # }
//! # impl Route for Buy {
//! #     const ID: RouteId = RouteId::new("shop.buy");
//! #     const SHAPE: RouteShape = RouteShape::INLINE;
//! #     const SCHEMA: u8 = 0;
//! #     type Payload = Self;
//! # }
//! # impl noctis_codec::DeclaredRuntime for Buy {}
//! # impl noctis_ui::Handled for Buy {}
//! const DEFAULTS: ButtonDefaults =
//!     ButtonDefaults { style: ButtonStyle::Secondary, label: Some(DefaultText::Literal("Buy")), emoji: None, disabled: false };
//!
//! // What `#[derive(Button)]`'s `Buy.build()` expands to, then one render's overrides.
//! let button = build_button(Buy, &DEFAULTS).overrides(ButtonOverrides {
//!     label: Some("Buy again".into()),
//!     disabled: Some(true),
//!     ..Default::default()
//! });
//! let json = serde_json::to_value(&button).unwrap();
//! assert_eq!((&json["label"], &json["disabled"], &json["style"]), (&"Buy again".into(), &true.into(), &2.into()));
//! ```

use std::borrow::Cow;

use twilight_model::channel::ChannelType;
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, RoleMarker, UserMarker};

use crate::component::{ButtonStyle, Emoji, SelectOption};
use crate::{MentionableId, Text};

/// Overrides for one render of a [`RouteButton`](crate::RouteButton),
/// applied with `.overrides(..)`.
#[derive(Debug, Clone, Default)]
pub struct ButtonOverrides {
    /// The style.
    pub style: Option<ButtonStyle>,
    /// The label (≤ 80, CV2-08).
    pub label: Option<Text>,
    /// The emoji.
    pub emoji: Option<Emoji>,
    /// Whether the button is greyed out.
    pub disabled: Option<bool>,
}

/// Overrides for one render of a [`StringSelect`](crate::StringSelect),
/// applied with `.overrides(..)` (a [`StringSelectBuilder`](crate::StringSelectBuilder)
/// gets `.options(..)` first).
#[derive(Debug, Clone, Default)]
pub struct StringSelectOverrides {
    /// The placeholder (≤ 150, CV2-09).
    pub placeholder: Option<Text>,
    /// `min_values` (0-25).
    pub min_values: Option<u8>,
    /// `max_values` (1-25).
    pub max_values: Option<u8>,
    /// Whether the select is greyed out.
    pub disabled: Option<bool>,
    /// The options, replacing the declared ones (1-25).
    pub options: Option<Vec<SelectOption>>,
    /// The values of the options selected when the select renders.
    pub default_values: Option<Vec<Cow<'static, str>>>,
}

/// Overrides for one render of a [`UserSelect`](crate::UserSelect).
#[derive(Debug, Clone, Default)]
pub struct UserSelectOverrides {
    /// The placeholder (≤ 150, CV2-09).
    pub placeholder: Option<Text>,
    /// `min_values` (0-25).
    pub min_values: Option<u8>,
    /// `max_values` (1-25).
    pub max_values: Option<u8>,
    /// Whether the select is greyed out.
    pub disabled: Option<bool>,
    /// The users selected when the select renders.
    pub default_values: Option<Vec<Id<UserMarker>>>,
}

/// Overrides for one render of a [`RoleSelect`](crate::RoleSelect).
#[derive(Debug, Clone, Default)]
pub struct RoleSelectOverrides {
    /// The placeholder (≤ 150, CV2-09).
    pub placeholder: Option<Text>,
    /// `min_values` (0-25).
    pub min_values: Option<u8>,
    /// `max_values` (1-25).
    pub max_values: Option<u8>,
    /// Whether the select is greyed out.
    pub disabled: Option<bool>,
    /// The roles selected when the select renders.
    pub default_values: Option<Vec<Id<RoleMarker>>>,
}

/// Overrides for one render of a [`MentionableSelect`](crate::MentionableSelect).
#[derive(Debug, Clone, Default)]
pub struct MentionableSelectOverrides {
    /// The placeholder (≤ 150, CV2-09).
    pub placeholder: Option<Text>,
    /// `min_values` (0-25).
    pub min_values: Option<u8>,
    /// `max_values` (1-25).
    pub max_values: Option<u8>,
    /// Whether the select is greyed out.
    pub disabled: Option<bool>,
    /// The users and roles selected when the select renders.
    pub default_values: Option<Vec<MentionableId>>,
}

/// Overrides for one render of a [`ChannelSelect`](crate::ChannelSelect).
#[derive(Debug, Clone, Default)]
pub struct ChannelSelectOverrides {
    /// The placeholder (≤ 150, CV2-09).
    pub placeholder: Option<Text>,
    /// `min_values` (0-25).
    pub min_values: Option<u8>,
    /// `max_values` (1-25).
    pub max_values: Option<u8>,
    /// Whether the select is greyed out.
    pub disabled: Option<bool>,
    /// The channels selected when the select renders.
    pub default_values: Option<Vec<Id<ChannelMarker>>>,
    /// The channel types offered, replacing the declared ones.
    pub channel_types: Option<Vec<ChannelType>>,
}
