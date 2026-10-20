//! `Module`: a nestable, path-named group of registrations (FR-44, AD-7).

use std::fmt;

use crate::events::EventHandler;

pub(crate) mod sealed {
    use crate::events::EventHandler;

    /// How a descriptor adds itself to a Module.
    pub trait Sealed {
        /// Appends the registration to `handlers`.
        fn register(self, handlers: &mut Vec<EventHandler>);
    }
}

/// Something [`Module::add`] accepts: an `#[event]` handler today; commands,
/// Routes and hooks join later. Sealed.
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
    fn register(self, handlers: &mut Vec<EventHandler>) {
        handlers.push(self);
    }
}

impl Descriptor for EventHandler {}

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
            children: Vec::new(),
        }
    }

    /// Adds a registration (an `#[event]` handler). Adding the same
    /// descriptor twice registers it twice.
    #[must_use = "Module::add returns the Module with the registration added"]
    #[expect(
        clippy::should_implement_trait,
        reason = "`Module::add` is the registration verb of the public API (AD-7), not `+`"
    )]
    pub fn add(mut self, descriptor: impl Descriptor) -> Self {
        descriptor.register(&mut self.handlers);
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

    /// The Module's id.
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Its own handlers, in `add` order.
    pub(crate) fn handlers(&self) -> &[EventHandler] {
        &self.handlers
    }

    /// Its children, in `module` order.
    pub(crate) fn children(&self) -> &[Module] {
        &self.children
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handlers: Vec<&str> = self.handlers.iter().map(EventHandler::name).collect();
        f.debug_struct("Module")
            .field("id", &self.id)
            .field("handlers", &handlers)
            .field("children", &self.children)
            .finish()
    }
}
