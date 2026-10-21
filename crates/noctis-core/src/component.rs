//! Components and modals declared as structs (AD-6, AD-19, FR-58): the
//! [`ComponentCtx`] and [`ModalSubmitCtx`] their `#[handler]` methods
//! receive, and the [`ComponentHandler`] and [`ModalHandler`] descriptors
//! `Module::add::<T>()` stores for them.
//!
//! Dispatch is live (Story 7.1): `App::build` folds every mounted
//! descriptor into one Route map keyed by Route id, and a click or a modal
//! submit runs the handler of its Route on the decoded struct. The
//! component and modal Responders, notices and the runtime routing
//! arguments arrive with Stories 7.2 to 7.7.

use std::any::Any;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use noctis_codec::{DecodeError, Decoded, PayloadDecode, PayloadSource, Route};
use noctis_ui::{ModalForm, ModalSubmission, SubmitError};
use twilight_model::id::Id;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};

use crate::State;
use crate::command::CtxInner;
use crate::handler::{HandlerError, HandlerResult};
use crate::pipeline::component::Rejection;
use crate::responder::{Ephemerality, Responded};

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
    ($ty:ident) => {
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

            /// The user's client locale (`locale`), such as `"tr"`.
            pub fn user_locale(&self) -> Option<&str> {
                self.0.user_locale.as_deref()
            }

            /// The guild's preferred locale (`guild_locale`), outside DMs.
            pub fn guild_locale(&self) -> Option<&str> {
                self.0.guild_locale.as_deref()
            }

            /// The state registered for `T` with `AppBuilder::state`, if any.
            pub fn state<T: Send + Sync + 'static>(&self) -> Option<State<T>> {
                self.0.app.as_ref()?.states.get::<T>()
            }

            /// A detached context (the `__testing` seams): no App, shard 0,
            /// user 1.
            pub(crate) fn detached() -> Self {
                Self(Arc::new(CtxInner::detached_parts(Ephemerality::Public)))
            }

            /// A context of `parts` (the component pipeline).
            pub(crate) fn from_parts(parts: CtxInner) -> Self {
                Self(Arc::new(parts))
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
                    .finish_non_exhaustive()
            }
        }
    };
}

/// The context a component's `#[handler]` method receives: the Module, the
/// shard, the guild, channel and user, the locales and the App's typed
/// state. The struct value itself (`self`) is the decoded Payload.
///
/// Cheap to clone (one `Arc`). Its Responder parameters arrive with Story
/// 7.2.
#[derive(Clone)]
pub struct ComponentCtx(Arc<CtxInner>);

ctx_accessors!(ComponentCtx);

/// The context a modal's `#[handler]` method receives; the same accessors
/// as [`ComponentCtx`]. The struct value itself (`self`) is the decoded
/// Payload plus the submitted inputs.
///
/// Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct ModalSubmitCtx(Arc<CtxInner>);

ctx_accessors!(ModalSubmitCtx);

/// A started component or modal handler.
pub(crate) type HandlerFuture = BoxFuture<'static, HandlerResult<Responded>>;

/// The typed start of a component handler: decodes the struct from the
/// decoded `custom_id`, then boxes the handler future.
type ComponentStart =
    dyn Fn(ComponentCtx, &Decoded<'_>) -> Result<HandlerFuture, Rejection> + Send + Sync;

/// The typed start of a modal handler: checks the decoded `custom_id`
/// against the Route, decodes the struct from the submission, then boxes
/// the handler future.
type ModalStart = dyn Fn(ModalSubmitCtx, &Decoded<'_>, &ModalSubmission) -> Result<HandlerFuture, Rejection>
    + Send
    + Sync;

/// Decodes the component struct `S` from `decoded` with no keys: the one
/// decode the typed start and the probe share.
fn decode_payload<S>(decoded: &Decoded<'_>) -> Result<PayloadSource<S>, DecodeError>
where
    S: Route<Payload = S> + for<'de> PayloadDecode<'de>,
{
    decoded.payload::<S>()
}

/// The allocation-free probe of the component struct `S`: the decode of
/// the typed start, without the handler.
fn probe<S>(decoded: &Decoded<'_>) -> Result<(), DecodeError>
where
    S: Route<Payload = S> + for<'de> PayloadDecode<'de>,
{
    decode_payload::<S>(decoded).map(|_| ())
}

/// The erased call: the struct value arrives type-erased and is checked
/// against the handler's struct type.
type ErasedCall<C> = dyn Fn(C, Box<dyn Any + Send>) -> HandlerFuture + Send + Sync;

/// The value handed to a handler of another struct type.
#[derive(Debug, thiserror::Error)]
#[error("the value is not a `{expected}`")]
struct WrongValue {
    expected: &'static str,
}

/// Wraps a typed handler `f` of `S` into the erased call.
fn erase<S, C, F, Fut>(f: F) -> Arc<ErasedCall<C>>
where
    S: Send + 'static,
    C: Send + 'static,
    F: Fn(S, C) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
{
    Arc::new(move |ctx: C, value: Box<dyn Any + Send>| -> HandlerFuture {
        match value.downcast::<S>() {
            Ok(value) => Box::pin(f(*value, ctx)),
            Err(_) => Box::pin(std::future::ready(Err(HandlerError::new(WrongValue {
                expected: std::any::type_name::<S>(),
            })))),
        }
    })
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
    call: Arc<ErasedCall<ComponentCtx>>,
    start: Arc<ComponentStart>,
    probe: fn(&Decoded<'_>) -> Result<(), DecodeError>,
}

impl ComponentHandler {
    /// The handler of the component struct `S`; what `#[handler]` binds.
    #[doc(hidden)]
    pub fn __new<S, F, Fut>(
        name: &'static str,
        route: &'static str,
        kind: ComponentKind,
        f: F,
    ) -> Self
    where
        S: Route<Payload = S> + for<'de> PayloadDecode<'de> + Send + 'static,
        F: Fn(S, ComponentCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
    {
        let f = Arc::new(f);
        let typed = Arc::clone(&f);
        let start =
            move |ctx: ComponentCtx, decoded: &Decoded<'_>| -> Result<HandlerFuture, Rejection> {
                match decode_payload::<S>(decoded) {
                    Ok(PayloadSource::Inline(value)) => Ok(Box::pin(typed(value, ctx))),
                    Ok(PayloadSource::Ref(_)) => Err(Rejection::RefUnresolved),
                    Err(error) => Err(Rejection::from_decode(&error)),
                }
            };
        Self {
            name,
            route,
            kind,
            call: erase::<S, _, _, _>(move |value: S, ctx: ComponentCtx| f(value, ctx)),
            start: Arc::new(start),
            probe: probe::<S>,
        }
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

    /// Starts the handler on `value` (the struct) with `ctx`.
    pub(crate) fn call(&self, ctx: ComponentCtx, value: Box<dyn Any + Send>) -> HandlerFuture {
        (self.call)(ctx, value)
    }

    /// Decodes the struct from `decoded` and starts the handler with
    /// `ctx`, or says why it does not run.
    pub(crate) fn start(
        &self,
        ctx: ComponentCtx,
        decoded: &Decoded<'_>,
    ) -> Result<HandlerFuture, Rejection> {
        (self.start)(ctx, decoded)
    }

    /// Decodes the struct from `decoded` and drops it: the decode of
    /// [`Self::start`] without the handler (the allocation proof).
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
            .finish_non_exhaustive()
    }
}

/// One modal handler, as `#[derive(Modal)]` and `#[handler]` build it;
/// `Module::add::<T>()` stores it.
///
/// Opaque and cheap to clone. `App::build` folds it into the App's Route
/// map; a modal submit whose `custom_id` names its Route runs it (one
/// handler per Route id, CID-04).
#[derive(Clone)]
pub struct ModalHandler {
    name: &'static str,
    route: &'static str,
    call: Arc<ErasedCall<ModalSubmitCtx>>,
    start: Arc<ModalStart>,
}

impl ModalHandler {
    /// The handler of the modal struct `S`; what `#[handler]` binds.
    #[doc(hidden)]
    pub fn __new<S, F, Fut>(name: &'static str, route: &'static str, f: F) -> Self
    where
        S: Route + ModalForm + Send + 'static,
        for<'de> S::Payload: PayloadDecode<'de>,
        F: Fn(S, ModalSubmitCtx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = HandlerResult<Responded>> + Send + 'static,
    {
        let f = Arc::new(f);
        let typed = Arc::clone(&f);
        let start = move |ctx: ModalSubmitCtx,
                          decoded: &Decoded<'_>,
                          submission: &ModalSubmission|
              -> Result<HandlerFuture, Rejection> {
            match decoded.payload::<S>() {
                Ok(PayloadSource::Inline(_)) => {}
                Ok(PayloadSource::Ref(_)) => return Err(Rejection::RefUnresolved),
                Err(error) => return Err(Rejection::from_decode(&error)),
            }
            match S::from_submission(submission) {
                Ok(value) => Ok(Box::pin(typed(value, ctx))),
                Err(SubmitError::Malformed { .. }) => Err(Rejection::Undecodable),
                Err(_) => Err(Rejection::SubmitInvalid),
            }
        };
        Self {
            name,
            route,
            call: erase::<S, _, _, _>(move |value: S, ctx: ModalSubmitCtx| f(value, ctx)),
            start: Arc::new(start),
        }
    }

    /// The struct's path (`module_path!()::Struct`).
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The Route id of the struct's `#[modal("id", ..)]`.
    pub fn route(&self) -> &'static str {
        self.route
    }

    /// Starts the handler on `value` (the struct) with `ctx`.
    pub(crate) fn call(&self, ctx: ModalSubmitCtx, value: Box<dyn Any + Send>) -> HandlerFuture {
        (self.call)(ctx, value)
    }

    /// Checks `decoded` against the Route, decodes the struct from
    /// `submission` and starts the handler with `ctx`, or says why it does
    /// not run.
    pub(crate) fn start(
        &self,
        ctx: ModalSubmitCtx,
        decoded: &Decoded<'_>,
        submission: &ModalSubmission,
    ) -> Result<HandlerFuture, Rejection> {
        (self.start)(ctx, decoded, submission)
    }
}

impl fmt::Debug for ModalHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModalHandler")
            .field("name", &self.name)
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

/// Runs `handler` on `value` over a detached context; the test seam (see
/// `crate::__testing`).
pub(crate) fn call_component<S: Send + 'static>(
    handler: &ComponentHandler,
    value: S,
) -> BoxFuture<'static, HandlerResult<Responded>> {
    handler.call(ComponentCtx::detached(), Box::new(value))
}

/// Runs `handler` on `value` over a detached context; the test seam (see
/// `crate::__testing`).
pub(crate) fn call_modal<S: Send + 'static>(
    handler: &ModalHandler,
    value: S,
) -> BoxFuture<'static, HandlerResult<Responded>> {
    handler.call(ModalSubmitCtx::detached(), Box::new(value))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use noctis_codec::{CodecError, FieldReader, FieldWriter, Payload, RouteId, RouteShape};

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
}
