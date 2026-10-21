//! Components built from Route values: [`Handled`], [`ComponentRoute`]
//! with its declared defaults, the hidden build functions the kind derives
//! call, and the hidden [`CustomId`] they carry (AD-18, AD-19, FR-58,
//! FR-59).

use core::fmt;
use std::borrow::Cow;
use std::sync::Arc;

use arrayvec::ArrayString;
use noctis_codec::{
    BodyKind, DeclaredRuntime, EncodeError, EncodedId, MAX_ID_LEN, Parts, PendingRef, Ref,
    RefPolicy, Route, RouteId, RouteShape, SelectNonce, SelectRoute, Sign, encode_id,
};
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;

use crate::component::{
    ButtonStyle, ChannelSelect, Emoji, MentionableSelect, RoleSelect, RouteButton, SelectOption,
    StringSelect, StringSelectBuilder, UserSelect,
};
use crate::modal::Choices;
use crate::{Modal, UiError};

/// A Route that has a handler.
///
/// The kind derives of `noctis` (`#[derive(Button)]`, the select derives,
/// `#[derive(Modal)]`) implement it; a kind-derived struct without a
/// `#[handler]` method does not compile (`[CID-04]`, FR-58). Its
/// [`DeclaredRuntime`] carries the Route's Ref policy (`ttl`, `one_shot`).
/// It is not public API.
#[diagnostic::on_unimplemented(
    message = "[CID-04] Route `{Self}` has no handler, so no component can be built for it",
    label = "no `#[handler]` for this Route",
    note = "derive a component kind (`#[derive(Button)]`, a select kind or `#[derive(Modal)]`) and add `impl {Self} {{ #[handler] async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> }}`"
)]
pub trait Handled: Route + DeclaredRuntime {}

/// A component or modal Route declared by a kind derive, with the defaults
/// of its `defaults(..)` block (AD-19, FR-59).
///
/// `X { .. }.build()` builds the component from [`DEFAULTS`](Self::DEFAULTS)
/// and the struct value as its Payload; the component's setters override one
/// field each. Implemented only by the kind derives; not public API.
#[doc(hidden)]
pub trait ComponentRoute: Handled + Send + Sync + 'static {
    /// The defaults type of the kind: [`ButtonDefaults`],
    /// [`SelectDefaults`], [`StringSelectDefaults`],
    /// [`ChannelSelectDefaults`] or [`ModalDefaults`].
    type Defaults: 'static;

    /// The declared defaults.
    const DEFAULTS: Self::Defaults;
}

/// The emoji of a Button's `defaults(emoji = "…")`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultEmoji {
    /// A Unicode emoji.
    Unicode(&'static str),
    /// A custom emoji, `<:name:id>` or `<a:name:id>`.
    Custom {
        /// The emoji id (never 0).
        id: u64,
        /// The emoji name.
        name: &'static str,
        /// `<a:…>`.
        animated: bool,
    },
}

impl DefaultEmoji {
    fn emoji(self) -> Emoji {
        match self {
            Self::Unicode(e) => Emoji::unicode(e),
            Self::Custom { id, name, animated } => match Id::new_checked(id) {
                Some(id) => Emoji::custom(id, name, animated),
                // The derive rejects id 0; keep the name rather than panic.
                None => Emoji::unicode(name),
            },
        }
    }
}

/// The `defaults(..)` of `#[button]`; omitted keys are not sent.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonDefaults {
    /// `style` (required).
    pub style: ButtonStyle,
    /// `label`.
    pub label: Option<&'static str>,
    /// `emoji`.
    pub emoji: Option<DefaultEmoji>,
    /// `disabled` (`false` when omitted).
    pub disabled: bool,
}

/// The `defaults(..)` every select kind shares; omitted counts are not sent
/// (Discord's `min_values = 1`, `max_values = 1` apply).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectDefaults {
    /// `placeholder` (required by the derives).
    pub placeholder: Option<&'static str>,
    /// `min_values`.
    pub min_values: Option<u8>,
    /// `max_values`.
    pub max_values: Option<u8>,
    /// `disabled` (`false` when omitted).
    pub disabled: bool,
}

/// The `defaults(..)` of `#[string_select]`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct StringSelectDefaults {
    /// The shared select defaults.
    pub select: SelectDefaults,
    /// `options = <Choices type>`: its options, or `None` when the options
    /// come with each render.
    pub options: Option<fn() -> Vec<SelectOption>>,
}

/// The `defaults(..)` of `#[channel_select]`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSelectDefaults {
    /// The shared select defaults.
    pub select: SelectDefaults,
    /// `channel_types = [..]`; empty means every type.
    pub channel_types: &'static [ChannelType],
}

/// The `defaults(..)` of `#[modal]`.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalDefaults {
    /// `title` (required).
    pub title: &'static str,
}

/// The select options of the [`Choices`] enum `C`, in declaration order;
/// what `defaults(options = C)` stores.
#[doc(hidden)]
pub fn choices_options<C: Choices>() -> Vec<SelectOption> {
    C::ALL
        .iter()
        .map(|c| {
            let option = SelectOption::new(c.label(), c.value());
            match c.description() {
                Some(d) => option.description(d),
                None => option,
            }
        })
        .collect()
}

/// The bounds of a component Route value: a Route that is its own
/// Payload, with a handler.
#[doc(hidden)]
pub trait ComponentValue: Route<Payload = Self> + Handled + Send + Sync + 'static {}

impl<R: Route<Payload = R> + Handled + Send + Sync + 'static> ComponentValue for R {}

/// `Button { .. }.build()`: the button of `route` with `defaults`.
#[doc(hidden)]
pub fn build_button<R: ComponentValue>(route: R, defaults: &ButtonDefaults) -> RouteButton<R> {
    let mut button = RouteButton::from_id(CustomId::new::<R>(route))
        .style(defaults.style)
        .disabled(defaults.disabled);
    if let Some(label) = defaults.label {
        button = button.label(label);
    }
    if let Some(emoji) = defaults.emoji {
        button = button.emoji(emoji.emoji());
    }
    button
}

/// `StringSelectMenu { .. }.build()` with `defaults(options = ..)`.
#[doc(hidden)]
pub fn build_string_select<R: ComponentValue + SelectRoute>(
    route: R,
    defaults: &StringSelectDefaults,
) -> StringSelect {
    let options = defaults.options.map(|f| f()).unwrap_or_default();
    StringSelect::from_id(CustomId::new::<R>(route), options).with_defaults(&defaults.select)
}

/// `StringSelectMenu { .. }.build()` without `defaults(options = ..)`: the
/// options come with `.options(..)`.
#[doc(hidden)]
pub fn build_string_select_builder<R: ComponentValue + SelectRoute>(
    route: R,
    defaults: &SelectDefaults,
) -> StringSelectBuilder<R> {
    StringSelectBuilder::new(
        StringSelect::from_id(CustomId::new::<R>(route), Vec::new()).with_defaults(defaults),
    )
}

/// `UserSelectMenu { .. }.build()`.
#[doc(hidden)]
pub fn build_user_select<R: ComponentValue + SelectRoute>(
    route: R,
    defaults: &SelectDefaults,
) -> UserSelect {
    UserSelect::from_id(CustomId::new::<R>(route)).with_defaults(defaults)
}

/// `RoleSelectMenu { .. }.build()`.
#[doc(hidden)]
pub fn build_role_select<R: ComponentValue + SelectRoute>(
    route: R,
    defaults: &SelectDefaults,
) -> RoleSelect {
    RoleSelect::from_id(CustomId::new::<R>(route)).with_defaults(defaults)
}

/// `MentionableSelectMenu { .. }.build()`.
#[doc(hidden)]
pub fn build_mentionable_select<R: ComponentValue + SelectRoute>(
    route: R,
    defaults: &SelectDefaults,
) -> MentionableSelect {
    MentionableSelect::from_id(CustomId::new::<R>(route)).with_defaults(defaults)
}

/// `ChannelSelectMenu { .. }.build()`.
#[doc(hidden)]
pub fn build_channel_select<R: ComponentValue + SelectRoute>(
    route: R,
    defaults: &ChannelSelectDefaults,
) -> ChannelSelect {
    ChannelSelect::from_id(CustomId::new::<R>(route))
        .with_defaults(&defaults.select)
        .channel_types(defaults.channel_types.iter().copied())
}

/// The empty [`Modal`] titled `title` of the modal Route `R` whose Payload
/// is `payload`; the derive pushes the inputs.
#[doc(hidden)]
pub fn build_modal<R>(payload: R::Payload, title: impl Into<Cow<'static, str>>) -> Modal
where
    R: Handled + Send + Sync + 'static,
    R::Payload: Send + Sync + 'static,
{
    Modal::from_id(CustomId::new::<R>(payload), title.into())
}

/// A Route value behind a vtable.
trait ErasedRoute: Send + Sync {
    fn route(&self) -> RouteId;
    fn shape(&self) -> RouteShape;
    fn encode(&self, parts: &Parts<'_>) -> Result<EncodedId, EncodeError>;
}

/// A Route value and the Ref policy of its Route.
struct Erased<R: Route> {
    payload: R::Payload,
    ref_policy: RefPolicy,
}

impl<R> ErasedRoute for Erased<R>
where
    R: Route + Send + Sync,
    R::Payload: Send + Sync,
{
    fn route(&self) -> RouteId {
        R::ID
    }

    fn shape(&self) -> RouteShape {
        R::SHAPE
    }

    fn encode(&self, parts: &Parts<'_>) -> Result<EncodedId, EncodeError> {
        if R::SHAPE.body() == BodyKind::Inline {
            encode_id::<R>(&self.payload, parts)
        } else {
            encode_id::<R>(&self.payload, &parts.with_ref_policy(self.ref_policy))
        }
    }
}

/// The `custom_id` of a Route component: the Route value and, once known,
/// its encoding.
///
/// A Route without a select nonce or integrity tag is encoded
/// when the component is built; any other `custom_id` stays unresolved until
/// core calls [`resolve`](Self::resolve) with the render's parts (and
/// [`finish`](Self::finish) for a Ref slot) before sending (AD-18).
/// Serializing an unresolved id is a serde error.
#[doc(hidden)]
#[derive(Clone)]
pub struct CustomId {
    route: Arc<dyn ErasedRoute>,
    id: Option<ArrayString<MAX_ID_LEN>>,
}

impl CustomId {
    pub(crate) fn new<R>(payload: R::Payload) -> Self
    where
        R: Handled + Send + Sync + 'static,
        R::Payload: Send + Sync + 'static,
    {
        let shape = R::SHAPE;
        let mut this = Self {
            route: Arc::new(Erased::<R> {
                payload,
                ref_policy: R::RUNTIME.ref_policy(),
            }),
            id: None,
        };
        if !shape.has_select() && !shape.is_signed() {
            // A slot or an error leaves the id unresolved.
            if let Ok(EncodedId::Inline(id)) = this.route.encode(&Parts::new()) {
                this.id = Some(id);
            }
        }
        this
    }

    /// The Route id.
    pub fn route(&self) -> RouteId {
        self.route.route()
    }

    /// The encoded `custom_id`, or `None` while unresolved.
    pub fn get(&self) -> Option<&str> {
        self.id.as_ref().map(ArrayString::as_str)
    }

    /// Encodes the Route value with `parts`.
    ///
    /// An inline id is stored and gives `Ok(None)`; a Payload stored by Ref
    /// gives `Ok(Some(slot))`, to be minted and passed to
    /// [`finish`](Self::finish).
    ///
    /// # Errors
    ///
    /// The [`EncodeError`] of [`encode_id`]; the id is left unchanged.
    pub fn resolve(&mut self, parts: &Parts<'_>) -> Result<Option<PendingRef>, EncodeError> {
        match self.route.encode(parts)? {
            EncodedId::Inline(id) => {
                self.id = Some(id);
                Ok(None)
            }
            EncodedId::RefSlot(slot) => Ok(Some(slot)),
        }
    }

    /// Resolves an unresolved id for one send: `nonce` goes only to a
    /// `select` Route and `signer` only to a `signed` Route (AD-18). An id
    /// that is already resolved is kept and gives `Ok(None)`.
    ///
    /// # Errors
    ///
    /// The [`EncodeError`] of [`encode_id`] (a `signed` Route without a
    /// `signer` is [`EncodeError::MissingSigner`]); the id is left
    /// unchanged.
    #[doc(hidden)]
    pub fn resolve_for_send(
        &mut self,
        nonce: SelectNonce,
        signer: Option<&dyn Sign>,
    ) -> Result<Option<PendingRef>, EncodeError> {
        if self.id.is_some() {
            return Ok(None);
        }
        let shape = self.route.shape();
        let mut parts = Parts::new();
        if shape.has_select() {
            parts = parts.with_nonce(nonce);
        }
        if shape.is_signed()
            && let Some(signer) = signer
        {
            parts = parts.with_signer(signer);
        }
        self.resolve(&parts)
    }

    /// Stores the final id of a Ref slot returned by
    /// [`resolve`](Self::resolve).
    ///
    /// # Errors
    ///
    /// [`UiError::SlotMismatch`] when `slot` belongs to another Route; the id
    /// is left unchanged.
    pub fn finish(&mut self, slot: &PendingRef, r: Ref) -> Result<(), UiError> {
        let expected = self.route();
        if slot.route() != expected {
            return Err(UiError::SlotMismatch {
                expected,
                actual: slot.route(),
            });
        }
        self.id = Some(noctis_codec::finish(slot, r));
        Ok(())
    }
}

/// The encoded id; an unresolved id is an error naming the Route.
impl serde::Serialize for CustomId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.get() {
            Some(id) => s.serialize_str(id),
            None => Err(serde::ser::Error::custom(format_args!(
                "custom_id of route `{}` is unresolved",
                self.route()
            ))),
        }
    }
}

impl fmt::Debug for CustomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomId")
            .field("route", &self.route())
            .field("id", &self.get())
            .finish()
    }
}
