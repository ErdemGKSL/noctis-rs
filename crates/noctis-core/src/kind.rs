//! The hidden traits the kind derives (`SlashCommand`, `UserCommand`,
//! `MessageCommand`, `EventHandler`, `Button`, the select derives, `Modal`)
//! implement and `#[handler]` binds through (AD-6, AD-26). Not public API.
//!
//! A derive implements [`__Kind`] (the kind marker, the declared defer
//! mode, the descriptor type and the struct's name) and its family trait.
//! `#[handler]` classifies the method's parameters syntactically into a
//! shape ([`__C`], [`__CX`], [`__CR`], [`__CXR`]) and calls
//! [`__Bind::__bind`] of the kind marker with the method itself, so a
//! parameter of the wrong type fails with the expected type in the error.

use std::future::Future;
use std::marker::PhantomData;

use noctis_ui::{ResolvedMessage, ResolvedUser};

use crate::command::{CommandCtx, CommandHandler, CommandPath};
use crate::component::{
    ComponentCtx, ComponentHandler, ComponentKind, ModalHandler, ModalSubmitCtx,
};
use crate::events::{EventCtx, EventHandler, EventPayload};
use crate::handler::HandlerResult;
use crate::menu::ContextMenuHandler;
use crate::metadata::CommandMetadata;
use crate::module::{Descriptor, Module, sealed};
use crate::options::Options;
use crate::responder::{
    AutoDefer, Command, Component, DeferMode, Ephemerality, IntoResponse, Manual, ModalSubmit,
    Pending, Responder, ResponderKind,
};

/// A kind-derived struct: what `#[handler]` binds through.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` has no kind derive, so `#[handler]` cannot bind it",
    label = "not a kind-derived struct",
    note = "derive SlashCommand, UserCommand, MessageCommand, EventHandler, Button, a select kind or Modal on `{Self}`"
)]
pub trait __Kind: Sized + Send + Sync + 'static {
    /// The kind marker: [`__SlashKind`], [`__MenuKind`], [`__EventKind`],
    /// [`__ComponentKind`] or [`__ModalKind`].
    type Kind;
    /// The declared defer mode (`auto_defer = false` is [`Manual`]).
    type Defer: DeferMode;
    /// The descriptor `Module::add::<T>()` stores.
    type Descriptor;
    /// `concat!(module_path!(), "::", "<Struct>")`.
    const NAME: &'static str;
}

/// The kind marker of `#[derive(SlashCommand)]`.
#[doc(hidden)]
#[derive(Debug)]
pub enum __SlashKind {}
/// The kind marker of `#[derive(UserCommand)]` and
/// `#[derive(MessageCommand)]`.
#[doc(hidden)]
#[derive(Debug)]
pub enum __MenuKind {}
/// The kind marker of `#[derive(EventHandler)]`.
#[doc(hidden)]
#[derive(Debug)]
pub enum __EventKind {}
/// The kind marker of `#[derive(Button)]` and the select derives.
#[doc(hidden)]
#[derive(Debug)]
pub enum __ComponentKind {}
/// The kind marker of `#[derive(Modal)]`.
#[doc(hidden)]
#[derive(Debug)]
pub enum __ModalKind {}

/// The shape `(self, ctx)`.
#[doc(hidden)]
#[derive(Debug)]
pub struct __C;
/// The shape `(self, ctx, x: X)`: a path, a target or an event payload.
#[doc(hidden)]
#[derive(Debug)]
pub struct __CX<X>(PhantomData<X>);
/// The shape `(self, ctx, r: R)` with `R` a responder.
#[doc(hidden)]
#[derive(Debug)]
pub struct __CR<R>(PhantomData<R>);
/// The shape `(self, ctx, x: X, r: R)`.
#[doc(hidden)]
#[derive(Debug)]
pub struct __CXR<X, R>(PhantomData<(X, R)>);

/// Binds a `#[handler]` method of shape `Shape` of the struct `S`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "the parameters of this `#[handler]` method do not fit `{S}`",
    label = "unexpected parameters",
    note = "commands take `(self, ctx: CommandCtx[, path][, r: CommandResponder<..>])`, context menus `(self, ctx: CommandCtx, target[, r])`, events `(self, ctx: EventCtx, event)`, components `(self, ctx: ComponentCtx[, r: ComponentResponder<..>])` and modals `(self, ctx: ModalSubmitCtx[, r: ModalResponder<..>])`"
)]
pub trait __Bind<Shape, S: __Kind, F> {
    /// The descriptor of the method `f`.
    fn __bind(name: &'static str, f: F) -> S::Descriptor;
}

// --- Slash Commands ---------------------------------------------------------

/// A `#[derive(SlashCommand)]` struct: its `#[command("…", ..)]`
/// declaration.
#[doc(hidden)]
pub trait __SlashCommand:
    __Kind<Kind = __SlashKind, Descriptor = CommandHandler> + Options
{
    /// The Command Path or Pattern as written.
    const PATTERN: &'static str;
    /// Every Command Path, in expansion order.
    const PATHS: &'static [&'static str];
    /// The literal `description = "…"`.
    const DESCRIPTION: &'static str;
    /// The declared metadata.
    const METADATA: CommandMetadata;
    /// The declared `response`.
    const EPHEMERALITY: Ephemerality;
}

/// A slash Command whose Pattern has no group: its handler takes no path.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "this command's Pattern has a group: take `path: {Self}Path` after `ctx`",
    label = "the path parameter is missing",
    note = "`#[derive(SlashCommand)]` generates `{Self}Path` for a Pattern with a group"
)]
pub trait __SinglePath: __SlashCommand {}

/// A slash Command whose Pattern has a group: its handler takes `Path`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "this command has a single path: remove the path parameter",
    label = "no path parameter for a single Command Path"
)]
pub trait __PatternPaths: __SlashCommand {
    /// The generated `<Struct>Path` enum.
    type Path: CommandPath;
}

fn slash<S: __SlashCommand>(handler: CommandHandler) -> CommandHandler {
    handler
        .__with_metadata(S::METADATA)
        .__with_ephemerality(S::EPHEMERALITY)
}

impl<S, F, Fut, R> __Bind<__C, S, F> for __SlashKind
where
    S: __SinglePath,
    F: Fn(S, CommandCtx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Command>,
{
    fn __bind(name: &'static str, f: F) -> CommandHandler {
        slash::<S>(CommandHandler::__new_plain_with_options::<
            S,
            S::Defer,
            _,
            _,
            _,
        >(
            name,
            S::PATHS,
            S::DESCRIPTION,
            move |ctx: CommandCtx, _: Responder<Command, Pending, S::Defer>, s: S| f(s, ctx),
        ))
    }
}

impl<S, D, F, Fut, R> __Bind<__CR<Responder<Command, Pending, D>>, S, F> for __SlashKind
where
    S: __SinglePath,
    D: DeferMode,
    F: Fn(S, CommandCtx, Responder<Command, Pending, D>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Command>,
{
    fn __bind(name: &'static str, f: F) -> CommandHandler {
        slash::<S>(CommandHandler::__new_plain_with_options::<S, D, _, _, _>(
            name,
            S::PATHS,
            S::DESCRIPTION,
            move |ctx, r, s| f(s, ctx, r),
        ))
    }
}

impl<S, P, F, Fut, R> __Bind<__CX<P>, S, F> for __SlashKind
where
    S: __PatternPaths<Path = P>,
    P: CommandPath,
    F: Fn(S, CommandCtx, P) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Command>,
{
    fn __bind(name: &'static str, f: F) -> CommandHandler {
        slash::<S>(
            CommandHandler::__new_with_options::<P, S, S::Defer, _, _, _>(
                name,
                S::PATTERN,
                S::DESCRIPTION,
                move |ctx: CommandCtx, path: P, _: Responder<Command, Pending, S::Defer>, s: S| {
                    f(s, ctx, path)
                },
            ),
        )
    }
}

impl<S, P, D, F, Fut, R> __Bind<__CXR<P, Responder<Command, Pending, D>>, S, F> for __SlashKind
where
    S: __PatternPaths<Path = P>,
    P: CommandPath,
    D: DeferMode,
    F: Fn(S, CommandCtx, P, Responder<Command, Pending, D>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Command>,
{
    fn __bind(name: &'static str, f: F) -> CommandHandler {
        slash::<S>(CommandHandler::__new_with_options::<P, S, D, _, _, _>(
            name,
            S::PATTERN,
            S::DESCRIPTION,
            move |ctx, path, r, s| f(s, ctx, path, r),
        ))
    }
}

// --- Context menus ----------------------------------------------------------

/// A `#[derive(UserCommand)]` or `#[derive(MessageCommand)]` unit struct.
#[doc(hidden)]
pub trait __ContextMenu: __Kind<Kind = __MenuKind, Descriptor = ContextMenuHandler> {
    /// [`ResolvedUser`] or [`ResolvedMessage`].
    type Target: __MenuTarget;
    /// The menu name as written.
    const MENU_NAME: &'static str;
    /// The declared metadata.
    const METADATA: CommandMetadata;
    /// The declared `response`.
    const EPHEMERALITY: Ephemerality;
    /// The unit struct.
    fn __new() -> Self;
}

/// A context-menu target: what the descriptor of its kind is built with.
#[doc(hidden)]
pub trait __MenuTarget: Sized + Send + 'static {
    /// The descriptor of `f`.
    fn __handler<D, F, Fut, R>(
        name: &'static str,
        menu: &'static str,
        metadata: CommandMetadata,
        f: F,
    ) -> ContextMenuHandler
    where
        D: DeferMode,
        F: Fn(CommandCtx, Self, Responder<Command, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>;
}

impl __MenuTarget for ResolvedUser {
    fn __handler<D, F, Fut, R>(
        name: &'static str,
        menu: &'static str,
        metadata: CommandMetadata,
        f: F,
    ) -> ContextMenuHandler
    where
        D: DeferMode,
        F: Fn(CommandCtx, Self, Responder<Command, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        ContextMenuHandler::__new_user::<D, _, _, _>(name, menu, metadata, f)
    }
}

impl __MenuTarget for ResolvedMessage {
    fn __handler<D, F, Fut, R>(
        name: &'static str,
        menu: &'static str,
        metadata: CommandMetadata,
        f: F,
    ) -> ContextMenuHandler
    where
        D: DeferMode,
        F: Fn(CommandCtx, Self, Responder<Command, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Command>,
    {
        ContextMenuHandler::__new_message::<D, _, _, _>(name, menu, metadata, f)
    }
}

/// Never implemented: a context-menu handler takes its target.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "context-menu handlers take `(self, ctx: CommandCtx, target: ResolvedUser | ResolvedMessage[, r: CommandResponder<..>])`",
    label = "the target parameter is missing"
)]
pub trait __TakesTarget {}

impl<S, F> __Bind<__C, S, F> for __MenuKind
where
    S: __ContextMenu + __TakesTarget,
{
    fn __bind(_: &'static str, _: F) -> ContextMenuHandler {
        unreachable!("`__TakesTarget` is never implemented")
    }
}

impl<S, R, F> __Bind<__CR<R>, S, F> for __MenuKind
where
    S: __ContextMenu + __TakesTarget,
{
    fn __bind(_: &'static str, _: F) -> ContextMenuHandler {
        unreachable!("`__TakesTarget` is never implemented")
    }
}

impl<S, X, F, Fut, R> __Bind<__CX<X>, S, F> for __MenuKind
where
    S: __ContextMenu<Target = X>,
    X: __MenuTarget,
    F: Fn(S, CommandCtx, X) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Command>,
{
    fn __bind(name: &'static str, f: F) -> ContextMenuHandler {
        X::__handler::<S::Defer, _, _, _>(
            name,
            S::MENU_NAME,
            S::METADATA,
            move |ctx: CommandCtx, target: X, _: Responder<Command, Pending, S::Defer>| {
                f(S::__new(), ctx, target)
            },
        )
        .__with_ephemerality(S::EPHEMERALITY)
    }
}

impl<S, X, D, F, Fut, R> __Bind<__CXR<X, Responder<Command, Pending, D>>, S, F> for __MenuKind
where
    S: __ContextMenu<Target = X>,
    X: __MenuTarget,
    D: DeferMode,
    F: Fn(S, CommandCtx, X, Responder<Command, Pending, D>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Command>,
{
    fn __bind(name: &'static str, f: F) -> ContextMenuHandler {
        X::__handler::<D, _, _, _>(name, S::MENU_NAME, S::METADATA, move |ctx, target, r| {
            f(S::__new(), ctx, target, r)
        })
        .__with_ephemerality(S::EPHEMERALITY)
    }
}

// --- Events -----------------------------------------------------------------

/// A `#[derive(EventHandler)]` unit struct and its `#[event(..)]` flags.
#[doc(hidden)]
pub trait __EventHandler: __Kind<Kind = __EventKind, Descriptor = EventHandler> {
    /// `ordered`.
    const ORDERED: bool;
    /// `droppable`.
    const DROPPABLE: bool;
    /// The unit struct.
    fn __new() -> Self;
}

/// Never implemented: an event handler takes its payload and no responder.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "event handlers take `(self, ctx: EventCtx, event: <payload>)`",
    label = "not an event handler's parameters"
)]
pub trait __EventParams {}

impl<S, F> __Bind<__C, S, F> for __EventKind
where
    S: __EventHandler + __EventParams,
{
    fn __bind(_: &'static str, _: F) -> EventHandler {
        unreachable!("`__EventParams` is never implemented")
    }
}

impl<S, R, F> __Bind<__CR<R>, S, F> for __EventKind
where
    S: __EventHandler + __EventParams,
{
    fn __bind(_: &'static str, _: F) -> EventHandler {
        unreachable!("`__EventParams` is never implemented")
    }
}

impl<S, X, R, F> __Bind<__CXR<X, R>, S, F> for __EventKind
where
    S: __EventHandler + __EventParams,
{
    fn __bind(_: &'static str, _: F) -> EventHandler {
        unreachable!("`__EventParams` is never implemented")
    }
}

impl<S, E, F, Fut> __Bind<__CX<E>, S, F> for __EventKind
where
    S: __EventHandler,
    E: EventPayload,
    F: Fn(S, EventCtx, E) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<()>> + Send + 'static,
{
    fn __bind(name: &'static str, f: F) -> EventHandler {
        EventHandler::__new::<E, _, _>(
            name,
            S::ORDERED,
            S::DROPPABLE,
            move |ctx: EventCtx, event: E| f(S::__new(), ctx, event),
        )
    }
}

// --- Components and modals --------------------------------------------------

/// The declared `response = update | ephemeral | public` of a component
/// attribute (AD-11).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum __Response {
    /// `response = update` (the default): the auto-defer is type 6 and a
    /// returned message updates the clicked message.
    Update,
    /// `response = ephemeral`: a new ephemeral message.
    Ephemeral,
    /// `response = public`: a new public message.
    Public,
}

/// A `#[derive(Button)]` or select-derived struct.
#[doc(hidden)]
pub trait __Component: __Kind<Kind = __ComponentKind, Descriptor = ComponentHandler> {
    /// The Route id.
    const ROUTE: &'static str;
    /// The component kind.
    const KIND: ComponentKind;
    /// The declared `response`.
    const RESPONSE: __Response;
}

/// A `#[derive(Modal)]` struct.
#[doc(hidden)]
pub trait __Modal: __Kind<Kind = __ModalKind, Descriptor = ModalHandler> {
    /// The Route id.
    const ROUTE: &'static str;
}

/// Never implemented: a component handler takes its context and at most a
/// responder.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "component handlers take `(self, ctx: ComponentCtx[, r: ComponentResponder<..>])`",
    label = "an extra parameter"
)]
pub trait __ComponentParams {}

/// Never implemented: a modal handler takes its context and at most a
/// responder.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "modal handlers take `(self, ctx: ModalSubmitCtx[, r: ModalResponder<..>])`",
    label = "an extra parameter"
)]
pub trait __ModalParams {}

macro_rules! no_extra {
    ($kind:ty, $family:ident, $params:ident, $descriptor:ty) => {
        impl<S, X, F> __Bind<__CX<X>, S, F> for $kind
        where
            S: $family + $params,
        {
            fn __bind(_: &'static str, _: F) -> $descriptor {
                unreachable!("never implemented")
            }
        }

        impl<S, X, R, F> __Bind<__CXR<X, R>, S, F> for $kind
        where
            S: $family + $params,
        {
            fn __bind(_: &'static str, _: F) -> $descriptor {
                unreachable!("never implemented")
            }
        }
    };
}

no_extra!(
    __ComponentKind,
    __Component,
    __ComponentParams,
    ComponentHandler
);
no_extra!(__ModalKind, __Modal, __ModalParams, ModalHandler);

impl<S, F, Fut, R> __Bind<__C, S, F> for __ComponentKind
where
    S: __Component
        + noctis_codec::Route<Payload = S>
        + for<'de> noctis_codec::PayloadDecode<'de>
        + Send
        + 'static,
    F: Fn(S, ComponentCtx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Component>,
{
    fn __bind(name: &'static str, f: F) -> ComponentHandler {
        let route = <S as noctis_codec::Route>::ID.as_str();
        ComponentHandler::__new_with_responder::<S, S::Defer, _, _, _>(
            name,
            route,
            S::KIND,
            S::RESPONSE,
            move |s: S, ctx: ComponentCtx, _: Responder<Component, Pending, S::Defer>| f(s, ctx),
        )
    }
}

impl<S, D, F, Fut, R> __Bind<__CR<Responder<Component, Pending, D>>, S, F> for __ComponentKind
where
    S: __Component
        + noctis_codec::Route<Payload = S>
        + for<'de> noctis_codec::PayloadDecode<'de>
        + Send
        + 'static,
    D: DeferMode,
    F: Fn(S, ComponentCtx, Responder<Component, Pending, D>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Component>,
{
    fn __bind(name: &'static str, f: F) -> ComponentHandler {
        let route = <S as noctis_codec::Route>::ID.as_str();
        ComponentHandler::__new_with_responder::<S, D, _, _, _>(
            name,
            route,
            S::KIND,
            S::RESPONSE,
            f,
        )
    }
}

impl<S, F, Fut, R> __Bind<__C, S, F> for __ModalKind
where
    S: __Modal + noctis_codec::Route + noctis_ui::ModalForm + Send + 'static,
    for<'de> <S as noctis_codec::Route>::Payload: noctis_codec::PayloadDecode<'de>,
    F: Fn(S, ModalSubmitCtx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<ModalSubmit>,
{
    fn __bind(name: &'static str, f: F) -> ModalHandler {
        let route = <S as noctis_codec::Route>::ID.as_str();
        ModalHandler::__new_with_responder::<S, S::Defer, _, _, _>(
            name,
            route,
            move |s: S, ctx: ModalSubmitCtx, _: Responder<ModalSubmit, Pending, S::Defer>| {
                f(s, ctx)
            },
        )
    }
}

impl<S, D, F, Fut, R> __Bind<__CR<Responder<ModalSubmit, Pending, D>>, S, F> for __ModalKind
where
    S: __Modal + noctis_codec::Route + noctis_ui::ModalForm + Send + 'static,
    for<'de> <S as noctis_codec::Route>::Payload: noctis_codec::PayloadDecode<'de>,
    D: DeferMode,
    F: Fn(S, ModalSubmitCtx, Responder<ModalSubmit, Pending, D>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<ModalSubmit>,
{
    fn __bind(name: &'static str, f: F) -> ModalHandler {
        let route = <S as noctis_codec::Route>::ID.as_str();
        ModalHandler::__new_with_responder::<S, D, _, _, _>(name, route, f)
    }
}

// --- INT-05 -----------------------------------------------------------------

/// The declared defer modes a handler without a responder may have.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-05] `auto_defer = false` handlers take a `Manual` responder",
    label = "declared `auto_defer = false`"
)]
pub trait __NoResponder {}

impl __NoResponder for AutoDefer {}

/// `Self` (the declared mode) accepts the responder `R`: an auto-deferred
/// handler takes any, a `Manual` one only a `Manual` responder.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-05] `auto_defer = false` is declared, so the handler must take the `Manual` responder (`CommandResponder<Manual>` or `ComponentResponder<Manual>`)",
    label = "take the `Manual` responder here"
)]
pub trait __ManualTakesManual<R> {}

impl<R> __ManualTakesManual<R> for AutoDefer {}
impl<K: ResponderKind> __ManualTakesManual<Responder<K, Pending, Manual>> for Manual {}

/// The responder `Self` fits the declared mode `D`: a `Manual` responder
/// needs `auto_defer = false`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-05] this handler takes a `Manual` responder, so its attribute must declare `auto_defer = false`",
    label = "declare `auto_defer = false` or take the auto-deferred responder"
)]
pub trait __ManualIsDeclared<D> {}

impl<K: ResponderKind, D> __ManualIsDeclared<D> for Responder<K, Pending, AutoDefer> {}
impl<K: ResponderKind> __ManualIsDeclared<Manual> for Responder<K, Pending, Manual> {}

/// What `#[handler]` asserts for a method without a responder (INT-05).
#[doc(hidden)]
pub fn __assert_no_responder<S>()
where
    S: __Kind,
    S::Defer: __NoResponder,
{
}

/// What `#[handler]` asserts for a method taking the responder `R`
/// (INT-05).
#[doc(hidden)]
pub fn __assert_responder<S, R>()
where
    S: __Kind,
    S::Defer: __ManualTakesManual<R>,
    R: __ManualIsDeclared<S::Defer>,
{
}

// --- Registration -------------------------------------------------------------

/// The fallback of a kind-derived struct without a `#[handler]` method: an
/// inherent item of the same name (which `#[handler]` emits) shadows it, so
/// the derive's missing-handler check reads `false` and registration gets an
/// [`__Unbound`] (CID-04).
#[doc(hidden)]
pub trait __NoHandler {
    /// Whether `#[handler]` marked a method.
    const __NOCTIS_HAS_HANDLER: bool = false;

    /// The stand-in descriptor.
    #[must_use]
    fn descriptor() -> __Unbound {
        __Unbound
    }
}

impl<T: ?Sized> __NoHandler for T {}

/// The descriptor of a struct without a handler; it registers nothing (the
/// derive's check fails compilation first).
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct __Unbound;

impl sealed::Sealed for __Unbound {
    fn register(self, _: &mut Module) {}
}

impl Descriptor for __Unbound {}
