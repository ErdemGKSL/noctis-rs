//! Components built from Route values: [`Handled`], [`RouteComponents`] and
//! the hidden [`CustomId`] they carry (AD-18, AD-19, FR-58).

use core::fmt;
use std::sync::Arc;

use arrayvec::ArrayString;
use noctis_codec::{
    EncodeError, EncodedId, MAX_ID_LEN, Parts, PendingRef, Ref, Route, RouteId, SelectRoute,
    encode_id,
};

use crate::UiError;
use crate::component::{
    ChannelSelect, MentionableSelect, RoleSelect, RouteButton, SelectOption, StringSelect,
    UserSelect,
};

/// A Route that has a handler.
///
/// Only the `#[handler]` macro implements it, through
/// `::noctis::__private::ui::Handled`; it is not public API. Every
/// [`RouteComponents`] constructor requires it, so a component for a Route
/// without a handler does not compile (`[CID-04]`, FR-58).
#[diagnostic::on_unimplemented(
    message = "[CID-04] Route `{Self}` has no handler, so no component can be built for it",
    label = "no `#[handler]` for this Route",
    note = "the `#[handler]` macro implements `Handled`; add a handler for `{Self}` to a mounted Module"
)]
pub trait Handled: Route {}

/// Builds message components from a Route value; in the prelude.
///
/// The value is the component's Payload, so its `custom_id` is always the
/// Route's encoding and never a hand-written string (AD-19). Every method
/// needs the Route to have a handler ([`Handled`], `[CID-04]`); the selects
/// also need a Route declared `select` ([`SelectRoute`]).
///
/// ```
/// # use core::fmt;
/// # use noctis_codec::{Field, FieldWriter, Payload, Route, RouteId, RouteShape};
/// use noctis_ui::{ButtonStyle, RouteComponents};
///
/// struct Buy { item: u64 }
/// # impl Payload for Buy {
/// #     const MAX_ENCODED_LEN: Option<usize> = <u64 as Field>::MAX_LEN;
/// #     fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result { out.field(&self.item) }
/// # }
/// # impl Route for Buy {
/// #     const ID: RouteId = RouteId::new("shop.buy");
/// #     const SHAPE: RouteShape = RouteShape::INLINE;
/// #     const SCHEMA: u8 = 0;
/// #     type Payload = Self;
/// # }
/// # impl noctis_ui::Handled for Buy {}
///
/// let button = Buy { item: 255 }.button().style(ButtonStyle::Primary).label("Buy");
/// let json = serde_json::to_value(&button).unwrap();
/// assert_eq!(json["custom_id"], "0shop.buy:47");
/// ```
///
/// A select needs a Route declared `select`:
///
/// ```compile_fail,E0277
/// # use core::fmt;
/// # use noctis_codec::{FieldWriter, Payload, Route, RouteId, RouteShape};
/// use noctis_ui::RouteComponents;
///
/// struct Pick;
/// # impl Payload for Pick {
/// #     const MAX_ENCODED_LEN: Option<usize> = Some(0);
/// #     fn encode_fields(&self, _: &mut FieldWriter<'_>) -> fmt::Result { Ok(()) }
/// # }
/// # impl Route for Pick {
/// #     const ID: RouteId = RouteId::new("pick");
/// #     const SHAPE: RouteShape = RouteShape::INLINE;
/// #     const SCHEMA: u8 = 0;
/// #     type Payload = Self;
/// # }
/// # impl noctis_ui::Handled for Pick {}
///
/// let _ = Pick.string_select([]); // `Pick` is not declared `select`
/// ```
pub trait RouteComponents: Route + Sized {
    /// An interactive button (styles 1-4) whose `custom_id` encodes `self`;
    /// Secondary by default.
    fn button(self) -> RouteButton<Self>
    where
        Self: Route<Payload = Self> + Handled + Send + Sync + 'static,
    {
        RouteButton::from_id(CustomId::new(self))
    }

    /// A String Select (type 3) with `options`.
    fn string_select(self, options: impl IntoIterator<Item = SelectOption>) -> StringSelect
    where
        Self: Route<Payload = Self> + Handled + SelectRoute + Send + Sync + 'static,
    {
        StringSelect::from_id(CustomId::new(self), options.into_iter().collect())
    }

    /// A User Select (type 5).
    fn user_select(self) -> UserSelect
    where
        Self: Route<Payload = Self> + Handled + SelectRoute + Send + Sync + 'static,
    {
        UserSelect::from_id(CustomId::new(self))
    }

    /// A Role Select (type 6).
    fn role_select(self) -> RoleSelect
    where
        Self: Route<Payload = Self> + Handled + SelectRoute + Send + Sync + 'static,
    {
        RoleSelect::from_id(CustomId::new(self))
    }

    /// A Mentionable Select (type 7).
    fn mentionable_select(self) -> MentionableSelect
    where
        Self: Route<Payload = Self> + Handled + SelectRoute + Send + Sync + 'static,
    {
        MentionableSelect::from_id(CustomId::new(self))
    }

    /// A Channel Select (type 8).
    fn channel_select(self) -> ChannelSelect
    where
        Self: Route<Payload = Self> + Handled + SelectRoute + Send + Sync + 'static,
    {
        ChannelSelect::from_id(CustomId::new(self))
    }
}

impl<R: Route> RouteComponents for R {}

/// A Route value behind a vtable.
trait ErasedRoute: Send + Sync {
    fn route(&self) -> RouteId;
    fn encode(&self, parts: &Parts<'_>) -> Result<EncodedId, EncodeError>;
}

struct Erased<R>(R);

impl<R> ErasedRoute for Erased<R>
where
    R: Route<Payload = R> + Send + Sync,
{
    fn route(&self) -> RouteId {
        R::ID
    }

    fn encode(&self, parts: &Parts<'_>) -> Result<EncodedId, EncodeError> {
        encode_id::<R>(&self.0, parts)
    }
}

/// The `custom_id` of a Route component: the Route value and, once known,
/// its encoding.
///
/// A Route without a View envelope, select nonce or integrity tag is encoded
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
    pub(crate) fn new<R>(route: R) -> Self
    where
        R: Route<Payload = R> + Send + Sync + 'static,
    {
        let shape = R::SHAPE;
        let mut this = Self {
            route: Arc::new(Erased(route)),
            id: None,
        };
        if !shape.has_view() && !shape.has_select() && !shape.is_signed() {
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
