//! Story 12.6: a 200-guild run of the `memory_per_guild` bench's
//! measurement (S1-S4). dhat allows one profiler per process and counts
//! process-wide, so this binary installs the dhat allocator and holds
//! exactly one `#[test]`, which builds its own multi-thread runtime.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic with the failing case"
)]

#[path = "../benches/memory_per_guild/budget.rs"]
#[expect(dead_code, reason = "only `Measurement` is used here")]
mod budget;
#[path = "../benches/memory_per_guild/measure.rs"]
mod measure;

#[global_allocator]
static ALLOC: noctis::testkit::dhat::Alloc = noctis::testkit::dhat::Alloc;

const GUILDS: u32 = 200;

#[test]
fn measure_200_guilds() {
    let profiler = measure::start_profiler().unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    // S1: `measure` waits until every run handled each of its guilds and
    // fails otherwise.
    let measured = runtime.block_on(measure::measure(GUILDS)).unwrap();
    drop(runtime);
    drop(profiler);
    assert_eq!(measured.guilds, GUILDS);
    // S2: the default cache holds the guilds' channels and roles.
    assert!(
        measured.default - measured.none > 0,
        "default {} none {}",
        measured.default,
        measured.none
    );
    // S3: idle guilds render nothing.
    assert_eq!(measured.render_cache_bytes, 0);
    // S4: gross-leak sanity only; the bench checks the budget.
    let per_guild = (measured.none - measured.baseline) / i64::from(GUILDS);
    assert!(per_guild < 16_384, "{per_guild} bytes per guild");
}
