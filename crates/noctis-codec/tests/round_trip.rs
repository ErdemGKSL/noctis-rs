//! Property tests: every field type round-trips, composite lists split
//! cleanly, and decoding is total and canonical (CID-07, FR-16).

use std::borrow::Cow;
use std::fmt::Debug;

use noctis_codec::check::round_trip;
use noctis_codec::{BoundedStr, Field, FieldDecode};
use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;
use twilight_model::id::Id;
use twilight_model::id::marker::GenericMarker;

type GId = Id<GenericMarker>;

fn enc<T: Field>(v: &T) -> String {
    let mut out = String::new();
    // Writing into a `String` cannot fail for the built-in fields.
    let _ = v.encode(&mut out);
    out
}

fn id() -> impl Strategy<Value = GId> {
    (1..=u64::MAX).prop_map(GId::new)
}

fn bounded() -> impl Strategy<Value = BoundedStr<8>> {
    "[ -~]{0,8}".prop_filter_map("printable ASCII", |s| BoundedStr::new(&s).ok())
}

/// `decode` never panics, and `Ok(v)` implies `encode(v) == raw`.
fn canonical<T>(raw: &str)
where
    T: Field + for<'de> FieldDecode<'de> + Debug,
{
    if let Ok(v) = T::decode(raw) {
        assert_eq!(enc(&v), raw, "{v:?}");
    }
}

/// [`canonical`] for every built-in field type.
fn canonical_all(raw: &str) {
    canonical::<bool>(raw);
    canonical::<u8>(raw);
    canonical::<u16>(raw);
    canonical::<u32>(raw);
    canonical::<u64>(raw);
    canonical::<i8>(raw);
    canonical::<i16>(raw);
    canonical::<i32>(raw);
    canonical::<i64>(raw);
    canonical::<GId>(raw);
    canonical::<Option<u8>>(raw);
    canonical::<Option<Option<i64>>>(raw);
    canonical::<BoundedStr<0>>(raw);
    canonical::<BoundedStr<8>>(raw);
    canonical::<Option<BoundedStr<8>>>(raw);
    canonical::<String>(raw);
    canonical::<Vec<u8>>(raw);
    canonical::<Vec<String>>(raw);
    canonical::<Vec<Vec<u8>>>(raw);
    canonical::<Vec<Option<BoundedStr<4>>>>(raw);
    if let Ok(v) = Cow::<'_, str>::decode(raw) {
        assert_eq!(enc(&v), raw);
    }
}

proptest! {
    #[test]
    fn cid_07_integers_round_trip(
        a in any::<u8>(), b in any::<u16>(), c in any::<u32>(), d in any::<u64>(),
        e in any::<i8>(), f in any::<i16>(), g in any::<i32>(), h in any::<i64>(),
    ) {
        prop_assert_eq!(round_trip(&a), Ok(()));
        prop_assert_eq!(round_trip(&b), Ok(()));
        prop_assert_eq!(round_trip(&c), Ok(()));
        prop_assert_eq!(round_trip(&d), Ok(()));
        prop_assert_eq!(round_trip(&e), Ok(()));
        prop_assert_eq!(round_trip(&f), Ok(()));
        prop_assert_eq!(round_trip(&g), Ok(()));
        prop_assert_eq!(round_trip(&h), Ok(()));
    }

    #[test]
    fn cid_07_scalars_round_trip(b in any::<bool>(), i in id(), s in bounded()) {
        prop_assert_eq!(round_trip(&b), Ok(()));
        prop_assert_eq!(round_trip(&i), Ok(()));
        prop_assert_eq!(round_trip(&s), Ok(()));
    }

    #[test]
    fn cid_07_options_round_trip(
        a in option::of(any::<u8>()),
        b in option::of(option::of(any::<i64>())),
        c in option::of(id()),
        d in option::of(bounded()),
        e in option::of(any::<String>()),
    ) {
        prop_assert_eq!(round_trip(&a), Ok(()));
        prop_assert_eq!(round_trip(&b), Ok(()));
        prop_assert_eq!(round_trip(&c), Ok(()));
        prop_assert_eq!(round_trip(&d), Ok(()));
        prop_assert_eq!(round_trip(&e), Ok(()));
    }

    #[test]
    fn cid_07_strings_round_trip(s in any::<String>()) {
        prop_assert_eq!(round_trip(&s), Ok(()));
        let cow: Cow<'_, str> = Cow::Owned(s.clone());
        let encoded = enc(&cow);
        prop_assert_eq!(Cow::<'_, str>::decode(&encoded), Ok(cow));
    }

    #[test]
    fn cid_07_vecs_round_trip(
        a in vec(any::<u8>(), 0..8),
        b in vec(any::<String>(), 0..8),
        c in vec(vec(any::<u8>(), 0..8), 0..8),
        d in vec(option::of(bounded()), 0..8),
    ) {
        prop_assert_eq!(round_trip(&a), Ok(()));
        prop_assert_eq!(round_trip(&b), Ok(()));
        prop_assert_eq!(round_trip(&c), Ok(()));
        prop_assert_eq!(round_trip(&d), Ok(()));
    }

    #[test]
    fn cid_07_composite_list_splits(
        a in any::<u8>(),
        b in any::<i64>(),
        c in option::of(id()),
        d in bounded(),
        e in any::<bool>(),
        f in any::<String>(),
        g in vec(any::<String>(), 0..8),
    ) {
        let parts = [enc(&a), enc(&b), enc(&c), enc(&d), enc(&e), enc(&f), enc(&g)];
        let joined = parts.join(",");
        let split: Vec<&str> = joined.split(',').collect();
        prop_assert_eq!(split.len(), 7);
        prop_assert_eq!(u8::decode(split[0]), Ok(a));
        prop_assert_eq!(i64::decode(split[1]), Ok(b));
        prop_assert_eq!(Option::<GId>::decode(split[2]), Ok(c));
        prop_assert_eq!(BoundedStr::<8>::decode(split[3]), Ok(d));
        prop_assert_eq!(bool::decode(split[4]), Ok(e));
        prop_assert_eq!(String::decode(split[5]), Ok(f));
        prop_assert_eq!(Vec::<String>::decode(split[6]), Ok(g));
    }

    #[test]
    fn decode_is_total_on_any_input(raw in ".*") {
        canonical_all(&raw);
    }

    #[test]
    fn decode_is_canonical_on_codec_alphabet(raw in "[ -~!,:~0-9a-zA-Z]{0,120}") {
        canonical_all(&raw);
    }

    #[test]
    fn decode_is_canonical_on_escape_shapes(raw in "(!|!u|[0-9a-f]|[0-3]|,|1){0,40}") {
        canonical_all(&raw);
    }
}
