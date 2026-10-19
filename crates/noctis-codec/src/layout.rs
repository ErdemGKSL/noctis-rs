//! The `custom_id` layout (AD-5): its constants, the encoder and the total,
//! two-phase decoder.
//!
//! `custom_id = F route ":" [S] [E] [N] body [T]`, ASCII only.
#![deny(clippy::indexing_slicing, clippy::string_slice)]

use core::fmt::{self, Write};
use core::time::Duration;

use arrayvec::ArrayString;

use crate::base62::{digit, digit_value};
use crate::parts::{DIGIT_MAX as MAX_SCHEMA, Flags, SelectNonce, Sign, Tag, ViewEnvelope};
use crate::refs::Ref;
use crate::route::{
    BodyKind, FieldReader, FieldWriter, MAX_ROUTE_LEN, Payload, PayloadDecode, Route, is_route_byte,
};
use crate::{CodecError, DecodeError, EncodeError, ForgedReason, Section, StaleReason};

/// Width of the format digit `F`.
pub const FMT_LEN: usize = 1;
/// Width of the route separator `:`.
pub const SEP_LEN: usize = 1;
/// Width of the schema version `S`.
pub const SCHEMA_LEN: usize = 1;
/// Width of the View envelope `E`.
pub const VIEW_ENV_LEN: usize = 5;
/// Width of the select nonce `N`.
pub const NONCE_LEN: usize = 2;
/// Width of the Ref marker `~`.
pub const REF_MARKER_LEN: usize = 1;
/// Width of a Ref.
pub const REF_LEN: usize = 11;
/// Width of the integrity tag `T`.
pub const TAG_LEN: usize = 6;
/// Longest `custom_id`, in characters (CID-01).
pub const MAX_ID_LEN: usize = 100;

/// How the Ref of a by-ref `custom_id` is kept: its time to live and
/// whether it is consumed by its first use (FR-17).
///
/// The default is the App's TTL, reusable. A non-default policy is only
/// accepted for `by_ref` and `spill` Routes, and applies to a `spill` Route
/// only when its Payload does not fit inline.
///
/// ```
/// use core::time::Duration;
/// use noctis_codec::RefPolicy;
///
/// const POLICY: RefPolicy = RefPolicy::new().with_ttl(Duration::from_secs(60)).one_shot();
/// assert_eq!(POLICY.ttl(), Some(Duration::from_secs(60)));
/// assert!(POLICY.is_one_shot());
/// assert_eq!(RefPolicy::new(), RefPolicy::default());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RefPolicy {
    ttl: Option<Duration>,
    one_shot: bool,
}

impl RefPolicy {
    /// The App's default TTL, reusable.
    pub const fn new() -> Self {
        Self {
            ttl: None,
            one_shot: false,
        }
    }

    /// Keeps the Ref for `ttl` instead of the App's default.
    #[must_use]
    pub const fn with_ttl(self, ttl: Duration) -> Self {
        Self {
            ttl: Some(ttl),
            ..self
        }
    }

    /// Consumes the Ref on its first use.
    #[must_use]
    pub const fn one_shot(self) -> Self {
        Self {
            one_shot: true,
            ..self
        }
    }

    /// The TTL; `None` means the App's default.
    pub const fn ttl(&self) -> Option<Duration> {
        self.ttl
    }

    /// Whether the Ref is consumed by its first use.
    pub const fn is_one_shot(&self) -> bool {
        self.one_shot
    }

    const fn is_default(&self) -> bool {
        self.ttl.is_none() && !self.one_shot
    }
}

/// The per-render sections of a `custom_id` that are not part of the Payload.
///
/// Give exactly the sections the Route's [`RouteShape`](crate::RouteShape)
/// declares; [`encode`] and [`encode_id`](crate::encode_id) reject a missing
/// or an unexpected one.
#[derive(Clone, Copy, Default)]
pub struct Parts<'s> {
    envelope: Option<ViewEnvelope>,
    nonce: Option<SelectNonce>,
    signer: Option<&'s dyn Sign>,
    ref_policy: RefPolicy,
}

impl<'s> Parts<'s> {
    /// No section.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the View envelope of a `view` Route.
    #[must_use]
    pub fn with_envelope(self, envelope: ViewEnvelope) -> Self {
        Self {
            envelope: Some(envelope),
            ..self
        }
    }

    /// Adds the select nonce of a `select` Route.
    #[must_use]
    pub fn with_nonce(self, nonce: SelectNonce) -> Self {
        Self {
            nonce: Some(nonce),
            ..self
        }
    }

    /// Adds the signer of a `signed` Route.
    #[must_use]
    pub fn with_signer(self, signer: &'s dyn Sign) -> Self {
        Self {
            signer: Some(signer),
            ..self
        }
    }

    /// Sets the [`RefPolicy`] of a `by_ref` or `spill` Route's Ref.
    #[must_use]
    pub fn with_ref_policy(self, ref_policy: RefPolicy) -> Self {
        Self { ref_policy, ..self }
    }
}

impl fmt::Debug for Parts<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parts")
            .field("envelope", &self.envelope)
            .field("nonce", &self.nonce)
            .field("signer", &self.signer.is_some())
            .field("ref_policy", &self.ref_policy)
            .finish()
    }
}

/// Matches a Route's declared section against the one given.
fn section<T>(
    declared: bool,
    given: Option<T>,
    missing: EncodeError,
    unexpected: EncodeError,
) -> Result<Option<T>, EncodeError> {
    match (declared, given) {
        (true, Some(v)) => Ok(Some(v)),
        (true, None) => Err(missing),
        (false, Some(_)) => Err(unexpected),
        (false, None) => Ok(None),
    }
}

/// The sections of [`Parts`] checked against a Route.
pub(crate) struct Checked<'s> {
    pub(crate) envelope: Option<ViewEnvelope>,
    pub(crate) nonce: Option<SelectNonce>,
    pub(crate) signer: Option<&'s dyn Sign>,
    pub(crate) ref_policy: RefPolicy,
}

/// Checks Route `R`'s schema and `parts` against its shape; shared by
/// [`encode`] and [`encode_id`](crate::encode_id).
///
/// Order: [`EncodeError::InvalidSchema`]; a missing or unexpected envelope,
/// nonce or signer; [`EncodeError::UnexpectedRefPolicy`];
/// [`EncodeError::ZeroTtl`].
pub(crate) fn check_parts<'s, R: Route>(parts: &Parts<'s>) -> Result<Checked<'s>, EncodeError> {
    let shape = R::SHAPE;
    if R::SCHEMA > MAX_SCHEMA {
        return Err(EncodeError::InvalidSchema { schema: R::SCHEMA });
    }
    let envelope = section(
        shape.has_view(),
        parts.envelope,
        EncodeError::MissingEnvelope,
        EncodeError::UnexpectedEnvelope,
    )?;
    let nonce = section(
        shape.has_select(),
        parts.nonce,
        EncodeError::MissingNonce,
        EncodeError::UnexpectedNonce,
    )?;
    let signer = section(
        shape.is_signed(),
        parts.signer,
        EncodeError::MissingSigner,
        EncodeError::UnexpectedSigner,
    )?;
    let ref_policy = parts.ref_policy;
    if !ref_policy.is_default() && shape.body() == BodyKind::Inline {
        return Err(EncodeError::UnexpectedRefPolicy);
    }
    if ref_policy.ttl() == Some(Duration::ZERO) {
        return Err(EncodeError::ZeroTtl);
    }
    Ok(Checked {
        envelope,
        nonce,
        signer,
        ref_policy,
    })
}

/// A `fmt::Write` into the id that fails once `limit` would be passed.
struct Limited<'a> {
    out: &'a mut ArrayString<MAX_ID_LEN>,
    limit: usize,
    overflow: bool,
}

impl Write for Limited<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if self.out.len().saturating_add(s.len()) > self.limit {
            self.overflow = true;
            return Err(fmt::Error);
        }
        self.out.try_push_str(s).map_err(|_| fmt::Error)
    }
}

/// Encodes an inline `custom_id` for Route `R`.
///
/// `F` is the base62 digit of the flags (generation 0), and only the
/// sections the Route declares are written. A signed Route's signer receives
/// exactly the characters before the tag.
///
/// # Errors
///
/// Checked in this order: [`EncodeError::InvalidSchema`]; a missing or
/// unexpected envelope, nonce or signer in `parts`;
/// [`EncodeError::UnexpectedRefPolicy`] for a non-default [`RefPolicy`] on
/// an inline Route; [`EncodeError::ZeroTtl`]; [`EncodeError::NeedsRef`]
/// for a `by_ref` Route; then, when the id would be over 100 characters
/// (tag included), [`EncodeError::TooLong`] for an inline Route and
/// [`EncodeError::NeedsRef`] for a `spill` Route. [`EncodeError::Field`] when
/// a field's own `encode` fails.
pub fn encode<R: Route>(
    payload: &R::Payload,
    parts: &Parts<'_>,
) -> Result<ArrayString<MAX_ID_LEN>, EncodeError> {
    let shape = R::SHAPE;
    let Checked {
        envelope,
        nonce,
        signer,
        ..
    } = check_parts::<R>(parts)?;
    if shape.body() == BodyKind::ByRef {
        return Err(EncodeError::NeedsRef);
    }

    let mut flags = Flags::EMPTY;
    for (present, flag) in [
        (R::SCHEMA > 0, Flags::S),
        (envelope.is_some(), Flags::E),
        (nonce.is_some(), Flags::N),
        (signer.is_some(), Flags::T),
    ] {
        if present {
            flags = flags | flag;
        }
    }

    let mut out = ArrayString::<MAX_ID_LEN>::new();
    // The prefix is at most 1 + 24 + 1 + 1 + 5 + 2 = 34 characters, so these
    // writes cannot overflow; an error is reported as `TooLong` regardless.
    let prefix = (|| -> fmt::Result {
        out.write_char(char::from(digit(flags.bits())))?;
        out.write_str(R::ID.as_str())?;
        out.write_char(':')?;
        if R::SCHEMA > 0 {
            out.write_char(char::from(digit(R::SCHEMA)))?;
        }
        if let Some(e) = envelope {
            write!(out, "{e}")?;
        }
        if let Some(n) = nonce {
            write!(out, "{n}")?;
        }
        Ok(())
    })();
    prefix.map_err(|_| EncodeError::TooLong)?;

    let limit = MAX_ID_LEN - if signer.is_some() { TAG_LEN } else { 0 };
    let mut sink = Limited {
        out: &mut out,
        limit,
        overflow: false,
    };
    let fields = payload.encode_fields(&mut FieldWriter::new(&mut sink));
    // Overflow wins even when the Payload swallowed the writer's error.
    match (sink.overflow, shape.body(), fields) {
        (true, BodyKind::Spill, _) => return Err(EncodeError::NeedsRef),
        (true, _, _) => return Err(EncodeError::TooLong),
        (false, _, Err(_)) => return Err(EncodeError::Field),
        (false, _, Ok(())) => {}
    }

    if let Some(signer) = signer {
        let tag = signer.sign(out.as_str());
        out.try_push_str(tag.as_str())
            .map_err(|_| EncodeError::TooLong)?;
    }
    Ok(out)
}

/// The body of a decoded `custom_id`, not yet decoded against a Payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawBody<'a> {
    /// An inline body: fields joined by `,`.
    Inline(&'a str),
    /// A Ref body: the 11 characters after `~`.
    Ref(Ref),
}

/// Where a Route's Payload is: decoded from an inline body, or behind a Ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadSource<P> {
    /// The Payload decoded from the inline body.
    Inline(P),
    /// The Ref the Payload is stored under; resolve it, then decode the
    /// stored record with [`decode_record`](crate::decode_record).
    Ref(Ref),
}

/// A well-formed `custom_id`, split into its sections (phase 1).
///
/// Its parts rebuild the input exactly. It is not yet checked against a
/// Route: look the Route up by [`Decoded::route`], then call
/// [`Decoded::payload`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded<'a> {
    raw: &'a str,
    flags: Flags,
    route_end: usize,
    schema: u8,
    envelope: Option<ViewEnvelope>,
    nonce: Option<SelectNonce>,
    body_start: usize,
    body_end: usize,
    is_ref: bool,
    tag: Option<Tag>,
}

impl<'a> Decoded<'a> {
    /// The section flags.
    pub const fn flags(&self) -> Flags {
        self.flags
    }

    /// The route id.
    pub fn route(&self) -> &'a str {
        self.raw.get(FMT_LEN..self.route_end).unwrap_or_default()
    }

    /// The schema version (`0` when `S` is absent).
    pub const fn schema(&self) -> u8 {
        self.schema
    }

    /// The View envelope, when `E` is present.
    pub const fn envelope(&self) -> Option<ViewEnvelope> {
        self.envelope
    }

    /// The select nonce, when `N` is present.
    pub const fn nonce(&self) -> Option<SelectNonce> {
        self.nonce
    }

    /// The body.
    pub fn body(&self) -> RawBody<'a> {
        if self.is_ref {
            let start = self.body_start.saturating_add(REF_MARKER_LEN);
            let digits = self
                .raw
                .as_bytes()
                .get(start..self.body_end)
                .and_then(|s| <[u8; REF_LEN]>::try_from(s).ok())
                .unwrap_or([b'0'; REF_LEN]);
            // `decode` checked the 11 bytes after `~` to be base62 digits.
            RawBody::Ref(Ref::from_digits(digits))
        } else {
            RawBody::Inline(
                self.raw
                    .get(self.body_start..self.body_end)
                    .unwrap_or_default(),
            )
        }
    }

    /// The integrity tag, when `T` is present. It is not verified here.
    pub const fn tag(&self) -> Option<Tag> {
        self.tag
    }

    /// The characters the tag covers (all before it), when `T` is present.
    pub fn signed_part(&self) -> Option<&'a str> {
        self.tag?;
        self.raw.get(..self.body_end)
    }

    /// The whole `custom_id`.
    pub const fn as_str(&self) -> &'a str {
        self.raw
    }

    /// Checks the id against Route `R` and decodes its Payload (phase 2).
    ///
    /// The tag is not verified here.
    ///
    /// # Errors
    ///
    /// Checked in this order: a different route is
    /// [`DecodeError::Forged`]; a different schema, a body kind the Route
    /// does not use, or `E`/`N`/`T` flags that differ from the Route's (`T`
    /// is expected for an inline body of a signed Route) are
    /// [`DecodeError::Stale`]; an inline body that does not decode is
    /// [`DecodeError::Malformed`].
    pub fn payload<R: Route>(&self) -> Result<PayloadSource<R::Payload>, DecodeError>
    where
        R::Payload: PayloadDecode<'a>,
    {
        if self.route() != R::ID.as_str() {
            return Err(DecodeError::Forged(ForgedReason::RouteMismatch));
        }
        if self.schema != R::SCHEMA {
            return Err(DecodeError::Stale(StaleReason::Schema {
                expected: R::SCHEMA,
                found: self.schema,
            }));
        }
        let shape = R::SHAPE;
        let allowed = match shape.body() {
            BodyKind::Inline => !self.is_ref,
            BodyKind::ByRef => self.is_ref,
            BodyKind::Spill => true,
        };
        if !allowed {
            return Err(DecodeError::Stale(StaleReason::Body {
                expected: shape.body(),
            }));
        }
        let mut expected = Flags::EMPTY;
        for (present, flag) in [
            (shape.has_view(), Flags::E),
            (shape.has_select(), Flags::N),
            (shape.is_signed() && !self.is_ref, Flags::T),
        ] {
            if present {
                expected = expected | flag;
            }
        }
        let found = self.flags.intersection(Flags::E | Flags::N | Flags::T);
        if found != expected {
            return Err(DecodeError::Stale(StaleReason::Flags { expected, found }));
        }
        match self.body() {
            RawBody::Ref(r) => Ok(PayloadSource::Ref(r)),
            RawBody::Inline(body) => {
                let mut reader = FieldReader::new(body);
                let payload = R::Payload::decode_fields(&mut reader)?;
                reader.finish()?;
                Ok(PayloadSource::Inline(payload))
            }
        }
    }
}

/// The base62 digit at byte `index`.
fn digit_at(bytes: &[u8], index: usize) -> Result<u8, CodecError> {
    bytes
        .get(index)
        .copied()
        .and_then(digit_value)
        .ok_or(CodecError::InvalidChar { index })
}

/// The `N` fixed bytes at `pos`, or `Truncated { section }`.
fn fixed<const N: usize>(
    bytes: &[u8],
    pos: usize,
    section: Section,
) -> Result<[u8; N], CodecError> {
    bytes
        .get(pos..pos.saturating_add(N))
        .and_then(|s| <[u8; N]>::try_from(s).ok())
        .ok_or(CodecError::Truncated { section })
}

/// Parses the format digit `F` at byte 0 into its flags (generation 0).
pub(crate) fn parse_format(bytes: &[u8]) -> Result<Flags, CodecError> {
    let format = digit_at(bytes, 0)?;
    if format >= 16 {
        return Err(CodecError::Generation {
            generation: format / 16,
        });
    }
    Ok(Flags::from_bits(format))
}

/// Scans the route after `F`; returns the byte index of its `:`.
pub(crate) fn parse_route(bytes: &[u8]) -> Result<usize, CodecError> {
    let len = bytes.len();
    let route_end = bytes
        .iter()
        .skip(FMT_LEN)
        .position(|&b| !is_route_byte(b))
        .map_or(len, |p| p + FMT_LEN);
    match bytes.get(route_end) {
        Some(b':') => {}
        Some(_) => return Err(CodecError::InvalidChar { index: route_end }),
        None => return Err(CodecError::MissingSeparator),
    }
    let route_len = route_end - FMT_LEN;
    if route_len == 0 {
        return Err(CodecError::EmptyRoute);
    }
    if route_len > MAX_ROUTE_LEN {
        return Err(CodecError::RouteTooLong { len: route_len });
    }
    Ok(route_end)
}

/// Parses `S` at byte `pos` when `flags` announce it; returns the schema
/// (`0` when absent) and the position after it.
pub(crate) fn parse_schema(
    bytes: &[u8],
    pos: usize,
    flags: Flags,
) -> Result<(u8, usize), CodecError> {
    if !flags.contains(Flags::S) {
        return Ok((0, pos));
    }
    match bytes.get(pos) {
        None => Err(CodecError::Truncated {
            section: Section::Schema,
        }),
        Some(b'0') => Err(CodecError::ZeroSchema),
        Some(_) => Ok((digit_at(bytes, pos)?, pos + SCHEMA_LEN)),
    }
}

/// Whether `b` may appear in an inline body: printable ASCII except `:`
/// and `~`.
pub(crate) const fn is_inline_byte(b: u8) -> bool {
    matches!(b, 0x20..=0x7E) && b != b':' && b != b'~'
}

/// Checks an inline body starting at byte `pos`.
pub(crate) fn check_inline(body: &[u8], pos: usize) -> Result<(), CodecError> {
    match body.iter().position(|&b| !is_inline_byte(b)) {
        Some(at) => Err(CodecError::InvalidChar { index: pos + at }),
        None => Ok(()),
    }
}

/// Checks a body starting at byte `pos`; returns whether it is a Ref.
fn check_body(body: &[u8], pos: usize, tagged: bool) -> Result<bool, CodecError> {
    let is_ref = body.first() == Some(&b'~');
    if is_ref {
        if tagged {
            return Err(CodecError::TagOnRef);
        }
        let ref_len = body.len() - REF_MARKER_LEN;
        if ref_len != REF_LEN {
            return Err(CodecError::RefLength { len: ref_len });
        }
        if let Some(at) = body
            .iter()
            .skip(REF_MARKER_LEN)
            .position(|&b| digit_value(b).is_none())
        {
            return Err(CodecError::InvalidChar {
                index: pos + REF_MARKER_LEN + at,
            });
        }
    } else {
        check_inline(body, pos)?;
    }
    Ok(is_ref)
}

/// Splits a `custom_id` into its sections (phase 1).
///
/// Total: every input gives `Ok` or a [`CodecError`], never a panic. The
/// result does not depend on any Route; [`Decoded::payload`] checks it
/// against one.
///
/// ```
/// use noctis_codec::{RawBody, decode};
///
/// let id = decode("0shop.buy:47,3")?;
/// assert_eq!(id.route(), "shop.buy");
/// assert_eq!(id.body(), RawBody::Inline("47,3"));
/// # Ok::<(), noctis_codec::CodecError>(())
/// ```
///
/// # Errors
///
/// The first failed check, in this order: empty; over 100 bytes; a
/// non-ASCII byte; a bad format digit or a generation other than 0; a bad,
/// unterminated, empty or over-long route; a missing, zero or bad schema
/// digit; a truncated or bad envelope, nonce or tag; a tag on a Ref body; a
/// Ref that is not 11 base62 characters; an inline body byte outside
/// printable ASCII or equal to `:` or `~`.
pub fn decode(raw: &str) -> Result<Decoded<'_>, CodecError> {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return Err(CodecError::Empty);
    }
    if len > MAX_ID_LEN {
        return Err(CodecError::TooLong { len });
    }
    if let Some(index) = bytes.iter().position(|b| !b.is_ascii()) {
        return Err(CodecError::NotAscii { index });
    }

    let flags = parse_format(bytes)?;
    let route_end = parse_route(bytes)?;
    let (schema, mut pos) = parse_schema(bytes, route_end + SEP_LEN, flags)?;

    let mut envelope = None;
    if flags.contains(Flags::E) {
        let env = fixed::<VIEW_ENV_LEN>(bytes, pos, Section::Envelope)?;
        envelope = Some(
            ViewEnvelope::parse(env).map_err(|at| CodecError::InvalidChar { index: pos + at })?,
        );
        pos += VIEW_ENV_LEN;
    }

    let mut nonce = None;
    if flags.contains(Flags::N) {
        let [hi, lo] = fixed::<NONCE_LEN>(bytes, pos, Section::Nonce)?;
        let hi = digit_value(hi).ok_or(CodecError::InvalidChar { index: pos })?;
        let lo = digit_value(lo).ok_or(CodecError::InvalidChar { index: pos + 1 })?;
        nonce = SelectNonce::new(u16::from(hi) * 62 + u16::from(lo));
        pos += NONCE_LEN;
    }

    let mut body_end = len;
    let mut tag = None;
    if flags.contains(Flags::T) {
        let start =
            len.checked_sub(TAG_LEN)
                .filter(|&s| s >= pos)
                .ok_or(CodecError::Truncated {
                    section: Section::Tag,
                })?;
        let digits = fixed::<TAG_LEN>(bytes, start, Section::Tag)?;
        if let Some(at) = digits.iter().position(|&b| digit_value(b).is_none()) {
            return Err(CodecError::InvalidChar { index: start + at });
        }
        tag = Some(Tag::from_digits(digits));
        body_end = start;
    }

    let body = bytes.get(pos..body_end).unwrap_or_default();
    let is_ref = check_body(body, pos, tag.is_some())?;

    Ok(Decoded {
        raw,
        flags,
        route_end,
        schema,
        envelope,
        nonce,
        body_start: pos,
        body_end,
        is_ref,
        tag,
    })
}

/// [`decode`] then [`Decoded::payload`] in one call.
///
/// # Errors
///
/// A phase-1 [`CodecError`] as [`DecodeError::Malformed`], then the errors of
/// [`Decoded::payload`].
pub fn decode_route<'a, R: Route>(raw: &'a str) -> Result<PayloadSource<R::Payload>, DecodeError>
where
    R::Payload: PayloadDecode<'a>,
{
    decode(raw)?.payload::<R>()
}

#[cfg(test)]
mod tests {
    use super::{
        Decoded, FMT_LEN, MAX_ID_LEN, NONCE_LEN, REF_LEN, REF_MARKER_LEN, RawBody, SCHEMA_LEN,
        SEP_LEN, TAG_LEN, VIEW_ENV_LEN, decode,
    };
    use crate::{CodecError, Flags, Section, SelectNonce, ViewEnvelope};

    fn err(raw: &str) -> CodecError {
        match decode(raw) {
            Ok(d) => panic!("{raw:?} decoded to {d:?}"),
            Err(e) => e,
        }
    }

    fn ok(raw: &str) -> Decoded<'_> {
        decode(raw).unwrap_or_else(|e| panic!("{raw:?}: {e}"))
    }

    const fn trunc(section: Section) -> CodecError {
        CodecError::Truncated { section }
    }

    const fn bad(index: usize) -> CodecError {
        CodecError::InvalidChar { index }
    }

    #[test]
    fn constants_match_ad_5() {
        assert_eq!(
            [
                FMT_LEN,
                SEP_LEN,
                SCHEMA_LEN,
                VIEW_ENV_LEN,
                NONCE_LEN,
                REF_MARKER_LEN,
                REF_LEN,
                TAG_LEN
            ],
            [1, 1, 1, 5, 2, 1, 11, 6]
        );
    }

    #[test]
    fn max_id_len_matches_rules() {
        assert_eq!(MAX_ID_LEN, noctis_rules::Limits::default().custom_id_len);
    }

    #[test]
    #[allow(clippy::too_many_lines, reason = "one row per matrix line")]
    fn cid_06_decode_matrix() {
        use CodecError as E;

        assert_eq!(err(""), E::Empty);
        assert_eq!(err(&"a".repeat(101)), E::TooLong { len: 101 });
        assert_eq!(
            err(&format!("0{}", "é".repeat(50))),
            E::TooLong { len: 101 }
        );
        assert_eq!(err("0é:1"), E::NotAscii { index: 1 });
        assert_eq!(err("Gé"), E::NotAscii { index: 1 });
        assert_eq!(err("!x:1"), bad(0));
        assert_eq!(err("Ga:"), E::Generation { generation: 1 });
        assert_eq!(err("za:"), E::Generation { generation: 3 });
        assert_eq!(err("0"), E::MissingSeparator);
        assert_eq!(err("0abc"), E::MissingSeparator);
        assert_eq!(err("0:1"), E::EmptyRoute);
        assert_eq!(err("0Shop:1"), bad(1));
        assert_eq!(err("0a b:1"), bad(2));
        assert_eq!(err("0a,b:1"), bad(2));

        let r24 = "a".repeat(24);
        assert_eq!(ok(&format!("0{r24}:1")).route(), r24);
        assert_eq!(
            err(&format!("0{}:1", "a".repeat(25))),
            E::RouteTooLong { len: 25 }
        );
        let reserved = ok("0_x:1");
        assert_eq!(reserved.route(), "_x");
        assert_eq!(reserved.body(), RawBody::Inline("1"));
        let punct = ok("0a.b-c_9:");
        assert_eq!(punct.route(), "a.b-c_9");
        assert_eq!(punct.body(), RawBody::Inline(""));

        assert_eq!(err("1a:"), trunc(Section::Schema));
        assert_eq!(err("1a:0"), E::ZeroSchema);
        assert_eq!(err("1a:!"), bad(3));

        assert_eq!(err("2a:_000"), trunc(Section::Envelope));
        assert_eq!(err("2a:!0000"), bad(3));
        assert_eq!(err("2a:__000"), bad(4));
        assert_eq!(err("2a:_0!00"), bad(5));
        assert_eq!(err("2a:_00!0"), bad(6));
        assert_eq!(err("2a:_000!"), bad(7));

        assert_eq!(err("6a:_0000z"), trunc(Section::Nonce));
        assert_eq!(err("4a:!0"), bad(3));
        assert_eq!(err("4a:0!"), bad(4));

        assert_eq!(err("8a:12345"), trunc(Section::Tag));
        assert_eq!(err("8a:~12"), trunc(Section::Tag));
        assert_eq!(err("8a:1234!6"), bad(7));
        let tagged = ok("8a:000000");
        assert_eq!(tagged.body(), RawBody::Inline(""));
        assert_eq!(
            tagged.tag().map(|t| t.to_string()).as_deref(),
            Some("000000")
        );
        assert_eq!(tagged.signed_part(), Some("8a:"));

        assert_eq!(err("8a:~0123456789A000000"), E::TagOnRef);
        assert_eq!(err("0a:~"), E::RefLength { len: 0 });
        assert_eq!(err("0a:~123"), E::RefLength { len: 3 });
        assert_eq!(err("0a:~0123456789AB"), E::RefLength { len: 12 });
        assert_eq!(err("0a:~0123456789!"), bad(14));
        let by_ref = ok("0a:~0123456789A");
        assert!(matches!(by_ref.body(), RawBody::Ref(r) if r.as_str() == "0123456789A"));
        assert_eq!(by_ref.tag(), None);
        assert_eq!(by_ref.signed_part(), None);

        assert_eq!(err("0a:1~2"), bad(4));
        assert_eq!(err("0a:1:2"), bad(4));
        assert_eq!(err("0a:\t"), bad(3));
        assert_eq!(err("0a:\u{7f}"), bad(3));

        let all = ok("Fall:z_zzzz000000001");
        assert_eq!(all.flags().bits(), 15);
        assert_eq!(all.flags(), Flags::S | Flags::E | Flags::N | Flags::T);
        assert_eq!(all.route(), "all");
        assert_eq!(all.schema(), 61);
        assert_eq!(all.envelope(), ViewEnvelope::new('_', 61, 3843, 61).ok());
        assert_eq!(all.nonce(), SelectNonce::new(0));
        assert_eq!(all.body(), RawBody::Inline("0"));
        assert_eq!(all.tag().map(|t| t.to_string()).as_deref(), Some("000001"));
        assert_eq!(all.signed_part(), Some("Fall:z_zzzz000"));
        assert_eq!(all.as_str(), "Fall:z_zzzz000000001");

        let full = format!("0a:{}", "x".repeat(97));
        assert_eq!(ok(&full).as_str().len(), 100);
        assert_eq!(err(&format!("{full}x")), E::TooLong { len: 101 });
    }
}
