//! `Module`: a nestable, path-named group of registrations (FR-44, AD-7).

use std::fmt;

use crate::command::CommandHandler;
use crate::component::{ComponentHandler, ModalHandler};
use crate::events::EventHandler;
use crate::menu::ContextMenuHandler;
use crate::metadata::BaseMeta;

pub(crate) mod sealed {
    use crate::Module;

    /// How a descriptor adds itself to a Module.
    pub trait Sealed {
        /// Appends the registration to `module`.
        fn register(self, module: &mut Module);
    }
}

/// A descriptor a Module stores: an [`EventHandler`], a
/// [`CommandHandler`], a [`ContextMenuHandler`], a
/// [`ComponentHandler`] or a [`ModalHandler`]. Sealed; what
/// [`Register`] impls push with [`Module::__add`].
#[doc(hidden)]
pub trait Descriptor: sealed::Sealed {}

impl sealed::Sealed for EventHandler {
    fn register(self, module: &mut Module) {
        module.handlers.push(self);
    }
}

impl Descriptor for EventHandler {}

impl sealed::Sealed for CommandHandler {
    fn register(self, module: &mut Module) {
        module.commands.push(self);
    }
}

impl Descriptor for CommandHandler {}

impl sealed::Sealed for ContextMenuHandler {
    fn register(self, module: &mut Module) {
        module.menus.push(self);
    }
}

impl Descriptor for ContextMenuHandler {}

impl sealed::Sealed for ComponentHandler {
    fn register(self, module: &mut Module) {
        module.components.push(self);
    }
}

impl Descriptor for ComponentHandler {}

impl sealed::Sealed for ModalHandler {
    fn register(self, module: &mut Module) {
        module.modals.push(self);
    }
}

impl Descriptor for ModalHandler {}

/// An interaction struct [`Module::add::<T>()`](Module::add) registers: a
/// struct with a kind derive (`SlashCommand`, `UserCommand`,
/// `MessageCommand`, `EventHandler`, `Button`, a select kind or `Modal`)
/// and a `#[handler]` method. Its only item is hidden, so only the derives
/// implement it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a Noctis interaction: derive SlashCommand, UserCommand, MessageCommand, EventHandler, Button, a select kind or Modal and add a #[handler] method",
    label = "not a Noctis interaction",
    note = "`Module::add::<T>()` registers one struct per interaction"
)]
pub trait Register {
    /// Appends the struct's descriptor to `module`.
    #[doc(hidden)]
    fn __register(module: Module) -> Module;
}

/// A description (and base metadata) a Module declares for a command node
/// with [`Module::base`] or [`Module::group`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NodeDecl {
    /// `Module::base(name, meta)`.
    Base { name: &'static str, meta: BaseMeta },
    /// `Module::group(path, description)`.
    Group {
        path: &'static str,
        description: &'static str,
    },
}

/// A group of registrations under a path-like id (`"a/b/c"`), mounted on an
/// App with `AppBuilder::module`.
///
/// Registration is data: nothing runs when you add or nest, and the call
/// order of [`add`](Self::add) and [`module`](Self::module) does not
/// matter. `build()` folds every mounted tree once, parent before child:
/// a Module's own registrations in `add` order, then each child in
/// `module` order. The same tree can be mounted on several Apps (it is
/// `Clone`). Ids are not validated yet.
///
/// Every interaction is one struct with a kind derive and a `#[handler]`
/// method (`#[derive(SlashCommand)]`, `#[derive(EventHandler)]`,
/// `#[derive(Button)]`, …); [`add::<T>()`](Self::add) registers it.
///
/// Commands of every mounted Module merge at `build()`: Modules sharing a
/// base word build one base command, and [`base`](Self::base) /
/// [`group`](Self::group) declare the descriptions (and base metadata) of
/// the nodes no single handler owns.
///
/// ```
/// # use noctis_core::events::{EventCtx, EventHandler, MessageCreate};
/// # use noctis_core::{HandlerResult, Module, Register};
/// # async fn award_xp_fn(_: EventCtx, _: MessageCreate) -> HandlerResult<()> {
/// #     Ok(())
/// # }
/// // `#[derive(EventHandler)] #[event] struct AwardXp;` with its `#[handler]` method.
/// struct AwardXp;
/// # impl Register for AwardXp {
/// #     fn __register(module: Module) -> Module {
/// #         module.__add(EventHandler::__new("bot::AwardXp", false, false, award_xp_fn))
/// #     }
/// # }
///
/// let profile = Module::new("profile")
///     .add::<AwardXp>()
///     .module(Module::new("profile/badges"));
/// assert_eq!(profile.id(), "profile");
/// ```
#[derive(Clone)]
pub struct Module {
    id: &'static str,
    handlers: Vec<EventHandler>,
    commands: Vec<CommandHandler>,
    menus: Vec<ContextMenuHandler>,
    components: Vec<ComponentHandler>,
    modals: Vec<ModalHandler>,
    uses: Vec<UsedRoute>,
    nodes: Vec<NodeDecl>,
    children: Vec<Module>,
}

/// A Route a Module renders but another Module handles
/// ([`Module::uses`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UsedRoute {
    /// The Route id.
    pub(crate) route: &'static str,
    /// The Route struct's type path.
    pub(crate) route_type: &'static str,
}

impl Module {
    /// An empty Module named `id`.
    ///
    /// ```
    /// assert_eq!(noctis_core::Module::new("a/b").id(), "a/b");
    /// ```
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            handlers: Vec::new(),
            commands: Vec::new(),
            menus: Vec::new(),
            components: Vec::new(),
            modals: Vec::new(),
            uses: Vec::new(),
            nodes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Registers the interaction struct `T`: its kind derive and
    /// `#[handler]` method give the descriptor (a slash Command with its
    /// `#[autocomplete(field)]` methods, a context menu, an event handler, a
    /// component or a modal). Adding the same struct twice registers it
    /// twice.
    #[must_use = "Module::add returns the Module with the registration added"]
    pub fn add<T: Register>(self) -> Self {
        T::__register(self)
    }

    /// Appends `descriptor`; what the derives' [`Register`] impls call.
    #[doc(hidden)]
    #[must_use]
    pub fn __add(mut self, descriptor: impl Descriptor) -> Self {
        descriptor.register(&mut self);
        self
    }

    /// Declares that this Module's Views build components of the Route `R`
    /// that another Module handles (CID-04, AD-19).
    ///
    /// `build()` checks it for mounted Modules only: some mounted Module
    /// (this one, a child, a sibling or another tree) must `add::<R>()`,
    /// else the build fails with ``[CID-04] Module `m` uses Route `id`
    /// (`path::R`), but no mounted Module handles it``. A Module that renders
    /// only its own Routes needs no declaration. `R` must be a kind-derived
    /// struct with a `#[handler]` method.
    ///
    /// ```
    /// use noctis_core::Module;
    /// # use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape};
    /// # struct Buy;
    /// # impl Payload for Buy {
    /// #     const MAX_ENCODED_LEN: Option<usize> = Some(0);
    /// #     fn encode_fields(&self, _: &mut FieldWriter<'_>) -> core::fmt::Result { Ok(()) }
    /// # }
    /// # impl Route for Buy {
    /// #     const ID: RouteId = RouteId::new("shop.buy");
    /// #     const SHAPE: RouteShape = RouteShape::INLINE;
    /// #     const SCHEMA: u8 = 0;
    /// #     type Payload = Self;
    /// # }
    /// # impl noctis_ui::Handled for Buy {}
    /// // `#[derive(Button)] #[button("shop.buy", ..)] struct Buy;`, added in Module "shop".
    /// let dashboard = Module::new("dash").uses::<Buy>();
    /// assert!(format!("{dashboard:?}").contains("shop.buy"));
    /// ```
    #[must_use = "Module::uses returns the Module with the declaration added"]
    pub fn uses<R: noctis_ui::Handled>(mut self) -> Self {
        self.uses.push(UsedRoute {
            route: R::ID.as_str(),
            route_type: std::any::type_name::<R>(),
        });
        self
    }

    /// Nests `child` under this Module; it folds after this Module's own
    /// registrations, with this Module in its ancestor path.
    ///
    /// ```
    /// use noctis_core::Module;
    ///
    /// let root = Module::new("ops").module(Module::new("ops/audit"));
    /// assert!(format!("{root:?}").contains("ops/audit"));
    /// ```
    #[must_use = "Module::module returns the Module with the child nested"]
    pub fn module(mut self, child: Module) -> Self {
        self.children.push(child);
        self
    }

    /// Declares the description and base metadata of the base command
    /// `name` (a 1-word Command Path, FR-57).
    ///
    /// Checked at `build()`, after merging every Module: the base must exist
    /// (some handler path starts with `name`, else REG-04), the description
    /// must be 1 to 100 characters (CMD-04), metadata fields must agree with
    /// the handlers that set them (META-01, META-02, CMD-07; unset handlers
    /// inherit them) and with every other Module's declaration (REG-04 for
    /// descriptions). The values apply in every scope the base exists in;
    /// `contexts` and `integration_types` on a guild-scoped base are META-03.
    /// A metadata field on a path below a base is META-02. Without a
    /// description, the base takes a handler's description (see the crate
    /// docs) and `build()` logs a warning.
    ///
    /// ```
    /// use noctis_core::{BaseMeta, Module, Permissions};
    ///
    /// let config = Module::new("config").base(
    ///     "config",
    ///     BaseMeta::new()
    ///         .description("Configure the bot")
    ///         .default_member_permissions(Permissions::MANAGE_GUILD),
    /// );
    /// assert!(format!("{config:?}").contains("Configure the bot"));
    /// ```
    #[must_use = "Module::base returns the Module with the declaration added"]
    pub fn base(mut self, name: &'static str, meta: BaseMeta) -> Self {
        self.nodes.push(NodeDecl::Base { name, meta });
        self
    }

    /// Declares the description of the subcommand group `path` (FR-57).
    ///
    /// Meant for 2-word group paths such as `"config set"`; any other
    /// registered path just gets the description (it must then match the
    /// handler's). Checked at `build()` like [`base`](Self::base): the path
    /// must exist in some handler path (REG-04), the description must be 1 to
    /// 100 characters (CMD-04) and agree with every other declaration of the
    /// path (REG-04); a path of more than 3 words is CMD-10. The description
    /// applies in every scope the group exists in.
    ///
    /// ```
    /// use noctis_core::Module;
    ///
    /// let config = Module::new("config").group("config set", "Change a setting");
    /// assert!(format!("{config:?}").contains("config set"));
    /// ```
    #[must_use = "Module::group returns the Module with the declaration added"]
    pub fn group(mut self, path: &'static str, description: &'static str) -> Self {
        self.nodes.push(NodeDecl::Group { path, description });
        self
    }

    /// The Module's id.
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Its own handlers, in `add` order.
    pub(crate) fn handlers(&self) -> &[EventHandler] {
        &self.handlers
    }

    /// Its own Commands, in `add` order.
    pub(crate) fn commands(&self) -> &[CommandHandler] {
        &self.commands
    }

    /// Its own context-menu commands, in `add` order.
    pub(crate) fn menus(&self) -> &[ContextMenuHandler] {
        &self.menus
    }

    /// Its own component handlers, in `add` order.
    pub(crate) fn components(&self) -> &[ComponentHandler] {
        &self.components
    }

    /// Its own modal handlers, in `add` order.
    pub(crate) fn modals(&self) -> &[ModalHandler] {
        &self.modals
    }

    /// The Routes it declared with [`Module::uses`], in call order.
    pub(crate) fn used_routes(&self) -> &[UsedRoute] {
        &self.uses
    }

    /// Its `base`/`group` declarations, in call order.
    pub(crate) fn nodes(&self) -> &[NodeDecl] {
        &self.nodes
    }

    /// Its children, in `module` order.
    pub(crate) fn children(&self) -> &[Module] {
        &self.children
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handlers: Vec<&str> = self.handlers.iter().map(EventHandler::name).collect();
        let commands: Vec<&str> = self.commands.iter().map(CommandHandler::name).collect();
        let menus: Vec<&str> = self
            .menus
            .iter()
            .map(ContextMenuHandler::menu_name)
            .collect();
        let components: Vec<&str> = self
            .components
            .iter()
            .map(ComponentHandler::route)
            .collect();
        let modals: Vec<&str> = self.modals.iter().map(ModalHandler::route).collect();
        let uses: Vec<&str> = self.uses.iter().map(|u| u.route).collect();
        f.debug_struct("Module")
            .field("id", &self.id)
            .field("handlers", &handlers)
            .field("commands", &commands)
            .field("menus", &menus)
            .field("components", &components)
            .field("modals", &modals)
            .field("uses", &uses)
            .field("nodes", &self.nodes)
            .field("children", &self.children)
            .finish()
    }
}
