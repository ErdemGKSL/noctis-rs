//! `custom_id` layout, `Route`/`Payload`/`Field` traits, `EncodedId`, `RefRecord` and `BoundedStr`.
//!
//! This crate is internal; use `noctis` instead. Its API is not stable and may
//! change in any release.
//!
//! # Payload fields
//!
//! A Payload field implements [`Field`] (encoding and worst-case width
//! `MAX_LEN`) and [`FieldDecode`] (decoding from a `&str` slice). Integers are
//! minimal-digit base62 (`0-9A-Za-z`), signed integers zig-zag first; `bool`
//! is `0`/`1`; `Option<T>` is `0` or `1` followed by `T`; Discord ids are
//! their `u64`.
//!
//! String-like fields (`BoundedStr<N>`, `String`, `Cow<str>`, `Vec<T>`) escape:
//!
//! | Character | Encoding |
//! |---|---|
//! | `!` | `!0` |
//! | `,` | `!1` |
//! | `:` | `!2` |
//! | `~` | `!3` |
//! | other printable ASCII (`0x20..=0x7E`) | itself |
//! | anything else (`String`/`Cow` only) | `!u` + 6 lowercase hex digits |
//!
//! Every encoding is printable ASCII without `,`, `:` or `~` (CID-07).
//! Decoding is canonical: when `decode(s)` returns `Ok(v)`, `encode(v)` is
//! exactly `s`, so leading zeros, signs, unnecessary escapes and over-long
//! inputs are [`FieldError`]s; decoding never panics.
//!
//! # Layout
//!
//! A `custom_id` is `F route ":" [S] [E] [N] body [T]`, ASCII only (AD-5):
//!
//! | Section | Width | Content |
//! |---|---|---|
//! | `F` | [`FMT_LEN`] | base62 digit of `16 * generation + flags`; flags [`Flags::S`] = 1, [`Flags::E`] = 2, [`Flags::N`] = 4, [`Flags::T`] = 8; generation 0 |
//! | route | 1..=24 | [`RouteId`]: `[a-z0-9._-]`, `_`-prefixed ids reserved |
//! | `:` | [`SEP_LEN`] | separator |
//! | `S` | [`SCHEMA_LEN`] | schema version 1..=61, absent for schema 0 |
//! | `E` | [`VIEW_ENV_LEN`] | [`ViewEnvelope`]: action, tab, page (2 digits), state version |
//! | `N` | [`NONCE_LEN`] | [`SelectNonce`] |
//! | body | | inline fields joined by `,`, or `~` ([`REF_MARKER_LEN`]) + a [`REF_LEN`]-character Ref |
//! | `T` | [`TAG_LEN`] | [`Tag`] over every preceding character; inline bodies of signed Routes only |
//!
//! [`encode`] writes an inline id for a [`Route`] into an
//! `ArrayString<100>` ([`MAX_ID_LEN`], CID-01).
//!
//! # Two-phase decode
//!
//! [`decode`] is total and Route-agnostic: it splits any `&str` into a
//! [`Decoded`] or returns a [`CodecError`], never panicking (CID-06), and the
//! parts of a [`Decoded`] rebuild the input exactly. [`Decoded::payload`]
//! (or [`decode_route`] in one call) then checks the id against a Route and
//! classifies it:
//!
//! - [`DecodeError::Forged`]: a different route.
//! - [`DecodeError::Stale`]: a different schema, body kind or `E`/`N`/`T`
//!   flags, because the Route changed since the id was sent (FR-60).
//! - [`DecodeError::Malformed`]: a phase-1 error, or an inline body that is
//!   not the Payload's encoding.
//!
//! Decoding is canonical with one exception: missing trailing fields take
//! their [`FieldDecode::absent`] value, so appending an `Option` field to a
//! Payload needs no schema bump, and an id with an explicit trailing `None`
//! and one without it decode to the same Payload.

mod base62;
mod bounded_str;
pub mod check;
mod error;
mod escape;
mod field;
mod layout;
mod parts;
mod route;

pub use bounded_str::{BoundedStr, BoundedStrError};
pub use error::{
    CodecError, DecodeError, EncodeError, EnvelopeError, FieldError, ForgedReason, Section,
    StaleReason,
};
pub use field::{Field, FieldDecode};
pub use layout::{
    Decoded, FMT_LEN, MAX_ID_LEN, NONCE_LEN, Parts, PayloadSource, REF_LEN, REF_MARKER_LEN,
    RawBody, SCHEMA_LEN, SEP_LEN, TAG_LEN, VIEW_ENV_LEN, decode, decode_route, encode,
};
pub use parts::{Flags, SelectNonce, Sign, Tag, ViewEnvelope};
pub use route::{
    BodyKind, FieldReader, FieldWriter, Payload, PayloadDecode, Route, RouteId, RouteShape,
};
