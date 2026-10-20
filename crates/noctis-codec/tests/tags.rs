//! Integrity tags of signed inline Payloads: keyed BLAKE3 signing,
//! verification, key rotation and fail-closed decoding (CID-06, FR-19,
//! AD-5, AD-29).

#[macro_use]
mod common;

use common::*;
use noctis_codec::{
    DecodeError, EncodeError, EncodedId, Flags, ForgedReason, Parts, PayloadDecode, PayloadKey,
    PayloadKeys, PayloadSource, RawBody, Ref, Route, RouteShape, SelectNonce, Sign, StaleReason,
    Tag, Verify, decode, decode_route, decode_route_verified, encode, encode_id, finish,
};
use proptest::prelude::*;

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn k1() -> PayloadKey {
    PayloadKey::from_bytes([1; 32])
}

fn k2() -> PayloadKey {
    PayloadKey::from_bytes([2; 32])
}

fn k3() -> PayloadKey {
    PayloadKey::from_bytes([3; 32])
}

/// The tag formula, recomputed independently of the crate.
fn reference_tag(key: [u8; 32], signed: &str) -> String {
    let hash = blake3::keyed_hash(&key, signed.as_bytes());
    let mut first = [0u8; 8];
    first.copy_from_slice(&hash.as_bytes()[..8]);
    Tag::from_u64(u64::from_le_bytes(first)).as_str().to_owned()
}

/// `R0`: the Ref `0123456789A`.
fn r0() -> Ref {
    match decode("0a:~0123456789A").map(|d| d.body()) {
        Ok(RawBody::Ref(r)) => r,
        other => panic!("not a ref id: {other:?}"),
    }
}

fn enc<R: Route>(p: &R::Payload, signer: &dyn Sign) -> Result<String, EncodeError> {
    encode::<R>(p, &Parts::new().with_signer(signer)).map(|id| id.to_string())
}

fn forged(reason: ForgedReason) -> DecodeError {
    DecodeError::Forged(reason)
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "compared with `decode_route_verified` results"
)]
fn inline<P>(p: P) -> Result<PayloadSource<P>, DecodeError> {
    Ok(PayloadSource::Inline(p))
}

/// Asserts `id` ends in the reference tag of `key` over everything before it.
fn assert_reference(key: [u8; 32], id: &str) {
    let (signed, tag) = id.split_at(id.len() - 6);
    assert_eq!(tag, reference_tag(key, signed), "{id}");
}

#[test]
fn cid_06_signing_matrix() {
    let one = NumP { n: 1 };
    for (bytes, key, want) in [
        ([1; 32], k1(), "8sig:1UU2sw1"),
        ([2; 32], k2(), "8sig:1a0cQ9C"),
        ([3; 32], k3(), "8sig:1DMzSul"),
    ] {
        let id = enc::<Sig>(&one, &key).unwrap();
        assert_eq!(id, want);
        assert_reference(bytes, &id);
    }
    let id = enc::<Sig>(&NumP { n: 2 }, &k1()).unwrap();
    assert_eq!(id, "8sig:2ktZWEa");
    assert_reference([1; 32], &id);

    let id = enc::<SpillSigned>(&text("a"), &k1()).unwrap();
    assert_eq!(id, "8ss:aOlrJxp");
    assert_reference([1; 32], &id);

    let id = enc::<TextSigned>(&text(""), &k1()).unwrap();
    assert_eq!(id, "8t:R0qOBY");
    assert_reference([1; 32], &id);

    let id = enc::<TextSigned>(&x(91), &k1()).unwrap();
    assert_eq!(id.len(), 100);
    assert!(id.ends_with("LIqs04"));
    assert_reference([1; 32], &id);
    assert_eq!(enc::<TextSigned>(&x(92), &k1()), Err(EncodeError::TooLong));

    let key = k1();
    let parts = Parts::new()
        .with_nonce(SelectNonce::new(0).unwrap())
        .with_signer(&key);
    let id = encode::<All>(&NumP { n: 0 }, &parts).unwrap();
    assert_eq!(id.as_str(), "Dall:z000DH3N5A");
    assert_reference([1; 32], &id);

    let ring = PayloadKeys::new(k1()).with_previous(k2());
    assert_eq!(enc::<Sig>(&one, &ring).unwrap(), "8sig:1UU2sw1");

    let signing = Parts::new().with_signer(&key);
    match encode_id::<SpillSigned>(&x(90), &signing) {
        Ok(EncodedId::Inline(id)) => assert_eq!(id.len(), 100),
        other => panic!("{other:?}"),
    }
    match encode_id::<SpillSigned>(&x(91), &signing) {
        Ok(EncodedId::RefSlot(slot)) => {
            assert_eq!(finish(&slot, r0()).as_str(), "0ss:~0123456789A");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "one row per matrix entry")]
fn cid_06_tampered_ids_are_forged() {
    let one = inline(NumP { n: 1 });
    let bad = forged(ForgedReason::BadTag);
    let missing = forged(ForgedReason::MissingTag);
    let id = "8sig:1UU2sw1";

    assert_eq!(decode_route_verified::<Sig>(id, &k1()), one);
    for keys in [
        PayloadKeys::new(k1()),
        PayloadKeys::new(k2()).with_previous(k1()),
        PayloadKeys::new(k1()).with_previous(k1()),
    ] {
        assert_eq!(decode_route_verified::<Sig>(id, &keys), one);
    }
    assert_eq!(decode_route_verified::<Sig>(id, &k2()), Err(bad));
    for keys in [
        PayloadKeys::new(k2()),
        PayloadKeys::new(k2()).with_previous(k3()),
    ] {
        assert_eq!(decode_route_verified::<Sig>(id, &keys), Err(bad));
    }

    for tampered in [
        "8sig:2UU2sw1",
        "8sig:1UU2sw2",
        "8sig:1uU2sw1",
        "8sig:1000000",
        "8sig:1UU2sw",
        "8sig:-UU2sw1",
    ] {
        assert_eq!(
            decode_route_verified::<Sig>(tampered, &k1()),
            Err(bad),
            "{tampered}"
        );
    }
    assert_eq!(decode_route_verified::<Sig>("0sig:1", &k1()), Err(missing));
    assert_eq!(
        decode_route_verified::<Sig>("4sig:001", &k1()),
        Err(missing)
    );
    assert_eq!(
        decode_route_verified::<SpillSigned>("0ss:a", &k1()),
        Err(missing)
    );
    assert_eq!(
        decode_route_verified::<SpillSigned>("8ss:aOlrJxp", &k1()),
        inline(text("a"))
    );
    assert_eq!(
        decode_route_verified::<SpillSigned>("0ss:~0123456789A", &k1()),
        Ok(PayloadSource::Ref(r0()))
    );
    assert_eq!(
        decode_route_verified::<All>("Dall:z000DH3N5A", &k1()),
        inline(NumP { n: 0 })
    );
    assert_eq!(
        decode_route_verified::<All>("Dall:z010DH3N5A", &k1()),
        Err(bad)
    );
    assert_eq!(
        decode_route_verified::<Buy>(id, &k1()),
        Err(forged(ForgedReason::RouteMismatch))
    );
    assert_eq!(
        decode_route_verified::<Sig>("9sig:21UU2sw1", &k1()),
        Err(DecodeError::Stale(StaleReason::Schema {
            expected: 0,
            found: 2
        }))
    );
    let schema = Err(DecodeError::Stale(StaleReason::Schema {
        expected: 0,
        found: 2,
    }));
    assert_eq!(decode_route_verified::<Sig>("1sig:21", &k1()), schema);
    assert_eq!(decode_route::<Sig>("1sig:21"), schema);
    assert_eq!(
        decode_route_verified::<Buy>("8shop.buy:47,3000000", &k1()),
        Err(DecodeError::Stale(StaleReason::Flags {
            expected: Flags::EMPTY,
            found: Flags::T
        }))
    );
    assert_eq!(
        decode_route_verified::<Buy>("0shop.buy:47,3", &k1()),
        inline(buy(255, 3))
    );
    assert_eq!(
        decode_route_verified::<Sig>("8sig:UU2sw", &k1()),
        Err(DecodeError::Malformed(
            noctis_codec::CodecError::Truncated {
                section: noctis_codec::Section::Tag
            }
        ))
    );

    // Without keys, a signed Route's inline id never decodes.
    assert_eq!(decode_route::<Sig>(id), Err(bad));
    assert_eq!(decode_route::<Sig>("0sig:1"), Err(missing));
    assert_eq!(decode_route::<Buy>("0shop.buy:47,3"), inline(buy(255, 3)));
    assert_eq!(
        decode_route::<SpillSigned>("0ss:~0123456789A"),
        Ok(PayloadSource::Ref(r0()))
    );
    assert_eq!(decode(id).unwrap().payload::<Sig>(), Err(bad));
}

#[test]
fn cid_06_key_rotation() {
    let old = enc::<Sig>(&NumP { n: 7 }, &k1()).unwrap();
    let rotated = PayloadKeys::new(k2()).with_previous(k1());
    assert_eq!(
        decode_route_verified::<Sig>(&old, &rotated),
        inline(NumP { n: 7 })
    );
    let new = enc::<Sig>(&NumP { n: 7 }, &rotated).unwrap();
    assert_eq!(new, enc::<Sig>(&NumP { n: 7 }, &k2()).unwrap());
    assert_eq!(
        decode_route_verified::<Sig>(&new, &rotated),
        inline(NumP { n: 7 })
    );
    // Rotating again drops the oldest key.
    let again = PayloadKeys::new(k3()).with_previous(k2());
    assert_eq!(
        decode_route_verified::<Sig>(&old, &again),
        Err(forged(ForgedReason::BadTag))
    );
    assert_eq!(
        decode_route_verified::<Sig>(&new, &again),
        inline(NumP { n: 7 })
    );
}

route!(MixSigned, "mix.sig", RouteShape::INLINE.signed(), 0, MixP);
route!(
    MixSpillSigned,
    "mix.ss",
    RouteShape::SPILL.signed(),
    1,
    MixP
);
route!(
    MixAllSigned,
    "mix.all",
    RouteShape::INLINE.select().signed(),
    3,
    MixP
);

/// Encodes `p` for `R` with `key`; checks the round trip and that changing
/// any one character of the id never decodes.
fn signed_round_trip<R>(
    p: &MixP,
    key: &PayloadKey,
    nonce: SelectNonce,
    replacement: u8,
) -> Result<(), TestCaseError>
where
    R: Route<Payload = MixP>,
    MixP: for<'de> PayloadDecode<'de>,
{
    let mut parts = Parts::new().with_signer(key);
    if R::SHAPE.has_select() {
        parts = parts.with_nonce(nonce);
    }
    let Ok(id) = encode::<R>(p, &parts) else {
        return Ok(());
    };
    prop_assert_eq!(
        decode_route_verified::<R>(&id, key),
        Ok(PayloadSource::Inline(p.clone()))
    );
    let mut bytes = id.as_bytes().to_vec();
    for i in 0..bytes.len() {
        let original = bytes[i];
        bytes[i] = if replacement == original {
            if original == b'0' { b'1' } else { b'0' }
        } else {
            replacement
        };
        let Ok(tampered) = String::from_utf8(bytes.clone()) else {
            return Err(TestCaseError::fail("printable ASCII is UTF-8"));
        };
        let got = decode_route_verified::<R>(&tampered, key);
        prop_assert!(got.is_err(), "{} -> {:?}", tampered, got);
        bytes[i] = original;
    }
    Ok(())
}

fn printable() -> impl Strategy<Value = String> {
    proptest::collection::vec(0x20u8..=0x7e, 0..=94)
        .prop_map(|v| v.into_iter().map(char::from).collect())
}

proptest! {
    #[test]
    fn cid_06_sign_verify(
        bytes in any::<[u8; 32]>(),
        other in any::<[u8; 32]>(),
        s in printable(),
        pos in 0..6usize,
        bump in 1..62usize,
    ) {
        let key = PayloadKey::from_bytes(bytes);
        let tag = key.sign(&s);
        prop_assert_eq!(tag.as_str(), reference_tag(bytes, &s));
        prop_assert!(key.verify(&s, tag));

        let mut digits = tag.as_str().as_bytes().to_vec();
        let at = ALPHABET.iter().position(|&a| a == digits[pos]).unwrap();
        digits[pos] = ALPHABET[(at + bump) % 62];
        let changed = decode(&format!("8sig:{}", core::str::from_utf8(&digits).unwrap()))
            .unwrap()
            .tag()
            .unwrap();
        prop_assert!(!key.verify(&s, changed));

        let k2 = PayloadKey::from_bytes(other);
        prop_assert!(PayloadKeys::new(k2.clone()).with_previous(key.clone()).verify(&s, tag));
        if bytes != other {
            prop_assert!(!PayloadKeys::new(k2).verify(&s, tag));
        }
    }

    #[test]
    fn cid_06_signed_round_trip_and_single_char_tamper(
        bytes in any::<[u8; 32]>(),
        a in any::<u8>(),
        b in any::<i64>(),
        c in any::<Option<u64>>(),
        d in ".{0,40}",
        nonce in 0..=3843u16,
        replacement in 0x20u8..=0x7e,
    ) {
        let key = PayloadKey::from_bytes(bytes);
        let p = MixP { a, b, c, d };
        let nonce = SelectNonce::new(nonce).unwrap();
        signed_round_trip::<MixSigned>(&p, &key, nonce, replacement)?;
        signed_round_trip::<MixSpillSigned>(&p, &key, nonce, replacement)?;
        signed_round_trip::<MixAllSigned>(&p, &key, nonce, replacement)?;
    }
}
