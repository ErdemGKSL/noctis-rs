//! Zero-allocation guard of fixed-width inline decode (AD-25, NFR-9, UX-DR8).
//!
//! A dhat counting allocator measures the heap while 10,000 valid inline ids
//! of four fixed-width Routes, and a set of rejected ids, are decoded through
//! the APIs bot dispatch uses. The block and byte deltas over that region
//! must be zero. A negative control decodes an escaped `String` field and
//! must see at least one allocation, so the guard cannot pass vacuously.
//!
//! dhat allows one profiler per process, so this binary holds exactly one
//! `#[test]`.

#[macro_use]
mod common;

use std::hint::black_box;

use arrayvec::ArrayString;
use noctis_codec::{
    BoundedStr, BoundedStrError, CodecError, DecodeError, FMT_LEN, Field, FieldDecode, FieldError,
    ForgedReason, MAX_ID_LEN, Parts, Payload, PayloadKey, PayloadKeys, PayloadSource, RouteShape,
    SEP_LEN, SelectNonce, StaleReason, TAG_LEN, ViewEnvelope, decode, decode_route,
    decode_route_verified, decode_variant, encode, encode_variant,
};
use twilight_model::id::{Id, marker::UserMarker};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Valid ids per Route.
const PER_ROUTE: usize = 2_500;
/// Decodes of each rejected row inside the measured region.
const REJECT_REPEATS: usize = 100;

/// A hand-written three-variant `PayloadEnum`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    A,
    B,
    C,
}

impl Field for Kind {
    const MAX_LEN: Option<usize> = Some(1);

    fn encode<W: core::fmt::Write + ?Sized>(&self, out: &mut W) -> core::fmt::Result {
        let index = match self {
            Self::A => 0,
            Self::B => 1,
            Self::C => 2,
        };
        encode_variant(index, out)
    }
}

impl FieldDecode<'_> for Kind {
    fn decode(raw: &str) -> Result<Self, FieldError> {
        Ok(match decode_variant(raw, 3)? {
            0 => Self::A,
            1 => Self::B,
            _ => Self::C,
        })
    }
}

payload!(FixedP {
    flag: bool,
    a: u8,
    b: i16,
    c: u32,
    d: i64,
    user: Id<UserMarker>,
    s: BoundedStr<8>,
    kind: Kind,
    opt: Option<u64>,
});

route!(F, "f", RouteShape::INLINE, 0, FixedP);
route!(FS, "f.s", RouteShape::INLINE, 61, FixedP);
route!(FV, "f.v", RouteShape::INLINE.view().select(), 0, FixedP);
route!(FT, "f.t", RouteShape::INLINE.signed(), 0, FixedP);

const _: () = noctis_codec::assert_fits::<F>();
const _: () = noctis_codec::assert_fits::<FS>();
const _: () = noctis_codec::assert_fits::<FV>();
const _: () = noctis_codec::assert_fits::<FT>();
const _: () = assert!(<FixedP as Payload>::MAX_ENCODED_LEN.is_some());

type Id100 = ArrayString<100>;

/// One step of splitmix64.
fn splitmix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic random values for id `index` of the stream `stream`.
struct Rng(u64);

impl Rng {
    fn new(stream: u64, index: usize) -> Self {
        Self(splitmix(
            stream ^ (index as u64).wrapping_mul(0x1_0000_0001),
        ))
    }

    fn next(&mut self) -> u64 {
        self.0 = splitmix(self.0);
        self.0
    }
}

fn min_payload() -> FixedP {
    FixedP {
        flag: false,
        a: 0,
        b: 0,
        c: 0,
        d: 0,
        user: Id::new(1),
        s: BoundedStr::from_static(""),
        kind: Kind::A,
        opt: None,
    }
}

fn max_payload() -> FixedP {
    FixedP {
        flag: true,
        a: u8::MAX,
        b: i16::MIN,
        c: u32::MAX,
        d: i64::MIN,
        user: Id::new(u64::MAX),
        s: BoundedStr::from_static("!,:~!,:~"),
        kind: Kind::C,
        opt: Some(u64::MAX),
    }
}

fn signed_max_payload() -> FixedP {
    FixedP {
        b: i16::MAX,
        d: i64::MAX,
        ..max_payload()
    }
}

/// A mid-value Payload drawn from `rng`.
fn mid_payload(rng: &mut Rng) -> Result<FixedP, BoundedStrError> {
    let [a, b0, b1, c0, c1, c2, c3, flag] = rng.next().to_le_bytes();
    let d = rng.next().cast_signed();
    let user = Id::new(rng.next().max(1));
    let text = rng.next().to_le_bytes();
    let len = usize::from(text[0] % 9);
    let mut s = ArrayString::<8>::new();
    for &byte in text.iter().cycle().skip(1).take(len) {
        s.push(char::from(0x20 + byte % 95));
    }
    let kind = match rng.next() % 3 {
        0 => Kind::A,
        1 => Kind::B,
        _ => Kind::C,
    };
    let opt = match rng.next() {
        n if n % 4 == 0 => None,
        n => Some(n >> (n % 64)),
    };
    Ok(FixedP {
        flag: flag & 1 == 1,
        a,
        b: i16::from_le_bytes([b0, b1]),
        c: u32::from_le_bytes([c0, c1, c2, c3]),
        d,
        user,
        s: BoundedStr::new(&s)?,
        kind,
        opt,
    })
}

/// The Payload of id `index` on any Route: the boundary rows first.
fn payload_at(stream: u64, index: usize) -> Result<FixedP, BoundedStrError> {
    match index {
        0 => Ok(min_payload()),
        1 => Ok(max_payload()),
        2 => Ok(signed_max_payload()),
        _ => mid_payload(&mut Rng::new(stream, index)),
    }
}

/// A valid id with its expected Payload.
struct Case {
    id: Id100,
    want: FixedP,
}

/// A valid `FV` id with its expected Payload, envelope and nonce.
struct ViewCase {
    case: Case,
    envelope: ViewEnvelope,
    nonce: SelectNonce,
}

/// A rejected id and the check of its expected error.
struct Rejected {
    id: String,
    check: fn(&str, &PayloadKeys, &PayloadKeys) -> bool,
}

fn rejected(id: &str, check: fn(&str, &PayloadKeys, &PayloadKeys) -> bool) -> Rejected {
    Rejected {
        id: id.to_owned(),
        check,
    }
}

/// `id`'s body fields, with field `index` replaced by `value`.
fn with_field(id: &str, prefix_len: usize, index: usize, value: &str) -> String {
    let (prefix, body) = id.split_at(prefix_len);
    let mut fields: Vec<&str> = body.split(',').collect();
    fields[index] = value;
    format!("{prefix}{}", fields.join(","))
}

#[test]
#[allow(clippy::too_many_lines, reason = "one row per matrix line")]
fn inline_decode_allocates_nothing() {
    let _profiler = dhat::Profiler::builder()
        .testing()
        .file_name(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/dhat-alloc-decode.json"
        ))
        .build();

    // --- Inputs, built before the measured region. ---
    let k0 = PayloadKey::from_bytes([0; 32]);
    let k1 = PayloadKey::from_bytes([1; 32]);
    let k2 = PayloadKey::from_bytes([2; 32]);
    let sign_current = PayloadKeys::new(k1.clone());
    let sign_previous = PayloadKeys::new(k0.clone());
    let keys = PayloadKeys::new(k1).with_previous(k0);
    let wrong_keys = PayloadKeys::new(k2);

    let max_envelope = ViewEnvelope::new('a', 61, 3843, 61).unwrap();
    let max_nonce = SelectNonce::new(3843).unwrap();

    let mut plain_cases = Vec::with_capacity(PER_ROUTE);
    let mut schema_cases = Vec::with_capacity(PER_ROUTE);
    let mut view_cases = Vec::with_capacity(PER_ROUTE);
    let mut signed_cases = Vec::with_capacity(PER_ROUTE);
    for index in 0..PER_ROUTE {
        // F: every 10th id leaves out its trailing `None`.
        let mut want = payload_at(1, index).unwrap();
        let strip = index % 10 == 0;
        if strip {
            want.opt = None;
        }
        let mut id = encode::<F>(&want, &Parts::new()).unwrap();
        if strip {
            assert!(id.ends_with(",0"), "{id}");
            id.truncate(id.len() - 2);
        }
        plain_cases.push(Case { id, want });

        let want = payload_at(2, index).unwrap();
        let id = encode::<FS>(&want, &Parts::new()).unwrap();
        assert!(id.starts_with("1f.s:z"), "{id}");
        schema_cases.push(Case { id, want });

        let want = payload_at(3, index).unwrap();
        let (envelope, nonce) = if index % 2 == 0 {
            (max_envelope, max_nonce)
        } else {
            let mut rng = Rng::new(4, index);
            let action = char::from(b"_aZ09"[usize::try_from(rng.next() % 5).unwrap()]);
            let [tab, page0, page1, version, nonce0, nonce1, ..] = rng.next().to_le_bytes();
            let page = u16::from_le_bytes([page0, page1]) % 3844;
            let envelope = ViewEnvelope::new(action, tab % 62, page, version % 62).unwrap();
            let nonce = SelectNonce::new(u16::from_le_bytes([nonce0, nonce1]) % 3844).unwrap();
            (envelope, nonce)
        };
        let parts = Parts::new().with_envelope(envelope).with_nonce(nonce);
        let id = encode::<FV>(&want, &parts).unwrap();
        view_cases.push(ViewCase {
            case: Case { id, want },
            envelope,
            nonce,
        });

        // FT: every 5th id is signed by the previous key.
        let want = payload_at(5, index).unwrap();
        let signer = if index % 5 == 0 {
            &sign_previous
        } else {
            &sign_current
        };
        let id = encode::<FT>(&want, &Parts::new().with_signer(signer)).unwrap();
        signed_cases.push(Case { id, want });
    }

    // The longest id: the max-boundary FT id fills `MAX_ENCODED_LEN`.
    let max_ft = encode::<FT>(&max_payload(), &Parts::new().with_signer(&sign_current)).unwrap();
    assert_eq!(<FixedP as Payload>::MAX_ENCODED_LEN, Some(71));
    assert_eq!(max_ft.len(), 82, "{max_ft}");
    assert_eq!(signed_cases[1].id.len(), 82);

    let full_f = encode::<F>(&min_payload(), &Parts::new()).unwrap();
    let schema_id = &schema_cases[3].id;
    let signed_id = &signed_cases[3].id;
    let f_prefix = "0f:".len();

    let mut stale_schema = String::from(schema_id.as_str());
    stale_schema.remove(FMT_LEN + "f.s".len() + SEP_LEN);
    stale_schema.replace_range(0..1, "0");
    let mut missing_tag = String::from("0");
    missing_tag.push_str(&signed_id[1..signed_id.len() - TAG_LEN]);
    let mut bad_tag = String::from(signed_id.as_str());
    let last = if bad_tag.ends_with('0') { "1" } else { "0" };
    bad_tag.replace_range(bad_tag.len() - 1.., last);
    let mut too_long = String::from(max_ft.as_str());
    while too_long.len() < MAX_ID_LEN + 1 {
        too_long.push('x');
    }
    let mut generation = String::from(full_f.as_str());
    generation.replace_range(0..1, "G");

    let rejects = vec![
        rejected(&full_f, |id, _, _| {
            matches!(
                black_box(decode_route::<FS>(id)),
                Err(DecodeError::Forged(ForgedReason::RouteMismatch))
            )
        }),
        rejected(&stale_schema, |id, _, _| {
            matches!(
                black_box(decode_route::<FS>(id)),
                Err(DecodeError::Stale(StaleReason::Schema {
                    expected: 61,
                    found: 0
                }))
            )
        }),
        rejected(&missing_tag, |id, keys, _| {
            matches!(
                black_box(decode_route_verified::<FT>(id, keys)),
                Err(DecodeError::Forged(ForgedReason::MissingTag))
            )
        }),
        rejected(&bad_tag, |id, keys, _| {
            matches!(
                black_box(decode_route_verified::<FT>(id, keys)),
                Err(DecodeError::Forged(ForgedReason::BadTag))
            )
        }),
        rejected(signed_id, |id, _, wrong| {
            matches!(
                black_box(decode_route_verified::<FT>(id, wrong)),
                Err(DecodeError::Forged(ForgedReason::BadTag))
            )
        }),
        rejected("0f:~0123456789A", |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Stale(StaleReason::Body { .. }))
            )
        }),
        rejected(&with_field(&full_f, f_prefix, 1, "4I"), |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::Field { field: 1, .. }))
            )
        }),
        rejected(&format!("{full_f},1"), |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::ExtraField { .. }))
            )
        }),
        rejected(&with_field(&full_f, f_prefix, 7, "3"), |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::Field {
                    field: 7,
                    error: FieldError::Overflow
                }))
            )
        }),
        rejected(&with_field(&full_f, f_prefix, 6, "!u0000e9"), |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::Field {
                    field: 6,
                    error: FieldError::BadEscape { .. }
                }))
            )
        }),
        rejected("", |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::Empty))
            )
        }),
        rejected(&too_long, |id, keys, _| {
            matches!(
                black_box(decode_route_verified::<FT>(id, keys)),
                Err(DecodeError::Malformed(CodecError::TooLong { len }))
                    if len == MAX_ID_LEN + 1
            )
        }),
        rejected(
            &format!("0f:{}", "x".repeat(MAX_ID_LEN - "0f:".len())),
            |id, _, _| {
                matches!(
                    black_box(decode_route::<F>(id)),
                    Err(DecodeError::Malformed(CodecError::Field { .. }))
                )
            },
        ),
        rejected("0f:é", |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::NotAscii { index: 3 }))
            )
        }),
        rejected(&generation, |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::Generation {
                    generation: 1
                }))
            )
        }),
        rejected("0f", |id, _, _| {
            matches!(
                black_box(decode_route::<F>(id)),
                Err(DecodeError::Malformed(CodecError::MissingSeparator))
            )
        }),
    ];
    assert_eq!(too_long.len(), MAX_ID_LEN + 1);

    // --- Measured region: no allocation, no formatting on success. ---
    let mut ok = 0_usize;
    let mut first_bad: Option<(&str, usize)> = None;
    let mut first_bad_reject: Option<usize> = None;
    let before = dhat::HeapStats::get();

    for (index, Case { id, want }) in plain_cases.iter().enumerate() {
        let id = black_box(id.as_str());
        let result = if index % 2 == 0 {
            decode_route::<F>(id)
        } else {
            decode(id)
                .map_err(DecodeError::from)
                .and_then(|d| d.payload::<F>())
        };
        if matches!(black_box(&result), Ok(PayloadSource::Inline(p)) if p == want) {
            ok += 1;
        } else if first_bad.is_none() {
            first_bad = Some(("f", index));
        }
    }
    for (index, Case { id, want }) in schema_cases.iter().enumerate() {
        let result = decode_route::<FS>(black_box(id.as_str()));
        if matches!(black_box(&result), Ok(PayloadSource::Inline(p)) if p == want) {
            ok += 1;
        } else if first_bad.is_none() {
            first_bad = Some(("f.s", index));
        }
    }
    for (index, view) in view_cases.iter().enumerate() {
        let id = black_box(view.case.id.as_str());
        let result = decode_route::<FV>(id);
        let sections = black_box(decode(id).map(|d| (d.envelope(), d.nonce())));
        if matches!(black_box(&result), Ok(PayloadSource::Inline(p)) if *p == view.case.want)
            && sections == Ok((Some(view.envelope), Some(view.nonce)))
        {
            ok += 1;
        } else if first_bad.is_none() {
            first_bad = Some(("f.v", index));
        }
    }
    for (index, Case { id, want }) in signed_cases.iter().enumerate() {
        let result = decode_route_verified::<FT>(black_box(id.as_str()), &keys);
        if matches!(black_box(&result), Ok(PayloadSource::Inline(p)) if p == want) {
            ok += 1;
        } else if first_bad.is_none() {
            first_bad = Some(("f.t", index));
        }
    }
    for _ in 0..REJECT_REPEATS {
        for (row, reject) in rejects.iter().enumerate() {
            if !(reject.check)(black_box(reject.id.as_str()), &keys, &wrong_keys)
                && first_bad_reject.is_none()
            {
                first_bad_reject = Some(row);
            }
        }
    }

    let after = dhat::HeapStats::get();
    // --- End of the measured region. ---

    dhat::assert_eq!(
        after.total_blocks - before.total_blocks,
        0,
        "inline decode made {} heap allocations",
        after.total_blocks - before.total_blocks
    );
    dhat::assert_eq!(
        after.total_bytes - before.total_bytes,
        0,
        "inline decode allocated {} heap bytes",
        after.total_bytes - before.total_bytes
    );
    assert_eq!(first_bad, None, "a valid id did not decode to its Payload");
    assert_eq!(ok, 4 * PER_ROUTE);
    assert_eq!(ok, 10_000);
    assert_eq!(
        first_bad_reject.map(|row| rejects[row].id.as_str()),
        None,
        "a rejected id did not give its expected error"
    );

    // --- Negative control: an escaped `String` field allocates. ---
    let before = dhat::HeapStats::get();
    let text = black_box(decode_route::<common::Text>(black_box("0t:a!0b")));
    let after = dhat::HeapStats::get();
    let blocks = after.total_blocks - before.total_blocks;
    dhat::assert!(
        blocks >= 1,
        "the negative control saw {} allocations; is the counting allocator installed?",
        blocks
    );
    assert_eq!(text, Ok(PayloadSource::Inline(common::text("a!b"))));
}
