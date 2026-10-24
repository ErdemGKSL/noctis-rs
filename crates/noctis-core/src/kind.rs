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

use std::any::TypeId;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;

use noctis_ui::{ResolvedMessage, ResolvedUser};

use crate::command::{CommandCtx, CommandHandler, CommandPath};
use crate::component::{
    __Absent, __ExpiredHook, __StaleHook, ComponentCtx, ComponentHandler, ComponentKind, Expired,
    ModalHandler, ModalSubmitCtx,
};
use crate::events::{EventCtx, EventHandler, EventPayload};
use crate::handler::{HandlerError, HandlerResult};
use crate::menu::ContextMenuHandler;
use crate::metadata::CommandMetadata;
use crate::middleware::{Ext, ExtensionMissing, Extensions};
use crate::module::{Descriptor, Module, sealed};
use crate::options::Options;
use crate::rate_limits::{__RateLimitedHook, RateLimit};
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
    /// The description of every path, parallel to `PATHS`: the literal
    /// `description = "…"` or `cmd.<path>.description` from the
    /// default-locale file.
    const DESCRIPTIONS: &'static [&'static str];
    /// The declared metadata.
    const METADATA: CommandMetadata;
    /// The declared `response`.
    const EPHEMERALITY: Ephemerality;
    /// The declared `rate_limit(..)`s, in declared order (INT-14).
    const RATE_LIMITS: &'static [RateLimit] = &[];
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

/// The first path's description, which the constructors take.
fn first_description<S: __SlashCommand>() -> &'static str {
    S::DESCRIPTIONS.first().copied().unwrap_or_default()
}

fn slash<S: __SlashCommand>(handler: CommandHandler) -> CommandHandler {
    handler
        .__with_descriptions(S::DESCRIPTIONS)
        .__with_metadata(S::METADATA)
        .__with_ephemerality(S::EPHEMERALITY)
        .__with_rate_limits(S::RATE_LIMITS)
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
            first_description::<S>(),
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
            first_description::<S>(),
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
                first_description::<S>(),
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
            first_description::<S>(),
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
    /// The declared `key = "…"`: the `ctxcmd.<key>.name` locale key
    /// segment, if any.
    const KEY: Option<&'static str> = None;
    /// The declared `rate_limit(..)`s, in declared order (INT-14).
    const RATE_LIMITS: &'static [RateLimit] = &[];
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
        .with_key_opt(S::KEY)
        .__with_rate_limits(S::RATE_LIMITS)
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
        .with_key_opt(S::KEY)
        .__with_rate_limits(S::RATE_LIMITS)
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

/// The declared `response = update | ephemeral | public` of a component or
/// `#[modal]` attribute (AD-11).
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
    /// The declared `rate_limit(..)`s, in declared order (INT-14).
    const RATE_LIMITS: &'static [RateLimit] = &[];
}

/// A `#[derive(Modal)]` struct.
#[doc(hidden)]
pub trait __Modal: __Kind<Kind = __ModalKind, Descriptor = ModalHandler> {
    /// The Route id.
    const ROUTE: &'static str;
    /// The declared `response` of the submit.
    const RESPONSE: __Response;
    /// The declared `rate_limit(..)`s, in declared order (INT-14).
    const RATE_LIMITS: &'static [RateLimit] = &[];
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
        + noctis_codec::DeclaredRuntime
        + for<'de> noctis_codec::PayloadDecode<'de>
        + crate::guard::__Guards<ComponentCtx>
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
        .__with_rate_limits(S::RATE_LIMITS)
    }
}

impl<S, D, F, Fut, R> __Bind<__CR<Responder<Component, Pending, D>>, S, F> for __ComponentKind
where
    S: __Component
        + noctis_codec::Route<Payload = S>
        + noctis_codec::DeclaredRuntime
        + for<'de> noctis_codec::PayloadDecode<'de>
        + crate::guard::__Guards<ComponentCtx>
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
        .__with_rate_limits(S::RATE_LIMITS)
    }
}

impl<S, F, Fut, R> __Bind<__C, S, F> for __ModalKind
where
    S: __Modal
        + noctis_ui::__macro::ModalFromPayload
        + noctis_codec::DeclaredRuntime
        + crate::guard::__Guards<ModalSubmitCtx>
        + Send
        + 'static,
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
            S::RESPONSE,
            move |s: S, ctx: ModalSubmitCtx, _: Responder<ModalSubmit, Pending, S::Defer>| {
                f(s, ctx)
            },
        )
        .__with_rate_limits(S::RATE_LIMITS)
    }
}

impl<S, D, F, Fut, R> __Bind<__CR<Responder<ModalSubmit, Pending, D>>, S, F> for __ModalKind
where
    S: __Modal
        + noctis_ui::__macro::ModalFromPayload
        + noctis_codec::DeclaredRuntime
        + crate::guard::__Guards<ModalSubmitCtx>
        + Send
        + 'static,
    for<'de> <S as noctis_codec::Route>::Payload: noctis_codec::PayloadDecode<'de>,
    D: DeferMode,
    F: Fn(S, ModalSubmitCtx, Responder<ModalSubmit, Pending, D>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<ModalSubmit>,
{
    fn __bind(name: &'static str, f: F) -> ModalHandler {
        let route = <S as noctis_codec::Route>::ID.as_str();
        ModalHandler::__new_with_responder::<S, D, _, _, _>(name, route, S::RESPONSE, f)
            .__with_rate_limits(S::RATE_LIMITS)
    }
}

// --- `#[on_stale]` and `#[on_expired]` -------------------------------------

/// The hook shape `(ctx: C)`.
#[doc(hidden)]
#[derive(Debug)]
pub struct __H<C>(PhantomData<C>);
/// The hook shape `(ctx: C, r: R)` (or a second parameter of another type).
#[doc(hidden)]
#[derive(Debug)]
pub struct __HR<C, R>(PhantomData<(C, R)>);
/// The hook shape `(ctx: C, expired: E)`.
#[doc(hidden)]
#[derive(Debug)]
pub struct __HE<C, E>(PhantomData<(C, E)>);
/// The hook shape `(ctx: C, expired: E, r: R)`.
#[doc(hidden)]
#[derive(Debug)]
pub struct __HER<C, E, R>(PhantomData<(C, E, R)>);

/// The context the `#[on_stale]`/`#[on_expired]` methods of a kind's
/// structs take: [`ComponentCtx`], [`ModalSubmitCtx`], or [`__NoHookCtx`]
/// for the kinds that have no hooks (their binding fails with
/// [`__NotAHookStruct`]).
#[doc(hidden)]
pub trait __HookKind {
    /// The hook context.
    type Ctx;
}

/// The hook context of a command, context-menu or event kind: none.
#[doc(hidden)]
#[derive(Debug)]
pub enum __NoHookCtx {}

impl __HookKind for __ComponentKind {
    type Ctx = ComponentCtx;
}

impl __HookKind for __ModalKind {
    type Ctx = ModalSubmitCtx;
}

/// Never implemented: only component and modal structs take hooks.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`#[on_stale]` and `#[on_expired]` are methods of a component or modal struct",
    label = "not a component or modal struct"
)]
pub trait __NotAHookStruct {}

/// Binds an `#[on_stale]` method of shape `Shape` of the struct `S`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "the parameters of this `#[on_stale]`/`#[on_expired]` method do not fit `{S}`",
    label = "unexpected parameters",
    note = "components take `#[on_stale] async fn ..(ctx: ComponentCtx[, r: ComponentResponder<..>])` and `#[on_expired] async fn ..(ctx: ComponentCtx, expired: Expired[, r: ComponentResponder<..>])`; modals take `#[on_stale] async fn ..(ctx: ModalSubmitCtx[, r: ModalResponder<..>])` and `#[on_expired] async fn ..(ctx: ModalSubmitCtx, expired: Expired[, r: ModalResponder<..>])`"
)]
pub trait __BindStale<Shape, S, F>: __HookKind {
    /// The hook of the method `f`.
    fn __bind(f: F) -> __StaleHook<Self::Ctx>;
}

/// Binds an `#[on_expired]` method of shape `Shape` of the struct `S`.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "the parameters of this `#[on_stale]`/`#[on_expired]` method do not fit `{S}`",
    label = "unexpected parameters",
    note = "components take `#[on_stale] async fn ..(ctx: ComponentCtx[, r: ComponentResponder<..>])` and `#[on_expired] async fn ..(ctx: ComponentCtx, expired: Expired[, r: ComponentResponder<..>])`; modals take `#[on_stale] async fn ..(ctx: ModalSubmitCtx[, r: ModalResponder<..>])` and `#[on_expired] async fn ..(ctx: ModalSubmitCtx, expired: Expired[, r: ModalResponder<..>])`"
)]
pub trait __BindExpired<Shape, S, F>: __HookKind {
    /// The hook of the method `f`.
    fn __bind(f: F) -> __ExpiredHook<Self::Ctx>;
}

/// The four hook bindings of a kind whose structs implement `$family`, take
/// `$ctx` and answer through responders of `$rk`.
macro_rules! hook_binds {
    ($kind:ty, $family:ident, $ctx:ty, $rk:ty) => {
        impl<S, F, Fut, R> __BindStale<__H<$ctx>, S, F> for $kind
        where
            S: $family,
            F: Fn($ctx) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = HandlerResult<R>> + Send + 'static,
            R: IntoResponse<$rk>,
        {
            fn __bind(f: F) -> __StaleHook<$ctx> {
                __StaleHook::__new::<S::Defer, _, _, _>(
                    move |ctx: $ctx, _: Responder<$rk, Pending, S::Defer>| f(ctx),
                )
            }
        }

        impl<S, D, F, Fut, R> __BindStale<__HR<$ctx, Responder<$rk, Pending, D>>, S, F> for $kind
        where
            S: $family,
            D: DeferMode,
            F: Fn($ctx, Responder<$rk, Pending, D>) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = HandlerResult<R>> + Send + 'static,
            R: IntoResponse<$rk>,
        {
            fn __bind(f: F) -> __StaleHook<$ctx> {
                __StaleHook::__new::<D, _, _, _>(f)
            }
        }

        impl<S, F, Fut, R> __BindExpired<__HE<$ctx, Expired>, S, F> for $kind
        where
            S: $family,
            F: Fn($ctx, Expired) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = HandlerResult<R>> + Send + 'static,
            R: IntoResponse<$rk>,
        {
            fn __bind(f: F) -> __ExpiredHook<$ctx> {
                __ExpiredHook::__new::<S::Defer, _, _, _>(
                    move |ctx: $ctx, expired: Expired, _: Responder<$rk, Pending, S::Defer>| {
                        f(ctx, expired)
                    },
                )
            }
        }

        impl<S, D, F, Fut, R> __BindExpired<__HER<$ctx, Expired, Responder<$rk, Pending, D>>, S, F>
            for $kind
        where
            S: $family,
            D: DeferMode,
            F: Fn($ctx, Expired, Responder<$rk, Pending, D>) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = HandlerResult<R>> + Send + 'static,
            R: IntoResponse<$rk>,
        {
            fn __bind(f: F) -> __ExpiredHook<$ctx> {
                __ExpiredHook::__new::<D, _, _, _>(f)
            }
        }
    };
}

hook_binds!(__ComponentKind, __Component, ComponentCtx, Component);
hook_binds!(__ModalKind, __Modal, ModalSubmitCtx, ModalSubmit);

/// The hook bindings of a kind without hooks: every shape fails with
/// [`__NotAHookStruct`].
macro_rules! no_hooks {
    ($($kind:ty),*) => {$(
        impl __HookKind for $kind {
            type Ctx = __NoHookCtx;
        }

        impl<Shape, S, F> __BindStale<Shape, S, F> for $kind
        where
            S: __NotAHookStruct,
        {
            fn __bind(_: F) -> __StaleHook<__NoHookCtx> {
                unreachable!("`__NotAHookStruct` is never implemented")
            }
        }

        impl<Shape, S, F> __BindExpired<Shape, S, F> for $kind
        where
            S: __NotAHookStruct,
        {
            fn __bind(_: F) -> __ExpiredHook<__NoHookCtx> {
                unreachable!("`__NotAHookStruct` is never implemented")
            }
        }
    )*};
}

no_hooks!(__SlashKind, __MenuKind, __EventKind);

/// The fallback of a struct without an `#[on_stale]` or `#[on_expired]`
/// method: the inherent items of the same names (which the attributes
/// emit) shadow it, so `#[handler]`'s `descriptor()` attaches exactly the
/// hooks the struct has.
#[doc(hidden)]
pub trait __NoHooks {
    /// No `#[on_stale]` method.
    fn __noctis_on_stale() -> __Absent {
        __Absent
    }

    /// No `#[on_expired]` method.
    fn __noctis_on_expired() -> __Absent {
        __Absent
    }

    /// No `#[on_rate_limited]` method.
    fn __noctis_on_rate_limited() -> __Absent {
        __Absent
    }
}

impl<T: ?Sized> __NoHooks for T {}

/// Attaches the hooks `St` and `Ex` (each a hook or [`__Absent`]) to a
/// descriptor; a command, context-menu or event descriptor takes none.
#[doc(hidden)]
pub trait __AttachHooks<St, Ex>: Sized {
    /// The descriptor with the hooks attached.
    #[must_use]
    fn __attach(self, on_stale: St, on_expired: Ex) -> Self;
}

/// What a command, context-menu or event descriptor attaches: nothing (a
/// hook on such a struct already failed with [`__NotAHookStruct`]).
#[doc(hidden)]
pub trait __NoHookSlot {}

impl __NoHookSlot for __Absent {}
impl __NoHookSlot for __StaleHook<__NoHookCtx> {}
impl __NoHookSlot for __ExpiredHook<__NoHookCtx> {}

macro_rules! attach_nothing {
    ($($descriptor:ty),*) => {$(
        impl<St: __NoHookSlot, Ex: __NoHookSlot> __AttachHooks<St, Ex> for $descriptor {
            fn __attach(self, _: St, _: Ex) -> Self {
                self
            }
        }
    )*};
}

attach_nothing!(CommandHandler, ContextMenuHandler, EventHandler);

macro_rules! attach_none {
    ($($descriptor:ty),*) => {$(
        impl __AttachHooks<__Absent, __Absent> for $descriptor {
            fn __attach(self, _: __Absent, _: __Absent) -> Self {
                self
            }
        }
    )*};
}

attach_none!(ComponentHandler, ModalHandler);

macro_rules! attach_hooks {
    ($descriptor:ty, $ctx:ty) => {
        impl __AttachHooks<__StaleHook<$ctx>, __Absent> for $descriptor {
            fn __attach(self, on_stale: __StaleHook<$ctx>, _: __Absent) -> Self {
                self.__with_on_stale(on_stale)
            }
        }

        impl __AttachHooks<__Absent, __ExpiredHook<$ctx>> for $descriptor {
            fn __attach(self, _: __Absent, on_expired: __ExpiredHook<$ctx>) -> Self {
                self.__with_on_expired(on_expired)
            }
        }

        impl __AttachHooks<__StaleHook<$ctx>, __ExpiredHook<$ctx>> for $descriptor {
            fn __attach(
                self,
                on_stale: __StaleHook<$ctx>,
                on_expired: __ExpiredHook<$ctx>,
            ) -> Self {
                self.__with_on_stale(on_stale).__with_on_expired(on_expired)
            }
        }
    };
}

attach_hooks!(ComponentHandler, ComponentCtx);
attach_hooks!(ModalHandler, ModalSubmitCtx);

// --- `#[on_rate_limited]` (INT-14) ------------------------------------------

/// Attaches the `#[on_rate_limited]` method `H` (a [`__RateLimitedHook`] or
/// [`__Absent`]) to a descriptor; an event descriptor takes none.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-14] #[on_rate_limited] needs a Command, context-menu, component or modal struct",
    label = "this struct has no rate limits",
    note = "event handlers are never rate-limited: remove the `#[on_rate_limited]` method"
)]
pub trait __AttachRateLimited<H>: Sized {
    /// The descriptor with the method attached.
    #[must_use]
    fn __attach_rate_limited(self, hook: H) -> Self;
}

macro_rules! attach_rate_limited {
    ($($descriptor:ty),*) => {$(
        impl __AttachRateLimited<__Absent> for $descriptor {
            fn __attach_rate_limited(self, _: __Absent) -> Self {
                self
            }
        }

        impl __AttachRateLimited<__RateLimitedHook> for $descriptor {
            fn __attach_rate_limited(self, hook: __RateLimitedHook) -> Self {
                self.__with_on_rate_limited(hook)
            }
        }
    )*};
}

attach_rate_limited!(
    CommandHandler,
    ContextMenuHandler,
    ComponentHandler,
    ModalHandler
);

/// What an event descriptor attaches: nothing. Generic over the slot, so a
/// `#[on_rate_limited]` method on an event struct fails on this bound with
/// the INT-14 message instead of a type mismatch.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "[INT-14] #[on_rate_limited] needs a Command, context-menu, component or modal struct",
    label = "an event handler has no rate limits",
    note = "event handlers are never rate-limited: remove the `#[on_rate_limited]` method"
)]
pub trait __NoRateLimitedSlot {}

impl __NoRateLimitedSlot for __Absent {}

impl<H: __NoRateLimitedSlot> __AttachRateLimited<H> for EventHandler {
    fn __attach_rate_limited(self, _: H) -> Self {
        self
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
    message = "[INT-05] `auto_defer = false` is declared, so the handler must take the `Manual` responder (`CommandResponder<Manual>`, `ComponentResponder<Manual>` or `ModalResponder<Manual>`)",
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
    fn register(self, _: &mut Module, _: &'static [&'static str], _: &'static [&'static str]) {}
}

impl Descriptor for __Unbound {}

// --- Extensions (`Ext<T>` parameters) -----------------------------------------

/// One Extension type a `#[handler]` method requires (an `Ext<T>`
/// parameter): its `TypeId` and type name, as functions so the list is a
/// `const`.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct __ExtType {
    id: fn() -> TypeId,
    name: fn() -> &'static str,
}

impl __ExtType {
    /// The Extension type `T`.
    pub const fn of<T: Send + Sync + 'static>() -> Self {
        Self {
            id: TypeId::of::<T>,
            name: std::any::type_name::<T>,
        }
    }

    /// Its `TypeId`.
    pub(crate) fn id(&self) -> TypeId {
        (self.id)()
    }

    /// Its type name.
    pub(crate) fn name(&self) -> &'static str {
        (self.name)()
    }
}

impl fmt::Debug for __ExtType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("__ExtType").field(&self.name()).finish()
    }
}

/// Sets the Extension types a descriptor's handler requires; what
/// `#[handler]` wraps a descriptor in when the method takes `Ext<T>`.
#[doc(hidden)]
pub trait __RequireExtensions: Sized {
    /// The descriptor requiring `types`.
    #[must_use]
    fn __require_extensions(self, types: &'static [__ExtType]) -> Self;
}

macro_rules! require_extensions {
    ($($descriptor:ty),*) => {$(
        impl __RequireExtensions for $descriptor {
            fn __require_extensions(self, types: &'static [__ExtType]) -> Self {
                self.__with_requires(types)
            }
        }
    )*};
}

require_extensions!(
    CommandHandler,
    ContextMenuHandler,
    EventHandler,
    ComponentHandler,
    ModalHandler
);

/// A handler context that carries its run's frozen Extensions.
#[doc(hidden)]
pub trait __Extensions {
    /// The frozen Extensions (an empty map while no chain froze one).
    fn __extensions(&self) -> &Extensions;
}

/// The `Ext<T>` parameter of the handler `handler`: an
/// [`ExtensionMissing`] error when no Middleware inserted `T`.
#[doc(hidden)]
pub fn __ext<T, C>(ctx: &C, handler: &'static str) -> HandlerResult<Ext<T>>
where
    T: Send + Sync + 'static,
    C: __Extensions,
{
    ctx.__extensions()
        .ext::<T>()
        .ok_or_else(|| HandlerError::new(ExtensionMissing::of::<T>(handler)))
}

/// The `Option<Ext<T>>` parameter of a handler.
#[doc(hidden)]
pub fn __ext_opt<T, C>(ctx: &C) -> Option<Ext<T>>
where
    T: Send + Sync + 'static,
    C: __Extensions,
{
    ctx.__extensions().ext::<T>()
}
