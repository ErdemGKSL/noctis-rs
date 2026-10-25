//! Frame counting on Songbird's real mixer (`internals`: `Mixer::mock` and
//! `MockScheduler`, mix + encode + AEAD, no voice connection), the encoder
//! bitrate and the driver config.
//!
//! The fixtures in `tests/fixtures/` were generated once with ffmpeg 6.1
//! (4 s of a 440 Hz sine):
//!
//! ```sh
//! SRC='-f lavfi -i sine=frequency=440:sample_rate=48000:duration=4'
//! BE='-map_metadata -1 -fflags +bitexact -flags:a +bitexact'
//! ffmpeg $SRC $BE -ac 2 -c:a libopus -b:a 32k -ar 48000 -frame_duration 20 sine-stereo-20ms.opus
//! ffmpeg $SRC $BE -ac 1 -c:a libopus -b:a 32k -ar 48000 -frame_duration 20 sine-mono-20ms.opus
//! ffmpeg $SRC $BE -ac 2 -c:a libopus -b:a 32k -ar 48000 -frame_duration 20 -f webm sine-stereo-20ms.webm
//! ffmpeg $SRC $BE -ac 2 -c:a libopus -b:a 32k -ar 48000 -frame_duration 60 sine-stereo-60ms.opus
//! ffmpeg $SRC $BE -ar 44100 -ac 2 -c:a libmp3lame -b:a 48k sine-44k.mp3
//! ```

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use metrics_util::debugging::{DebugValue, DebuggingRecorder, Snapshotter};
use songbird::driver::MixMode;
use songbird::driver::bench_internals::mixer::Mixer;
use songbird::driver::bench_internals::track_context;
use songbird::driver::{Listeners, MockScheduler};
use songbird::input::File;
use songbird::tracks::{Track as SongbirdTrack, TrackHandle};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::*;

/// The fixtures' tick budget: 4 s is 200 ticks, plus slack.
const TICKS: usize = 300;

/// The bound on a counter wait (ended tracks drop on Songbird's disposal
/// thread).
const WAIT: Duration = Duration::from_secs(15);

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Samples per channel at 48 kHz in an Opus packet (its TOC byte), as
/// Songbird's `opus2::packet::get_nb_samples` reads them.
fn opus_samples(packet: &[u8]) -> u64 {
    let Some(&toc) = packet.first() else {
        return 0;
    };
    let config = toc >> 3;
    let frame: u64 = match config {
        0..=11 => [480, 960, 1920, 2880][usize::from(config % 4)],
        12..=15 => [480, 960][usize::from(config % 2)],
        _ => [120, 240, 480, 960][usize::from(config % 4)],
    };
    let count = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => packet.get(1).map_or(0, |b| u64::from(b & 0x3f)),
    };
    frame * count
}

/// What a fixture holds, from demuxing (and decoding) it directly.
#[derive(Debug, Default)]
struct Census {
    /// The 960-sample Opus packets (`P`).
    full: u64,
    /// The samples of the other Opus packets (`S`).
    short_samples: u64,
    /// Decoded frames and their rate (every packet decoded).
    decoded: u64,
    rate: u32,
}

impl Census {
    fn of(name: &str) -> Self {
        let file = std::fs::File::open(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
        let mut format = get_probe()
            .format(
                &Hint::new(),
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .format;
        let track = format
            .default_track()
            .unwrap_or_else(|| panic!("{name}: a track"))
            .clone();
        let mut decoder = get_codec_registry()
            .make(&track.codec_params, &DecoderOptions::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let opus = track.codec_params.codec == CODEC_TYPE_OPUS;
        let mut census = Self::default();
        while let Ok(packet) = format.next_packet() {
            if packet.track_id() != track.id {
                continue;
            }
            if opus {
                match opus_samples(packet.buf()) {
                    FRAME => census.full += 1,
                    n => census.short_samples += n,
                }
            }
            if let Ok(buf) = decoder.decode(&packet) {
                census.decoded += u64::try_from(buf.frames()).unwrap_or_default();
                census.rate = buf.spec().rate;
            }
        }
        census
    }

    /// The 20 ms frames of the short packets, decoded.
    fn short_frames(&self) -> u64 {
        self.short_samples.div_ceil(FRAME)
    }

    /// The 20 ms frames of every decoded sample.
    fn all_frames(&self) -> u64 {
        (self.decoded * RATE).div_ceil(FRAME * u64::from(self.rate))
    }
}

/// The two counters of one track, read back from a `DebuggingRecorder`
/// that resolved them (labels checked: `app` only).
struct Meter {
    snapshotter: Snapshotter,
    app: &'static str,
    passthrough: u64,
    encoded: u64,
}

impl Meter {
    fn new(app: &'static str) -> (Self, FrameCounters) {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let counters =
            metrics::with_local_recorder(&recorder, || FrameCounters::new(Some(&Arc::from(app))));
        (
            Self {
                snapshotter,
                app,
                passthrough: 0,
                encoded: 0,
            },
            counters,
        )
    }

    /// `(passthrough, encoded)` so far (a snapshot swaps them to zero).
    fn read(&mut self) -> (u64, u64) {
        for (key, _, _, value) in self.snapshotter.snapshot().into_vec() {
            let key = key.key();
            let labels: Vec<_> = key
                .labels()
                .map(|l| (l.key().to_owned(), l.value().to_owned()))
                .collect();
            assert_eq!(labels, [("app".to_owned(), self.app.to_owned())]);
            let DebugValue::Counter(n) = value else {
                panic!("{} is a counter", key.name());
            };
            match key.name() {
                PASSTHROUGH_FRAMES => self.passthrough += n,
                ENCODED_FRAMES => self.encoded += n,
                other => panic!("unexpected metric {other}"),
            }
        }
        (self.passthrough, self.encoded)
    }

    /// Waits (bounded) until the counters read `want`, then asserts them.
    fn expect(&mut self, want: (u64, u64), what: &str) {
        let deadline = Instant::now() + WAIT;
        while self.read() != want && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(self.read(), want, "{what}: (passthrough, encoded)");
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap_or_else(|e| panic!("runtime: {e}"))
}

/// The fixture `name` through the player's own path ([`prepare`]).
fn prepared(rt: &tokio::runtime::Runtime, name: &str, counters: FrameCounters) -> Input {
    let input: Input = File::new(fixture(name)).into();
    rt.block_on(prepare(input, counters))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Songbird's mock mixer on a mock scheduler: `run_once` mixes, encodes,
/// encrypts, then applies track commands and removes ended tracks.
struct Rig {
    rt: tokio::runtime::Runtime,
    scheduler: Option<MockScheduler>,
    mixer: Option<Mixer>,
    _listeners: Listeners,
}

impl Rig {
    fn new() -> Self {
        let rt = runtime();
        let (mixer, listeners) = Mixer::mock(rt.handle().clone(), true);
        Self {
            rt,
            scheduler: None,
            mixer: Some(mixer),
            _listeners: listeners,
        }
    }

    /// Adds the fixture `name` as a track at `volume` (before the first
    /// tick).
    fn add(&mut self, name: &str, volume: f32, counters: FrameCounters) -> TrackHandle {
        let input = prepared(&self.rt, name, counters);
        let (handle, ctx) = track_context(SongbirdTrack::new(input).volume(volume));
        self.mixer
            .as_mut()
            .unwrap_or_else(|| panic!("tracks are added before the first tick"))
            .add_track(ctx)
            .unwrap_or_else(|e| panic!("add_track: {e:?}"));
        handle
    }

    /// Runs `n` ticks.
    fn ticks(&mut self, n: usize) {
        let scheduler = self.scheduler.get_or_insert_with(|| {
            let mut scheduler = MockScheduler::new(None);
            if let Some(mixer) = self.mixer.take() {
                scheduler.add_mixer_direct(mixer);
            }
            scheduler
        });
        for _ in 0..n {
            scheduler.core.run_once();
        }
    }

    /// Drops the mixer and its remaining tracks.
    fn finish(mut self) {
        self.scheduler.take();
        self.mixer.take();
    }
}

/// One fixture alone at `volume`, run to its end.
fn alone(name: &str, volume: f32, app: &'static str) -> Meter {
    let (meter, counters) = Meter::new(app);
    let mut rig = Rig::new();
    rig.add(name, volume, counters);
    rig.ticks(TICKS);
    rig.finish();
    meter
}

#[cfg(feature = "ogg")]
#[test]
fn passthrough_stereo() {
    let census = Census::of("sine-stereo-20ms.opus");
    assert!(census.full >= 199, "{census:?}");
    alone("sine-stereo-20ms.opus", 1.0, "stereo")
        .expect((census.full, census.short_frames()), "stereo at 1.0");
}

#[cfg(feature = "ogg")]
#[test]
fn passthrough_mono() {
    let census = Census::of("sine-mono-20ms.opus");
    assert!(census.full >= 199, "{census:?}");
    alone("sine-mono-20ms.opus", 1.0, "mono")
        .expect((census.full, census.short_frames()), "mono at 1.0");
}

#[cfg(feature = "mkv")]
#[test]
fn passthrough_webm() {
    let census = Census::of("sine-stereo-20ms.webm");
    assert!(census.full >= 199, "{census:?}");
    alone("sine-stereo-20ms.webm", 1.0, "webm")
        .expect((census.full, census.short_frames()), "webm at 1.0");
}

/// Any volume but unity decodes every packet, including one just below
/// 1.0 (Songbird's `abs(v - 1.0) < EPSILON`).
#[cfg(feature = "ogg")]
#[test]
fn volume_off_unity_decodes() {
    let census = Census::of("sine-stereo-20ms.opus");
    let want = (0, census.full + census.short_frames());
    for (volume, app) in [
        (0.5, "half"),
        (0.0, "zero"),
        (2.0, "double"),
        (1.0 - 2.0 * f32::EPSILON, "below-unity"),
    ] {
        alone("sine-stereo-20ms.opus", volume, app).expect(want, app);
    }
}

/// A volume change stops passthrough at the next tick; the return to 1.0
/// resumes it.
#[cfg(feature = "ogg")]
#[test]
fn switch_and_back() {
    let census = Census::of("sine-stereo-20ms.opus");
    let (mut meter, counters) = Meter::new("switch");
    let mut rig = Rig::new();
    let handle = rig.add("sine-stereo-20ms.opus", 1.0, counters);
    rig.ticks(30);
    handle
        .set_volume(0.5)
        .unwrap_or_else(|e| panic!("volume: {e}"));
    rig.ticks(30);
    handle
        .set_volume(1.0)
        .unwrap_or_else(|e| panic!("volume: {e}"));
    rig.ticks(TICKS);
    rig.finish();
    meter.expect(
        (census.full - 30, 30 + census.short_frames()),
        "30 ticks at 0.5",
    );
}

/// Two playing tracks are mixed: neither passes through, each counts its
/// own decoded frames.
#[cfg(feature = "ogg")]
#[test]
fn two_tracks_mix() {
    let census = Census::of("sine-stereo-20ms.opus");
    let want = (0, census.full + census.short_frames());
    let (mut first, a) = Meter::new("first");
    let (mut second, b) = Meter::new("second");
    let mut rig = Rig::new();
    rig.add("sine-stereo-20ms.opus", 1.0, a);
    rig.add("sine-stereo-20ms.opus", 1.0, b);
    rig.ticks(TICKS);
    rig.finish();
    first.expect(want, "first of two");
    second.expect(want, "second of two");
}

/// A paused second track does not count as live: the first passes
/// through; the paused one counts nothing.
#[cfg(feature = "ogg")]
#[test]
fn second_paused() {
    let census = Census::of("sine-stereo-20ms.opus");
    let (mut first, a) = Meter::new("playing");
    let (mut second, b) = Meter::new("paused");
    let mut rig = Rig::new();
    rig.add("sine-stereo-20ms.opus", 1.0, a);
    let input = prepared(&rig.rt, "sine-stereo-20ms.opus", b);
    let (_handle, ctx) = track_context(SongbirdTrack::new(input).pause());
    rig.mixer
        .as_mut()
        .unwrap_or_else(|| panic!("before the first tick"))
        .add_track(ctx)
        .unwrap_or_else(|e| panic!("add_track: {e:?}"));
    rig.ticks(TICKS);
    rig.finish();
    first.expect((census.full, census.short_frames()), "the playing track");
    second.expect((0, 0), "the paused track");
}

/// MP3 never passes through: every decoded frame is resampled to 48 kHz
/// and encoded.
#[cfg(feature = "mp3")]
#[test]
fn non_opus_decodes() {
    let census = Census::of("sine-44k.mp3");
    assert_eq!(census.rate, 44_100);
    alone("sine-44k.mp3", 1.0, "mp3").expect((0, census.all_frames()), "mp3 at 1.0");
}

/// 60 ms Opus frames are not 960-sample packets: Songbird passes the
/// first strikes through, then blocks passthrough for the track and
/// decodes the rest.
#[cfg(feature = "ogg")]
#[test]
fn sixty_ms_frames_decode() {
    let census = Census::of("sine-stereo-60ms.opus");
    assert_eq!(census.full, 0, "{census:?}");
    let packets = census.short_samples / 2880;
    let struck = SONGBIRD_STRIKES.min(packets);
    alone("sine-stereo-60ms.opus", 1.0, "sixty").expect(
        (
            struck,
            (census.short_samples - struck * 2880).div_ceil(FRAME),
        ),
        "60 ms frames",
    );
}

/// Songbird 0.6.1's `OPUS_PASSTHROUGH_STRIKE_LIMIT`: the non-960-sample
/// packets it still passes through before blocking passthrough.
const SONGBIRD_STRIKES: u64 = 3;

/// A paused track reads nothing and counts nothing; resumed, it finishes
/// as if never paused.
#[cfg(feature = "ogg")]
#[test]
fn paused_counts_nothing() {
    let census = Census::of("sine-stereo-20ms.opus");
    let (mut meter, counters) = Meter::new("pause");
    let mut rig = Rig::new();
    let handle = rig.add("sine-stereo-20ms.opus", 1.0, counters);
    rig.ticks(30);
    handle.pause().unwrap_or_else(|e| panic!("pause: {e}"));
    rig.ticks(1);
    let before = meter.read();
    rig.ticks(20);
    assert_eq!(meter.read(), before, "20 paused ticks");
    handle.play().unwrap_or_else(|e| panic!("play: {e}"));
    rig.ticks(TICKS);
    rig.finish();
    meter.expect((census.full, census.short_frames()), "paused, then resumed");
}

/// Without a label the counters are no-ops, and a played track still
/// counts nothing anywhere.
#[cfg(feature = "ogg")]
#[test]
fn unbound_label_is_a_noop() {
    let recorder = DebuggingRecorder::new();
    let snapshotter = recorder.snapshotter();
    let counters = metrics::with_local_recorder(&recorder, || FrameCounters::new(None));
    let mut rig = Rig::new();
    rig.add("sine-stereo-20ms.opus", 1.0, counters);
    rig.add("sine-stereo-20ms.opus", 0.5, FrameCounters::noop());
    rig.ticks(TICKS);
    rig.finish();
    assert!(snapshotter.snapshot().into_vec().is_empty());
}

/// The wrappers delegate; a seek settles the packet read before it as
/// sent.
#[cfg(feature = "ogg")]
#[test]
fn wrappers_delegate() {
    let rt = runtime();
    let plain: Input = File::new(fixture("sine-stereo-20ms.opus")).into();
    let Input::Live(LiveInput::Parsed(plain), _) = rt
        .block_on(plain.make_playable_async(get_codec_registry(), get_probe()))
        .unwrap_or_else(|e| panic!("{e}"))
    else {
        panic!("parsed");
    };
    let seen = |seek: bool| {
        let passthrough = Arc::new(AtomicU64::new(0));
        let encoded = Arc::new(AtomicU64::new(0));
        let counters = FrameCounters::from_handles(
            Counter::from_arc(Arc::clone(&passthrough)),
            Counter::from_arc(Arc::clone(&encoded)),
        );
        let Input::Live(LiveInput::Parsed(mut parsed), _) =
            prepared(&rt, "sine-stereo-20ms.opus", counters)
        else {
            panic!("parsed");
        };
        assert_eq!(parsed.decoder.codec_params().codec, CODEC_TYPE_OPUS);
        assert_eq!(
            parsed.decoder.codec_params().channels,
            plain.decoder.codec_params().channels
        );
        assert_eq!(parsed.format.tracks().len(), plain.format.tracks().len());
        assert_eq!(
            parsed.format.default_track().map(|t| t.id),
            plain.format.default_track().map(|t| t.id)
        );
        assert!(parsed.format.cues().is_empty());
        let packet = parsed
            .format
            .next_packet()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(packet.track_id(), parsed.track_id);
        if seek {
            parsed
                .format
                .seek(
                    SeekMode::Coarse,
                    SeekTo::TimeStamp {
                        ts: 0,
                        track_id: parsed.track_id,
                    },
                )
                .unwrap_or_else(|e| panic!("{e}"));
        }
        drop(parsed);
        (
            passthrough.load(Ordering::Relaxed),
            encoded.load(Ordering::Relaxed),
        )
    };
    assert_eq!(
        seen(false),
        (1, 0),
        "a read, undecoded packet passed through"
    );
    assert_eq!(
        seen(true),
        (1, 0),
        "a seek settles the read packet as passed through"
    );
}

/// The wrappers are never built by symphonia; `instrument` refuses an
/// unparsed input.
#[test]
fn static_constructors_refuse() {
    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(Vec::new())),
        MediaSourceStreamOptions::default(),
    );
    assert!(CountingFormat::try_new(stream, &FormatOptions::default()).is_err());
    assert!(CountingDecoder::try_new(&CodecParameters::new(), &DecoderOptions::default()).is_err());
    assert!(CountingDecoder::supported_codecs().is_empty());
    let lazy: Input = File::new(fixture("sine-stereo-20ms.opus")).into();
    assert!(instrument(lazy, FrameCounters::noop()).is_err());
}

/// A decoded partial frame is counted when the decoder drops (the mixer
/// pads and encodes it); a rate of 0 is skipped.
#[test]
fn decoded_rounds_up() {
    let encoded = Arc::new(AtomicU64::new(0));
    let mut decoded = Decoded {
        acc: 0,
        encoded: Counter::from_arc(Arc::clone(&encoded)),
    };
    decoded.add(1152, 44_100);
    decoded.add(1152, 0);
    assert_eq!(encoded.load(Ordering::Relaxed), 1);
    decoded.add(960 * 3, 48_000);
    assert_eq!(encoded.load(Ordering::Relaxed), 4);
    drop(decoded);
    assert_eq!(encoded.load(Ordering::Relaxed), 5);
}

#[test]
fn effective_bitrate_rows() {
    for (channel, max, want) in [
        (Some(96_000), None, 96_000),
        (Some(384_000), Some(128_000), 128_000),
        (Some(64_000), Some(128_000), 64_000),
        (Some(8_000), None, 8_000),
        (Some(0), None, 8_000),
        (Some(7_999), None, 8_000),
        (Some(1_000_000), None, 384_000),
        (None, None, 64_000),
        (None, Some(32_000), 32_000),
        (None, Some(128_000), 64_000),
    ] {
        assert_eq!(
            effective_bitrate(channel, max),
            want,
            "{channel:?} / {max:?}"
        );
    }
}

/// Both counter names have a `METRICS` row: a counter labelled `app` only.
#[test]
fn counters_match_the_metrics_catalog() {
    use noctis_core::observe::{METRICS, MetricType};
    for name in [PASSTHROUGH_FRAMES, ENCODED_FRAMES] {
        let row = METRICS
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("{name} has a METRICS row"));
        assert_eq!(row.metric_type, MetricType::Counter, "{name}");
        assert_eq!(row.labels, ["app"], "{name}");
    }
}

#[test]
fn driver_config_fields() {
    let config = driver_config();
    assert!(config.gateway_timeout.is_none());
    assert_eq!(config.mix_mode, MixMode::Stereo);
    assert!(config.use_softclip);
    assert_eq!(config.preallocated_tracks, 1);
}
