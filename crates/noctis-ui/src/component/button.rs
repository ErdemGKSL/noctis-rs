//! Buttons (type 2): [`RouteButton`], [`LinkButton`], [`PremiumButton`] and
//! the opaque [`Button`] (CV2-08).

use core::fmt;
use core::marker::PhantomData;
use std::borrow::Cow;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use twilight_model::id::Id;
use twilight_model::id::marker::{EmojiMarker, SkuMarker};

use crate::route::CustomId;
use crate::wire::{Ids, Wire, flag, head, opt, serialize_via_wire};

/// An emoji on a button or select option.
///
/// ```
/// use noctis_ui::Emoji;
/// use twilight_model::id::Id;
///
/// assert_eq!(serde_json::to_value(Emoji::unicode("🛒")).unwrap(), serde_json::json!({"name": "🛒"}));
/// assert_eq!(
///     serde_json::to_value(Emoji::custom(Id::new(42), "pog", true)).unwrap(),
///     serde_json::json!({"id": "42", "name": "pog", "animated": true})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emoji(EmojiKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum EmojiKind {
    Unicode(Cow<'static, str>),
    Custom {
        id: Id<EmojiMarker>,
        name: Cow<'static, str>,
        animated: bool,
    },
}

impl Emoji {
    /// A Unicode emoji such as `"🔄"`.
    pub fn unicode(emoji: impl Into<Cow<'static, str>>) -> Self {
        Self(EmojiKind::Unicode(emoji.into()))
    }

    /// A custom emoji of a guild or the application.
    pub fn custom(id: Id<EmojiMarker>, name: impl Into<Cow<'static, str>>, animated: bool) -> Self {
        Self(EmojiKind::Custom {
            id,
            name: name.into(),
            animated,
        })
    }
}

impl Serialize for Emoji {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(None)?;
        match &self.0 {
            EmojiKind::Unicode(name) => map.serialize_entry("name", name)?,
            EmojiKind::Custom { id, name, animated } => {
                map.serialize_entry("id", &id.get().to_string())?;
                map.serialize_entry("name", name)?;
                flag(&mut map, "animated", *animated)?;
            }
        }
        map.end()
    }
}

/// The style of a [`RouteButton`]; Secondary by default. Link and Premium
/// buttons are their own types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ButtonStyle {
    /// Blurple (`1`): the one main action of a group.
    Primary,
    /// Grey (`2`), the default.
    #[default]
    Secondary,
    /// Green (`3`).
    Success,
    /// Red (`4`): destructive actions only.
    Danger,
}

impl ButtonStyle {
    const fn value(self) -> u8 {
        match self {
            Self::Primary => 1,
            Self::Secondary => 2,
            Self::Success => 3,
            Self::Danger => 4,
        }
    }
}

/// The Route button's fields, shared by every `RouteButton<R>`.
#[derive(Debug, Clone)]
struct RouteButtonData {
    style: ButtonStyle,
    label: Option<Cow<'static, str>>,
    emoji: Option<Emoji>,
    disabled: bool,
    custom_id: CustomId,
}

/// An interactive button (styles 1-4) whose click reaches the handler of
/// Route `R`; built with [`RouteComponents::button`](crate::RouteComponents::button).
///
/// Placement: an Action Row or a Section accessory.
pub struct RouteButton<R> {
    data: RouteButtonData,
    route: PhantomData<fn() -> R>,
}

impl<R> RouteButton<R> {
    pub(crate) fn from_id(custom_id: CustomId) -> Self {
        Self {
            data: RouteButtonData {
                style: ButtonStyle::Secondary,
                label: None,
                emoji: None,
                disabled: false,
                custom_id,
            },
            route: PhantomData,
        }
    }

    fn map(mut self, f: impl FnOnce(&mut RouteButtonData)) -> Self {
        f(&mut self.data);
        self
    }

    /// The style (default Secondary).
    #[must_use]
    pub fn style(self, style: ButtonStyle) -> Self {
        self.map(|d| d.style = style)
    }

    /// The label text.
    #[must_use]
    pub fn label(self, label: impl Into<Cow<'static, str>>) -> Self {
        self.map(|d| d.label = Some(label.into()))
    }

    /// The emoji shown before the label.
    #[must_use]
    pub fn emoji(self, emoji: Emoji) -> Self {
        self.map(|d| d.emoji = Some(emoji))
    }

    /// Whether the button is greyed out.
    #[must_use]
    pub fn disabled(self, disabled: bool) -> Self {
        self.map(|d| d.disabled = disabled)
    }
}

impl<R> Clone for RouteButton<R> {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            route: PhantomData,
        }
    }
}

impl<R> fmt::Debug for RouteButton<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteButton")
            .field("style", &self.data.style)
            .field("label", &self.data.label)
            .field("emoji", &self.data.emoji)
            .field("disabled", &self.data.disabled)
            .field("custom_id", &self.data.custom_id)
            .finish()
    }
}

impl Wire for RouteButtonData {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 2, ids)?;
        map.serialize_entry("style", &self.style.value())?;
        opt(&mut map, "label", self.label.as_deref())?;
        opt(&mut map, "emoji", self.emoji.as_ref())?;
        map.serialize_entry("custom_id", &self.custom_id)?;
        flag(&mut map, "disabled", self.disabled)?;
        map.end()
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        out.push(&mut self.custom_id);
    }
}

impl<R> Wire for RouteButton<R> {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        self.data.wire(ids, s)
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        self.data.custom_ids_mut(out);
    }
}

impl<R> Serialize for RouteButton<R> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.wire(None, s)
    }
}

/// A Link button (style 5): opens `url`, sends no interaction and has no
/// `custom_id`.
///
/// Placement: an Action Row or a Section accessory.
///
/// ```
/// use noctis_ui::LinkButton;
///
/// let docs = LinkButton::new("https://x.y").label("Docs");
/// assert_eq!(
///     serde_json::to_value(&docs).unwrap(),
///     serde_json::json!({"type": 2, "style": 5, "label": "Docs", "url": "https://x.y"})
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkButton {
    url: Cow<'static, str>,
    label: Option<Cow<'static, str>>,
    emoji: Option<Emoji>,
    disabled: bool,
}

impl LinkButton {
    /// A Link button opening `url`.
    pub fn new(url: impl Into<Cow<'static, str>>) -> Self {
        Self {
            url: url.into(),
            label: None,
            emoji: None,
            disabled: false,
        }
    }

    /// The label text.
    #[must_use]
    pub fn label(self, label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            label: Some(label.into()),
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

    /// Whether the button is greyed out.
    #[must_use]
    pub fn disabled(self, disabled: bool) -> Self {
        Self { disabled, ..self }
    }
}

impl Wire for LinkButton {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 2, ids)?;
        map.serialize_entry("style", &5_u8)?;
        opt(&mut map, "label", self.label.as_deref())?;
        opt(&mut map, "emoji", self.emoji.as_ref())?;
        map.serialize_entry("url", &self.url)?;
        flag(&mut map, "disabled", self.disabled)?;
        map.end()
    }
}

/// A Premium button (style 6): buys the SKU. It has no label, emoji, URL or
/// `custom_id`.
///
/// Placement: an Action Row or a Section accessory.
///
/// ```
/// use noctis_ui::PremiumButton;
/// use twilight_model::id::Id;
///
/// assert_eq!(
///     serde_json::to_value(PremiumButton::new(Id::new(7)).disabled(true)).unwrap(),
///     serde_json::json!({"type": 2, "style": 6, "sku_id": "7", "disabled": true})
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PremiumButton {
    sku_id: Id<SkuMarker>,
    disabled: bool,
}

impl PremiumButton {
    /// A Premium button for the SKU `sku_id`.
    pub const fn new(sku_id: Id<SkuMarker>) -> Self {
        Self {
            sku_id,
            disabled: false,
        }
    }

    /// Whether the button is greyed out.
    #[must_use]
    pub const fn disabled(self, disabled: bool) -> Self {
        Self { disabled, ..self }
    }
}

impl Wire for PremiumButton {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 2, ids)?;
        map.serialize_entry("style", &6_u8)?;
        map.serialize_entry("sku_id", &self.sku_id.get().to_string())?;
        flag(&mut map, "disabled", self.disabled)?;
        map.end()
    }
}

/// Any button: a [`RouteButton`], [`LinkButton`] or [`PremiumButton`]
/// converted with `From`, so one Action Row can mix them.
///
/// ```
/// use noctis_ui::{ActionRow, Button, LinkButton, PremiumButton};
/// use twilight_model::id::Id;
///
/// let row = ActionRow::buttons([
///     Button::from(LinkButton::new("https://x.y").label("Docs")),
///     Button::from(PremiumButton::new(Id::new(7))),
/// ]);
/// assert_eq!(serde_json::to_value(&row).unwrap()["components"][1]["style"], 6);
/// ```
#[derive(Debug, Clone)]
pub struct Button(ButtonNode);

#[derive(Debug, Clone)]
enum ButtonNode {
    Route(RouteButtonData),
    Link(LinkButton),
    Premium(PremiumButton),
}

impl<R> From<RouteButton<R>> for Button {
    fn from(b: RouteButton<R>) -> Self {
        Self(ButtonNode::Route(b.data))
    }
}

impl From<LinkButton> for Button {
    fn from(b: LinkButton) -> Self {
        Self(ButtonNode::Link(b))
    }
}

impl From<PremiumButton> for Button {
    fn from(b: PremiumButton) -> Self {
        Self(ButtonNode::Premium(b))
    }
}

impl Wire for Button {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ButtonNode::Route(b) => b.wire(ids, s),
            ButtonNode::Link(b) => b.wire(ids, s),
            ButtonNode::Premium(b) => b.wire(ids, s),
        }
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        if let ButtonNode::Route(b) = &mut self.0 {
            b.custom_ids_mut(out);
        }
    }
}

serialize_via_wire!(LinkButton, PremiumButton, Button);
