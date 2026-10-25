//! The optimized Opus path (Story 17.3, AD-30): input preparation, frame
//! counting, the driver config and the encoder bitrate.
//!
//! Songbird's mixer sends an Opus packet by passthrough (no decode, no
//! encode) when exactly one track plays at volume 1.0 and the packet holds
//! 960 samples (20 ms at 48 kHz); otherwise it decodes, mixes and encodes.
//! It reports neither, so Noctis parses each current track's input itself
//! and wraps the parsed demuxer and decoder in counting wrappers:
//!
//! - a packet of the played track that the mixer reads and never hands to
//!   the decoder was sent by passthrough
//!   (`noctis_voice_passthrough_frames_total`);
//! - the decoder's output, in 20 ms frames at 48 kHz, is what the mixer
//!   encodes for a one-track call (`noctis_voice_encoded_frames_total`).
//!
//! The wrappers only delegate and touch an atomic flag, a plain counter and
//! two `metrics::Counter` handles resolved once per track, so a steady-state
//! frame allocates nothing in Noctis.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use metrics::Counter;
use songbird::Config;
use songbird::driver::MixMode;
use songbird::input::codecs::{get_codec_registry, get_probe};
use songbird::input::{Input, LiveInput, Parsed};
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{
    CODEC_TYPE_OPUS, CodecDescriptor, CodecParameters, Decoder, DecoderOptions, FinalizeResult,
};
use symphonia::core::errors::{Error, Result as SymphResult};
use symphonia::core::formats::{
    Cue, FormatOptions, FormatReader, Packet, SeekMode, SeekTo, SeekedTo, Track,
};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::Metadata;

use crate::config::MAX_BITRATE_RANGE;

/// The counter of the 20 ms Opus frames sent by passthrough.
pub(crate) const PASSTHROUGH_FRAMES: &str = "noctis_voice_passthrough_frames_total";

/// The counter of the 20 ms frames decoded for mixing and encoding.
pub(crate) const ENCODED_FRAMES: &str = "noctis_voice_encoded_frames_total";

/// Songbird's output rate.
const RATE: u64 = 48_000;

/// Samples per channel in one 20 ms frame at [`RATE`].
const FRAME: u64 = 960;

/// The encoder bitrate when the channel's is unknown: Discord's default
/// channel bitrate.
pub(crate) const FALLBACK_BITRATE: u32 = 64_000;

/// The encoder bitrate of a call: the channel's bitrate clamped to
/// Discord's channel range (the same as [`MAX_BITRATE_RANGE`]), or
/// [`FALLBACK_BITRATE`] when unknown, capped by `max`.
pub(crate) fn effective_bitrate(channel: Option<u32>, max: Option<u32>) -> u32 {
    let channel = channel.map_or(FALLBACK_BITRATE, |bitrate| {
        bitrate.clamp(*MAX_BITRATE_RANGE.start(), *MAX_BITRATE_RANGE.end())
    });
    max.map_or(channel, |max| channel.min(max))
}

/// The driver config of every call: no Songbird gateway timeout (Noctis's
/// join timeout covers the join), stereo music mixing, soft clipping (which
/// Songbird applies to mixed PCM frames only, never to passthrough frames)
/// and one preallocated track slot (the queue plays one track at a time).
pub(crate) fn driver_config() -> Config {
    Config::default()
        .gateway_timeout(None)
        .mix_mode(MixMode::Stereo)
        .use_softclip(true)
        .preallocated_tracks(1)
}

/// The two frame counters of one track.
#[derive(Debug, Clone)]
pub(crate) struct FrameCounters {
    passthrough: Counter,
    encoded: Counter,
}

impl FrameCounters {
    /// No-op counters (no `app` label bound yet).
    pub(crate) fn noop() -> Self {
        Self {
            passthrough: Counter::noop(),
            encoded: Counter::noop(),
        }
    }

    /// The counters labelled `app`, resolved on the current recorder; no-ops
    /// without a label.
    pub(crate) fn new(app: Option<&Arc<str>>) -> Self {
        let Some(app) = app else {
            return Self::noop();
        };
        Self {
            passthrough: metrics::counter!(PASSTHROUGH_FRAMES, "app" => Arc::clone(app)),
            encoded: metrics::counter!(ENCODED_FRAMES, "app" => Arc::clone(app)),
        }
    }

    /// Counters over two given handles (tests).
    #[cfg(test)]
    pub(crate) fn from_handles(passthrough: Counter, encoded: Counter) -> Self {
        Self {
            passthrough,
            encoded,
        }
    }
}

/// Parses `input` (creating it first when lazy) and wraps it with
/// `counters`; the error is the `MakePlayableError` text.
pub(crate) async fn prepare(input: Input, counters: FrameCounters) -> Result<Input, String> {
    let input = input
        .make_playable_async(get_codec_registry(), get_probe())
        .await
        .map_err(|error| error.to_string())?;
    instrument(input, counters).map_err(|_| "the input was not parsed".to_owned())
}

/// Wraps an already-parsed input with `counters`; any other input comes back
/// unchanged as `Err`.
pub(crate) fn instrument(input: Input, counters: FrameCounters) -> Result<Input, Input> {
    match input {
        Input::Live(LiveInput::Parsed(parsed), lazy) => {
            Ok(Input::Live(LiveInput::Parsed(wrap(parsed, counters)), lazy))
        }
        other => Err(other),
    }
}

fn wrap(parsed: Parsed, counters: FrameCounters) -> Parsed {
    let opus = parsed.decoder.codec_params().codec == CODEC_TYPE_OPUS;
    let pending = Arc::new(AtomicBool::new(false));
    let FrameCounters {
        passthrough,
        encoded,
    } = counters;
    Parsed {
        format: Box::new(CountingFormat {
            inner: parsed.format,
            track_id: parsed.track_id,
            opus,
            pending: Pending {
                flag: Arc::clone(&pending),
                passthrough,
            },
        }),
        decoder: Box::new(CountingDecoder {
            inner: parsed.decoder,
            pending,
            decoded: Decoded { acc: 0, encoded },
        }),
        ..parsed
    }
}

/// The played track's last Opus packet that no decode has taken yet: when
/// the next packet is read (or the reader ends or drops), it was sent by
/// passthrough.
struct Pending {
    flag: Arc<AtomicBool>,
    passthrough: Counter,
}

impl Pending {
    /// Settles the previous packet; `next` is whether the new one may pass
    /// through.
    fn settle(&self, next: bool) {
        if self.flag.swap(next, Ordering::Relaxed) {
            self.passthrough.increment(1);
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.settle(false);
    }
}

/// A demuxer that counts the packets the mixer passes through.
struct CountingFormat {
    inner: Box<dyn FormatReader>,
    track_id: u32,
    /// Whether the played track is Opus (only Opus passes through).
    opus: bool,
    pending: Pending,
}

impl FormatReader for CountingFormat {
    fn try_new(_: MediaSourceStream, _: &FormatOptions) -> SymphResult<Self> {
        Err(Error::Unsupported("noctis frame counter"))
    }

    fn cues(&self) -> &[Cue] {
        self.inner.cues()
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.inner.metadata()
    }

    fn seek(&mut self, mode: SeekMode, to: SeekTo) -> SymphResult<SeekedTo> {
        // A packet read before a seek was dropped, not sent.
        self.pending.flag.store(false, Ordering::Relaxed);
        self.inner.seek(mode, to)
    }

    fn tracks(&self) -> &[Track] {
        self.inner.tracks()
    }

    fn default_track(&self) -> Option<&Track> {
        self.inner.default_track()
    }

    fn next_packet(&mut self) -> SymphResult<Packet> {
        let packet = self.inner.next_packet();
        match &packet {
            Ok(packet) if packet.track_id() == self.track_id => self.pending.settle(self.opus),
            Ok(_) => {}
            Err(_) => self.pending.settle(false),
        }
        packet
    }

    fn into_inner(self: Box<Self>) -> MediaSourceStream {
        let Self { inner, .. } = *self;
        inner.into_inner()
    }
}

/// The decoded samples not yet counted as a whole 20 ms frame.
struct Decoded {
    /// Decoded frames × 48 000 (a source rate of `r` makes one 20 ms output
    /// frame every `960 × r`).
    acc: u64,
    encoded: Counter,
}

impl Decoded {
    fn add(&mut self, frames: usize, rate: u32) {
        if rate == 0 {
            return;
        }
        let unit = FRAME * u64::from(rate);
        self.acc += u64::try_from(frames).unwrap_or(u64::MAX / RATE) * RATE;
        let whole = self.acc / unit;
        if whole > 0 {
            self.acc %= unit;
            self.encoded.increment(whole);
        }
    }
}

impl Drop for Decoded {
    fn drop(&mut self) {
        // The mixer pads and encodes the last partial frame.
        if self.acc > 0 {
            self.encoded.increment(1);
        }
    }
}

/// A decoder that counts the 20 ms frames it supplies.
struct CountingDecoder {
    inner: Box<dyn Decoder>,
    pending: Arc<AtomicBool>,
    decoded: Decoded,
}

impl Decoder for CountingDecoder {
    fn try_new(_: &CodecParameters, _: &DecoderOptions) -> SymphResult<Self> {
        Err(Error::Unsupported("noctis frame counter"))
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[]
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn codec_params(&self) -> &CodecParameters {
        self.inner.codec_params()
    }

    fn decode(&mut self, packet: &Packet) -> SymphResult<AudioBufferRef<'_>> {
        // A decoded packet was not passed through.
        self.pending.store(false, Ordering::Relaxed);
        let decoded = self.inner.decode(packet);
        if let Ok(buf) = &decoded {
            self.decoded.add(buf.frames(), buf.spec().rate);
        }
        decoded
    }

    fn finalize(&mut self) -> FinalizeResult {
        self.inner.finalize()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.inner.last_decoded()
    }
}

#[cfg(test)]
mod tests;
