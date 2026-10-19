//! Two-phase encoding (AD-18): [`encode_id`] yields an inline id or a
//! [`PendingRef`] slot, and [`finish`] turns a slot and a minted [`Ref`]
//! into the final `custom_id`.
#![deny(clippy::indexing_slicing, clippy::string_slice)]

use core::fmt::Write;
use core::time::Duration;

use arrayvec::ArrayString;
use bytes::Bytes;

use crate::base62::digit;
use crate::layout::{
    Checked, FMT_LEN, MAX_ID_LEN, NONCE_LEN, REF_LEN, REF_MARKER_LEN, SCHEMA_LEN, SEP_LEN,
    VIEW_ENV_LEN, check_parts, encode,
};
use crate::parts::{Flags, SelectNonce, ViewEnvelope};
use crate::record::RefRecord;
use crate::refs::Ref;
use crate::route::{BodyKind, FieldWriter, MAX_ROUTE_LEN, Payload, Route, RouteId};
use crate::{EncodeError, Parts};

/// Longest by-Ref `custom_id`: a 24-character route with `S`, `E` and `N`,
/// then `~` and the Ref (46 characters).
pub const MAX_REF_ID_LEN: usize = FMT_LEN
    + MAX_ROUTE_LEN
    + SEP_LEN
    + SCHEMA_LEN
    + VIEW_ENV_LEN
    + NONCE_LEN
    + REF_MARKER_LEN
    + REF_LEN;

// Every by-Ref id fits `ArrayString<MAX_ID_LEN>`, so `finish` cannot fail.
const _: () = assert!(MAX_REF_ID_LEN == 46 && MAX_REF_ID_LEN <= MAX_ID_LEN);

/// The result of [`encode_id`]: a final inline id, or a slot waiting for a
/// Ref.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EncodedId {
    /// A complete inline `custom_id`.
    Inline(ArrayString<MAX_ID_LEN>),
    /// A Payload stored by Ref: mint a [`Ref`], store
    /// [`PendingRef::record`] under it, then call [`finish`].
    RefSlot(PendingRef),
}

/// A by-Ref `custom_id` waiting for its Ref (AD-18).
///
/// It carries what core stores under the Ref (route, schema, payload, TTL,
/// one-shot flag) and the per-render sections [`finish`] writes into the
/// id. Identical slots are equal, so core can share one Ref between them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PendingRef {
    route: RouteId,
    schema: u8,
    payload: Bytes,
    ttl: Option<Duration>,
    one_shot: bool,
    envelope: Option<ViewEnvelope>,
    nonce: Option<SelectNonce>,
}

impl PendingRef {
    /// The Route id.
    pub const fn route(&self) -> RouteId {
        self.route
    }

    /// The Route's schema version.
    pub const fn schema(&self) -> u8 {
        self.schema
    }

    /// The Payload's inline field body (ASCII, no length cap).
    pub const fn payload(&self) -> &Bytes {
        &self.payload
    }

    /// The Ref's TTL; `None` means the App's default.
    pub const fn ttl(&self) -> Option<Duration> {
        self.ttl
    }

    /// Whether the Ref is consumed by its first use.
    pub const fn is_one_shot(&self) -> bool {
        self.one_shot
    }

    /// The View envelope the final id carries.
    pub const fn envelope(&self) -> Option<ViewEnvelope> {
        self.envelope
    }

    /// The select nonce the final id carries.
    pub const fn nonce(&self) -> Option<SelectNonce> {
        self.nonce
    }

    /// The encoded [`RefRecord`] to store under the Ref.
    pub fn record(&self) -> Bytes {
        // `encode_id` checked the payload to be an inline body.
        let payload = core::str::from_utf8(&self.payload).unwrap_or_default();
        RefRecord::new_unchecked(self.route.as_str(), self.schema, payload).encode()
    }
}

/// Encodes a `custom_id` for Route `R`, by Ref when the Route needs one.
///
/// An inline Route gives [`encode`]'s result; a `by_ref` Route always gives
/// a [`EncodedId::RefSlot`]; a `spill` Route gives [`EncodedId::Inline`]
/// when the Payload fits and a slot otherwise. It never returns
/// [`EncodeError::NeedsRef`]. A slot's id carries no tag, even for a signed
/// Route: the Ref is already unguessable.
///
/// ```
/// use noctis_codec::{EncodedId, Parts, Ref, encode_id, finish, decode_route, PayloadSource};
/// # use core::fmt;
/// # use noctis_codec::{CodecError, FieldReader, FieldWriter, Payload, PayloadDecode, Route, RouteId, RouteShape};
/// # #[derive(Debug, PartialEq)]
/// # struct Qty(u8);
/// # impl Payload for Qty {
/// #     fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result { out.field(&self.0) }
/// # }
/// # impl<'de> PayloadDecode<'de> for Qty {
/// #     fn decode_fields(f: &mut FieldReader<'de>) -> Result<Self, CodecError> { Ok(Self(f.field()?)) }
/// # }
/// struct Stored;
/// impl Route for Stored {
///     const ID: RouteId = RouteId::new("r");
///     const SHAPE: RouteShape = RouteShape::BY_REF;
///     const SCHEMA: u8 = 0;
///     type Payload = Qty;
/// }
///
/// // Phase 1: encode. Phase 2: mint and store. Phase 3: finish.
/// let EncodedId::RefSlot(slot) = encode_id::<Stored>(&Qty(3), &Parts::new())? else {
///     unreachable!("a by_ref route always gives a slot")
/// };
/// assert_eq!(&slot.record()[..], b"0r:3");
/// let r = Ref::mint()?;
/// // core: store `slot.record()` under `r`.
/// let id = finish(&slot, r);
/// assert_eq!(id.as_str(), format!("0r:~{r}"));
/// assert_eq!(decode_route::<Stored>(&id), Ok(PayloadSource::Ref(r)));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
///
/// The parts checks of [`encode`] in the same order
/// ([`EncodeError::InvalidSchema`], the envelope, nonce and signer,
/// [`EncodeError::UnexpectedRefPolicy`], [`EncodeError::ZeroTtl`]); then
/// [`encode`]'s errors for an inline Route or a fitting `spill` Route;
/// [`EncodeError::Field`] when a field's own `encode` fails;
/// [`EncodeError::InvalidBody`] when a field writes a byte an inline body
/// cannot hold.
pub fn encode_id<R: Route>(
    payload: &R::Payload,
    parts: &Parts<'_>,
) -> Result<EncodedId, EncodeError> {
    let checked = check_parts::<R>(parts)?;
    match R::SHAPE.body() {
        BodyKind::Inline => encode::<R>(payload, parts).map(EncodedId::Inline),
        BodyKind::Spill => match encode::<R>(payload, parts) {
            Err(EncodeError::NeedsRef) => slot::<R>(payload, &checked),
            other => other.map(EncodedId::Inline),
        },
        BodyKind::ByRef => slot::<R>(payload, &checked),
    }
}

/// Writes `payload`'s fields into a [`PendingRef`].
fn slot<R: Route>(payload: &R::Payload, checked: &Checked<'_>) -> Result<EncodedId, EncodeError> {
    let mut body = String::new();
    payload
        .encode_fields(&mut FieldWriter::new(&mut body))
        .map_err(|_| EncodeError::Field)?;
    RefRecord::new(R::ID.as_str(), R::SCHEMA, &body)?;
    Ok(EncodedId::RefSlot(PendingRef {
        route: R::ID,
        schema: R::SCHEMA,
        payload: Bytes::from(body),
        ttl: checked.ref_policy.ttl(),
        one_shot: checked.ref_policy.is_one_shot(),
        envelope: checked.envelope,
        nonce: checked.nonce,
    }))
}

/// Builds the final `custom_id` of a slot: `F route ":" [S] [E] [N] "~" ref`.
///
/// `F` carries the `S`, `E` and `N` flags present, never `T`: a Ref body is
/// never tagged. The id is at most [`MAX_REF_ID_LEN`] (46) characters.
///
/// # Panics
///
/// Never in practice: a by-ref id is at most [`MAX_REF_ID_LEN`] characters,
/// which is compile-time asserted to fit [`MAX_ID_LEN`], so the internal
/// capacity assertion cannot fire.
pub fn finish(slot: &PendingRef, r: Ref) -> ArrayString<MAX_ID_LEN> {
    let mut flags = Flags::EMPTY;
    for (present, flag) in [
        (slot.schema > 0, Flags::S),
        (slot.envelope.is_some(), Flags::E),
        (slot.nonce.is_some(), Flags::N),
    ] {
        if present {
            flags = flags | flag;
        }
    }
    let mut out = ArrayString::<MAX_ID_LEN>::new();
    // At most `MAX_REF_ID_LEN` <= `MAX_ID_LEN` characters (asserted above),
    // so no write can fail.
    let written = (|| -> core::fmt::Result {
        out.write_char(char::from(digit(flags.bits())))?;
        out.write_str(slot.route.as_str())?;
        out.write_char(':')?;
        if slot.schema > 0 {
            out.write_char(char::from(digit(slot.schema)))?;
        }
        if let Some(e) = slot.envelope {
            write!(out, "{e}")?;
        }
        if let Some(n) = slot.nonce {
            write!(out, "{n}")?;
        }
        out.write_char('~')?;
        out.write_str(r.as_str())
    })();
    assert!(written.is_ok(), "a by-ref id fits MAX_ID_LEN");
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{MAX_REF_ID_LEN, PendingRef, finish};
    use crate::{RawBody, Ref, RouteId, SelectNonce, ViewEnvelope, decode};

    fn envelope() -> impl Strategy<Value = Option<ViewEnvelope>> {
        proptest::option::of(
            (
                proptest::char::range('0', 'z'),
                0..=61u8,
                0..=3843u16,
                0..=61u8,
            )
                .prop_filter_map("valid envelope", |(a, t, p, v)| {
                    ViewEnvelope::new(a, t, p, v).ok()
                }),
        )
    }

    proptest! {
        #[test]
        fn finish_decodes_as_ref(
            route in "[a-z0-9._-]{1,24}",
            schema in 0..=61u8,
            envelope in envelope(),
            nonce in proptest::option::of((0..=3843u16).prop_filter_map("nonce", SelectNonce::new)),
        ) {
            let slot = PendingRef {
                route: RouteId::new(Box::leak(route.clone().into_boxed_str())),
                schema,
                payload: bytes::Bytes::new(),
                ttl: None,
                one_shot: false,
                envelope,
                nonce,
            };
            let r = Ref::mint().unwrap();
            let id = finish(&slot, r);
            let predicted = 1 + route.len() + 1 + usize::from(schema > 0)
                + if envelope.is_some() { 5 } else { 0 }
                + if nonce.is_some() { 2 } else { 0 }
                + 1 + 11;
            prop_assert_eq!(id.len(), predicted);
            prop_assert!(id.len() <= MAX_REF_ID_LEN);
            let decoded = decode(&id).unwrap();
            prop_assert_eq!(decoded.route(), route.as_str());
            prop_assert_eq!(decoded.schema(), schema);
            prop_assert_eq!(decoded.envelope(), envelope);
            prop_assert_eq!(decoded.nonce(), nonce);
            prop_assert_eq!(decoded.tag(), None);
            prop_assert_eq!(decoded.body(), RawBody::Ref(r));
        }
    }
}
