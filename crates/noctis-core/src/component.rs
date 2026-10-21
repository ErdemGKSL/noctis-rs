//! Components and modals declared as structs (AD-6, AD-19, FR-58): the
//! [`ComponentCtx`] and [`ModalSubmitCtx`] their `#[handler]` methods
//! receive, and the [`ComponentHandler`] and [`ModalHandler`] descriptors
//! `Module::add::<T>()` stores for them.
//!
//! `App::build` folds every mounted descriptor into one Route map keyed by
//! Route id, and a click or a modal submit runs the handler of its Route on
//! the decoded struct with a [`ComponentResponder`](crate::ComponentResponder)
//! or [`ModalResponder`](crate::ModalResponder) (Stories 7.1 and 7.2); the
//! declared `response` and `auto_defer` shape the auto-defer and a returned
//! message.
//!
//! A Ref id is resolved through the App's `RefStore` first (Story 7.5,
//! AD-23 stage 2, FR-17): the handler runs on the stored record. An id that
//! is stale, or whose Ref is missing, never reaches the handler (Story 7.3,
//! FR-18, FR-60): it runs the Route's `#[on_stale]` or `#[on_expired]`
//! method (bound into the descriptor beside the handler), else the App's
//! `expired_handler` with an [`ExpiredCtx`], else the default
//! `noctis.expired` notice; [`Expired`] says why.

use std::any::Any;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use noctis_codec::{
    DeclaredRuntime, DecodeError, Decoded, PayloadDecode, PayloadSource, Ref, RefRecord, Route,
    Verify, decode_record,
};
use noctis_rules::Limits;
use noctis_ui::__macro::ModalFromPayload;
use noctis_ui::{MessageFormat, ModalSubmission, SubmitError};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::State;
use crate::command::CtxInner;
use crate::guard::{__Guards, GuardDenied};
use crate::handler::{HandlerError, HandlerResult};
use crate::kind::__Response;
use crate::pipeline::component::Rejection;
use crate::responder::{
    AutoDefer, Component, Declared, DeferMode, Ephemerality, GuardPhase, IntoResponse, ModalSubmit,
    Pending, Responded, Responder, ResponderKind, Response, Shared, SourceMessage, Transport,
};

/// The kind of a message component a [`ComponentHandler`] answers.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentKind {
    /// `#[derive(Button)]` (component type 2).
    Button,
    /// `#[derive(StringSelectMenu)]` (type 3).
    StringSelect,
    /// `#[derive(UserSelectMenu)]` (type 5).
    UserSelect,
    /// `#[derive(RoleSelectMenu)]` (type 6).
    RoleSelect,
    /// `#[derive(MentionableSelectMenu)]` (type 7).
    MentionableSelect,
    /// `#[derive(ChannelSelectMenu)]` (type 8).
    ChannelSelect,
}

/// The accessors [`ComponentCtx`] and [`ModalSubmitCtx`] share.
macro_rules! ctx_accessors {
    ($ty:ident $(, $extra:ident = $field:ident)*) => {
        impl $ty {
            /// The id of the Module the handler was added to.
            pub fn module(&self) -> &'static str {
                self.0.module
            }

            /// The number of the shard that received the interaction.
            pub fn shard(&self) -> u32 {
                self.0.shard
            }

            /// The guild the interaction came from, if any.
            pub fn guild_id(&self) -> Option<Id<GuildMarker>> {
                self.0.guild_id
            }

            /// The channel the interaction came from, if Discord sent it.
            pub fn channel_id(&self) -> Option<Id<ChannelMarker>> {
                self.0.channel_id
            }

            /// The user who clicked or submitted.
            pub fn user_id(&self) -> Id<UserMarker> {
                self.0.user_id
            }

            /// The member's permissions in the channel, including
            /// overwrites, as Discord resolved them; `None` in DMs.
            pub fn member_permissions(&self) -> Option<crate::Permissions> {
                self.0.member_permissions
            }

            /// The user's client locale (`locale`), such as `"tr"`.
            pub fn user_locale(&self) -> Option<&str> {
                self.0.user_locale.as_deref()
            }

            /// The guild's preferred locale (`guild_locale`), outside DMs.
            pub fn guild_locale(&self) -> Option<&str> {
                self.0.guild_locale.as_deref()
            }

            /// The locale the response should use (FR-43): for a handler
            /// declared `response = ephemeral`, or one updating an
            /// ephemeral message under `response = update`, the User
            /// Locale, then the Guild Locale; for a public response (or no
            /// source message) the Guild Locale, then the User Locale;
            /// `"en-US"` when neither is known.
            pub fn response_locale(&self) -> &str {
                self.0.response_locale()
            }

            /// The Ref the clicked or submitted `custom_id` carried (a
            /// `by_ref` Route, or a spilled `overflow = spill` one); `None`
            /// for an inline id. Return it in a
            /// [`RefOps`](crate::RefOps) to delete its record after the
            /// response is sent.
            pub fn current_ref(&self) -> Option<Ref> {
                self.0.current_ref
            }

            /// The state registered for `T` with `AppBuilder::state`, if any.
            pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
                self.0.app.as_ref()?.states.get::<T>()
            }

            /// The App's typed key-value handle over its namespaced
            /// `CacheStore` (a cheap clone).
            pub fn kv(&self) -> crate::Kv {
                crate::Kv::of(self.0.app.as_ref())
            }

            /// A detached context (the `__testing` seams): no App, shard 0,
            /// user 1.
            #[allow(dead_code, reason = "`ExpiredCtx` has no detached seam outside the tests")]
            pub(crate) fn detached() -> Self {
                Self(Arc::new(CtxInner::detached_parts(Ephemerality::Public)))
            }

            /// A detached context bound to `shared`, whose responses are
            /// `ephemerality` (the `__testing` seams).
            #[allow(dead_code, reason = "`ExpiredCtx` has no detached seam")]
            pub(crate) fn detached_bound(shared: &Arc<Shared>, ephemerality: Ephemerality) -> Self {
                Self(Arc::new(CtxInner {
                    shared: Some(Arc::clone(shared)),
                    ..CtxInner::detached_parts(ephemerality)
                }))
            }

            /// A context over the shared `parts` (the component pipeline,
            /// which keeps them for the Expired Handler chain).
            pub(crate) fn from_inner(parts: Arc<CtxInner>) -> Self {
                Self(parts)
            }
        }

        impl fmt::Debug for $ty {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($ty))
                    .field("module", &self.0.module)
                    .field("shard", &self.0.shard)
                    .field("guild_id", &self.0.guild_id)
                    .field("channel_id", &self.0.channel_id)
                    .field("user_id", &self.0.user_id)
                    .field("user_locale", &self.0.user_locale)
                    .field("guild_locale", &self.0.guild_locale)
                    $(.field(stringify!($extra), &self.0.$field))*
                    .finish_non_exhaustive()
            }
        }
    };
}

/// The context a component's `#[handler]` method receives: the Module, the
/// shard, the guild, channel and user, the locales and the App's typed
/// state. The struct value itself (`self`) is the decoded Payload.
///
/// Cheap to clone (one `Arc`). Answer through a
/// [`ComponentResponder`](crate::ComponentResponder) parameter, or return
/// a message: it updates the clicked message under the default
/// `response = update`.
#[derive(Clone)]
pub struct ComponentCtx(Arc<CtxInner>);

ctx_accessors!(ComponentCtx);
crate::responder::ergonomics::ctx_methods!(ComponentCtx, update = Component);

impl crate::kind::__Extensions for ComponentCtx {
    fn __extensions(&self) -> &crate::Extensions {
        self.0.extensions()
    }
}

/// The context a modal's `#[handler]` method receives; the same accessors
/// as [`ComponentCtx`]. The struct value itself (`self`) is the decoded
/// Payload plus the submitted inputs.
///
/// Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct ModalSubmitCtx(Arc<CtxInner>);

ctx_accessors!(ModalSubmitCtx);
crate::responder::ergonomics::ctx_methods!(ModalSubmitCtx, update = ModalSubmit);

impl crate::kind::__Extensions for ModalSubmitCtx {
    fn __extensions(&self) -> &crate::Extensions {
        self.0.extensions()
    }
}

/// Why a component or modal interaction went to the Expired Handler chain
/// instead of its handler (FR-18, FR-60).
///
/// ```
/// use noctis_core::Expired;
///
/// assert_eq!(Expired::Stale.as_str(), "stale");
/// assert_eq!(Expired::RefMissing.as_str(), "ref_missing");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Expired {
    /// The id is from another `schema`, body kind (inline or Ref) or flag
    /// set (`N`, `T`) of the Route: a component rendered before a deploy
    /// that changed the Route's declaration.
    Stale,
    /// The id carries a Ref (`by_ref`, `overflow = spill`) whose record is
    /// missing or expired.
    RefMissing,
}

impl Expired {
    /// `"stale"` or `"ref_missing"`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stale => "stale",
            Self::RefMissing => "ref_missing",
        }
    }
}

/// The context the App's `expired_handler` receives: the accessors of
/// [`ComponentCtx`] plus [`route`](Self::route), the Route id the expired
/// or stale id named. Nothing of the stale body is decoded.
///
/// Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct ExpiredCtx(Arc<CtxInner>);

ctx_accessors!(ExpiredCtx, route = path);
crate::responder::ergonomics::ctx_methods!(ExpiredCtx, update = Component);

impl ExpiredCtx {
    /// The Route id of the component or modal (`"shop.buy"`).
    pub fn route(&self) -> &'static str {
        self.0.path
    }
}

/// A started component or modal handler: what it returned, as the pipeline
/// sends it.
pub(crate) type HandlerFuture = crate::command::HandlerFuture;

/// Boxes a handler future of kind `K`, converting its value into a
/// response.
fn boxed<K, R, Fut>(fut: Fut) -> HandlerFuture
where
    K: ResponderKind,
    R: IntoResponse<K>,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
{
    Box::pin(async move { fut.await.map(<R as IntoResponse<K>>::__into_response) })
}

/// How a typed start begins: the handler future of an inline id, or the
/// Ref of a Ref id, whose record the pipeline resolves and hands to the
/// record start.
pub(crate) enum Begin {
    /// The handler started on the decoded inline Payload.
    Ready(HandlerFuture),
    /// The id carries this Ref.
    Ref(Ref),
}

/// The typed start of a component handler: decodes the struct from the
/// decoded `custom_id` (verifying a `signed` Route's tag with the key
/// ring), then boxes the handler future with a responder over the shared
/// state; a Ref id gives its Ref.
type ComponentStart = dyn Fn(ComponentCtx, &Decoded<'_>, Arc<Shared>, Option<&dyn Verify>) -> Result<Begin, Rejection>
    + Send
    + Sync;

/// The record start of a component handler: decodes the struct from the
/// stored record of the id's Ref, then boxes the handler future.
type ComponentRecordStart =
    dyn Fn(ComponentCtx, &[u8], Arc<Shared>) -> Result<HandlerFuture, Rejection> + Send + Sync;

/// The typed start of a modal handler: checks the decoded `custom_id`
/// against the Route (verifying a `signed` Route's tag), decodes the struct
/// from the submission, then boxes the handler future; a Ref id gives its
/// Ref.
type ModalStart = dyn Fn(
        ModalSubmitCtx,
        &Decoded<'_>,
        &ModalSubmission,
        Arc<Shared>,
        Option<&dyn Verify>,
    ) -> Result<Begin, Rejection>
    + Send
    + Sync;

/// The record start of a modal handler: decodes the Payload from the
/// stored record of the id's Ref and the struct from it and the
/// submission, then boxes the handler future.
type ModalRecordStart = dyn Fn(ModalSubmitCtx, &[u8], &ModalSubmission, Arc<Shared>) -> Result<HandlerFuture, Rejection>
    + Send
    + Sync;

/// Decodes the Payload of Route `R` from the stored `record` of a Ref:
/// a record that does not parse, or one of another route, is forged; one
/// of another schema is stale; a body that does not decode is undecodable.
fn record_payload<R>(record: &[u8]) -> Result<R::Payload, Rejection>
where
    R: Route,
    for<'de> R::Payload: PayloadDecode<'de>,
{
    // Only Noctis writes records: one that does not parse is forged.
    if RefRecord::decode(record).is_err() {
        return Err(Rejection::Forged);
    }
    decode_record::<R>(record).map_err(|error| Rejection::from_decode(&error))
}

/// Decodes the component struct `S` from `decoded`, verifying a `signed`
/// Route's tag with `keys` (no keys: a signed id is `forged`).
fn decode_payload<S>(
    decoded: &Decoded<'_>,
    keys: Option<&dyn Verify>,
) -> Result<PayloadSource<S>, DecodeError>
where
    S: Route<Payload = S> + for<'de> PayloadDecode<'de>,
{
    match keys {
        Some(keys) => decoded.verified_payload::<S>(keys),
        None => decoded.payload::<S>(),
    }
}

/// The allocation-free probe of the component struct `S`: the decode of
/// the typed start without keys and without the handler.
fn probe<S>(decoded: &Decoded<'_>) -> Result<(), DecodeError>
where
    S: Route<Payload = S> + for<'de> PayloadDecode<'de>,
{
    decode_payload::<S>(decoded, None).map(|_| ())
}

/// Wraps the typed start `typed` of `S` on `route` with its Guards (AD-23
/// stage 8): when `S` declares one, the future first runs them and, on a
/// denial, returns the [`GuardDenied`] user condition without starting the
/// handler. An unguarded `S` gets `typed`'s future as is.
///
/// The guarded future records its Guard phase on [`Shared`]: `GUARDING`
/// while the Guards run, `BODY` once they allowed the run and the handler
/// body starts, so the pipeline knows whether the body ever started.
fn guarded<S, C, F>(
    route: &'static str,
    typed: Arc<F>,
) -> impl Fn(S, C, Arc<Shared>) -> HandlerFuture + Send + Sync + 'static
where
    S: __Guards<C> + Send + 'static,
    C: Send + 'static,
    F: Fn(S, C, Arc<Shared>) -> HandlerFuture + Send + Sync + 'static,
{
    move |value: S, ctx: C, shared: Arc<Shared>| -> HandlerFuture {
        if !S::GUARDED {
            return typed(value, ctx, shared);
        }
        let typed = Arc::clone(&typed);
        Box::pin(async move {
            shared.set_guard_phase(GuardPhase::GUARDING);
            let check = value.__guard(&ctx);
            if let Some(denial) = check.await? {
                return Err(GuardDenied::error(route, denial));
            }
            shared.set_guard_phase(GuardPhase::BODY);
            typed(value, ctx, shared).await
        })
    }
}

/// The erased call: the struct value arrives type-erased and is checked
/// against the handler's struct type; the handler builds its own responder
/// over the shared state.
type ErasedCall<C> = dyn Fn(C, Arc<Shared>, Box<dyn Any + Send>) -> HandlerFuture + Send + Sync;

/// The value handed to a handler of another struct type.
#[derive(Debug, thiserror::Error)]
#[error("the value is not a `{expected}`")]
struct WrongValue {
    expected: &'static str,
}

/// Wraps a typed handler `f` of `S` into the erased call.
fn erase<S, C, F>(f: Arc<F>) -> Arc<ErasedCall<C>>
where
    S: Send + 'static,
    C: Send + 'static,
    F: Fn(S, C, Arc<Shared>) -> HandlerFuture + Send + Sync + 'static,
{
    Arc::new(
        move |ctx: C, shared: Arc<Shared>, value: Box<dyn Any + Send>| -> HandlerFuture {
            match value.downcast::<S>() {
                Ok(value) => f(*value, ctx, shared),
                Err(_) => Box::pin(std::future::ready(Err(HandlerError::new(WrongValue {
                    expected: std::any::type_name::<S>(),
                })))),
            }
        },
    )
}

/// The declared response of a component or modal attribute
/// (`response = update | ephemeral | public`).
fn declared(response: __Response) -> Declared {
    match response {
        __Response::Update => Declared::Update,
        __Response::Ephemeral => Declared::Reply(Ephemerality::Ephemeral),
        __Response::Public => Declared::Reply(Ephemerality::Public),
    }
}

/// An erased `#[on_stale]` or `#[on_expired]` method: it builds its own
/// responder (of the Route's defer mode) over the shared state.
type HookFn<C> = dyn Fn(C, Expired, Arc<Shared>) -> HandlerFuture + Send + Sync;

/// The context a hook of a component or modal struct takes, and its
/// responder kind.
#[doc(hidden)]
pub trait __HookCtx: Send + 'static {
    /// [`Component`] or [`ModalSubmit`].
    type Kind: ResponderKind;
}

impl __HookCtx for ComponentCtx {
    type Kind = Component;
}

impl __HookCtx for ModalSubmitCtx {
    type Kind = ModalSubmit;
}

/// A bound `#[on_stale]` method of a struct whose context is `C`; what the
/// attribute's hidden `__noctis_on_stale()` returns.
#[doc(hidden)]
pub struct __StaleHook<C>(Arc<HookFn<C>>);

/// A bound `#[on_expired]` method of a struct whose context is `C`.
#[doc(hidden)]
pub struct __ExpiredHook<C>(Arc<HookFn<C>>);

/// No `#[on_stale]` or `#[on_expired]` method: what the fallback of a
/// struct without one returns.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct __Absent;

impl<C: __HookCtx> __StaleHook<C> {
    /// Binds `f`, taking a responder of defer mode `D`.
    #[doc(hidden)]
    pub fn __new<D, F, Fut, R>(f: F) -> Self
    where
        D: DeferMode,
        F: Fn(C, Responder<C::Kind, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<C::Kind>,
    {
        Self(Arc::new(move |ctx: C, _: Expired, shared: Arc<Shared>| {
            boxed::<C::Kind, R, _>(f(ctx, Responder::new(shared)))
        }))
    }
}

impl<C: __HookCtx> __ExpiredHook<C> {
    /// Binds `f`, taking a responder of defer mode `D`.
    #[doc(hidden)]
    pub fn __new<D, F, Fut, R>(f: F) -> Self
    where
        D: DeferMode,
        F: Fn(C, Expired, Responder<C::Kind, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<C::Kind>,
    {
        Self(Arc::new(
            move |ctx: C, expired: Expired, shared: Arc<Shared>| {
                boxed::<C::Kind, R, _>(f(ctx, expired, Responder::new(shared)))
            },
        ))
    }
}

impl<C> fmt::Debug for __StaleHook<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("__StaleHook")
    }
}

impl<C> fmt::Debug for __ExpiredHook<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("__ExpiredHook")
    }
}

/// The erased App-level Expired Handler (`AppBuilder::expired_handler`):
/// `modal` picks the responder kind its returned message converts with.
pub(crate) type AppExpiredFn = dyn Fn(ExpiredCtx, Expired, bool) -> HandlerFuture + Send + Sync;

/// Erases the App-level Expired Handler `f`.
pub(crate) fn app_expired<F, Fut, R>(f: F) -> Arc<AppExpiredFn>
where
    F: Fn(ExpiredCtx, Expired) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<R>> + Send + 'static,
    R: IntoResponse<Component> + IntoResponse<ModalSubmit>,
{
    Arc::new(move |ctx: ExpiredCtx, expired: Expired, modal: bool| {
        let fut = f(ctx, expired);
        if modal {
            boxed::<ModalSubmit, R, _>(fut)
        } else {
            boxed::<Component, R, _>(fut)
        }
    })
}

/// Which link of the Expired Handler chain answers a stale or Ref id
/// (AD-23 stage 2); its [`as_str`](Self::as_str) is the `hook` log field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpiredLink {
    /// The Route's `#[on_stale]` method.
    OnStale,
    /// The Route's `#[on_expired]` method.
    OnExpired,
    /// The App's `expired_handler`.
    App,
    /// The default `noctis.expired` notice.
    Default,
}

impl ExpiredLink {
    /// The log label.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OnStale => "on_stale",
            Self::OnExpired => "on_expired",
            Self::App => "app",
            Self::Default => "default",
        }
    }

    /// The link of `expired` for a Route with or without each hook and an
    /// App with or without an `expired_handler`: a stale id tries
    /// `on_stale`, then `on_expired`; a Ref id only `on_expired`.
    pub(crate) const fn choose(
        expired: Expired,
        on_stale: bool,
        on_expired: bool,
        app: bool,
    ) -> Self {
        if on_stale && matches!(expired, Expired::Stale) {
            Self::OnStale
        } else if on_expired {
            Self::OnExpired
        } else if app {
            Self::App
        } else {
            Self::Default
        }
    }
}

/// One component handler, as `#[derive(Button)]` (or a select derive) and
/// `#[handler]` build it; `Module::add::<T>()` stores it.
///
/// Opaque and cheap to clone. `App::build` folds it into the App's Route
/// map; a click whose `custom_id` names its Route runs it (one handler per
/// Route id, CID-04).
#[derive(Clone)]
pub struct ComponentHandler {
    name: &'static str,
    route: &'static str,
    kind: ComponentKind,
    response: Declared,
    auto_defer: bool,
    signed: bool,
    one_shot: bool,
    /// The Route's declared `ttl`, if any.
    ref_ttl: Option<Duration>,
    call: Arc<ErasedCall<ComponentCtx>>,
    start: Arc<ComponentStart>,
    start_record: Arc<ComponentRecordStart>,
    probe: fn(&Decoded<'_>) -> Result<(), DecodeError>,
    on_stale: Option<Arc<HookFn<ComponentCtx>>>,
    on_expired: Option<Arc<HookFn<ComponentCtx>>>,
    /// The Extension types its `Ext<T>` parameters require.
    requires: &'static [crate::kind::__ExtType],
}

impl ComponentHandler {
    /// The handler of the component struct `S` taking no responder,
    /// declared `response = update` and auto-deferred; the tests'
    /// constructor.
    #[doc(hidden)]
    pub fn __new<S, F, Fut>(
        name: &'static str,
        route: &'static str,
        kind: ComponentKind,
        f: F,
    ) -> Self
    where
        S: Route<Payload = S>
            + DeclaredRuntime
            + for<'de> PayloadDecode<'de>
            + __Guards<ComponentCtx>
            + Send
            + 'static,
        F: Fn(S, ComponentCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
    {
        Self::__new_with_responder::<S, AutoDefer, _, _, _>(
            name,
            route,
            kind,
            __Response::Update,
            move |value: S, ctx: ComponentCtx, _: Responder<Component, Pending, AutoDefer>| {
                f(value, ctx)
            },
        )
    }

    /// The handler of the component struct `S` declared with `response`,
    /// whose method takes a responder of defer mode `D`; what `#[handler]`
    /// binds.
    #[doc(hidden)]
    pub fn __new_with_responder<S, D, F, Fut, R>(
        name: &'static str,
        route: &'static str,
        kind: ComponentKind,
        response: __Response,
        f: F,
    ) -> Self
    where
        S: Route<Payload = S>
            + DeclaredRuntime
            + for<'de> PayloadDecode<'de>
            + __Guards<ComponentCtx>
            + Send
            + 'static,
        D: DeferMode,
        F: Fn(S, ComponentCtx, Responder<Component, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<Component>,
    {
        let typed = Arc::new(
            move |value: S, ctx: ComponentCtx, shared: Arc<Shared>| -> HandlerFuture {
                boxed::<Component, R, _>(f(value, ctx, Responder::new(shared)))
            },
        );
        // Only the pipeline starters run the Guards; `call` (the detached
        // seams) does not.
        let guarded = Arc::new(guarded::<S, ComponentCtx, _>(route, Arc::clone(&typed)));
        let starter = Arc::clone(&guarded);
        let start = move |ctx: ComponentCtx,
                          decoded: &Decoded<'_>,
                          shared: Arc<Shared>,
                          keys: Option<&dyn Verify>|
              -> Result<Begin, Rejection> {
            match decode_payload::<S>(decoded, keys) {
                Ok(PayloadSource::Inline(value)) => Ok(Begin::Ready(starter(value, ctx, shared))),
                Ok(PayloadSource::Ref(r)) => Ok(Begin::Ref(r)),
                Err(error) => Err(Rejection::from_decode(&error)),
            }
        };
        let starter = guarded;
        let start_record = move |ctx: ComponentCtx,
                                 record: &[u8],
                                 shared: Arc<Shared>|
              -> Result<HandlerFuture, Rejection> {
            let value = record_payload::<S>(record)?;
            Ok(starter(value, ctx, shared))
        };
        Self {
            name,
            route,
            kind,
            response: declared(response),
            auto_defer: D::AUTO_DEFER,
            signed: S::SHAPE.is_signed(),
            one_shot: S::RUNTIME.ref_policy().is_one_shot(),
            ref_ttl: S::RUNTIME.ref_policy().ttl(),
            call: erase::<S, _, _>(typed),
            start: Arc::new(start),
            start_record: Arc::new(start_record),
            probe: probe::<S>,
            on_stale: None,
            on_expired: None,
            requires: &[],
        }
    }

    /// Attaches the struct's `#[on_stale]` method.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_on_stale(mut self, hook: __StaleHook<ComponentCtx>) -> Self {
        self.on_stale = Some(hook.0);
        self
    }

    /// Attaches the struct's `#[on_expired]` method.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_on_expired(mut self, hook: __ExpiredHook<ComponentCtx>) -> Self {
        self.on_expired = Some(hook.0);
        self
    }

    /// Sets the Extension types the handler requires; what
    /// `__RequireExtensions` calls.
    pub(crate) fn __with_requires(self, requires: &'static [crate::kind::__ExtType]) -> Self {
        Self { requires, ..self }
    }

    /// The Extension types its `Ext<T>` parameters require, in parameter
    /// order.
    pub(crate) fn requires(&self) -> &'static [crate::kind::__ExtType] {
        self.requires
    }

    /// The link of the Expired Handler chain that answers `expired`.
    pub(crate) fn expired_link(&self, expired: Expired, app: bool) -> ExpiredLink {
        ExpiredLink::choose(
            expired,
            self.on_stale.is_some(),
            self.on_expired.is_some(),
            app,
        )
    }

    /// Starts the hook of `link` (`OnStale` or `OnExpired`) with `ctx` and a
    /// responder over `shared`; `None` for any other link.
    pub(crate) fn start_hook(
        &self,
        link: ExpiredLink,
        ctx: ComponentCtx,
        expired: Expired,
        shared: Arc<Shared>,
    ) -> Option<HandlerFuture> {
        let hook = match link {
            ExpiredLink::OnStale => self.on_stale.as_ref()?,
            ExpiredLink::OnExpired => self.on_expired.as_ref()?,
            ExpiredLink::App | ExpiredLink::Default => return None,
        };
        Some(hook(ctx, expired, shared))
    }

    /// The struct's path (`module_path!()::Struct`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The Route id of the struct's attribute (`"shop.buy"`).
    pub fn route(&self) -> &'static str {
        self.route
    }

    /// The component kind its derive names.
    pub fn kind(&self) -> ComponentKind {
        self.kind
    }

    /// The declared `response`.
    pub(crate) fn response(&self) -> Declared {
        self.response
    }

    /// Whether the pipeline arms the auto-defer (`auto_defer = true`).
    pub(crate) fn auto_defer(&self) -> bool {
        self.auto_defer
    }

    /// Whether the Route is `signed`.
    pub(crate) fn signed(&self) -> bool {
        self.signed
    }

    /// Whether the Route's Refs are consumed by their first use
    /// (`one_shot`: resolved with `take`).
    pub(crate) fn one_shot(&self) -> bool {
        self.one_shot
    }

    /// The Route's declared Ref `ttl`; `None` for the App's `ref_ttl`.
    pub(crate) fn ref_ttl(&self) -> Option<Duration> {
        self.ref_ttl
    }

    /// Starts the handler on `value` (the struct) with `ctx` and a
    /// responder over `shared`.
    pub(crate) fn call(
        &self,
        ctx: ComponentCtx,
        shared: Arc<Shared>,
        value: Box<dyn Any + Send>,
    ) -> HandlerFuture {
        (self.call)(ctx, shared, value)
    }

    /// Decodes the struct from `decoded` (verifying a signed Route's tag
    /// with `keys`) and starts the handler with `ctx` and a responder over
    /// `shared`, or says why it does not run.
    pub(crate) fn start(
        &self,
        ctx: ComponentCtx,
        decoded: &Decoded<'_>,
        shared: Arc<Shared>,
        keys: Option<&dyn Verify>,
    ) -> Result<Begin, Rejection> {
        (self.start)(ctx, decoded, shared, keys)
    }

    /// Decodes the struct from the stored `record` of the id's Ref and
    /// starts the handler, or says why it does not run.
    pub(crate) fn start_record(
        &self,
        ctx: ComponentCtx,
        record: &[u8],
        shared: Arc<Shared>,
    ) -> Result<HandlerFuture, Rejection> {
        (self.start_record)(ctx, record, shared)
    }

    /// Decodes the struct from `decoded` and drops it: the decode of
    /// [`Self::start`] without keys and without the handler (the allocation
    /// proof).
    pub(crate) fn probe(&self, decoded: &Decoded<'_>) -> Result<(), DecodeError> {
        (self.probe)(decoded)
    }
}

impl fmt::Debug for ComponentHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentHandler")
            .field("name", &self.name)
            .field("route", &self.route)
            .field("kind", &self.kind)
            .field("response", &self.response)
            .field("auto_defer", &self.auto_defer)
            .field("signed", &self.signed)
            .field("one_shot", &self.one_shot)
            .field("on_stale", &self.on_stale.is_some())
            .field("on_expired", &self.on_expired.is_some())
            .finish_non_exhaustive()
    }
}

/// One modal handler, as `#[derive(Modal)]` and `#[handler]` build it;
/// `Module::add::<T>()` stores it.
///
/// Opaque and cheap to clone. `App::build` folds it into the App's Route
/// map; a modal submit whose `custom_id` names its Route runs it (one
/// handler per Route id, CID-04). Its declared response and auto-defer
/// come from `#[modal(.., response = .., auto_defer = ..)]` (default
/// `update`, auto-deferred).
#[derive(Clone)]
pub struct ModalHandler {
    name: &'static str,
    route: &'static str,
    response: Declared,
    signed: bool,
    auto_defer: bool,
    one_shot: bool,
    /// The Route's declared `ttl`, if any.
    ref_ttl: Option<Duration>,
    call: Arc<ErasedCall<ModalSubmitCtx>>,
    start: Arc<ModalStart>,
    start_record: Arc<ModalRecordStart>,
    on_stale: Option<Arc<HookFn<ModalSubmitCtx>>>,
    on_expired: Option<Arc<HookFn<ModalSubmitCtx>>>,
    /// The Extension types its `Ext<T>` parameters require.
    requires: &'static [crate::kind::__ExtType],
}

impl ModalHandler {
    /// The handler of the modal struct `S` taking no responder; the tests'
    /// constructor.
    #[doc(hidden)]
    pub fn __new<S, F, Fut>(name: &'static str, route: &'static str, f: F) -> Self
    where
        S: ModalFromPayload + DeclaredRuntime + __Guards<ModalSubmitCtx> + Send + 'static,
        for<'de> S::Payload: PayloadDecode<'de>,
        F: Fn(S, ModalSubmitCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
    {
        Self::__new_with_responder::<S, AutoDefer, _, _, _>(
            name,
            route,
            __Response::Update,
            move |value: S, ctx: ModalSubmitCtx, _: Responder<ModalSubmit, Pending, AutoDefer>| {
                f(value, ctx)
            },
        )
    }

    /// The handler of the modal struct `S` whose method takes a responder
    /// of defer mode `D`, with the declared `response`; what `#[handler]`
    /// binds.
    #[doc(hidden)]
    pub fn __new_with_responder<S, D, F, Fut, R>(
        name: &'static str,
        route: &'static str,
        response: __Response,
        f: F,
    ) -> Self
    where
        S: ModalFromPayload + DeclaredRuntime + __Guards<ModalSubmitCtx> + Send + 'static,
        for<'de> S::Payload: PayloadDecode<'de>,
        D: DeferMode,
        F: Fn(S, ModalSubmitCtx, Responder<ModalSubmit, Pending, D>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<R>> + Send + 'static,
        R: IntoResponse<ModalSubmit>,
    {
        let typed = Arc::new(
            move |value: S, ctx: ModalSubmitCtx, shared: Arc<Shared>| -> HandlerFuture {
                boxed::<ModalSubmit, R, _>(f(value, ctx, Responder::new(shared)))
            },
        );
        // Only the pipeline starters run the Guards; `call` (the detached
        // seams) does not.
        let guarded = Arc::new(guarded::<S, ModalSubmitCtx, _>(route, Arc::clone(&typed)));
        let starter = Arc::clone(&guarded);
        let start = move |ctx: ModalSubmitCtx,
                          decoded: &Decoded<'_>,
                          submission: &ModalSubmission,
                          shared: Arc<Shared>,
                          keys: Option<&dyn Verify>|
              -> Result<Begin, Rejection> {
            // The `custom_id` is decoded exactly once, with the key ring
            // when one exists; the inputs are decoded from that Payload.
            let checked = match keys {
                Some(keys) => decoded.verified_payload::<S>(keys),
                None => decoded.payload::<S>(),
            };
            let payload = match checked {
                Ok(PayloadSource::Inline(payload)) => payload,
                Ok(PayloadSource::Ref(r)) => return Ok(Begin::Ref(r)),
                Err(error) => return Err(Rejection::from_decode(&error)),
            };
            modal_value::<S>(payload, submission)
                .map(|value| Begin::Ready(starter(value, ctx, shared)))
        };
        let starter = guarded;
        let start_record = move |ctx: ModalSubmitCtx,
                                 record: &[u8],
                                 submission: &ModalSubmission,
                                 shared: Arc<Shared>|
              -> Result<HandlerFuture, Rejection> {
            let payload = record_payload::<S>(record)?;
            modal_value::<S>(payload, submission).map(|value| starter(value, ctx, shared))
        };
        Self {
            name,
            route,
            response: declared(response),
            signed: S::SHAPE.is_signed(),
            auto_defer: D::AUTO_DEFER,
            one_shot: S::RUNTIME.ref_policy().is_one_shot(),
            ref_ttl: S::RUNTIME.ref_policy().ttl(),
            call: erase::<S, _, _>(typed),
            start: Arc::new(start),
            start_record: Arc::new(start_record),
            on_stale: None,
            on_expired: None,
            requires: &[],
        }
    }

    /// Attaches the struct's `#[on_stale]` method.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_on_stale(mut self, hook: __StaleHook<ModalSubmitCtx>) -> Self {
        self.on_stale = Some(hook.0);
        self
    }

    /// Attaches the struct's `#[on_expired]` method.
    #[doc(hidden)]
    #[must_use]
    pub fn __with_on_expired(mut self, hook: __ExpiredHook<ModalSubmitCtx>) -> Self {
        self.on_expired = Some(hook.0);
        self
    }

    /// Sets the Extension types the handler requires; what
    /// `__RequireExtensions` calls.
    pub(crate) fn __with_requires(self, requires: &'static [crate::kind::__ExtType]) -> Self {
        Self { requires, ..self }
    }

    /// The Extension types its `Ext<T>` parameters require, in parameter
    /// order.
    pub(crate) fn requires(&self) -> &'static [crate::kind::__ExtType] {
        self.requires
    }

    /// The link of the Expired Handler chain that answers `expired`.
    pub(crate) fn expired_link(&self, expired: Expired, app: bool) -> ExpiredLink {
        ExpiredLink::choose(
            expired,
            self.on_stale.is_some(),
            self.on_expired.is_some(),
            app,
        )
    }

    /// Starts the hook of `link` (`OnStale` or `OnExpired`) with `ctx` and a
    /// responder over `shared`; `None` for any other link.
    pub(crate) fn start_hook(
        &self,
        link: ExpiredLink,
        ctx: ModalSubmitCtx,
        expired: Expired,
        shared: Arc<Shared>,
    ) -> Option<HandlerFuture> {
        let hook = match link {
            ExpiredLink::OnStale => self.on_stale.as_ref()?,
            ExpiredLink::OnExpired => self.on_expired.as_ref()?,
            ExpiredLink::App | ExpiredLink::Default => return None,
        };
        Some(hook(ctx, expired, shared))
    }

    /// The struct's path (`module_path!()::Struct`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The Route id of the struct's `#[modal("id", ..)]`.
    pub fn route(&self) -> &'static str {
        self.route
    }

    /// The declared `response` of `#[modal(.., response = ..)]` (default
    /// `update`).
    pub(crate) fn response(&self) -> Declared {
        self.response
    }

    /// Whether the pipeline arms the auto-defer.
    pub(crate) fn auto_defer(&self) -> bool {
        self.auto_defer
    }

    /// Whether the Route is `signed`.
    pub(crate) fn signed(&self) -> bool {
        self.signed
    }

    /// Starts the handler on `value` (the struct) with `ctx` and a
    /// responder over `shared`.
    pub(crate) fn call(
        &self,
        ctx: ModalSubmitCtx,
        shared: Arc<Shared>,
        value: Box<dyn Any + Send>,
    ) -> HandlerFuture {
        (self.call)(ctx, shared, value)
    }

    /// Checks `decoded` against the Route (verifying a signed Route's tag
    /// with `keys`), decodes the struct from `submission` and starts the
    /// handler with `ctx` and a responder over `shared`, or says why it does
    /// not run.
    pub(crate) fn start(
        &self,
        ctx: ModalSubmitCtx,
        decoded: &Decoded<'_>,
        submission: &ModalSubmission,
        shared: Arc<Shared>,
        keys: Option<&dyn Verify>,
    ) -> Result<Begin, Rejection> {
        (self.start)(ctx, decoded, submission, shared, keys)
    }

    /// Decodes the Payload from the stored `record` of the id's Ref and the
    /// struct from it and `submission`, then starts the handler, or says
    /// why it does not run.
    pub(crate) fn start_record(
        &self,
        ctx: ModalSubmitCtx,
        record: &[u8],
        submission: &ModalSubmission,
        shared: Arc<Shared>,
    ) -> Result<HandlerFuture, Rejection> {
        (self.start_record)(ctx, record, submission, shared)
    }

    /// Whether the Route's Refs are consumed by their first use
    /// (`one_shot`: resolved with `take`).
    pub(crate) fn one_shot(&self) -> bool {
        self.one_shot
    }

    /// The Route's declared Ref `ttl`; `None` for the App's `ref_ttl`.
    pub(crate) fn ref_ttl(&self) -> Option<Duration> {
        self.ref_ttl
    }
}

impl fmt::Debug for ModalHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModalHandler")
            .field("name", &self.name)
            .field("route", &self.route)
            .field("response", &self.response)
            .field("auto_defer", &self.auto_defer)
            .field("signed", &self.signed)
            .field("one_shot", &self.one_shot)
            .field("on_stale", &self.on_stale.is_some())
            .field("on_expired", &self.on_expired.is_some())
            .finish_non_exhaustive()
    }
}

/// The modal struct `S` from its decoded `payload` and the `submission`.
fn modal_value<S: ModalFromPayload>(
    payload: S::Payload,
    submission: &ModalSubmission,
) -> Result<S, Rejection> {
    match S::from_payload(payload, submission) {
        Ok(value) => Ok(value),
        Err(SubmitError::Malformed { .. }) => Err(Rejection::Undecodable),
        Err(_) => Err(Rejection::SubmitInvalid),
    }
}

/// Runs a started handler over the detached `shared`: a returned message
/// is validated and encoded, never sent; a responder error becomes the
/// handler error.
fn run_detached(
    fut: HandlerFuture,
    shared: Arc<Shared>,
) -> BoxFuture<'static, HandlerResult<Responded>> {
    Box::pin(async move {
        if let Response::Message(msg) = fut.await?.0 {
            shared.finish(msg).await.map_err(HandlerError::new)?;
        }
        Ok(Responded::new())
    })
}

/// The placeholder source message of the detached seams: a public V2
/// message, so `update` and `ctx.update` succeed there.
const DETACHED_SOURCE: SourceMessage = SourceMessage {
    id: Id::new(1),
    channel_id: Id::new(1),
    format: MessageFormat::V2,
    ephemerality: Ephemerality::Public,
};

/// A detached shared state declaring `response`, on the placeholder
/// source message, and the ephemerality of its responses.
fn detached(response: Declared) -> (Arc<Shared>, Ephemerality) {
    let shared = Shared::declared(
        Transport::Detached,
        response,
        Some(DETACHED_SOURCE),
        Limits::default(),
        None,
    );
    let ephemerality = shared.response_ephemerality();
    (shared, ephemerality)
}

/// Runs `handler` on `value` over a detached context and responder; the
/// test seam (see `crate::__testing`).
pub(crate) fn call_component<S: Send + 'static>(
    handler: &ComponentHandler,
    value: S,
) -> BoxFuture<'static, HandlerResult<Responded>> {
    let (shared, ephemerality) = detached(handler.response);
    let fut = handler.call(
        ComponentCtx::detached_bound(&shared, ephemerality),
        Arc::clone(&shared),
        Box::new(value),
    );
    run_detached(fut, shared)
}

/// Runs `handler` on `value` over a detached context and responder; the
/// test seam (see `crate::__testing`).
pub(crate) fn call_modal<S: Send + 'static>(
    handler: &ModalHandler,
    value: S,
) -> BoxFuture<'static, HandlerResult<Responded>> {
    let (shared, ephemerality) = detached(handler.response());
    let fut = handler.call(
        ModalSubmitCtx::detached_bound(&shared, ephemerality),
        Arc::clone(&shared),
        Box::new(value),
    );
    run_detached(fut, shared)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use noctis_codec::{CodecError, FieldReader, FieldWriter, Payload, RouteId, RouteShape};
    use noctis_ui::ModalForm;

    use super::*;

    struct Buy {
        item: u64,
    }

    impl Payload for Buy {
        const MAX_ENCODED_LEN: Option<usize> = Some(20);

        fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
            out.field(&self.item)
        }
    }

    impl PayloadDecode<'_> for Buy {
        fn decode_fields(f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
            Ok(Self { item: f.field()? })
        }
    }

    impl DeclaredRuntime for Buy {}
    impl __Guards<ComponentCtx> for Buy {}
    impl __Guards<ModalSubmitCtx> for Buy {}

    impl Route for Buy {
        const ID: RouteId = RouteId::new("shop.buy");
        const SHAPE: RouteShape = RouteShape::INLINE;
        const SCHEMA: u8 = 0;
        type Payload = Self;
    }

    impl ModalForm for Buy {
        fn from_submission(submission: &ModalSubmission) -> Result<Self, SubmitError> {
            let decoded = noctis_codec::decode(submission.custom_id()).map_err(|e| {
                SubmitError::Malformed {
                    reason: e.to_string(),
                }
            })?;
            match decoded.payload::<Self>() {
                Ok(PayloadSource::Inline(buy)) => Ok(buy),
                _ => Err(SubmitError::Malformed {
                    reason: "bad payload".to_owned(),
                }),
            }
        }
    }

    impl ModalFromPayload for Buy {
        fn from_payload(payload: Self, _: &ModalSubmission) -> Result<Self, SubmitError> {
            Ok(payload)
        }
    }

    static SEEN: Mutex<Vec<u64>> = Mutex::new(Vec::new());

    async fn press(buy: Buy, ctx: ComponentCtx) -> HandlerResult<Responded> {
        assert_eq!(ctx.user_id(), Id::new(1));
        SEEN.lock().map_err(|_| fmt::Error)?.push(buy.item);
        Ok(Responded::new())
    }

    async fn submit(buy: Buy, _: ModalSubmitCtx) -> HandlerResult<Responded> {
        SEEN.lock().map_err(|_| fmt::Error)?.push(buy.item + 100);
        Ok(Responded::new())
    }

    /// Matrix 51.
    #[tokio::test]
    async fn component_handler_runs_on_the_value() {
        let handler = ComponentHandler::__new::<Buy, _, _>(
            "m::Buy",
            "shop.buy",
            ComponentKind::Button,
            press,
        );
        assert_eq!(
            (handler.name(), handler.route(), handler.kind()),
            ("m::Buy", "shop.buy", ComponentKind::Button)
        );
        let _ = call_component(&handler, Buy { item: 7 })
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(SEEN.lock().is_ok_and(|s| s.contains(&7)));
        let err = call_component(&handler, 5_u8).await.unwrap_err();
        assert!(err.to_string().contains("is not a"), "{err}");
        assert!(format!("{handler:?}").contains("shop.buy"));
    }

    /// Matrix 52.
    #[tokio::test]
    async fn modal_handler_runs_on_the_value() {
        let handler = ModalHandler::__new::<Buy, _, _>("m::Bio", "bio", submit);
        assert_eq!((handler.name(), handler.route()), ("m::Bio", "bio"));
        let _ = call_modal(&handler, Buy { item: 1 })
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(SEEN.lock().is_ok_and(|s| s.contains(&101)));
        let ctx = ModalSubmitCtx::detached();
        assert_eq!(ctx.module(), "");
        assert_eq!(
            (ctx.shard(), ctx.guild_id(), ctx.channel_id()),
            (0, None, None)
        );
        assert_eq!((ctx.user_locale(), ctx.guild_locale()), (None, None));
        assert!(ctx.state::<u8>().is_none());
        assert!(format!("{ctx:?}").starts_with("ModalSubmitCtx"));
    }

    /// Story 7.3: `Expired`, the chain's precedence, the attached hooks and
    /// the descriptors' and `ExpiredCtx`'s `Debug`.
    #[tokio::test]
    async fn hooks_attach_and_debug() {
        assert_eq!(
            (Expired::Stale.as_str(), Expired::RefMissing.as_str()),
            ("stale", "ref_missing")
        );
        let choose = ExpiredLink::choose;
        let stale = Expired::Stale;
        let missing = Expired::RefMissing;
        assert_eq!(choose(stale, true, true, true), ExpiredLink::OnStale);
        assert_eq!(choose(stale, false, true, true), ExpiredLink::OnExpired);
        assert_eq!(choose(stale, false, false, true), ExpiredLink::App);
        assert_eq!(choose(stale, false, false, false), ExpiredLink::Default);
        assert_eq!(choose(missing, true, false, true), ExpiredLink::App);
        assert_eq!(choose(missing, true, true, false), ExpiredLink::OnExpired);
        assert_eq!(choose(missing, true, false, false), ExpiredLink::Default);
        assert_eq!(
            [
                ExpiredLink::OnStale,
                ExpiredLink::OnExpired,
                ExpiredLink::App,
                ExpiredLink::Default
            ]
            .map(ExpiredLink::as_str),
            ["on_stale", "on_expired", "app", "default"]
        );

        let bare = ComponentHandler::__new::<Buy, _, _>(
            "m::Buy",
            "shop.buy",
            ComponentKind::Button,
            press,
        );
        let debug = format!("{bare:?}");
        assert!(
            debug.contains("on_stale: false") && debug.contains("on_expired: false"),
            "{debug}"
        );
        assert_eq!(bare.expired_link(stale, false), ExpiredLink::Default);
        let hooked = bare
            .__with_on_stale(__StaleHook::__new::<AutoDefer, _, _, _>(
                |_: ComponentCtx, _: Responder<Component, Pending, AutoDefer>| async move {
                    Ok(Responded::new())
                },
            ))
            .__with_on_expired(__ExpiredHook::__new::<AutoDefer, _, _, _>(
                |_: ComponentCtx, _: Expired, _: Responder<Component, Pending, AutoDefer>| async move {
                    Ok(Responded::new())
                },
            ));
        let debug = format!("{hooked:?}");
        assert!(
            debug.contains("on_stale: true") && debug.contains("on_expired: true"),
            "{debug}"
        );
        assert_eq!(hooked.expired_link(stale, false), ExpiredLink::OnStale);
        assert_eq!(hooked.expired_link(missing, false), ExpiredLink::OnExpired);
        let (shared, _) = detached(Declared::Update);
        let started = hooked.start_hook(
            ExpiredLink::OnStale,
            ComponentCtx::detached(),
            stale,
            Arc::clone(&shared),
        );
        assert!(started.is_some());
        assert!(
            hooked
                .start_hook(ExpiredLink::App, ComponentCtx::detached(), stale, shared)
                .is_none()
        );

        let modal = ModalHandler::__new::<Buy, _, _>("m::Bio", "bio", submit).__with_on_stale(
            __StaleHook::__new::<AutoDefer, _, _, _>(
                |_: ModalSubmitCtx, _: Responder<ModalSubmit, Pending, AutoDefer>| async move {
                    Ok(Responded::new())
                },
            ),
        );
        let debug = format!("{modal:?}");
        assert!(
            debug.contains("on_stale: true") && debug.contains("on_expired: false"),
            "{debug}"
        );
        assert_eq!(modal.expired_link(missing, true), ExpiredLink::App);

        let mut parts = CtxInner::detached_parts(Ephemerality::Public);
        parts.path = "shop.buy";
        let ctx = ExpiredCtx::from_inner(Arc::new(parts));
        assert_eq!((ctx.route(), ctx.user_id()), ("shop.buy", Id::new(1)));
        assert!(ExpiredCtx::detached().route().is_empty());
        let debug = format!("{ctx:?}");
        assert!(
            debug.starts_with("ExpiredCtx") && debug.contains("route: \"shop.buy\""),
            "{debug}"
        );
        assert_eq!(format!("{__Absent:?}"), "__Absent");
        let hook = __StaleHook::<ComponentCtx>(Arc::new(|_, _, _| unreachable!()));
        assert_eq!(format!("{hook:?}"), "__StaleHook");
    }
}
