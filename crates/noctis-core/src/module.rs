//! `Module`: a nestable, path-named group of registrations (FR-44, AD-7).

use std::fmt;

use noctis_ports::{Template, TemplateDescriptor};

use crate::boot_flags::BootFlags;
use crate::command::CommandHandler;
use crate::component::{ComponentHandler, ModalHandler};
use crate::events::EventHandler;
use crate::lifecycle::LifecycleHook;
use crate::menu::ContextMenuHandler;
use crate::metadata::BaseMeta;
use crate::middleware::{AfterMiddleware, BeforeMiddleware};

pub(crate) mod sealed {
    use crate::Module;

    /// How a descriptor adds itself to a Module.
    pub trait Sealed {
        /// Appends the registration to `module`, declaring the Boot Flags
        /// `flags` (none for an unflagged registration) and the App ids
        /// `apps` (none for an unrestricted registration).
        fn register(
            self,
            module: &mut Module,
            flags: &'static [&'static str],
            apps: &'static [&'static str],
        );
    }
}

/// A descriptor a Module stores: an [`EventHandler`], a
/// [`CommandHandler`], a [`ContextMenuHandler`], a
/// [`ComponentHandler`], a [`ModalHandler`] or a [`LifecycleHook`].
/// Sealed; what
/// [`Register`] impls push with [`Module::__add`].
#[doc(hidden)]
pub trait Descriptor: sealed::Sealed {}

impl sealed::Sealed for EventHandler {
    fn register(
        self,
        module: &mut Module,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
    ) {
        module.handlers.push(self, flags, apps);
    }
}

impl Descriptor for EventHandler {}

impl sealed::Sealed for CommandHandler {
    fn register(
        self,
        module: &mut Module,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
    ) {
        module.commands.push(self, flags, apps);
    }
}

impl Descriptor for CommandHandler {}

impl sealed::Sealed for ContextMenuHandler {
    fn register(
        self,
        module: &mut Module,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
    ) {
        module.menus.push(self, flags, apps);
    }
}

impl Descriptor for ContextMenuHandler {}

impl sealed::Sealed for ComponentHandler {
    fn register(
        self,
        module: &mut Module,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
    ) {
        module.components.push(self, flags, apps);
    }
}

impl Descriptor for ComponentHandler {}

impl sealed::Sealed for ModalHandler {
    fn register(
        self,
        module: &mut Module,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
    ) {
        module.modals.push(self, flags, apps);
    }
}

impl Descriptor for ModalHandler {}

impl sealed::Sealed for LifecycleHook {
    fn register(
        self,
        module: &mut Module,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
    ) {
        module.hooks.push(self, flags, apps);
    }
}

impl Descriptor for LifecycleHook {}

/// An interaction struct [`Module::add::<T>()`](Module::add) registers: a
/// struct with a kind derive (`SlashCommand`, `UserCommand`,
/// `MessageCommand`, `EventHandler`, `Button`, a select kind, `Modal` or
/// `Hook`) and a `#[handler]` method. Its only item is hidden, so only the derives
/// implement it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a Noctis interaction: derive SlashCommand, UserCommand, MessageCommand, EventHandler, Button, a select kind, Modal or Hook and add a #[handler] method",
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
/// `Clone`).
///
/// # Ids
///
/// The id is the Module's full path in its tree (`"profile"`,
/// `"profile/badges"`): one or more segments of `a-z`, `0-9`, `_` and `-`
/// joined by single `/`, 1 to 64 characters. A root Module (mounted with
/// `AppBuilder::module`, plugins included) has one segment, and a child's
/// id is its parent's id plus one segment. Ids are unique per App across
/// every mounted tree. `build()` checks all three and reports each broken
/// id as [`BuildViolation::InvalidModuleId`](crate::BuildViolation::InvalidModuleId)
/// (with a [`ModuleIdError`] reason) and each repeated id as
/// [`BuildViolation::DuplicateModule`](crate::BuildViolation::DuplicateModule).
/// The id is the `module` field of every span and `ErrorReport` of its
/// registrations.
///
/// # Boot Flags
///
/// A Module declares Boot Flags with [`flags`](Self::flags) and a single
/// registration with `flags = [..]` on its kind attribute
/// (`#[command("beta stats", flags = ["beta"])]`). A flag is 1 to 32
/// characters of `a-z`, `0-9`, `_` and `-`; `all` and `none` are reserved.
/// The App's selection ([`BootFlags`](crate::BootFlags): `--features`,
/// `NOCTIS_FEATURES` or `AppBuilder::boot_flags`, default `all`) prunes the
/// trees before `build()` folds them: an unflagged Module or registration
/// always exists, a flagged one exists when the selection is `all` or holds
/// any of its flags, and a pruned Module drops its whole subtree
/// (registrations, children, Middleware, `uses`, `base`/`group`
/// declarations). A kept Module's `base`/`group` declaration whose node
/// loses its last Command to the selection is dropped too. Pruned
/// registrations are not dispatched, add no intents and are not published.
/// Locale catalogs and Templates of pruned Modules stay App-wide. Module
/// ids and flags are checked on the full trees, whatever is selected.
///
/// ```
/// use noctis_core::Module;
///
/// // Exists only when `economy` is selected (or `all`).
/// let eco = Module::new("eco").flags(["economy"]);
/// assert!(format!("{eco:?}").contains("economy"));
/// ```
///
/// # App restriction
///
/// The same tree can be mounted on several Apps of one process. A Module
/// restricted with [`apps`](Self::apps) and a single registration with
/// `apps = [..]` on its kind attribute (`#[command("admin", apps =
/// ["main"])]`, `#[user_command]`, `#[message_command]`) exist only on an
/// App whose effective App id is listed: the explicit `AppBuilder::app_id`,
/// else the id derived from the token, else `default`. Ids use the App id
/// grammar `[a-z0-9-]{1,32}`. The restriction works like Boot Flags, in the
/// same pruning pass before the fold: an unrestricted Module or
/// registration always exists, a pruned Module drops its whole subtree,
/// and a kept Module's `base`/`group` declaration whose node loses its last
/// Command is dropped. A pruned registration is not dispatched, adds no
/// intents and is not published. A registration exists only when both its
/// Boot Flags and its App restriction keep it. Set `.app_id(..)`
/// explicitly on every App that restricts registrations.
///
/// ```
/// use noctis_core::Module;
///
/// // Exists only on the App built with `.app_id("main")`.
/// let admin = Module::new("admin").apps(["main"]);
/// assert!(format!("{admin:?}").contains("main"));
/// ```
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
/// Locale catalogs added with [`locales`](Self::locales) merge with the
/// App's (`AppBuilder::locales`) at `build()` into one App-wide set: every
/// Module resolves every key, and disabling a Module never removes its
/// strings.
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
    /// The Boot Flags of `Module::flags`, in call order (validated at
    /// `build()`).
    flags: Vec<&'static str>,
    /// The App ids of `Module::apps`, in call order (validated at
    /// `build()`); empty when unrestricted.
    apps: Vec<&'static str>,
    handlers: Flagged<EventHandler>,
    commands: Flagged<CommandHandler>,
    menus: Flagged<ContextMenuHandler>,
    components: Flagged<ComponentHandler>,
    modals: Flagged<ModalHandler>,
    hooks: Flagged<LifecycleHook>,
    uses: Vec<UsedRoute>,
    nodes: Vec<NodeDecl>,
    before: Vec<BeforeMiddleware>,
    after: Vec<AfterMiddleware>,
    templates: Vec<(TemplateDescriptor, &'static str)>,
    locales: Vec<noctis_i18n::Catalog>,
    children: Vec<Module>,
}

/// One kind of registration in `add` order, each with the Boot Flags its
/// kind attribute declares (`flags = [..]`; empty when unflagged) and the
/// App ids it is restricted to (`apps = [..]`; empty when unrestricted).
#[derive(Clone)]
struct Flagged<T> {
    items: Vec<T>,
    /// Parallel to `items`.
    flags: Vec<&'static [&'static str]>,
    /// Parallel to `items`.
    apps: Vec<&'static [&'static str]>,
}

impl<T> Flagged<T> {
    const fn new() -> Self {
        Self {
            items: Vec::new(),
            flags: Vec::new(),
            apps: Vec::new(),
        }
    }

    fn push(&mut self, item: T, flags: &'static [&'static str], apps: &'static [&'static str]) {
        self.items.push(item);
        self.flags.push(flags);
        self.apps.push(apps);
    }

    fn iter(&self) -> impl Iterator<Item = (&T, &'static [&'static str])> {
        self.items.iter().zip(self.flags.iter().copied())
    }

    /// Each registration with its App restriction.
    fn restricted(&self) -> impl Iterator<Item = (&T, &'static [&'static str])> {
        self.items.iter().zip(self.apps.iter().copied())
    }
}

impl<T: Clone> Flagged<T> {
    /// The registrations `selection` keeps on the App `app`, in order.
    fn selected(&self, selection: &BootFlags, app: &str) -> Self {
        let mut kept = Self::new();
        for ((item, flags), apps) in self.iter().zip(self.apps.iter().copied()) {
            if selection.selects(flags) && serves_app(apps, app) {
                kept.push(item.clone(), flags, apps);
            }
        }
        kept
    }
}

/// Whether an App restriction (`apps = [..]` or `Module::apps`) keeps a
/// registration on the App `app`: an empty restriction keeps it everywhere.
fn serves_app(apps: &[&str], app: &str) -> bool {
    apps.is_empty() || apps.contains(&app)
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
    /// An empty Module named `id`, its full path (see [Ids](Module#ids)).
    ///
    /// Never fails: `build()` checks the id's grammar, tree position and
    /// uniqueness once the Module is mounted.
    ///
    /// ```
    /// assert_eq!(noctis_core::Module::new("a/b").id(), "a/b");
    /// ```
    pub fn new(id: &'static str) -> Self {
        Self {
            id,
            flags: Vec::new(),
            apps: Vec::new(),
            handlers: Flagged::new(),
            commands: Flagged::new(),
            menus: Flagged::new(),
            components: Flagged::new(),
            modals: Flagged::new(),
            hooks: Flagged::new(),
            uses: Vec::new(),
            nodes: Vec::new(),
            before: Vec::new(),
            after: Vec::new(),
            templates: Vec::new(),
            locales: Vec::new(),
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
        descriptor.register(&mut self, &[], &[]);
        self
    }

    /// Appends `descriptor` with the Boot Flags of its kind attribute's
    /// `flags = [..]` (checked by the macro); what the derives' [`Register`]
    /// impls call for a flagged registration.
    #[doc(hidden)]
    #[must_use]
    pub fn __add_flagged(
        mut self,
        flags: &'static [&'static str],
        descriptor: impl Descriptor,
    ) -> Self {
        descriptor.register(&mut self, flags, &[]);
        self
    }

    /// Appends `descriptor` with the Boot Flags of its kind attribute's
    /// `flags = [..]` and the App ids of its `apps = [..]` (both checked by
    /// the macro); what the derives' [`Register`] impls call for a
    /// registration restricted to some Apps.
    #[doc(hidden)]
    #[must_use]
    pub fn __add_scoped(
        mut self,
        flags: &'static [&'static str],
        apps: &'static [&'static str],
        descriptor: impl Descriptor,
    ) -> Self {
        descriptor.register(&mut self, flags, apps);
        self
    }

    /// Adds the Boot Flags `flags` to this Module (see
    /// [Boot Flags](Module#boot-flags)); repeated calls append. No flags
    /// leaves the Module unflagged.
    ///
    /// `build()` checks each flag's grammar and reports a broken or
    /// repeated flag as
    /// [`BuildViolation::InvalidBootFlag`](crate::BuildViolation::InvalidBootFlag).
    ///
    /// ```
    /// use noctis_core::Module;
    ///
    /// let eco = Module::new("eco").flags(["economy"]);
    /// assert!(format!("{eco:?}").contains("economy"));
    /// ```
    #[must_use = "Module::flags returns the Module with the flags added"]
    pub fn flags<I: IntoIterator<Item = &'static str>>(mut self, flags: I) -> Self {
        self.flags.extend(flags);
        self
    }

    /// Restricts this Module to the Apps whose effective App id is in
    /// `apps` (see [App restriction](Module#app-restriction)); repeated
    /// calls append. No ids leaves the Module unrestricted.
    ///
    /// `build()` checks each id against the App id grammar
    /// (`[a-z0-9-]{1,32}`) and reports a broken one as
    /// [`BuildViolation::InvalidAppRestriction`](crate::BuildViolation::InvalidAppRestriction).
    ///
    /// ```
    /// use noctis_core::Module;
    ///
    /// // Exists only on the App built with `.app_id("main")`.
    /// let admin = Module::new("admin").apps(["main"]);
    /// assert!(format!("{admin:?}").contains("main"));
    /// ```
    #[must_use = "Module::apps returns the Module with the App restriction added"]
    pub fn apps<I: IntoIterator<Item = &'static str>>(mut self, apps: I) -> Self {
        self.apps.extend(apps);
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
    /// # impl noctis_codec::DeclaredRuntime for Buy {}
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

    /// Registers the image Template `T` (FR-32, AD-22): its id, version and
    /// view reach the App's Renderer.
    ///
    /// `build()` rejects a Template id registered twice across the mounted
    /// Modules (any version, the same type included) with
    /// [`BuildViolation::DuplicateTemplate`](crate::BuildViolation::DuplicateTemplate),
    /// naming both types and Modules.
    ///
    /// ```
    /// use noctis_core::Module;
    /// # use noctis_ports::{AnimationSpec, ErasedView, Format, Size, Template};
    /// # #[derive(serde::Serialize, serde::Deserialize)]
    /// # struct ProfileCard;
    /// # impl Template for ProfileCard {
    /// #     const ID: &'static str = "profile.card";
    /// #     const VERSION: u32 = 1;
    /// #     const SIZE: Size = Size::new(934, 282);
    /// #     const FORMAT: Format = Format::Png;
    /// #     const ANIMATION: Option<AnimationSpec> = None;
    /// #     fn view() -> ErasedView { ErasedView::new(()) }
    /// # }
    /// // `#[derive(Template)] #[template(id = "profile.card", ..)] struct ProfileCard { .. }`
    /// let profile = Module::new("profile").template::<ProfileCard>();
    /// assert!(format!("{profile:?}").contains("profile.card"));
    /// ```
    #[must_use = "Module::template returns the Module with the Template registered"]
    pub fn template<T: Template>(mut self) -> Self {
        self.templates
            .push((TemplateDescriptor::of::<T>(), std::any::type_name::<T>()));
        self
    }

    /// Nests `child` under this Module; it folds after this Module's own
    /// registrations, with this Module in its ancestor path.
    ///
    /// The child's id must be this Module's id plus one segment
    /// (`"ops"` → `"ops/audit"`); `build()` reports any other id as
    /// [`BuildViolation::InvalidModuleId`](crate::BuildViolation::InvalidModuleId)
    /// with [`ModuleIdError::NotUnderParent`]. The id is not rewritten.
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

    /// Registers the before-Middleware `m` (FR-20, AD-23 stage 7).
    ///
    /// It runs before every handler of this Module and its descendants
    /// that its scope covers: the root Module's Middleware first, then each
    /// Module's in call order. It may insert typed Extensions (declare them
    /// with [`provides`](BeforeMiddleware::provides) so a handler can take
    /// `Ext<T>`) and stop the run with [`Flow::Stop`](crate::Flow::Stop).
    ///
    /// ```
    /// use noctis_core::{BeforeCtx, Flow, HandlerResult, Middleware, Module};
    ///
    /// async fn guild_only(ctx: BeforeCtx) -> HandlerResult<Flow> {
    ///     Ok(if ctx.guild_id().is_some() { Flow::Continue } else { Flow::Stop(None) })
    /// }
    ///
    /// let module = Module::new("mod").before(Middleware::before(guild_only).skip_events());
    /// assert!(format!("{module:?}").contains("guild_only"));
    /// ```
    #[must_use = "Module::before returns the Module with the Middleware added"]
    pub fn before(mut self, m: BeforeMiddleware) -> Self {
        self.before.push(m);
        self
    }

    /// Registers the after-Middleware `m` (FR-20, AD-23 stage 10).
    ///
    /// It runs after every handler run of this Module and its descendants
    /// that its scope covers, whatever the outcome (not after a shutdown
    /// cancellation), in reverse chain order: this Module's last-registered
    /// first, the root's first-registered last.
    ///
    /// ```
    /// use noctis_core::{AfterCtx, Middleware, Module};
    ///
    /// async fn audit(ctx: AfterCtx) {
    ///     tracing::info!(outcome = ?ctx.outcome(), "ran");
    /// }
    ///
    /// let module = Module::new("mod").after(Middleware::after(audit));
    /// assert!(format!("{module:?}").contains("audit"));
    /// ```
    #[must_use = "Module::after returns the Module with the Middleware added"]
    pub fn after(mut self, m: AfterMiddleware) -> Self {
        self.after.push(m);
        self
    }

    /// Adds the locale catalog of an `i18n!` invocation (its `catalog()`),
    /// so a reusable Module ships its own strings.
    ///
    /// At `build()` the App's catalogs (`AppBuilder::locales`, in call
    /// order) and then every mounted Module's (in fold order: parent before
    /// children, each Module's in call order) merge into one App-wide set.
    /// A key resolves App-wide: any Module's handler can send another
    /// Module's keys, `noctis.*` overrides apply to every Module, and
    /// disabling a Module never removes its strings. Catalogs with one
    /// `origin()` count once; a key that two catalogs define is a
    /// `BuildViolation::DuplicateLocaleKey` naming both owners.
    ///
    /// ```
    /// use noctis_core::Module;
    /// use noctis_i18n::Catalog;
    /// use noctis_rules::Locale;
    ///
    /// // What `i18n!` generates as `catalog()`.
    /// static CATALOG: Catalog = Catalog::__new(
    ///     "shop/locales",
    ///     Locale::EnUs,
    ///     &[(Locale::EnUs, &[("shop.title", "Shop")])],
    ///     &[("shop.title", &[])],
    /// );
    ///
    /// let shop = Module::new("shop").locales(CATALOG);
    /// assert!(format!("{shop:?}").contains("shop/locales"));
    /// ```
    #[must_use = "Module::locales returns the Module with the catalog added"]
    pub fn locales(mut self, catalog: noctis_i18n::Catalog) -> Self {
        self.locales.push(catalog);
        self
    }

    /// The Module's id.
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Its own handlers, in `add` order.
    pub(crate) fn handlers(&self) -> &[EventHandler] {
        &self.handlers.items
    }

    /// Its own Commands, in `add` order.
    pub(crate) fn commands(&self) -> &[CommandHandler] {
        &self.commands.items
    }

    /// Its own context-menu commands, in `add` order.
    pub(crate) fn menus(&self) -> &[ContextMenuHandler] {
        &self.menus.items
    }

    /// Its own component handlers, in `add` order.
    pub(crate) fn components(&self) -> &[ComponentHandler] {
        &self.components.items
    }

    /// Its own modal handlers, in `add` order.
    pub(crate) fn modals(&self) -> &[ModalHandler] {
        &self.modals.items
    }

    /// Its own Lifecycle Hooks, in `add` order.
    pub(crate) fn hooks(&self) -> &[LifecycleHook] {
        &self.hooks.items
    }

    /// The Routes it declared with [`Module::uses`], in call order.
    pub(crate) fn used_routes(&self) -> &[UsedRoute] {
        &self.uses
    }

    /// Its `base`/`group` declarations, in call order.
    pub(crate) fn nodes(&self) -> &[NodeDecl] {
        &self.nodes
    }

    /// Its before-Middleware, in call order.
    pub(crate) fn before_middleware(&self) -> &[BeforeMiddleware] {
        &self.before
    }

    /// Its after-Middleware, in call order.
    pub(crate) fn after_middleware(&self) -> &[AfterMiddleware] {
        &self.after
    }

    /// Its Templates with their type paths, in `template` order.
    pub(crate) fn templates(&self) -> &[(TemplateDescriptor, &'static str)] {
        &self.templates
    }

    /// Its locale catalogs, in `locales` call order.
    pub(crate) fn catalogs(&self) -> &[noctis_i18n::Catalog] {
        &self.locales
    }

    /// Its children, in `module` order.
    pub(crate) fn children(&self) -> &[Module] {
        &self.children
    }

    /// Its own Boot Flags (`Module::flags`), in call order.
    pub(crate) fn boot_flags(&self) -> &[&'static str] {
        &self.flags
    }

    /// Its own App restriction (`Module::apps`), in call order.
    pub(crate) fn app_restriction(&self) -> &[&'static str] {
        &self.apps
    }

    /// Whether this tree holds any App restriction (`Module::apps` or a
    /// registration's `apps = [..]`).
    pub(crate) fn has_app_restriction(&self) -> bool {
        !self.apps.is_empty()
            || self.handlers.apps.iter().any(|apps| !apps.is_empty())
            || self.commands.apps.iter().any(|apps| !apps.is_empty())
            || self.menus.apps.iter().any(|apps| !apps.is_empty())
            || self.components.apps.iter().any(|apps| !apps.is_empty())
            || self.modals.apps.iter().any(|apps| !apps.is_empty())
            || self.children.iter().any(Self::has_app_restriction)
    }

    /// The Boot Flags its own registrations declare, in kind then `add`
    /// order (repeats included).
    pub(crate) fn registration_flags(&self) -> impl Iterator<Item = &'static str> + '_ {
        let handlers = self.handlers.flags.iter();
        let commands = self.commands.flags.iter();
        let menus = self.menus.flags.iter();
        let components = self.components.flags.iter();
        let modals = self.modals.flags.iter();
        let hooks = self.hooks.flags.iter();
        handlers
            .chain(commands)
            .chain(menus)
            .chain(components)
            .chain(modals)
            .chain(hooks)
            .flat_map(|flags| flags.iter().copied())
    }

    /// This tree as `selection` keeps it on the App whose effective App id
    /// is `app` (AD-7, filter before fold): `None` when this Module's flags
    /// are not selected or its App restriction does not hold `app` (its
    /// whole subtree goes), else a copy with only the kept registrations
    /// and children, in order. Templates, catalogs, Middleware, `uses` and
    /// `base`/`group` declarations of a kept Module stay; see
    /// [`retain_nodes`](Self::retain_nodes) for the declarations.
    pub(crate) fn selected(&self, selection: &BootFlags, app: &str) -> Option<Self> {
        if !selection.selects(&self.flags) || !serves_app(&self.apps, app) {
            return None;
        }
        Some(Self {
            id: self.id,
            flags: self.flags.clone(),
            apps: self.apps.clone(),
            handlers: self.handlers.selected(selection, app),
            commands: self.commands.selected(selection, app),
            menus: self.menus.selected(selection, app),
            components: self.components.selected(selection, app),
            modals: self.modals.selected(selection, app),
            hooks: self.hooks.selected(selection, app),
            uses: self.uses.clone(),
            nodes: self.nodes.clone(),
            before: self.before.clone(),
            after: self.after.clone(),
            templates: self.templates.clone(),
            locales: self.locales.clone(),
            children: self
                .children
                .iter()
                .filter_map(|child| child.selected(selection, app))
                .collect(),
        })
    }

    /// Keeps the `base`/`group` declarations of this tree for which `keep`
    /// holds.
    pub(crate) fn retain_nodes(&mut self, keep: &impl Fn(&NodeDecl) -> bool) {
        self.nodes.retain(keep);
        for child in &mut self.children {
            child.retain_nodes(keep);
        }
    }
}

impl NodeDecl {
    /// The declared node's Command Path.
    pub(crate) fn path(&self) -> &'static str {
        match *self {
            Self::Base { name, .. } => name,
            Self::Group { path, .. } => path,
        }
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handlers: Vec<&str> = self.handlers.items.iter().map(EventHandler::name).collect();
        let commands: Vec<&str> = self
            .commands
            .items
            .iter()
            .map(CommandHandler::name)
            .collect();
        let menus: Vec<&str> = self
            .menus
            .items
            .iter()
            .map(ContextMenuHandler::menu_name)
            .collect();
        let components: Vec<&str> = self
            .components
            .items
            .iter()
            .map(ComponentHandler::route)
            .collect();
        let modals: Vec<&str> = self.modals.items.iter().map(ModalHandler::route).collect();
        let hooks: Vec<&str> = self.hooks.items.iter().map(LifecycleHook::name).collect();
        // The flagged registrations: (name or Route id, flags).
        let flagged: Vec<(&str, &[&str])> = self
            .handlers
            .iter()
            .map(|(h, f)| (h.name(), f))
            .chain(self.commands.iter().map(|(h, f)| (h.name(), f)))
            .chain(self.menus.iter().map(|(h, f)| (h.menu_name(), f)))
            .chain(self.components.iter().map(|(h, f)| (h.route(), f)))
            .chain(self.modals.iter().map(|(h, f)| (h.route(), f)))
            .chain(self.hooks.iter().map(|(h, f)| (h.name(), f)))
            .filter(|(_, f)| !f.is_empty())
            .collect();
        // The App-restricted registrations: (name or Route id, App ids).
        let restricted: Vec<(&str, &[&str])> = self
            .handlers
            .restricted()
            .map(|(h, a)| (h.name(), a))
            .chain(self.commands.restricted().map(|(h, a)| (h.name(), a)))
            .chain(self.menus.restricted().map(|(h, a)| (h.menu_name(), a)))
            .chain(self.components.restricted().map(|(h, a)| (h.route(), a)))
            .chain(self.modals.restricted().map(|(h, a)| (h.route(), a)))
            .filter(|(_, a)| !a.is_empty())
            .collect();
        let uses: Vec<&str> = self.uses.iter().map(|u| u.route).collect();
        let before: Vec<&str> = self.before.iter().map(BeforeMiddleware::name).collect();
        let after: Vec<&str> = self.after.iter().map(AfterMiddleware::name).collect();
        let templates: Vec<&str> = self.templates.iter().map(|(t, _)| t.id).collect();
        let locales: Vec<&str> = self
            .locales
            .iter()
            .map(noctis_i18n::Catalog::origin)
            .collect();
        f.debug_struct("Module")
            .field("id", &self.id)
            .field("flags", &self.flags)
            .field("handlers", &handlers)
            .field("commands", &commands)
            .field("menus", &menus)
            .field("components", &components)
            .field("modals", &modals)
            .field("hooks", &hooks)
            .field("flagged", &flagged)
            .field("apps", &self.apps)
            .field("restricted", &restricted)
            .field("uses", &uses)
            .field("nodes", &self.nodes)
            .field("before", &before)
            .field("after", &after)
            .field("templates", &templates)
            .field("locales", &locales)
            .field("children", &self.children)
            .finish()
    }
}

/// The most characters a Module id may have.
const MAX_ID_LEN: usize = 64;

/// Why a Module id was rejected: the reason of
/// [`BuildViolation::InvalidModuleId`](crate::BuildViolation::InvalidModuleId).
///
/// The grammar reasons ([`Empty`](Self::Empty), [`TooLong`](Self::TooLong),
/// [`InvalidChar`](Self::InvalidChar), [`EmptySegment`](Self::EmptySegment))
/// apply to every mounted Module; the tree-position reasons
/// ([`PathAtRoot`](Self::PathAtRoot), [`NotUnderParent`](Self::NotUnderParent))
/// only when the id's grammar is valid. Indexes count characters.
///
/// ```
/// use noctis_core::ModuleIdError;
///
/// let err = ModuleIdError::InvalidChar { index: 4, ch: '.' };
/// assert_eq!(
///     err.to_string(),
///     "`.` at index 4 is not allowed: use only a-z, 0-9, `_` and `-`, with `/` between segments"
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ModuleIdError {
    /// The id is empty.
    ///
    /// ```
    /// use noctis_core::ModuleIdError;
    ///
    /// assert_eq!(
    ///     ModuleIdError::Empty.to_string(),
    ///     "the id is empty: use segments of a-z, 0-9, `_` and `-` joined by `/`, such as \
    ///      `profile/badges`"
    /// );
    /// ```
    #[error(
        "the id is empty: use segments of a-z, 0-9, `_` and `-` joined by `/`, such as \
         `profile/badges`"
    )]
    Empty,
    /// The id has more than 64 characters.
    ///
    /// ```
    /// use noctis_core::ModuleIdError;
    ///
    /// assert_eq!(
    ///     ModuleIdError::TooLong { len: 65 }.to_string(),
    ///     "the id has 65 characters: shorten it to at most 64"
    /// );
    /// ```
    #[error("the id has {len} characters: shorten it to at most {MAX_ID_LEN}")]
    TooLong {
        /// The number of characters.
        len: usize,
    },
    /// The id has a character outside `a-z`, `0-9`, `_`, `-` and the `/`
    /// separator.
    #[error(
        "`{ch}` at index {index} is not allowed: use only a-z, 0-9, `_` and `-`, with `/` \
         between segments"
    )]
    InvalidChar {
        /// The character index of the first invalid character.
        index: usize,
        /// The invalid character.
        ch: char,
    },
    /// A segment is empty: the id starts or ends with `/`, or has `//`.
    ///
    /// ```
    /// use noctis_core::ModuleIdError;
    ///
    /// assert_eq!(
    ///     ModuleIdError::EmptySegment { index: 2 }.to_string(),
    ///     "empty segment at index 2: separate segments with exactly one `/`, with none at the \
    ///      start or end"
    /// );
    /// ```
    #[error(
        "empty segment at index {index}: separate segments with exactly one `/`, with none at \
         the start or end"
    )]
    EmptySegment {
        /// The character index where the empty segment starts.
        index: usize,
    },
    /// A root Module (mounted with `AppBuilder::module`) has more than one
    /// segment.
    ///
    /// ```
    /// use noctis_core::ModuleIdError;
    ///
    /// assert_eq!(
    ///     ModuleIdError::PathAtRoot { segments: 2 }.to_string(),
    ///     "it is mounted at the root with 2 segments: a root id is one segment, so nest it \
    ///      with `.module(..)` in its parent Module"
    /// );
    /// ```
    #[error(
        "it is mounted at the root with {segments} segments: a root id is one segment, so nest \
         it with `.module(..)` in its parent Module"
    )]
    PathAtRoot {
        /// The number of segments of the id.
        segments: usize,
    },
    /// A nested Module's id is not its parent's id plus one segment.
    ///
    /// ```
    /// use noctis_core::ModuleIdError;
    ///
    /// assert_eq!(
    ///     ModuleIdError::NotUnderParent { parent: "profile" }.to_string(),
    ///     "it is nested in Module `profile`, so it must be `profile/<name>` with exactly one \
    ///      segment after `profile/`"
    /// );
    /// ```
    #[error(
        "it is nested in Module `{parent}`, so it must be `{parent}/<name>` with exactly one \
         segment after `{parent}/`"
    )]
    NotUnderParent {
        /// The id of the Module it is nested in.
        parent: &'static str,
    },
}

/// Checks the Module id grammar: 1..=64 characters, segments of `a-z`,
/// `0-9`, `_` and `-` joined by single `/`. The first problem left to right
/// wins, after the empty and length checks.
pub(crate) fn check_id(id: &str) -> Result<(), ModuleIdError> {
    if id.is_empty() {
        return Err(ModuleIdError::Empty);
    }
    let len = id.chars().count();
    if len > MAX_ID_LEN {
        return Err(ModuleIdError::TooLong { len });
    }
    // Whether the previous character was `/` (true before the first one, so
    // a leading `/` is an empty segment).
    let mut after_separator = true;
    for (index, ch) in id.chars().enumerate() {
        match ch {
            '/' if after_separator => return Err(ModuleIdError::EmptySegment { index }),
            '/' => after_separator = true,
            'a'..='z' | '0'..='9' | '_' | '-' => after_separator = false,
            _ => return Err(ModuleIdError::InvalidChar { index, ch }),
        }
    }
    if after_separator {
        return Err(ModuleIdError::EmptySegment { index: len });
    }
    Ok(())
}

/// Checks a grammar-valid id's tree position: a root (`parent` is `None`)
/// has one segment, a child is `<parent>/<one segment>`.
pub(crate) fn check_position(id: &str, parent: Option<&'static str>) -> Result<(), ModuleIdError> {
    match parent {
        None => {
            let segments = id.split('/').count();
            if segments == 1 {
                Ok(())
            } else {
                Err(ModuleIdError::PathAtRoot { segments })
            }
        }
        Some(parent) => {
            let under = id
                .strip_prefix(parent)
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|name| !name.is_empty() && !name.contains('/'));
            if under {
                Ok(())
            } else {
                Err(ModuleIdError::NotUnderParent { parent })
            }
        }
    }
}
