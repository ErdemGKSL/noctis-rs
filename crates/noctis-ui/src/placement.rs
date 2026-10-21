//! Where a component may appear, as sealed marker traits (CV2-05, CV2-06,
//! CV2-07, MOD-03; catalog §13).
//!
//! Each trait's bound sits on the method that places a component, so a
//! misplaced component fails `cargo check` with the rule id in the message.

use crate::component::{
    AccessoryNode, ActionRow, Button, ChannelSelect, Container, ContainerComponent, File,
    LinkButton, MediaGallery, MentionableSelect, PremiumButton, RoleSelect, RouteButton, Section,
    SelectNode, Separator, StringSelect, StringSelectBuilder, TextDisplay, Thumbnail,
    TopLevelComponent, UserSelect,
};
use crate::modal::{
    Checkbox, CheckboxGroup, FileUpload, ModalChannelSelect, ModalComponent, ModalInputNode,
    ModalLabel, ModalMentionableSelect, ModalRoleSelect, ModalSelectNode, ModalStringSelect,
    ModalUserSelect, RadioGroup, TextInput,
};

/// The private supertraits: only Noctis types implement the placement
/// traits, and each carries its conversion into the node it becomes.
pub(crate) mod sealed {
    use crate::component::{
        AccessoryNode, Button, ContainerComponent, SelectNode, TopLevelComponent,
    };
    use crate::modal::{ModalComponent, ModalInputNode};

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

    /// Conversion behind [`RowOne`](super::RowOne).
    pub trait IntoRowOne: Sealed {
        fn into_row(self) -> crate::component::ActionRow;
    }

    /// Conversion behind [`ModalTopLevel`](super::ModalTopLevel).
    pub trait IntoModalTop: Sealed {
        fn into_modal_top(self) -> ModalComponent;
    }

    /// Conversion behind [`ModalInput`](super::ModalInput).
    pub trait IntoModalInput: Sealed {
        fn into_modal_input(self) -> ModalInputNode;
    }

    /// The checks behind [`Validate`](crate::Validate), implemented in
    /// `limits` for the two message types and `Modal`. It lives here so every sealed
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

/// A component that fills a one-item Action Row: any button or any message
/// select; what a one-item `row [ .. ]` of `components!` takes.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[CV2-06] `{Self}` cannot go in an Action Row",
    label = "not a button or a select",
    note = "an Action Row holds 1 to 5 buttons or one select"
)]
pub trait RowOne: sealed::IntoRowOne {}

/// A button in a `row [ .. ]` of 2 to 5 items in `components!`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[CV2-06] a select fills its Action Row alone: `{Self}` is not a button",
    label = "not a button",
    note = "a row of 2 to 5 items holds only buttons; put the select in a `row [ .. ]` of its own"
)]
pub trait RowButtonItem: sealed::IntoRowButton {}

/// Never implemented: a [`StringSelectBuilder`] of `R` still needs its
/// options before it is placed (CV2-09).
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[CV2-09] this String Select has no options: call `.options(..)` or declare `defaults(options = …)`",
    label = "a String Select of `{Self}` without options"
)]
pub trait OptionsGiven {}

/// A component that may sit at the top level of a modal: a
/// [`ModalLabel`] or a [`TextDisplay`].
///
/// ```
/// use noctis_ui::{ModalComponent, ModalLabel, ModalTopLevel, TextDisplay};
///
/// fn top<C: ModalTopLevel>() {}
/// top::<ModalLabel>();
/// top::<TextDisplay>();
/// top::<ModalComponent>();
/// ```
#[diagnostic::on_unimplemented(
    message = "[MOD-03] `{Self}` cannot be a top-level modal component",
    label = "not allowed at the top level of a modal",
    note = "a modal's top level takes a ModalLabel or a TextDisplay (MOD-02: 1 to 5)",
    note = "buttons, Sections, Containers, galleries, files and Action Rows never go in a modal"
)]
pub trait ModalTopLevel: sealed::IntoModalTop {}

/// The input a [`ModalLabel`] wraps: a [`TextInput`], a modal select, a
/// [`FileUpload`], a [`RadioGroup`], a [`CheckboxGroup`] or a [`Checkbox`].
///
/// ```
/// use noctis_ui::{Checkbox, ModalInput, ModalUserSelect, TextInput};
///
/// fn input<C: ModalInput>() {}
/// input::<TextInput>();
/// input::<ModalUserSelect>();
/// input::<Checkbox>();
/// ```
#[diagnostic::on_unimplemented(
    message = "[MOD-03] `{Self}` cannot be a modal Label input",
    label = "not a modal input",
    note = "a ModalLabel takes a TextInput, ModalStringSelect, ModalUserSelect, ModalRoleSelect, ModalMentionableSelect, ModalChannelSelect, FileUpload, RadioGroup, CheckboxGroup or Checkbox",
    note = "message selects are not modal selects: use ModalStringSelect, ModalUserSelect, ModalRoleSelect, ModalMentionableSelect or ModalChannelSelect"
)]
pub trait ModalInput: sealed::IntoModalInput {}

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
        impl RowButtonItem for $ty {}
        impl sealed::IntoRowOne for $ty {
            fn into_row(self) -> ActionRow {
                ActionRow::buttons([self])
            }
        }
        impl RowOne for $ty {}
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
        impl sealed::IntoRowOne for $ty {
            fn into_row(self) -> ActionRow {
                ActionRow::select(self)
            }
        }
        impl RowOne for $ty {}
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

sealed!(
    ModalLabel,
    ModalComponent,
    TextInput,
    ModalStringSelect,
    ModalUserSelect,
    ModalRoleSelect,
    ModalMentionableSelect,
    ModalChannelSelect,
    FileUpload,
    RadioGroup,
    CheckboxGroup,
    Checkbox,
);

impl sealed::IntoModalTop for ModalLabel {
    fn into_modal_top(self) -> ModalComponent {
        ModalComponent::from(self)
    }
}
impl ModalTopLevel for ModalLabel {}

impl sealed::IntoModalTop for TextDisplay {
    fn into_modal_top(self) -> ModalComponent {
        ModalComponent::from(self)
    }
}
impl ModalTopLevel for TextDisplay {}

impl sealed::IntoModalTop for ModalComponent {
    fn into_modal_top(self) -> ModalComponent {
        self
    }
}
impl ModalTopLevel for ModalComponent {}

macro_rules! modal_input {
    ($($ty:ident => |$x:ident| $node:expr),* $(,)?) => {$(
        impl sealed::IntoModalInput for $ty {
            fn into_modal_input(self) -> ModalInputNode {
                let $x = self;
                $node
            }
        }
        impl ModalInput for $ty {}
    )*};
}

modal_input!(
    TextInput => |x| ModalInputNode::TextInput(x),
    ModalStringSelect => |x| ModalInputNode::Select(ModalSelectNode::String(x)),
    ModalUserSelect => |x| ModalInputNode::Select(ModalSelectNode::User(x)),
    ModalRoleSelect => |x| ModalInputNode::Select(ModalSelectNode::Role(x)),
    ModalMentionableSelect => |x| ModalInputNode::Select(ModalSelectNode::Mentionable(x)),
    ModalChannelSelect => |x| ModalInputNode::Select(ModalSelectNode::Channel(x)),
    FileUpload => |x| ModalInputNode::FileUpload(x),
    RadioGroup => |x| ModalInputNode::RadioGroup(x),
    CheckboxGroup => |x| ModalInputNode::CheckboxGroup(x),
    Checkbox => |x| ModalInputNode::Checkbox(x),
);

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
impl<R> RowButtonItem for RouteButton<R> {}
impl<R> sealed::IntoRowOne for RouteButton<R> {
    fn into_row(self) -> ActionRow {
        ActionRow::buttons([self])
    }
}
impl<R> RowOne for RouteButton<R> {}

impl<R> sealed::Sealed for StringSelectBuilder<R> {}
impl<R: OptionsGiven> sealed::IntoSelect for StringSelectBuilder<R> {
    fn into_select(self) -> SelectNode {
        SelectNode::String(self.into_empty())
    }
}
impl<R: OptionsGiven> RowSelect for StringSelectBuilder<R> {}
impl<R: OptionsGiven> sealed::IntoRowOne for StringSelectBuilder<R> {
    fn into_row(self) -> ActionRow {
        ActionRow::select(self.into_empty())
    }
}
impl<R: OptionsGiven> RowOne for StringSelectBuilder<R> {}

impl sealed::IntoAccessory for Thumbnail {
    fn into_accessory(self) -> AccessoryNode {
        AccessoryNode::Thumbnail(self)
    }
}
impl SectionAccessory for Thumbnail {}
