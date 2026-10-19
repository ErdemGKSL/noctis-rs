//! Layout components: Container (type 17), Section (type 9) and Action Row
//! (type 1), plus the opaque [`ContainerComponent`] and
//! [`TopLevelComponent`].

use serde::Serializer;
use serde::ser::SerializeMap;

use super::{Button, File, MediaGallery, SelectNode, Separator, TextDisplay, Thumbnail};
use crate::placement::sealed::IntoRowButton;
use crate::placement::{ContainerChild, RowItem, RowSelect, SectionAccessory};
use crate::route::CustomId;
use crate::wire::{Ids, Wire, WiredSeq, flag, head, serialize_via_wire};

/// A 24-bit RGB colour, such as a Container's accent (CV2-13).
///
/// ```
/// use noctis_ui::Rgb;
///
/// const BLURPLE: Rgb = Rgb::new(0x58, 0x65, 0xF2);
/// assert_eq!(BLURPLE.value(), 0x5865F2);
/// assert_eq!(Rgb::new(255, 255, 255).value(), 16_777_215);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb(u32);

impl Rgb {
    /// The colour with red `r`, green `g` and blue `b`.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        // Widening casts; `u32::from` is not const.
        Self(((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    /// The colour as `0xRRGGBB`.
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// A Section's accessory.
#[derive(Debug, Clone)]
pub enum AccessoryNode {
    Button(Button),
    Thumbnail(Thumbnail),
}

impl Wire for AccessoryNode {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Button(b) => b.wire(ids, s),
            Self::Thumbnail(t) => t.wire(ids, s),
        }
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        if let Self::Button(b) = self {
            b.custom_ids_mut(out);
        }
    }
}

/// A Section (type 9): 1 to 3 Text Displays beside exactly one accessory, a
/// button or a [`Thumbnail`] (CV2-07).
///
/// Placement: top level or a Container. There is no constructor without an
/// accessory.
///
/// ```
/// use noctis_ui::{Media, Section, TextDisplay, Thumbnail};
///
/// let section = Section::new(
///     [TextDisplay::new("a")],
///     Thumbnail::new(Media::url("https://i/x.png")),
/// );
/// assert_eq!(
///     serde_json::to_value(&section).unwrap(),
///     serde_json::json!({
///         "type": 9,
///         "components": [{"type": 10, "content": "a"}],
///         "accessory": {"type": 11, "media": {"url": "https://i/x.png"}}
///     })
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Section {
    texts: Vec<TextDisplay>,
    accessory: AccessoryNode,
}

impl Section {
    /// A Section of `texts` with `accessory` (the text count is checked by the
    /// runtime validator).
    pub fn new<A: SectionAccessory, const N: usize>(texts: [TextDisplay; N], accessory: A) -> Self {
        Self {
            texts: texts.into(),
            accessory: accessory.into_accessory(),
        }
    }
}

impl Wire for Section {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 9, ids)?;
        map.serialize_entry("components", &WiredSeq(&self.texts, ids))?;
        map.serialize_entry("accessory", &crate::wire::Wired(&self.accessory, ids))?;
        map.end()
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        self.accessory.custom_ids_mut(out);
    }
}

#[derive(Debug, Clone)]
enum RowNode {
    Buttons(Vec<Button>),
    Select(SelectNode),
}

/// An Action Row (type 1): 1 to 5 buttons, or exactly one select (CV2-06).
///
/// Placement: top level or a Container.
///
/// ```
/// use noctis_ui::{ActionRow, LinkButton};
///
/// let row = ActionRow::buttons([LinkButton::new("https://a.b"), LinkButton::new("https://c.d")]);
/// assert_eq!(serde_json::to_value(&row).unwrap()["components"].as_array().unwrap().len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct ActionRow(RowNode);

impl ActionRow {
    /// A row of `buttons`, in order. Mix kinds by converting each to
    /// [`Button`]. The count is checked by the runtime validator.
    pub fn buttons<B: RowItem, const N: usize>(buttons: [B; N]) -> Self {
        Self(RowNode::Buttons(
            buttons
                .into_iter()
                .map(IntoRowButton::into_row_button)
                .collect(),
        ))
    }

    /// A row holding the single `select`.
    pub fn select<S: RowSelect>(select: S) -> Self {
        Self(RowNode::Select(select.into_select()))
    }
}

impl Wire for ActionRow {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 1, ids)?;
        match &self.0 {
            RowNode::Buttons(buttons) => {
                map.serialize_entry("components", &WiredSeq(buttons, ids))?;
            }
            RowNode::Select(select) => {
                map.serialize_entry("components", &WiredSeq(core::slice::from_ref(select), ids))?;
            }
        }
        map.end()
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        match &mut self.0 {
            RowNode::Buttons(buttons) => {
                for b in buttons {
                    b.custom_ids_mut(out);
                }
            }
            RowNode::Select(select) => select.custom_ids_mut(out),
        }
    }
}

#[derive(Debug, Clone)]
enum ChildNode {
    Section(Section),
    Text(TextDisplay),
    Gallery(MediaGallery),
    File(File),
    Separator(Separator),
    Row(ActionRow),
}

/// Any component a Container may hold, converted with `From`: Section, Text
/// Display, Media Gallery, File, Separator or Action Row.
///
/// ```
/// use noctis_ui::{Container, ContainerComponent, Separator, TextDisplay};
///
/// let children: Vec<ContainerComponent> = vec![TextDisplay::new("a").into(), Separator::new().into()];
/// let container = children.into_iter().fold(Container::new(), Container::push);
/// assert_eq!(serde_json::to_value(&container).unwrap()["components"][1]["type"], 14);
/// ```
#[derive(Debug, Clone)]
pub struct ContainerComponent(ChildNode);

impl Wire for ContainerComponent {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ChildNode::Section(x) => x.wire(ids, s),
            ChildNode::Text(x) => x.wire(ids, s),
            ChildNode::Gallery(x) => x.wire(ids, s),
            ChildNode::File(x) => x.wire(ids, s),
            ChildNode::Separator(x) => x.wire(ids, s),
            ChildNode::Row(x) => x.wire(ids, s),
        }
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        match &mut self.0 {
            ChildNode::Section(x) => x.custom_ids_mut(out),
            ChildNode::Row(x) => x.custom_ids_mut(out),
            ChildNode::Text(_)
            | ChildNode::Gallery(_)
            | ChildNode::File(_)
            | ChildNode::Separator(_) => {}
        }
    }
}

/// A Container (type 17): a box around other components, with an optional
/// accent colour.
///
/// Placement: top level only, never inside another Container (CV2-05):
///
/// ```compile_fail,E0277
/// use noctis_ui::Container;
///
/// let _ = Container::new().push(Container::new());
/// ```
///
/// ```
/// use noctis_ui::{Container, Rgb, TextDisplay};
///
/// let container = Container::new()
///     .accent(Rgb::new(0x58, 0x65, 0xF2))
///     .spoiler(true)
///     .push(TextDisplay::new("a"));
/// assert_eq!(
///     serde_json::to_value(&container).unwrap(),
///     serde_json::json!({
///         "type": 17,
///         "components": [{"type": 10, "content": "a"}],
///         "accent_color": 5793266,
///         "spoiler": true
///     })
/// );
/// ```
#[derive(Debug, Clone, Default)]
pub struct Container {
    children: Vec<ContainerComponent>,
    accent: Option<Rgb>,
    spoiler: bool,
}

impl Container {
    /// An empty Container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `child`.
    #[must_use]
    pub fn push<C: ContainerChild>(mut self, child: C) -> Self {
        self.children.push(child.into_child());
        self
    }

    /// The accent colour of the left border.
    #[must_use]
    pub fn accent(self, accent: Rgb) -> Self {
        Self {
            accent: Some(accent),
            ..self
        }
    }

    /// Whether the content is blurred until clicked.
    #[must_use]
    pub fn spoiler(self, spoiler: bool) -> Self {
        Self { spoiler, ..self }
    }
}

impl Wire for Container {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        let mut map = head(s, 17, ids)?;
        map.serialize_entry("components", &WiredSeq(&self.children, ids))?;
        if let Some(accent) = self.accent {
            map.serialize_entry("accent_color", &accent.value())?;
        }
        flag(&mut map, "spoiler", self.spoiler)?;
        map.end()
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        for child in &mut self.children {
            child.custom_ids_mut(out);
        }
    }
}

#[derive(Debug, Clone)]
enum TopNode {
    Container(Container),
    Child(ContainerComponent),
}

/// Any component the top level of a message may hold, converted with
/// `From`: a Container or anything a Container may hold.
///
/// ```
/// use noctis_ui::{Components, Container, TextDisplay, TopLevelComponent};
///
/// let parts: Vec<TopLevelComponent> = vec![Container::new().into(), TextDisplay::new("a").into()];
/// let body = parts.into_iter().fold(Components::new(), Components::push);
/// assert_eq!(body.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct TopLevelComponent(TopNode);

impl Wire for TopLevelComponent {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            TopNode::Container(x) => x.wire(ids, s),
            TopNode::Child(x) => x.wire(ids, s),
        }
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        match &mut self.0 {
            TopNode::Container(x) => x.custom_ids_mut(out),
            TopNode::Child(x) => x.custom_ids_mut(out),
        }
    }
}

macro_rules! child_from {
    ($($ty:ident => $variant:ident),*) => {$(
        impl From<$ty> for ContainerComponent {
            fn from(x: $ty) -> Self {
                Self(ChildNode::$variant(x))
            }
        }

        impl From<$ty> for TopLevelComponent {
            fn from(x: $ty) -> Self {
                Self(TopNode::Child(ContainerComponent::from(x)))
            }
        }
    )*};
}

child_from!(
    Section => Section,
    TextDisplay => Text,
    MediaGallery => Gallery,
    File => File,
    Separator => Separator,
    ActionRow => Row
);

impl From<ContainerComponent> for TopLevelComponent {
    fn from(x: ContainerComponent) -> Self {
        Self(TopNode::Child(x))
    }
}

impl From<Container> for TopLevelComponent {
    fn from(x: Container) -> Self {
        Self(TopNode::Container(x))
    }
}

serialize_via_wire!(
    Section,
    ActionRow,
    ContainerComponent,
    Container,
    TopLevelComponent
);
