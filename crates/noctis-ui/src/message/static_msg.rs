//! [`StaticMessage`]: a Components V2 message without interactive
//! components, the only message a webhook the app does not own may send
//! (WH-02), and the static builders it is made of.
//!
//! [`StaticComponents`], [`StaticContainer`], [`StaticSection`] and
//! [`StaticActionRow`] mirror [`Components`], [`Container`], [`Section`] and
//! [`ActionRow`], but their push methods are bounded by sealed traits that no
//! button with a `custom_id` and no select implements, so an interactive
//! component fails `cargo check` with `[WH-02]` where it is placed. Each
//! converts into the erased node it mirrors, so serialization, numbering,
//! [`validate`](crate::validate) and [`cost`](crate::cost) are the V2
//! message's own. A tree built at run time (`components!`, raw JSON) goes
//! through [`StaticMessage::try_from`].
//!
//! A Route button is not a static row button:
//!
//! ```compile_fail,E0277
//! use noctis_ui::{RouteButton, StaticSection, TextDisplay};
//!
//! fn render<R>(button: RouteButton<R>) {
//!     let _ = StaticSection::new([TextDisplay::new("a")], button); // [WH-02]
//! }
//! ```
//!
//! nor is an erased [`Button`], which may hold one:
//!
//! ```compile_fail,E0277
//! use noctis_ui::{Button, StaticActionRow};
//!
//! fn render(button: Button) {
//!     let _ = StaticActionRow::buttons([button]); // [WH-02]
//! }
//! ```
//!
//! An erased [`Container`] may hold selects, so a static Container takes
//! only static children:
//!
//! ```compile_fail,E0277
//! use noctis_ui::{Container, StaticContainer};
//!
//! let _ = StaticContainer::new().push(Container::new()); // [WH-02]
//! ```
//!
//! A static message has no legacy field (WH-03):
//!
//! ```compile_fail,E0599
//! use noctis_ui::{StaticComponents, StaticMessage};
//!
//! let _ = StaticMessage::new(StaticComponents::new()).content("x"); // [WH-03]
//! ```

use crate::arity::{Arity, RowButtons, SectionTexts};
use crate::component::{
    AccessoryNode, ActionRow, Button, Container, ContainerComponent, File, LinkButton,
    MediaGallery, RowNode, Section, Separator, TextDisplay, Thumbnail, TopLevelComponent,
};
use crate::error::checked;
use crate::{AttachmentSet, Components, Mentions, Rgb, UiError, V2Message};

/// The private supertraits: only Noctis types implement the static
/// placement traits, and each carries its conversion into the erased node.
mod sealed {
    use crate::component::{AccessoryNode, Button, ContainerComponent, TopLevelComponent};

    pub trait Sealed {}

    pub trait IntoStaticTop: Sealed {
        fn into_static_top(self) -> TopLevelComponent;
    }

    pub trait IntoStaticChild: Sealed {
        fn into_static_child(self) -> ContainerComponent;
    }

    pub trait IntoStaticAccessory: Sealed {
        fn into_static_accessory(self) -> AccessoryNode;
    }

    pub trait IntoStaticRowButton: Sealed {
        fn into_static_row_button(self) -> Button;
    }

    pub trait IntoStatic: Sealed {
        fn into_static(self) -> super::StaticMessage;
    }
}

/// A component that may sit at the top level of a [`StaticMessage`]:
/// [`StaticContainer`], [`StaticSection`], [`StaticActionRow`],
/// [`TextDisplay`], [`MediaGallery`], [`File`] or [`Separator`].
#[diagnostic::on_unimplemented(
    message = "[WH-02] `{Self}` cannot be a top-level component of a StaticMessage: it may hold interactive components",
    label = "not a static top-level component",
    note = "a StaticComponents takes a StaticContainer, StaticSection, StaticActionRow, TextDisplay, MediaGallery, File or Separator",
    note = "send interactive components through an owned webhook, or convert a run-time tree with StaticMessage::try_from(msg)"
)]
pub trait StaticTopLevel: sealed::IntoStaticTop {}

/// A component that may sit in a [`StaticContainer`]: [`StaticSection`],
/// [`StaticActionRow`], [`TextDisplay`], [`MediaGallery`], [`File`] or
/// [`Separator`]. Never another Container.
#[diagnostic::on_unimplemented(
    message = "[WH-02] `{Self}` cannot be a StaticContainer child: it may hold interactive components or is a Container",
    label = "not a static Container child",
    note = "a StaticContainer takes a StaticSection, StaticActionRow, TextDisplay, MediaGallery, File or Separator",
    note = "send interactive components through an owned webhook, or convert a run-time tree with StaticMessage::try_from(msg)"
)]
pub trait StaticContainerChild: sealed::IntoStaticChild {}

/// A [`StaticSection`] accessory: a [`Thumbnail`] or a [`LinkButton`].
#[diagnostic::on_unimplemented(
    message = "[WH-02] `{Self}` is not a static Section accessory: a webhook the app does not own sends no interactive button",
    label = "not a Thumbnail or a LinkButton",
    note = "a StaticSection takes a Thumbnail or a LinkButton as its accessory",
    note = "send interactive components through an owned webhook, or convert a run-time tree with StaticMessage::try_from(msg)"
)]
pub trait StaticAccessory: sealed::IntoStaticAccessory {}

/// A button a [`StaticActionRow`] may hold: a [`LinkButton`].
#[diagnostic::on_unimplemented(
    message = "[WH-02] `{Self}` is not a static row button: a webhook the app does not own sends only link buttons",
    label = "not a LinkButton",
    note = "StaticActionRow::buttons takes LinkButton values; selects and buttons with a custom_id are interactive",
    note = "send interactive components through an owned webhook, or convert a run-time tree with StaticMessage::try_from(msg)"
)]
pub trait StaticRowButton: sealed::IntoStaticRowButton {}

/// What a webhook the app does not own sends: a [`StaticMessage`] or a
/// [`StaticComponents`].
#[diagnostic::on_unimplemented(
    message = "[WH-02] `{Self}` may hold interactive components: a webhook the app does not own sends only a `StaticMessage`",
    label = "not a StaticMessage",
    note = "build it with StaticComponents/StaticContainer/StaticSection/StaticActionRow, or convert at run time with StaticMessage::try_from(msg)"
)]
pub trait IntoStaticMessage: sealed::IntoStatic {}

/// The top-level components of a [`StaticMessage`], in order; it converts
/// into [`Components`].
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct StaticComponents(Components);

impl StaticComponents {
    /// An empty body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `component`.
    pub fn push<T: StaticTopLevel>(mut self, component: T) -> Self {
        self.0.items.push(component.into_static_top());
        self
    }

    /// The number of top-level components.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there is no component.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<StaticComponents> for Components {
    fn from(value: StaticComponents) -> Self {
        value.0
    }
}

/// A [`Container`] of static children; it converts into [`Container`].
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct StaticContainer(Container);

impl StaticContainer {
    /// An empty Container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `child`.
    pub fn push<C: StaticContainerChild>(mut self, child: C) -> Self {
        self.0.children.push(child.into_static_child());
        self
    }

    /// The accent colour of the left border.
    pub fn accent(self, accent: Rgb) -> Self {
        Self(self.0.accent(accent))
    }

    /// Whether the content is blurred until clicked.
    pub fn spoiler(self, spoiler: bool) -> Self {
        Self(self.0.spoiler(spoiler))
    }
}

impl From<StaticContainer> for Container {
    fn from(value: StaticContainer) -> Self {
        value.0
    }
}

/// A [`Section`] whose accessory is a [`Thumbnail`] or a [`LinkButton`]; it
/// converts into [`Section`].
#[derive(Debug, Clone)]
#[must_use]
pub struct StaticSection(Section);

impl StaticSection {
    /// A Section of 1 to 3 `texts` with `accessory`; the count is checked at
    /// compile time (CV2-07).
    pub fn new<A: StaticAccessory, const N: usize>(texts: [TextDisplay; N], accessory: A) -> Self
    where
        Arity<N>: SectionTexts,
    {
        Self(Section {
            texts: texts.into(),
            accessory: accessory.into_static_accessory(),
        })
    }
}

impl From<StaticSection> for Section {
    fn from(value: StaticSection) -> Self {
        value.0
    }
}

/// An [`ActionRow`] of 1 to 5 [`LinkButton`]s; it converts into
/// [`ActionRow`].
#[derive(Debug, Clone)]
#[must_use]
pub struct StaticActionRow(ActionRow);

impl StaticActionRow {
    /// A row of 1 to 5 `buttons`, in order; the count is checked at compile
    /// time (CV2-06).
    pub fn buttons<B: StaticRowButton, const N: usize>(buttons: [B; N]) -> Self
    where
        Arity<N>: RowButtons,
    {
        Self(ActionRow(RowNode::Buttons(
            buttons
                .into_iter()
                .map(sealed::IntoStaticRowButton::into_static_row_button)
                .collect(),
        )))
    }
}

impl From<StaticActionRow> for ActionRow {
    fn from(value: StaticActionRow) -> Self {
        value.0
    }
}

/// A Components V2 message without interactive components: no button with
/// a `custom_id` (Premium buttons included) and no select (WH-02).
///
/// It is the only message a [webhook the app does not
/// own](https://discord.com/developers/docs/resources/webhook#execute-webhook)
/// sends. Build it from the static builders, which reject an interactive
/// component at `cargo check`, or convert a [`V2Message`] built at run time
/// with [`StaticMessage::try_from`]. It converts into [`V2Message`], so an
/// owned webhook or a reply sends it too.
///
/// ```
/// use noctis_ui::{
///     ActionRow, Components, Container, CreateFlags, LinkButton, Section, StaticActionRow,
///     StaticComponents, StaticContainer, StaticMessage, StaticSection, TextDisplay, V2Message,
/// };
///
/// let link = || LinkButton::new("https://a.b").label("Docs");
/// let msg = StaticMessage::new(StaticComponents::new().push(
///     StaticContainer::new()
///         .push(TextDisplay::new("x"))
///         .push(StaticSection::new([TextDisplay::new("y")], link()))
///         .push(StaticActionRow::buttons([link()])),
/// ));
/// let same = V2Message::new(Components::new().push(
///     Container::new()
///         .push(TextDisplay::new("x"))
///         .push(Section::new([TextDisplay::new("y")], link()))
///         .push(ActionRow::buttons([link()])),
/// ));
/// assert_eq!(
///     serde_json::to_value(msg.as_v2().create_body(CreateFlags::empty())).unwrap(),
///     serde_json::to_value(same.create_body(CreateFlags::empty())).unwrap(),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct StaticMessage(V2Message);

impl StaticMessage {
    /// A message of `components`, with no attachment and no ping.
    pub fn new(components: StaticComponents) -> Self {
        Self(V2Message::new(components.0))
    }

    /// Replaces the attachment set, as [`V2Message::attachments`].
    #[must_use]
    pub fn attachments(self, attachments: AttachmentSet) -> Self {
        Self(self.0.attachments(attachments))
    }

    /// Replaces who the message may ping, as [`V2Message::mentions`].
    #[must_use]
    pub fn mentions(self, mentions: Mentions) -> Self {
        Self(self.0.mentions(mentions))
    }

    /// The message as a [`V2Message`], for its bodies, attachments and
    /// validation.
    #[must_use]
    pub fn as_v2(&self) -> &V2Message {
        &self.0
    }
}

impl From<StaticComponents> for StaticMessage {
    fn from(components: StaticComponents) -> Self {
        Self::new(components)
    }
}

impl From<StaticMessage> for V2Message {
    fn from(value: StaticMessage) -> Self {
        value.0
    }
}

impl From<StaticComponents> for V2Message {
    /// A static tree is a valid V2 message, so an owned webhook (or any
    /// reply) sends it as is.
    fn from(components: StaticComponents) -> Self {
        Self::new(components.into())
    }
}

impl TryFrom<V2Message> for StaticMessage {
    type Error = UiError;

    /// Checks that `msg` holds no interactive component (WH-02): no button
    /// whose style is not Link, no select, at any depth, and no such node in
    /// the JSON of a [`RawComponent`](crate::RawComponent).
    ///
    /// # Errors
    ///
    /// [`UiError::Limits`] with every WH-02 violation, at the paths of
    /// [`validate`](crate::validate) (`row[0].button[1]`,
    /// `container[0].section[2].accessory`, `opaque[1].components[0]`);
    /// never a partial message.
    ///
    /// ```
    /// use noctis_rules::RuleId;
    /// use noctis_ui::{ActionRow, Components, PremiumButton, StaticMessage, UiError, V2Message};
    /// use twilight_model::id::Id;
    ///
    /// let premium = V2Message::new(Components::new().push(ActionRow::buttons([PremiumButton::new(Id::new(1))])));
    /// let Err(UiError::Limits { violations }) = StaticMessage::try_from(premium) else {
    ///     panic!("a premium button is interactive");
    /// };
    /// assert_eq!((violations[0].rule, violations[0].path.as_str()), (RuleId::WH_02, "row[0].button[0]"));
    /// ```
    fn try_from(msg: V2Message) -> Result<Self, UiError> {
        let violations = crate::limits::static_violations(&msg.components);
        checked(Self(msg), violations)
    }
}

macro_rules! sealed {
    ($($ty:ty),* $(,)?) => {$( impl sealed::Sealed for $ty {} )*};
}

sealed!(
    StaticContainer,
    StaticSection,
    StaticActionRow,
    TextDisplay,
    MediaGallery,
    File,
    Separator,
    Thumbnail,
    LinkButton,
    StaticMessage,
    StaticComponents,
);

macro_rules! child {
    ($($ty:ty),* $(,)?) => {$(
        impl sealed::IntoStaticChild for $ty {
            fn into_static_child(self) -> ContainerComponent {
                ContainerComponent::from(self)
            }
        }
        impl StaticContainerChild for $ty {}

        impl sealed::IntoStaticTop for $ty {
            fn into_static_top(self) -> TopLevelComponent {
                TopLevelComponent::from(self)
            }
        }
        impl StaticTopLevel for $ty {}
    )*};
}

child!(TextDisplay, MediaGallery, File, Separator);

impl sealed::IntoStaticChild for StaticSection {
    fn into_static_child(self) -> ContainerComponent {
        ContainerComponent::from(self.0)
    }
}
impl StaticContainerChild for StaticSection {}
impl sealed::IntoStaticTop for StaticSection {
    fn into_static_top(self) -> TopLevelComponent {
        TopLevelComponent::from(self.0)
    }
}
impl StaticTopLevel for StaticSection {}

impl sealed::IntoStaticChild for StaticActionRow {
    fn into_static_child(self) -> ContainerComponent {
        ContainerComponent::from(self.0)
    }
}
impl StaticContainerChild for StaticActionRow {}
impl sealed::IntoStaticTop for StaticActionRow {
    fn into_static_top(self) -> TopLevelComponent {
        TopLevelComponent::from(self.0)
    }
}
impl StaticTopLevel for StaticActionRow {}

impl sealed::IntoStaticTop for StaticContainer {
    fn into_static_top(self) -> TopLevelComponent {
        TopLevelComponent::from(self.0)
    }
}
impl StaticTopLevel for StaticContainer {}

impl sealed::IntoStaticAccessory for Thumbnail {
    fn into_static_accessory(self) -> AccessoryNode {
        AccessoryNode::Thumbnail(self)
    }
}
impl StaticAccessory for Thumbnail {}

impl sealed::IntoStaticAccessory for LinkButton {
    fn into_static_accessory(self) -> AccessoryNode {
        AccessoryNode::Button(Button::from(self))
    }
}
impl StaticAccessory for LinkButton {}

impl sealed::IntoStaticRowButton for LinkButton {
    fn into_static_row_button(self) -> Button {
        Button::from(self)
    }
}
impl StaticRowButton for LinkButton {}

impl sealed::IntoStatic for StaticMessage {
    fn into_static(self) -> StaticMessage {
        self
    }
}
impl IntoStaticMessage for StaticMessage {}

impl sealed::IntoStatic for StaticComponents {
    fn into_static(self) -> StaticMessage {
        StaticMessage::new(self)
    }
}
impl IntoStaticMessage for StaticComponents {}

/// Converts `msg` into its [`StaticMessage`]; for core's foreign webhook
/// sends. Not public API.
#[doc(hidden)]
pub fn __into_static<M: IntoStaticMessage>(msg: M) -> StaticMessage {
    sealed::IntoStatic::into_static(msg)
}

#[cfg(test)]
mod tests {
    use core::fmt;

    use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape, SelectRoute};
    use noctis_rules::{RuleId, Violation};
    use serde_json::json;

    use super::*;
    use crate::route::CustomId;
    use crate::{
        CreateFlags, Handled, PremiumButton, RawComponent, RouteButton, SelectOption, StringSelect,
    };

    struct Pick;

    impl Payload for Pick {
        const MAX_ENCODED_LEN: Option<usize> = Some(0);
        fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result {
            Ok(())
        }
    }

    impl Route for Pick {
        const ID: RouteId = RouteId::new("pick");
        const SHAPE: RouteShape = RouteShape::INLINE.select();
        const SCHEMA: u8 = 0;
        type Payload = Self;
    }

    impl SelectRoute for Pick {}
    impl noctis_codec::DeclaredRuntime for Pick {}
    impl Handled for Pick {}

    fn select() -> StringSelect {
        StringSelect::from_id(
            CustomId::new::<Pick>(Pick),
            vec![SelectOption::new("a", "a")],
        )
    }

    fn route_button() -> RouteButton<Pick> {
        RouteButton::from_id(CustomId::new::<Pick>(Pick)).label("Go")
    }

    fn wh02(msg: V2Message) -> Vec<(RuleId, String)> {
        match StaticMessage::try_from(msg) {
            Err(UiError::Limits { violations }) => violations
                .into_iter()
                .map(|v: Violation| (v.rule, v.path))
                .collect(),
            Ok(_) => Vec::new(),
            Err(other) => panic!("unexpected {other:?}"),
        }
    }

    fn link() -> LinkButton {
        LinkButton::new("https://a.b").label("Docs")
    }

    #[test]
    fn wh_02_static_build_matches_erased_tree() {
        let msg = StaticMessage::new(
            StaticComponents::new()
                .push(
                    StaticContainer::new()
                        .accent(Rgb::new(1, 2, 3))
                        .spoiler(true)
                        .push(TextDisplay::new("x"))
                        .push(StaticSection::new([TextDisplay::new("y")], link()))
                        .push(StaticActionRow::buttons([link()])),
                )
                .push(Separator::new()),
        );
        let same = V2Message::new(
            Components::new()
                .push(
                    Container::new()
                        .accent(Rgb::new(1, 2, 3))
                        .spoiler(true)
                        .push(TextDisplay::new("x"))
                        .push(Section::new([TextDisplay::new("y")], link()))
                        .push(ActionRow::buttons([link()])),
                )
                .push(Separator::new()),
        );
        assert_eq!(
            serde_json::to_value(msg.as_v2().create_body(CreateFlags::empty())).unwrap(),
            serde_json::to_value(same.create_body(CreateFlags::empty())).unwrap()
        );
        let v2: V2Message = msg.into();
        assert_eq!(v2.components().len(), 2);
    }

    #[test]
    fn wh_02_try_from_reports_every_interactive_node() {
        let route = route_button();
        let msg = V2Message::new(
            Components::new().push(
                Container::new()
                    .push(Section::new([TextDisplay::new("a")], route))
                    .push(ActionRow::select(select())),
            ),
        );
        assert_eq!(
            wh02(msg),
            [
                (
                    RuleId::WH_02,
                    "container[0].section[0].accessory".to_owned()
                ),
                (RuleId::WH_02, "container[0].row[1].select[0]".to_owned()),
            ]
        );
        let premium =
            V2Message::new(
                Components::new().push(ActionRow::buttons([PremiumButton::new(
                    twilight_model::id::Id::new(1),
                )])),
            );
        assert_eq!(wh02(premium).len(), 1);
        let links = V2Message::new(
            Components::new()
                .push(ActionRow::buttons([link()]))
                .push(Section::new([TextDisplay::new("b")], link())),
        );
        assert!(StaticMessage::try_from(links).is_ok());
    }

    #[test]
    fn wh_02_try_from_walks_raw_json() {
        let raw = |v| RawComponent::new(v).unwrap();
        let msg = V2Message::new(
            Components::new()
                .push(raw(json!({"type": 1, "components": [{"type": 2, "style": 1, "custom_id": "x"}]})))
                .push(raw(json!({"type": 1, "components": [{"type": 2, "style": 5, "url": "https://a.b"}]})))
                .push(Container::new().push(raw(json!({"type": 17, "components": [{"type": 3, "custom_id": "s"}]}))))
                .push(raw(json!({"type": 10, "content": "a"}))),
        );
        assert_eq!(
            wh02(msg),
            [
                (RuleId::WH_02, "opaque[0].components[0]".to_owned()),
                (
                    RuleId::WH_02,
                    "container[2].opaque[0].components[0]".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn static_components_len() {
        let c = StaticComponents::new();
        assert!(c.is_empty());
        let c = c.push(TextDisplay::new("a"));
        assert_eq!((c.len(), c.is_empty()), (1, false));
        let msg: StaticMessage = c.into();
        assert_eq!(msg.as_v2().components().len(), 1);
    }
}
