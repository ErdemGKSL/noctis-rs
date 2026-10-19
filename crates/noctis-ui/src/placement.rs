//! Where a component may appear, as sealed marker traits (CV2-05, CV2-06,
//! CV2-07; catalog §13).
//!
//! Each trait's bound sits on the method that places a component, so a
//! misplaced component fails `cargo check` with the rule id in the message.

use crate::component::{
    AccessoryNode, ActionRow, Button, ChannelSelect, Container, ContainerComponent, File,
    LinkButton, MediaGallery, MentionableSelect, PremiumButton, RoleSelect, RouteButton, Section,
    SelectNode, Separator, StringSelect, TextDisplay, Thumbnail, TopLevelComponent, UserSelect,
};

/// The private supertraits: only Noctis types implement the placement
/// traits, and each carries its conversion into the node it becomes.
pub(crate) mod sealed {
    use crate::component::{
        AccessoryNode, Button, ContainerComponent, SelectNode, TopLevelComponent,
    };

    /// Implemented only by Noctis component types and the legal
    /// [`Arity`](crate::Arity) values.
    pub trait Sealed {}

    /// Conversion behind [`TopLevel`](super::TopLevel).
    pub trait IntoTop: Sealed {
        fn into_top(self) -> TopLevelComponent;
    }

    /// Conversion behind [`ContainerChild`](super::ContainerChild).
    pub trait IntoChild: Sealed {
        fn into_child(self) -> ContainerComponent;
    }

    /// Conversion behind [`SectionAccessory`](super::SectionAccessory).
    pub trait IntoAccessory: Sealed {
        fn into_accessory(self) -> AccessoryNode;
    }

    /// Conversion behind [`RowItem`](super::RowItem).
    pub trait IntoRowButton: Sealed {
        fn into_row_button(self) -> Button;
    }

    /// Conversion behind [`RowSelect`](super::RowSelect).
    pub trait IntoSelect: Sealed {
        fn into_select(self) -> SelectNode;
    }

    /// The checks behind [`Validate`](crate::Validate), implemented in
    /// `limits` for the two message types. It lives here so every sealed
    /// supertrait of the public API has one path.
    pub trait Check: Sealed {
        fn validate_with(
            &mut self,
            limits: &noctis_rules::Limits,
        ) -> Result<crate::CostReport, crate::UiError>;

        fn cost_with(&self, limits: &noctis_rules::Limits) -> crate::CostReport;
    }
}

/// A component that may sit at the top level of a message: Container,
/// Section, Text Display, Media Gallery, File, Separator or Action Row.
#[diagnostic::on_unimplemented(
    message = "[CV2-05] `{Self}` cannot be a top-level message component",
    label = "not allowed at the top level of a message",
    note = "the top level takes a Container, Section, TextDisplay, MediaGallery, File, Separator or ActionRow",
    note = "a Thumbnail goes only in a Section accessory; a button or select goes in an ActionRow"
)]
pub trait TopLevel: sealed::IntoTop {}

/// A component that may sit in a Container: Section, Text Display, Media
/// Gallery, File, Separator or Action Row. Never another Container.
#[diagnostic::on_unimplemented(
    message = "[CV2-05] `{Self}` cannot be a Container child",
    label = "not allowed inside a Container",
    note = "a Container takes a Section, TextDisplay, MediaGallery, File, Separator or ActionRow; never another Container",
    note = "a Thumbnail goes only in a Section accessory; a button or select goes in an ActionRow"
)]
pub trait ContainerChild: sealed::IntoChild {}

/// A Section's accessory: a button (any kind) or a Thumbnail.
#[diagnostic::on_unimplemented(
    message = "[CV2-07] `{Self}` is not a Section accessory: a Section needs exactly one accessory, a Button or a Thumbnail",
    label = "not a Section accessory",
    note = "pass a RouteButton, LinkButton, PremiumButton, Button or Thumbnail as the second argument of Section::new",
    note = "a Section without an accessory does not exist: use a TextDisplay instead"
)]
pub trait SectionAccessory: sealed::IntoAccessory {}

/// A button an Action Row may hold.
#[diagnostic::on_unimplemented(
    message = "[CV2-05] `{Self}` cannot be an Action Row button",
    label = "not a button",
    note = "ActionRow::buttons takes RouteButton, LinkButton, PremiumButton or Button values",
    note = "a select goes alone in its row: ActionRow::select(select)"
)]
pub trait RowItem: sealed::IntoRowButton {}

/// A message select an Action Row may hold, alone.
#[diagnostic::on_unimplemented(
    message = "[CV2-06] `{Self}` is not a message select",
    label = "not a select",
    note = "ActionRow::select takes a StringSelect, UserSelect, RoleSelect, MentionableSelect or ChannelSelect",
    note = "buttons go in ActionRow::buttons([..])"
)]
pub trait RowSelect: sealed::IntoSelect {}

macro_rules! sealed {
    ($($ty:ty),* $(,)?) => {$( impl sealed::Sealed for $ty {} )*};
}

macro_rules! top_level {
    ($($ty:ty),* $(,)?) => {$(
        impl sealed::IntoTop for $ty {
            fn into_top(self) -> TopLevelComponent {
                TopLevelComponent::from(self)
            }
        }
        impl TopLevel for $ty {}
    )*};
}

macro_rules! container_child {
    ($($ty:ty),* $(,)?) => {$(
        impl sealed::IntoChild for $ty {
            fn into_child(self) -> ContainerComponent {
                ContainerComponent::from(self)
            }
        }
        impl ContainerChild for $ty {}
    )*};
}

macro_rules! button {
    ($($ty:ty),* $(,)?) => {$(
        impl sealed::IntoAccessory for $ty {
            fn into_accessory(self) -> AccessoryNode {
                AccessoryNode::Button(Button::from(self))
            }
        }
        impl SectionAccessory for $ty {}
        impl sealed::IntoRowButton for $ty {
            fn into_row_button(self) -> Button {
                Button::from(self)
            }
        }
        impl RowItem for $ty {}
    )*};
}

macro_rules! select {
    ($($ty:ident => $variant:ident),* $(,)?) => {$(
        impl sealed::IntoSelect for $ty {
            fn into_select(self) -> SelectNode {
                SelectNode::$variant(self)
            }
        }
        impl RowSelect for $ty {}
    )*};
}

sealed!(
    Container,
    Section,
    TextDisplay,
    MediaGallery,
    File,
    Separator,
    ActionRow,
    TopLevelComponent,
    ContainerComponent,
    LinkButton,
    PremiumButton,
    Button,
    Thumbnail,
    StringSelect,
    UserSelect,
    RoleSelect,
    MentionableSelect,
    ChannelSelect,
);
impl<R> sealed::Sealed for RouteButton<R> {}

top_level!(
    Container,
    Section,
    TextDisplay,
    MediaGallery,
    File,
    Separator,
    ActionRow,
    TopLevelComponent,
);
container_child!(
    Section,
    TextDisplay,
    MediaGallery,
    File,
    Separator,
    ActionRow,
    ContainerComponent,
);
button!(LinkButton, PremiumButton, Button);
select!(
    StringSelect => String,
    UserSelect => User,
    RoleSelect => Role,
    MentionableSelect => Mentionable,
    ChannelSelect => Channel,
);

impl<R> sealed::IntoAccessory for RouteButton<R> {
    fn into_accessory(self) -> AccessoryNode {
        AccessoryNode::Button(Button::from(self))
    }
}
impl<R> SectionAccessory for RouteButton<R> {}
impl<R> sealed::IntoRowButton for RouteButton<R> {
    fn into_row_button(self) -> Button {
        Button::from(self)
    }
}
impl<R> RowItem for RouteButton<R> {}

impl sealed::IntoAccessory for Thumbnail {
    fn into_accessory(self) -> AccessoryNode {
        AccessoryNode::Thumbnail(self)
    }
}
impl SectionAccessory for Thumbnail {}
