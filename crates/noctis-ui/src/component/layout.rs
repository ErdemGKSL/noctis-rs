//! Layout components: Container (type 17), Section (type 9) and Action Row
//! (type 1), plus the opaque [`ContainerComponent`] and
//! [`TopLevelComponent`].

use serde::Serializer;
use serde::ser::SerializeMap;

use noctis_rules::Limits;
use noctis_rules::component::{
    validate_accent_color, validate_row_buttons, validate_section_texts,
};

use super::{
    Button, File, MediaGallery, RawComponent, SelectNode, Separator, TextDisplay, Thumbnail,
};
use crate::Text;
use crate::arity::{Arity, RowButtons, SectionTexts};
use crate::error::{UiError, checked};
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
///
/// A `0xRRGGBB` value known only at runtime goes through `TryFrom<u32>`,
/// which rejects anything above `0xFFFFFF` (CV2-13); a literal goes through
/// `rgb!`, which checks it at `cargo check`:
///
/// ```
/// use noctis_rules::RuleId;
/// use noctis_ui::{Rgb, UiError};
///
/// assert_eq!(Rgb::try_from(0xFF_FFFF)?, Rgb::new(255, 255, 255));
/// let Err(UiError::Limits { violations }) = Rgb::try_from(0x100_0000) else {
///     panic!("out of range");
/// };
/// assert_eq!(violations[0].rule, RuleId::CV2_13);
/// # Ok::<(), UiError>(())
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

    /// The colour `value`, already checked by `rgb!`; not public API.
    #[doc(hidden)]
    pub const fn __from_checked(value: u32) -> Self {
        Self(value)
    }
}

impl TryFrom<u32> for Rgb {
    type Error = UiError;

    /// Checks `value` against CV2-13 (`0x000000..=0xFFFFFF`) with
    /// `noctis_rules::component::validate_accent_color`, the function `rgb!`
    /// calls.
    fn try_from(value: u32) -> Result<Self, UiError> {
        checked(
            Self(value),
            validate_accent_color(value, &Limits::default()),
        )
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

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        if let Self::Button(b) = self {
            b.texts_mut(out);
        }
    }
}

/// A Section (type 9): 1 to 3 Text Displays beside exactly one accessory, a
/// button or a [`Thumbnail`] (CV2-07).
///
/// Placement: top level or a Container. There is no constructor without an
/// accessory. [`Section::new`] takes an array of 1 to 3 texts
/// (`Arity<N>: `[`SectionTexts`](crate::SectionTexts)); any other length
/// fails `cargo check` with `[CV2-07]`:
///
/// ```compile_fail,E0277
/// use noctis_ui::{LinkButton, Section, TextDisplay};
///
/// let t = || TextDisplay::new("a");
/// let _ = Section::new([t(), t(), t(), t()], LinkButton::new("https://x.y")); // [CV2-07]
/// ```
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
    pub(crate) texts: Vec<TextDisplay>,
    pub(crate) accessory: AccessoryNode,
}

impl Section {
    /// A Section of 1 to 3 `texts` with `accessory`; the count is checked at
    /// compile time (CV2-07).
    pub fn new<A: SectionAccessory, const N: usize>(texts: [TextDisplay; N], accessory: A) -> Self
    where
        Arity<N>: SectionTexts,
    {
        Self {
            texts: texts.into(),
            accessory: accessory.into_accessory(),
        }
    }

    /// A Section of `texts` with `accessory`, for a text count known only at
    /// runtime.
    ///
    /// # Errors
    ///
    /// [`UiError::Limits`] with CV2-07 (empty path) when the count is not 1
    /// to `limits.section_texts`.
    ///
    /// ```
    /// use noctis_rules::{Limits, RuleId};
    /// use noctis_ui::{LinkButton, Section, TextDisplay, UiError};
    ///
    /// let lines = ["a", "b"].map(TextDisplay::new);
    /// let section = Section::try_new(lines, LinkButton::new("https://x.y"), &Limits::default())?;
    /// # let _ = section;
    /// let none = Section::try_new([], LinkButton::new("https://x.y"), &Limits::default());
    /// assert!(matches!(none, Err(UiError::Limits { violations }) if violations[0].rule == RuleId::CV2_07));
    /// # Ok::<(), UiError>(())
    /// ```
    pub fn try_new<A: SectionAccessory>(
        texts: impl IntoIterator<Item = TextDisplay>,
        accessory: A,
        limits: &Limits,
    ) -> Result<Self, UiError> {
        let texts: Vec<TextDisplay> = texts.into_iter().collect();
        let violations = validate_section_texts(texts.len(), limits);
        checked(
            Self {
                texts,
                accessory: accessory.into_accessory(),
            },
            violations,
        )
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

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        self.accessory.texts_mut(out);
    }
}

#[derive(Debug, Clone)]
pub(crate) enum RowNode {
    Buttons(Vec<Button>),
    Select(SelectNode),
}

/// An Action Row (type 1): 1 to 5 buttons, or exactly one select (CV2-06).
///
/// Placement: top level or a Container. [`ActionRow::buttons`] takes an array
/// of 1 to 5 buttons (`Arity<N>: `[`RowButtons`](crate::RowButtons)); any
/// other length fails `cargo check` with `[CV2-06]`:
///
/// ```compile_fail,E0277
/// use noctis_ui::{ActionRow, LinkButton};
///
/// let _ = ActionRow::buttons::<LinkButton, 0>([]); // [CV2-06]
/// ```
///
/// ```
/// use noctis_ui::{ActionRow, LinkButton};
///
/// let row = ActionRow::buttons([LinkButton::new("https://a.b"), LinkButton::new("https://c.d")]);
/// assert_eq!(serde_json::to_value(&row).unwrap()["components"].as_array().unwrap().len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct ActionRow(pub(crate) RowNode);

impl ActionRow {
    /// A row of 1 to 5 `buttons`, in order; the count is checked at compile
    /// time (CV2-06). Mix kinds by converting each to [`Button`].
    pub fn buttons<B: RowItem, const N: usize>(buttons: [B; N]) -> Self
    where
        Arity<N>: RowButtons,
    {
        Self(RowNode::Buttons(
            buttons
                .into_iter()
                .map(IntoRowButton::into_row_button)
                .collect(),
        ))
    }

    /// A row of `buttons`, for a count known only at runtime.
    ///
    /// # Errors
    ///
    /// [`UiError::Limits`] with CV2-06 (empty path) when the count is not 1
    /// to `limits.row_buttons`.
    ///
    /// ```
    /// use noctis_rules::{Limits, RuleId};
    /// use noctis_ui::{ActionRow, LinkButton, UiError};
    ///
    /// let links = ["https://a.b", "https://c.d"].map(LinkButton::new);
    /// let row = ActionRow::try_buttons(links, &Limits::default())?;
    /// # let _ = row;
    /// let six = (0..6).map(|_| LinkButton::new("https://a.b"));
    /// assert!(matches!(
    ///     ActionRow::try_buttons(six, &Limits::default()),
    ///     Err(UiError::Limits { violations }) if violations[0].rule == RuleId::CV2_06
    /// ));
    /// # Ok::<(), UiError>(())
    /// ```
    pub fn try_buttons<B: RowItem>(
        buttons: impl IntoIterator<Item = B>,
        limits: &Limits,
    ) -> Result<Self, UiError> {
        let buttons: Vec<Button> = buttons
            .into_iter()
            .map(IntoRowButton::into_row_button)
            .collect();
        let violations = validate_row_buttons(buttons.len(), limits);
        checked(Self(RowNode::Buttons(buttons)), violations)
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

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        match &mut self.0 {
            RowNode::Buttons(buttons) => {
                for b in buttons {
                    b.texts_mut(out);
                }
            }
            RowNode::Select(select) => select.texts_mut(out),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ChildNode {
    Section(Section),
    Text(TextDisplay),
    Gallery(MediaGallery),
    File(File),
    Separator(Separator),
    Row(ActionRow),
    Raw(RawComponent),
}

/// Any component a Container may hold, converted with `From`: Section, Text
/// Display, Media Gallery, File, Separator, Action Row or
/// [`RawComponent`].
///
/// ```
/// use noctis_ui::{Container, ContainerComponent, Separator, TextDisplay};
///
/// let children: Vec<ContainerComponent> = vec![TextDisplay::new("a").into(), Separator::new().into()];
/// let container = children.into_iter().fold(Container::new(), Container::push);
/// assert_eq!(serde_json::to_value(&container).unwrap()["components"][1]["type"], 14);
/// ```
#[derive(Debug, Clone)]
pub struct ContainerComponent(pub(crate) ChildNode);

impl Wire for ContainerComponent {
    fn wire<S: Serializer>(&self, ids: Option<&Ids>, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            ChildNode::Section(x) => x.wire(ids, s),
            ChildNode::Text(x) => x.wire(ids, s),
            ChildNode::Gallery(x) => x.wire(ids, s),
            ChildNode::File(x) => x.wire(ids, s),
            ChildNode::Separator(x) => x.wire(ids, s),
            ChildNode::Row(x) => x.wire(ids, s),
            ChildNode::Raw(x) => x.wire(ids, s),
        }
    }

    fn custom_ids_mut<'a>(&'a mut self, out: &mut Vec<&'a mut CustomId>) {
        match &mut self.0 {
            ChildNode::Section(x) => x.custom_ids_mut(out),
            ChildNode::Row(x) => x.custom_ids_mut(out),
            ChildNode::Text(_)
            | ChildNode::Gallery(_)
            | ChildNode::File(_)
            | ChildNode::Separator(_)
            | ChildNode::Raw(_) => {}
        }
    }

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        match &mut self.0 {
            ChildNode::Section(x) => x.texts_mut(out),
            ChildNode::Row(x) => x.texts_mut(out),
            ChildNode::Text(_)
            | ChildNode::Gallery(_)
            | ChildNode::File(_)
            | ChildNode::Separator(_)
            | ChildNode::Raw(_) => {}
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
    pub(crate) children: Vec<ContainerComponent>,
    pub(crate) accent: Option<Rgb>,
    pub(crate) spoiler: bool,
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

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        for child in &mut self.children {
            child.texts_mut(out);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TopNode {
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
pub struct TopLevelComponent(pub(crate) TopNode);

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

    fn texts_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Text>) {
        match &mut self.0 {
            TopNode::Container(x) => x.texts_mut(out),
            TopNode::Child(x) => x.texts_mut(out),
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
    ActionRow => Row,
    RawComponent => Raw
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
