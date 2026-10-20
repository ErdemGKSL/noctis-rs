//! `Module`: a nestable, path-named group of registrations (FR-44, AD-7).

use std::fmt;

use crate::command::CommandHandler;
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

/// Something [`Module::add`] accepts: an `#[event]` handler, a
/// `#[command]` descriptor or a `#[user_command]` / `#[message_command]`
/// descriptor today; Routes and hooks join later. Sealed.
///
/// Event handlers come from `#[event]`: annotating `async fn on_user` gives
/// a `fn on_user()` that returns the handler.
///
/// ```
/// # use noctis_core::events::{EventCtx, EventHandler, UserUpdate};
/// # use noctis_core::{Descriptor, HandlerResult};
/// # async fn on_user_fn(_: EventCtx, _: UserUpdate) -> HandlerResult<()> {
/// #     Ok(())
/// # }
/// # fn on_user() -> EventHandler {
/// #     EventHandler::__new("bot::on_user", false, false, on_user_fn)
/// # }
/// fn is_descriptor(_: impl Descriptor) {}
/// is_descriptor(on_user());
/// ```
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
/// Event handlers come from `#[event]`: annotating `async fn award_xp`
/// gives a `fn award_xp()` that returns the handler to [`add`](Self::add).
/// Slash Commands come from `#[command]` the same way, context-menu
/// commands from `#[user_command]` and `#[message_command]`.
///
/// Commands of every mounted Module merge at `build()`: Modules sharing a
/// base word build one base command, and [`base`](Self::base) /
/// [`group`](Self::group) declare the descriptions (and base metadata) of
/// the nodes no single handler owns.
///
/// ```
/// # use noctis_core::events::{EventCtx, EventHandler, MessageCreate};
/// # use noctis_core::{HandlerResult, Module};
/// # async fn award_xp_fn(_: EventCtx, _: MessageCreate) -> HandlerResult<()> {
/// #     Ok(())
/// # }
/// # fn award_xp() -> EventHandler {
/// #     EventHandler::__new("bot::award_xp", false, false, award_xp_fn)
/// # }
/// let profile = Module::new("profile")
///     .add(award_xp())
///     .module(Module::new("profile/badges"));
/// assert_eq!(profile.id(), "profile");
/// ```
#[derive(Clone)]
pub struct Module {
    id: &'static str,
    handlers: Vec<EventHandler>,
    commands: Vec<CommandHandler>,
    menus: Vec<ContextMenuHandler>,
    nodes: Vec<NodeDecl>,
    children: Vec<Module>,
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
            nodes: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Adds a registration: an `#[event]` handler, a `#[command]`
    /// descriptor or a context-menu descriptor. Adding the same descriptor twice registers it twice.
    #[must_use = "Module::add returns the Module with the registration added"]
    #[expect(
        clippy::should_implement_trait,
        reason = "`Module::add` is the registration verb of the public API (AD-7), not `+`"
    )]
    pub fn add(mut self, descriptor: impl Descriptor) -> Self {
        descriptor.register(&mut self);
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
        f.debug_struct("Module")
            .field("id", &self.id)
            .field("handlers", &handlers)
            .field("commands", &commands)
            .field("menus", &menus)
            .field("nodes", &self.nodes)
            .field("children", &self.children)
            .finish()
    }
}
