//! Zero-allocation guard of component dispatch (Story 7.1 Matrix 28-29;
//! AD-25, NFR-9).
//!
//! A dhat counting allocator measures the heap while valid fixed-width ids
//! of derived Routes and a set of rejected ids go through dispatch stage 2
//! (`RouteTable::probe`: `custom_id` decode, Route lookup by borrowed key,
//! kind check and the struct decode the handler start uses), once over 10
//! registered Routes and once over 10 000. The block and byte deltas over
//! each region must be zero. A negative control probes a `String` field and
//! must allocate, so the guard cannot pass vacuously.
//!
//! dhat allows one profiler per process, so this binary holds exactly one
//! `#[test]`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

use std::hint::black_box;

use noctis::__private::codec::{MAX_ID_LEN, Parts, SelectNonce, encode};
use noctis::__private::core::__testing::{RouteTable, responded, route_table};
use noctis::prelude::*;
use noctis::{ComponentHandler, ComponentKind};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Valid ids probed per region.
const VALID: usize = 10_000;
/// Probes of each rejected id per region.
const REJECT_REPEATS: usize = 100;

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }
}

#[derive(StringSelectMenu)]
#[string_select("pick", defaults(placeholder = "Pick"))]
struct Pick {
    item: u64,
}

impl Pick {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }
}

/// The negative control: a `String` field allocates when decoded.
#[derive(Button)]
#[button("neg", overflow = spill, defaults(style = Primary, label = "Neg"))]
struct Named {
    name: String,
}

impl Named {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        Ok(responded())
    }
}

async fn extra(_: Buy, _: ComponentCtx) -> HandlerResult<Responded> {
    Ok(responded())
}

/// One step of splitmix64.
fn splitmix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The table of `Buy`, `Pick`, `Named` and `extras` more Routes `r0`, …
/// (ids leaked before measuring).
fn table(extras: usize) -> RouteTable {
    let module = (0..extras).fold(
        Module::new("shop")
            .add::<Buy>()
            .add::<Pick>()
            .add::<Named>(),
        |module, i| {
            let id: &'static str = Box::leak(format!("r{i}").into_boxed_str());
            module.__add(ComponentHandler::__new::<Buy, _, _>(
                "alloc_dispatch::Extra",
                id,
                ComponentKind::Button,
                extra,
            ))
        },
    );
    route_table(&[module]).unwrap()
}

/// A probe input, built before measuring: the id, the clicked kind and the
/// expected result.
type Probe = (String, ComponentKind, Result<&'static str, &'static str>);

/// Valid ids of `Buy` and `Pick`, with random items and nonces.
fn valid() -> Vec<Probe> {
    let mut seed = 7;
    (0..VALID)
        .map(|i| {
            seed = splitmix(seed);
            if i % 2 == 0 {
                let id = encode::<Buy>(&Buy { item: seed }, &Parts::new()).unwrap();
                (
                    id.to_string(),
                    ComponentKind::Button,
                    Ok("alloc_dispatch::Buy"),
                )
            } else {
                let nonce = SelectNonce::new(u16::try_from(seed % 3844).unwrap()).unwrap();
                let id =
                    encode::<Pick>(&Pick { item: seed }, &Parts::new().with_nonce(nonce)).unwrap();
                (
                    id.to_string(),
                    ComponentKind::StringSelect,
                    Ok("alloc_dispatch::Pick"),
                )
            }
        })
        .collect()
}

/// Rejected ids, one per rejection label the probe reaches.
fn rejected() -> Vec<Probe> {
    let b = ComponentKind::Button;
    vec![
        (String::new(), b, Err("malformed")),
        ("x".repeat(MAX_ID_LEN + 1), b, Err("malformed")),
        ("0shöp:1".to_owned(), b, Err("malformed")),
        ("0Shop.buy:47".to_owned(), b, Err("malformed")),
        ("0nope:".to_owned(), b, Err("unknown_route")),
        (
            "0shop.buy:47".to_owned(),
            ComponentKind::StringSelect,
            Err("kind_mismatch"),
        ),
        ("1shop.buy:147".to_owned(), b, Err("stale")),
        ("0shop.buy:~0123456789A".to_owned(), b, Err("stale")),
        ("0r0:47".to_owned(), b, Err("forged")),
        ("0shop.buy:".to_owned(), b, Err("undecodable")),
        ("0shop.buy:47,9".to_owned(), b, Err("undecodable")),
    ]
}

/// The heap delta (blocks, bytes) of probing every input `repeats` times;
/// every result must match its expectation.
fn measure(table: &RouteTable, inputs: &[Probe], repeats: usize) -> (u64, u64) {
    let before = dhat::HeapStats::get();
    let mut mismatches = 0_usize;
    for _ in 0..repeats {
        for (id, kind, expected) in inputs {
            let got = black_box(table.probe(black_box(id.as_str()), *kind));
            mismatches += usize::from(got != *expected);
        }
    }
    let after = dhat::HeapStats::get();
    assert_eq!(mismatches, 0);
    (
        after.total_blocks - before.total_blocks,
        after.total_bytes - before.total_bytes,
    )
}

#[test]
fn dispatch_lookup_and_decode_allocate_nothing() {
    let _profiler = dhat::Profiler::builder()
        .testing()
        .file_name(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/dhat-alloc-dispatch.json"
        ))
        .build();

    let valid = valid();
    let rejected = rejected();
    let control = vec![(
        "0neg:abc".to_owned(),
        ComponentKind::Button,
        Ok("alloc_dispatch::Named"),
    )];

    for extras in [7, 9_997] {
        let table = table(extras);
        assert!(format!("{table:?}").contains(&format!("len: {}", extras + 3)));
        // Warm up once outside the measured regions.
        let _ = measure(&table, &valid, 1);
        assert_eq!(measure(&table, &valid, 1), (0, 0), "valid, {extras} extras");
        assert_eq!(
            measure(&table, &rejected, REJECT_REPEATS),
            (0, 0),
            "rejected, {extras} extras"
        );
        let (blocks, _) = measure(&table, &control, 1);
        assert!(blocks >= 1, "the negative control saw {blocks} allocations");
    }
}
