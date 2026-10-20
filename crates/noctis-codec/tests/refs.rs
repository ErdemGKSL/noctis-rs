//! Refs and two-phase encoding: Ref entropy, `encode_id`, `finish`, the
//! stored `RefRecord` and its Route binding (CID-05, FR-17, AD-18).

#[macro_use]
mod common;

use core::fmt;
use core::time::Duration;
use std::collections::HashSet;

use common::*;
use noctis_codec::{
    CodecError, DecodeError, EncodeError, EncodedId, Field, FieldError, FieldReader, FieldWriter,
    ForgedReason, MAX_REF_ID_LEN, Parts, Payload, PayloadDecode, PayloadSource, PendingRef,
    RawBody, Ref, RefPolicy, RefRecord, Route, RouteShape, Section, SelectNonce, StaleReason,
    decode, decode_record, decode_route, decode_route_verified, encode, encode_id, finish,
};
use proptest::prelude::*;

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The Ref of a decoded `~`-bodied id.
fn ref_of(digits: &str) -> Ref {
    let id = format!("0a:~{digits}");
    match decode(&id).map(|d| d.body()) {
        Ok(RawBody::Ref(r)) => r,
        other => panic!("{id:?} is not a ref id: {other:?}"),
    }
}

/// `R0`: the Ref `0123456789A`.
fn r0() -> Ref {
    ref_of("0123456789A")
}

/// The base62 successor of `s` (wrapping), as a string.
fn successor(s: &str) -> String {
    let mut digits: Vec<usize> = s
        .bytes()
        .map(|b| {
            ALPHABET
                .iter()
                .position(|&a| a == b)
                .unwrap_or_else(|| panic!("{b} is not base62"))
        })
        .collect();
    for d in digits.iter_mut().rev() {
        *d = (*d + 1) % 62;
        if *d != 0 {
            break;
        }
    }
    digits.iter().map(|&d| char::from(ALPHABET[d])).collect()
}

/// Pearson's χ² of `counts` against a uniform distribution.
fn chi_squared(counts: &[u32]) -> f64 {
    let total: u32 = counts.iter().sum();
    let expected = f64::from(total) / 62.0;
    counts
        .iter()
        .map(|&c| (f64::from(c) - expected).powi(2) / expected)
        .sum()
}

#[test]
fn cid_05_refs_unique_and_uniform() {
    const MINTS: usize = 100_000;
    let mut seen = HashSet::with_capacity(MINTS);
    let mut overall = [0u32; 62];
    let mut per_position = [[0u32; 62]; 11];
    let mut previous: Option<Ref> = None;
    for _ in 0..MINTS {
        let r = Ref::mint().unwrap();
        let s = r.as_str();
        assert_eq!(s.len(), 11);
        for (pos, b) in s.bytes().enumerate() {
            let d = ALPHABET.iter().position(|&a| a == b).unwrap();
            overall[d] += 1;
            per_position[pos][d] += 1;
        }
        if let Some(p) = previous {
            assert_ne!(successor(p.as_str()), s, "{p} then {r}");
        }
        assert!(seen.insert(r), "duplicate ref {r}");
        previous = Some(r);
    }
    let chi = chi_squared(&overall);
    assert!(chi < 130.0, "overall χ² = {chi}");
    for (pos, counts) in per_position.iter().enumerate() {
        let chi = chi_squared(counts);
        assert!(chi < 140.0, "position {pos} χ² = {chi}");
    }
}

#[test]
fn successor_helper() {
    assert_eq!(successor("0000000000z"), "00000000010");
    assert_eq!(successor("zzzzzzzzzzz"), "00000000000");
    assert_eq!(successor("0123456789A"), "0123456789B");
}

/// A field whose own `encode` fails.
#[derive(Debug)]
struct Failing;

impl Field for Failing {
    const MAX_LEN: Option<usize> = Some(0);

    fn encode<W: fmt::Write + ?Sized>(&self, _out: &mut W) -> fmt::Result {
        Err(fmt::Error)
    }
}

/// A field that writes a `:`, which no inline body can hold.
#[derive(Debug)]
struct Colon;

impl Field for Colon {
    const MAX_LEN: Option<usize> = Some(1);

    fn encode<W: fmt::Write + ?Sized>(&self, out: &mut W) -> fmt::Result {
        out.write_char(':')
    }
}

/// A Payload of one hand-written field.
#[derive(Debug)]
struct One<F>(F);

impl<F: Field> Payload for One<F> {
    const MAX_ENCODED_LEN: Option<usize> = F::MAX_LEN;

    fn encode_fields(&self, out: &mut FieldWriter<'_>) -> fmt::Result {
        out.field(&self.0)
    }
}

impl<F: Field> PayloadDecode<'_> for One<F> {
    fn decode_fields(_f: &mut FieldReader<'_>) -> Result<Self, CodecError> {
        Err(CodecError::Empty)
    }
}

route!(FailRef, "f", RouteShape::BY_REF, 0, One<Failing>);
route!(ColonRef, "c", RouteShape::BY_REF, 0, One<Colon>);
route!(BadSchemaRef, "x", RouteShape::BY_REF, 62, NumP);

fn slot<R: Route>(p: &R::Payload, parts: &Parts<'_>) -> PendingRef {
    match encode_id::<R>(p, parts) {
        Ok(EncodedId::RefSlot(slot)) => slot,
        other => panic!("expected a ref slot, got {other:?}"),
    }
}

fn inline_id<R: Route>(p: &R::Payload, parts: &Parts<'_>) -> String {
    match encode_id::<R>(p, parts) {
        Ok(EncodedId::Inline(id)) => id.as_str().to_owned(),
        other => panic!("expected an inline id, got {other:?}"),
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "one row per matrix line")]
fn encode_id_matrix() {
    let none = Parts::new();
    assert_eq!(inline_id::<Buy>(&buy(255, 3), &none), "0shop.buy:47,3");

    let s = slot::<Stored>(&buy(255, 3), &none);
    assert_eq!(s.route().as_str(), "r");
    assert_eq!(s.schema(), 0);
    assert_eq!(&s.payload()[..], b"47,3");
    assert_eq!(s.ttl(), None);
    assert!(!s.is_one_shot());
    assert_eq!(s.nonce(), None);
    assert_eq!(&s.record()[..], b"0r:47,3");

    let s2 = slot::<StoredS2>(&buy(255, 3), &none);
    assert_eq!(&s2.record()[..], b"1r:247,3");

    let esc = slot::<StoredText>(&text("é,~"), &none);
    assert_eq!(&esc.payload()[..], b"!u0000e9!1!3");

    let big = slot::<StoredText>(&x(10_000), &none);
    assert_eq!(big.payload().len(), 10_000);
    assert_eq!(finish(&big, r0()).len(), 16);

    assert_eq!(inline_id::<TextSpill>(&x(97), &none).len(), 100);
    let spilled = slot::<TextSpill>(&x(98), &none);
    assert_eq!(&spilled.payload()[..], "x".repeat(98).as_bytes());

    // "0ss:" + body + a 6-character tag: 90 is the longest body that fits.
    let signer = FakeSigner::new(0);
    let signing = Parts::new().with_signer(&signer);
    assert_eq!(inline_id::<SpillSigned>(&x(90), &signing).len(), 100);
    let signed_slot = slot::<SpillSigned>(&x(91), &signing);
    assert_eq!(finish(&signed_slot, r0()).as_str(), "0ss:~0123456789A");

    assert_eq!(encode_id::<Text>(&x(98), &none), Err(EncodeError::TooLong));

    let ttl = Duration::from_mins(1);
    let policy = RefPolicy::new().with_ttl(ttl).one_shot();
    let with_policy = Parts::new().with_ref_policy(policy);
    let kept = slot::<Stored>(&buy(1, 1), &with_policy);
    assert_eq!(kept.ttl(), Some(ttl));
    assert!(kept.is_one_shot());

    let zero = Parts::new().with_ref_policy(RefPolicy::new().with_ttl(Duration::ZERO));
    assert_eq!(
        encode_id::<Stored>(&buy(1, 1), &zero),
        Err(EncodeError::ZeroTtl)
    );
    assert_eq!(
        encode_id::<TextSpill>(&text("a"), &zero),
        Err(EncodeError::ZeroTtl)
    );
    assert_eq!(
        encode::<Stored>(&buy(1, 1), &zero),
        Err(EncodeError::ZeroTtl)
    );
    assert_eq!(
        encode_id::<Buy>(&buy(1, 1), &zero),
        Err(EncodeError::UnexpectedRefPolicy)
    );
    assert_eq!(
        encode::<Stored>(
            &buy(1, 1),
            &Parts::new().with_ref_policy(RefPolicy::new().with_ttl(ttl))
        ),
        Err(EncodeError::NeedsRef)
    );

    for p in [
        policy,
        RefPolicy::new().with_ttl(ttl),
        RefPolicy::new().one_shot(),
    ] {
        let parts = Parts::new().with_ref_policy(p);
        assert_eq!(
            encode_id::<Buy>(&buy(1, 1), &parts),
            Err(EncodeError::UnexpectedRefPolicy)
        );
        assert_eq!(
            encode::<Buy>(&buy(1, 1), &parts),
            Err(EncodeError::UnexpectedRefPolicy)
        );
    }
    let default_policy = Parts::new().with_ref_policy(RefPolicy::new());
    assert_eq!(
        inline_id::<Buy>(&buy(255, 3), &default_policy),
        "0shop.buy:47,3"
    );

    let ttl_only = Parts::new().with_ref_policy(RefPolicy::new().with_ttl(ttl));
    assert_eq!(inline_id::<TextSpill>(&text("a"), &ttl_only), "0t:a");

    assert_eq!(
        encode_id::<Stored>(&buy(1, 1), &signing),
        Err(EncodeError::UnexpectedSigner)
    );
    let with_nonce = Parts::new().with_nonce(SelectNonce::new(1).unwrap());
    assert_eq!(
        encode_id::<Stored>(&buy(1, 1), &with_nonce),
        Err(EncodeError::UnexpectedNonce)
    );
    assert_eq!(
        encode_id::<BadSchemaRef>(&NumP { n: 1 }, &zero),
        Err(EncodeError::InvalidSchema { schema: 62 })
    );
    assert_eq!(
        encode_id::<FailRef>(&One(Failing), &none),
        Err(EncodeError::Field)
    );
    assert_eq!(
        encode_id::<ColonRef>(&One(Colon), &none),
        Err(EncodeError::InvalidBody { index: 0 })
    );
}

#[test]
fn finish_matrix() {
    let none = Parts::new();
    let s = slot::<Stored>(&buy(255, 3), &none);
    let id = finish(&s, r0());
    assert_eq!(id.as_str(), "0r:~0123456789A");
    assert_eq!(id.len(), 15);
    assert_eq!(decode_route::<Stored>(&id), Ok(PayloadSource::Ref(r0())));

    let s2 = slot::<StoredS2>(&buy(255, 3), &none);
    let id = finish(&s2, r0());
    assert_eq!(id.as_str(), "1r:2~0123456789A");
    assert_eq!(decode_route::<StoredS2>(&id), Ok(PayloadSource::Ref(r0())));

    let parts = Parts::new().with_nonce(SelectNonce::new(3843).unwrap());
    let all = slot::<StoredAll>(&NumP { n: 1 }, &parts);
    let zs = ref_of("zzzzzzzzzzz");
    let id = finish(&all, zs);
    assert_eq!(id.as_str(), format!("5{}:zzz~zzzzzzzzzzz", "a".repeat(24)));
    assert_eq!(id.len(), 41);
    assert_eq!(id.len(), MAX_REF_ID_LEN);
    assert_eq!(decode_route::<StoredAll>(&id), Ok(PayloadSource::Ref(zs)));

    let minted = Ref::mint().unwrap();
    let text_slot = slot::<StoredText>(&text("hi"), &none);
    assert_eq!(
        decode_route::<StoredText>(&finish(&text_slot, minted)),
        Ok(PayloadSource::Ref(minted))
    );
}

#[test]
fn cid_05_record_route_mismatch_is_forged() {
    let forged = Err(DecodeError::Forged(ForgedReason::RecordRouteMismatch));
    assert_eq!(decode_record::<Stored>(b"0x:47,3"), forged);
    // The route is checked before the fields.
    assert_eq!(decode_record::<Stored>(b"0x:junk"), forged);
    // A slot's record decodes only against its own Route.
    let s = slot::<StoredText>(&text("47,3"), &Parts::new());
    assert_eq!(decode_record::<Stored>(&s.record()), forged);
}

#[test]
fn decode_record_outcomes() {
    let malformed = |e| Err(DecodeError::Malformed(e));
    let stale = |expected, found| Err(DecodeError::Stale(StaleReason::Schema { expected, found }));

    assert_eq!(decode_record::<Stored>(b"0r:47,3"), Ok(buy(255, 3)));
    assert_eq!(decode_record::<Stored>(b"1r:247,3"), stale(0, 2));
    assert_eq!(decode_record::<StoredS2>(b"0r:47,3"), stale(2, 0));
    assert_eq!(
        decode_record::<Stored>(b"0r:47"),
        malformed(CodecError::MissingField { field: 1 })
    );
    assert_eq!(
        decode_record::<Stored>(b"0r:47,3,1"),
        malformed(CodecError::ExtraField { field: 2 })
    );
    assert_eq!(
        decode_record::<Stored>(b"0r:47,-"),
        malformed(CodecError::Field {
            field: 1,
            error: FieldError::InvalidChar { index: 0 }
        })
    );
    assert_eq!(decode_record::<Stored>(b""), malformed(CodecError::Empty));
    assert_eq!(
        decode_record::<Stored>(b"2r:x"),
        malformed(CodecError::ReservedFlag)
    );
    assert_eq!(
        decode_record::<Stored>(b"4r:00"),
        malformed(CodecError::UnexpectedSection {
            section: Section::Nonce
        })
    );
    assert_eq!(
        decode_record::<StoredV2>(b"0r:47,3"),
        Ok(BuyV2Payload {
            item: 255,
            qty: 3,
            note: None,
            gift: None
        })
    );
}

route!(MixRef, "mix.ref", RouteShape::BY_REF, 0, MixP);
route!(MixRefAll, "mix.ref", RouteShape::BY_REF.select(), 7, MixP);
route!(MixSpill, "mix.spill", RouteShape::SPILL, 0, MixP);
route!(
    MixSpillAll,
    "mix.spill",
    RouteShape::SPILL.select().signed(),
    3,
    MixP
);

/// `encode_id`, then the inline decode or `finish` + Ref decode + record
/// decode, gives `p` back.
fn id_round_trip<R>(p: &MixP, nonce: SelectNonce)
where
    R: Route<Payload = MixP>,
{
    let signer = FakeSigner::new(5);
    let mut parts = Parts::new();
    if R::SHAPE.has_select() {
        parts = parts.with_nonce(nonce);
    }
    if R::SHAPE.is_signed() {
        parts = parts.with_signer(&signer);
    }
    let encoded = encode_id::<R>(p, &parts);
    match encoded {
        Err(e) => panic!("{p:?} did not encode: {e}"),
        Ok(EncodedId::Inline(id)) => {
            assert_eq!(
                decode_route_verified::<R>(&id, &signer),
                Ok(PayloadSource::Inline(p.clone()))
            );
        }
        Ok(EncodedId::RefSlot(slot)) => {
            let Ok(r) = Ref::mint() else {
                panic!("the OS random source failed")
            };
            let id = finish(&slot, r);
            assert!(id.len() <= MAX_REF_ID_LEN);
            assert_eq!(decode_route::<R>(&id), Ok(PayloadSource::Ref(r)));
            assert_eq!(decode_record::<R>(&slot.record()), Ok(p.clone()));
        }
    }
}

proptest! {
    #[test]
    fn record_round_trip(
        route in "[a-z0-9._-]{1,24}",
        schema in 0..=61u8,
        payload in "[ -9;-}]{0,300}",
    ) {
        let record = RefRecord::new(&route, schema, &payload).unwrap();
        let bytes = record.encode();
        prop_assert_eq!(RefRecord::decode(&bytes), Ok(record));
        prop_assert_eq!(record.route(), route.as_str());
        prop_assert_eq!(record.schema(), schema);
        prop_assert_eq!(record.payload(), payload.as_str());
    }

    #[test]
    fn record_decode_never_panics(raw in proptest::collection::vec(any::<u8>(), 0..300)) {
        if let Ok(r) = RefRecord::decode(&raw) {
            prop_assert_eq!(&r.encode()[..], &raw[..]);
        }
    }

    #[test]
    fn encode_id_round_trip(
        a in any::<u8>(),
        b in any::<i64>(),
        c in any::<Option<u64>>(),
        d in prop_oneof![".{0,30}", ".{0,200}"],
        nonce in (0..=3843u16).prop_filter_map("nonce", SelectNonce::new),
    ) {
        let p = MixP { a, b, c, d };
        id_round_trip::<MixRef>(&p, nonce);
        id_round_trip::<MixRefAll>(&p, nonce);
        id_round_trip::<MixSpill>(&p, nonce);
        id_round_trip::<MixSpillAll>(&p, nonce);
    }
}
