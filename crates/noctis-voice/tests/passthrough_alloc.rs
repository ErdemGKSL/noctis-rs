//! Allocation counts of the voice mixer (Story 17.3, NFR-9): a dhat
//! counting allocator measures the heap blocks and bytes of 100 ticks of
//! Songbird's real mixer (`Mixer::mix_and_build_packet`: mix, encode or
//! passthrough, AEAD; `internals` mock, no UDP send), for:
//!
//! - `passthrough_plain`: the Ogg Opus fixture as Songbird parses it;
//! - `passthrough`: the same input wrapped with Noctis's frame counters
//!   (`__testing::instrument`, what every played track gets);
//! - `decode_plain` / `decode`: the same for the MP3 fixture (decode,
//!   resample, encode).
//!
//! Noctis adds nothing per frame: each wrapped count must equal its plain
//! count, and the wrapped counts must equal the recorded constants (a later
//! allocation in the frame path fails here).
//!
//! dhat counts the whole process and allows one profiler per process, so
//! this binary has no test harness (`harness = false`) and runs a one-worker
//! runtime without a metrics recorder or a tracing subscriber. Every input
//! is prepared before its window; each window builds a mixer, runs 50
//! warm-up ticks (the first second), then measures the next 100 ticks; each
//! case reports the smallest of 7 windows.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a harness-less test: it panics with the failing case and reports on stdout"
)]

use std::path::PathBuf;

use songbird::constants::VOICE_PACKET_MAX;
use songbird::driver::bench_internals::mixer::Mixer;
use songbird::driver::bench_internals::track_context;
use songbird::input::codecs::{get_codec_registry, get_probe};
use songbird::input::{File, Input};
use songbird::tracks::Track;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Heap blocks of 100 passthrough ticks of the Ogg Opus fixture, measured
/// at Story 17.3: one per tick, symphonia's Ogg demuxer boxing each packet
/// (Noctis adds none).
const PASSTHROUGH_BLOCKS_PER_100: u64 = 100;

/// Heap blocks of 100 decode-and-encode ticks of the MP3 fixture, measured
/// at Story 17.3: symphonia's MP3 demuxer and decoder, about two per tick
/// (Noctis adds none).
const DECODE_BLOCKS_PER_100: u64 = 202;

/// Warm-up ticks per window.
const WARM_UPS: usize = 50;
/// Measured ticks per window.
const TICKS: usize = 100;
/// Windows per case (the smallest counts).
const WINDOWS: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    blocks: u64,
    bytes: u64,
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The fixture `name`, parsed, and wrapped when `instrumented`.
fn input(rt: &tokio::runtime::Runtime, name: &str, instrumented: bool) -> Input {
    let input: Input = File::new(fixture(name)).into();
    let input = rt
        .block_on(input.make_playable_async(get_codec_registry(), get_probe()))
        .unwrap();
    if instrumented {
        noctis_voice::__testing::instrument(input, "alloc")
            .unwrap_or_else(|_| panic!("a parsed input"))
    } else {
        input
    }
}

/// One window: a fresh mixer playing the input alone at volume 1.0.
fn window(rt: &tokio::runtime::Runtime, name: &str, instrumented: bool) -> Cost {
    let (mut mixer, listeners) = Mixer::mock(rt.handle().clone(), true);
    let (handle, ctx) = track_context(Track::new(input(rt, name, instrumented)));
    mixer.add_track(ctx).unwrap();
    let mut packet = vec![0_u8; VOICE_PACKET_MAX];
    for _ in 0..WARM_UPS {
        mixer.mix_and_build_packet(&mut packet).unwrap();
    }
    let before = dhat::HeapStats::get();
    for _ in 0..TICKS {
        let written = mixer.mix_and_build_packet(&mut packet).unwrap();
        assert!(written > 0, "{name}: the track plays through the window");
    }
    let after = dhat::HeapStats::get();
    drop((mixer, listeners, handle));
    Cost {
        blocks: after.total_blocks - before.total_blocks,
        bytes: after.total_bytes - before.total_bytes,
    }
}

/// The smallest cost of [`WINDOWS`] windows.
fn quiet(rt: &tokio::runtime::Runtime, name: &str, instrumented: bool) -> Cost {
    (0..WINDOWS)
        .map(|_| window(rt, name, instrumented))
        .min()
        .unwrap()
}

fn main() {
    let _profiler = dhat::Profiler::builder()
        .testing()
        .file_name(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/dhat-passthrough-alloc.json"
        ))
        .build();
    let probe = std::hint::black_box(Box::new([0_u8; 64]));
    let tracked = dhat::HeapStats::get().total_blocks;
    drop(probe);
    assert!(tracked > 0, "dhat counts no allocations");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let cases = [
        (
            "passthrough_plain",
            quiet(&rt, "sine-stereo-20ms.opus", false),
        ),
        ("passthrough", quiet(&rt, "sine-stereo-20ms.opus", true)),
        ("decode_plain", quiet(&rt, "sine-44k.mp3", false)),
        ("decode", quiet(&rt, "sine-44k.mp3", true)),
    ];
    drop(rt);
    for (name, cost) in &cases {
        println!("case {name}: blocks {}, bytes {}", cost.blocks, cost.bytes);
    }
    let [
        (_, passthrough_plain),
        (_, passthrough),
        (_, decode_plain),
        (_, decode),
    ] = cases;
    assert_eq!(
        passthrough, passthrough_plain,
        "passthrough: the frame counters add an allocation"
    );
    assert_eq!(
        decode, decode_plain,
        "decode: the frame counters add an allocation"
    );
    assert_eq!(
        passthrough.blocks, PASSTHROUGH_BLOCKS_PER_100,
        "passthrough: recorded {PASSTHROUGH_BLOCKS_PER_100}, measured {} (a removed \
         allocation updates the constant in the same commit)",
        passthrough.blocks
    );
    assert_eq!(
        decode.blocks, DECODE_BLOCKS_PER_100,
        "decode: recorded {DECODE_BLOCKS_PER_100}, measured {} (a removed allocation \
         updates the constant in the same commit)",
        decode.blocks
    );
}
