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
//! A `custom_id` is `F route ":" [S] [N] body [T]`, ASCII only (AD-5):
//!
//! | Section | Width | Content |
//! |---|---|---|
//! | `F` | [`FMT_LEN`] | base62 digit of `16 * generation + flags`; flags [`Flags::S`] = 1, [`Flags::N`] = 4, [`Flags::T`] = 8 (flag value 2, `0b0010`, is reserved and decodes as [`CodecError::ReservedFlag`]); generation 0 |
//! | route | 1..=24 | [`RouteId`]: `[a-z0-9._-]`, `_`-prefixed ids reserved |
//! | `:` | [`SEP_LEN`] | separator |
//! | `S` | [`SCHEMA_LEN`] | schema version 1..=61, absent for schema 0 |
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
//! - [`DecodeError::Forged`]: a different route, or a signed Route's
//!   inline id whose tag is missing or does not verify.
//! - [`DecodeError::Stale`]: a different schema, body kind or `N`/`T`
//!   flags, because the Route changed since the id was sent (FR-60).
//! - [`DecodeError::Malformed`]: a phase-1 error, or an inline body that is
//!   not the Payload's encoding.
//!
//! Decoding is canonical with one exception: missing trailing fields take
//! their [`FieldDecode::absent`] value, so appending an `Option` field to a
//! Payload needs no schema bump, and an id with an explicit trailing `None`
//! and one without it decode to the same Payload.
//!
//! [`Decoded::payload`] and [`decode_route`] hold no keys and fail closed;
//! signed Routes decode through [`Decoded::verified_payload`] or
//! [`decode_route_verified`] (see below).
//!
//! # Integrity tags
//!
//! The inline id of a `signed` Route ends in a [`Tag`] (AD-5, FR-19):
//!
//! ```text
//! tag = Tag::from_u64(u64::from_le_bytes(blake3::keyed_hash(key, signed)[..8]))
//! ```
//!
//! where `signed` is every character before `T` (no prefix, no domain
//! string), so the tag binds `F`, the route, `S`, `N` and the body. Ref
//! bodies are never tagged.
//!
//! - [`PayloadKey`] is the 32-byte key, parsed from 64 hex digits
//!   (`NOCTIS_PAYLOAD_KEY`) by [`PayloadKey::from_hex`] or built by
//!   [`PayloadKey::from_bytes`]. It implements [`Sign`] and [`Verify`].
//! - [`PayloadKeys`] is the rotation ring: the current key signs; the
//!   current key or one previous key ([`PayloadKeys::with_previous`])
//!   verifies.
//! - Tags are compared in constant time, and the ring checks both keys
//!   without short-circuiting.
//! - [`decode_route_verified`] reports a signed Route's inline id without a
//!   tag as [`ForgedReason::MissingTag`] and a tag that verifies under no
//!   key as [`ForgedReason::BadTag`]; a tag on an unsigned Route stays
//!   [`DecodeError::Stale`].
//! - The tag is about 35.7 bits (6 base62 digits; a guess forges with
//!   chance about 1.8e-11) and binds only the id's characters, not a user,
//!   message or time, so whoever sees a valid id can replay it: a tag proves
//!   integrity, not authorization.
//! - Keys are redacted: `Debug` prints `PayloadKey(<redacted>)`, `Display`
//!   prints `<redacted>`, there is no byte getter, and [`KeyError`] carries
//!   only lengths and indices (AD-29, NFR-6).
//!
//! # Refs
//!
//! A `by_ref` Route, or a `spill` Route whose Payload does not fit inline,
//! stores its Payload under a [`Ref`]: 11 base62 characters minted by
//! [`Ref::mint`] from the OS CSPRNG (`getrandom`) by unbiased rejection
//! sampling (≈ 65.5 bits), never derived from a counter, a clock or
//! interaction data (CID-05). Encoding takes two phases (AD-18):
//!
//! 1. [`encode_id`] returns [`EncodedId::Inline`] for an id that is
//!    complete, or [`EncodedId::RefSlot`] with a [`PendingRef`] carrying the
//!    route, schema, payload, TTL and one-shot flag (set through
//!    [`Parts::with_ref_policy`] and [`RefPolicy`]).
//! 2. Core mints a [`Ref`], stores [`PendingRef::record`] under it, and
//!    calls [`finish`], which writes `F route ":" [S] [N] "~" ref`, at
//!    most [`MAX_REF_ID_LEN`] (41) characters and never tagged.
//!
//! The stored [`RefRecord`] is `F route ":" [S] body` in the id's grammar,
//! with no length cap and no `N` or `T`. Decoding a by-Ref id gives
//! [`PayloadSource::Ref`]; after resolving the Ref, [`decode_record`]
//! checks the record against the Route:
//!
//! - a malformed record or body is [`DecodeError::Malformed`];
//! - a record of another route is [`DecodeError::Forged`] with
//!   [`ForgedReason::RecordRouteMismatch`];
//! - a different schema is [`DecodeError::Stale`].
//!
//! # Derive support
//!
//! A [`Payload`] declares its worst-case body width,
//! [`Payload::MAX_ENCODED_LEN`]. The `noctis` derives (`#[derive(Route)]`,
//! `#[derive(Payload)]`, `#[derive(PayloadEnum)]`) use a few hidden items of
//! this crate through `::noctis::__private::codec`: `sum_fields` adds field
//! widths, `route_width` and `RouteWidth` compute a Route's worst-case id
//! section by section, and `assert_fits` fails const evaluation, naming the
//! route id and the worst case, when a Route can exceed 100 characters
//! (CID-01). `encode_variant` and `decode_variant` write and read a
//! `PayloadEnum` variant index as one base62 digit, and `RouteRuntime` is
//! the (still empty) runtime declaration of a derived Route.
//!
//! # Allocation
//!
//! Decoding a fixed-width inline id allocates nothing (AD-25, NFR-9):
//! [`decode`], [`decode_route`], [`decode_route_verified`],
//! [`Decoded::payload`] and [`Decoded::verified_payload`] read the crate's
//! built-in fixed-width fields and `PayloadEnum` fields straight from `&str`
//! slices into stack values, and every error is `Copy`. A custom
//! [`FieldDecode`] impl must keep this guarantee itself. `String` fields,
//! `Cow<str>` fields with escapes and `Vec` fields may allocate. The crate's
//! `tests/alloc_decode.rs` guards this with a counting allocator.

mod base62;
mod bounded_str;
pub mod check;
mod derive;
mod encoded;
mod error;
mod escape;
mod field;
mod key;
mod layout;
mod parts;
mod record;
mod refs;
mod route;

pub use bounded_str::{BoundedStr, BoundedStrError};
#[doc(hidden)]
pub use derive::{
    DeclaredRuntime, RouteRuntime, RouteWidth, assert_fits, decode_variant, encode_variant,
    route_width, sum_fields,
};
pub use encoded::{EncodedId, MAX_REF_ID_LEN, PendingRef, encode_id, finish};
pub use error::{
    CodecError, DecodeError, EncodeError, FieldError, ForgedReason, KeyError, RefError, Section,
    StaleReason,
};
pub use field::{Field, FieldDecode};
pub use key::{PayloadKey, PayloadKeys};
pub use layout::{
    Decoded, FMT_LEN, MAX_ID_LEN, NONCE_LEN, Parts, PayloadSource, REF_LEN, REF_MARKER_LEN,
    RawBody, RefPolicy, SCHEMA_LEN, SEP_LEN, TAG_LEN, decode, decode_route, decode_route_verified,
    encode,
};
pub use parts::{Flags, SelectNonce, Sign, Tag, Verify};
pub use record::{RefRecord, decode_record};
pub use refs::Ref;
pub use route::{
    BodyKind, FieldReader, FieldWriter, Payload, PayloadDecode, Route, RouteId, RouteShape,
    SelectRoute,
};
