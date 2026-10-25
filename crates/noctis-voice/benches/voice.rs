//! The voice bench (Story 17.3, NFR-3): the CPU cost of one call's 20 ms
//! frame on Songbird's real mixer (`internals` mock: mix, Opus encode or
//! passthrough, AEAD; no UDP send), with the frame counters every played
//! track gets (`__testing::instrument`):
//!
//! - `passthrough_ogg_opus`: one Ogg Opus 20 ms track at volume 1.0;
//! - `mp3_decode_encode`: one MP3 44.1 kHz track (decode, resample,
//!   encode);
//! - `two_mixed_tracks`: two Ogg Opus tracks (decode both, mix, encode).
//!
//! Each measures per-frame time with `iter_custom`: an untimed setup builds
//! a mixer with fresh inputs primed by a few warm-up ticks (again whenever
//! the 4 s fixture nears its end), and the timed part runs
//! `mix_and_build_packet`. After criterion, `main` prints a summary table
//! from its own 2 s timing per scenario (10 frames with `--test`):
//! `scenario | ns/frame | calls/core`, where calls/core =
//! floor(20 ms / ns per frame).
//!
//! Results: `benches/VOICE.md`. Not a gate.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a bench: it panics on a broken setup and reports on stdout"
)]

use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group};
use songbird::constants::VOICE_PACKET_MAX;
use songbird::driver::Listeners;
use songbird::driver::bench_internals::mixer::Mixer;
use songbird::driver::bench_internals::track_context;
use songbird::input::codecs::{get_codec_registry, get_probe};
use songbird::input::{File, Input};
use songbird::tracks::Track;

/// Warm-up ticks after each (untimed) mixer build.
const PRIME: u64 = 5;
/// Ticks a mixer runs before it is rebuilt (the fixtures last 200 ticks).
const LIFE: u64 = 180;
/// One frame.
const FRAME: Duration = Duration::from_millis(20);

/// A scenario: its name and its fixtures (one track each).
#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    fixtures: &'static [&'static str],
}

const SCENARIOS: [Scenario; 3] = [
    Scenario {
        name: "passthrough_ogg_opus",
        fixtures: &["sine-stereo-20ms.opus"],
    },
    Scenario {
        name: "mp3_decode_encode",
        fixtures: &["sine-44k.mp3"],
    },
    Scenario {
        name: "two_mixed_tracks",
        fixtures: &["sine-stereo-20ms.opus", "sine-stereo-20ms.opus"],
    },
];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A mixer playing a scenario, and how many ticks it ran.
struct Live {
    mixer: Mixer,
    _listeners: Listeners,
    used: u64,
}

/// Builds and primes a mixer for `scenario` (untimed).
fn build(rt: &tokio::runtime::Runtime, scenario: Scenario, packet: &mut [u8]) -> Live {
    let (mut mixer, listeners) = Mixer::mock(rt.handle().clone(), true);
    for name in scenario.fixtures {
        let input: Input = File::new(fixture(name)).into();
        let input = rt
            .block_on(input.make_playable_async(get_codec_registry(), get_probe()))
            .unwrap();
        let input = noctis_voice::__testing::instrument(input, "bench")
            .unwrap_or_else(|_| panic!("a parsed input"));
        let (_handle, ctx) = track_context(Track::new(input));
        mixer.add_track(ctx).unwrap();
    }
    for _ in 0..PRIME {
        mixer.mix_and_build_packet(packet).unwrap();
    }
    Live {
        mixer,
        _listeners: listeners,
        used: PRIME,
    }
}

/// Times `frames` frames of `scenario`, rebuilding the mixer (untimed) as
/// its input nears the end.
fn timed(rt: &tokio::runtime::Runtime, scenario: Scenario, frames: u64) -> Duration {
    let mut packet = vec![0_u8; VOICE_PACKET_MAX];
    let mut live: Option<Live> = None;
    let mut total = Duration::ZERO;
    let mut left = frames;
    while left > 0 {
        let current = match live.as_mut() {
            Some(current) if current.used < LIFE => current,
            _ => live.insert(build(rt, scenario, &mut packet)),
        };
        let n = left.min(LIFE - current.used);
        let start = Instant::now();
        for _ in 0..n {
            black_box(current.mixer.mix_and_build_packet(&mut packet).unwrap());
        }
        total += start.elapsed();
        current.used += n;
        left -= n;
    }
    total
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

fn voice(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("voice");
    for scenario in SCENARIOS {
        group.bench_function(scenario.name, |b| {
            b.iter_custom(|iters| timed(&rt, scenario, iters));
        });
    }
    group.finish();
}

criterion_group!(benches, voice);

/// `scenario | ns/frame | calls/core` from a 2 s timing per scenario (10
/// frames with `--test`).
fn summary() {
    let smoke = std::env::args().any(|arg| arg == "--test");
    let rt = runtime();
    println!();
    println!("| scenario | ns/frame | calls/core |");
    println!("| --- | ---: | ---: |");
    for scenario in SCENARIOS {
        let (frames, elapsed) = if smoke {
            (10_u64, timed(&rt, scenario, 10))
        } else {
            let mut frames: u64 = 0;
            let mut elapsed = Duration::ZERO;
            while elapsed < Duration::from_secs(2) {
                elapsed += timed(&rt, scenario, 1000);
                frames += 1000;
            }
            (frames, elapsed)
        };
        let ns = elapsed.as_nanos() / u128::from(frames);
        let calls = FRAME.as_nanos().checked_div(ns).unwrap_or(0);
        println!("| {} | {ns} | {calls} |", scenario.name);
    }
}

/// `criterion_main!(benches)`, then the summary table.
fn main() {
    benches();
    Criterion::default().configure_from_args().final_summary();
    summary();
}
