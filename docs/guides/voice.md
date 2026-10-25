# Voice: passthrough and encoding

This guide covers the cost side of Noctis voice (feature `voice`, FR-80): which audio
is sent to Discord as it is, which is decoded and encoded again, how the encoder
bitrate is chosen, and how to check it on a running bot. Story 17.4 extends it into
the full voice guide.

## How a track reaches the wire

Every 20 ms the voice driver (Songbird 0.6) builds one Opus packet per call. Noctis
plays one track per call at a time (the queue's current track), and it parses that
track's input itself before the driver gets it.

- **Passthrough.** When the track is the call's only playing track, its volume is
  exactly `1.0` and the next packet is Opus holding 20 ms of audio (960 samples at
  48 kHz), the packet goes to the wire unchanged: no decode, no mix, no encode. The
  cost is a copy and the encryption.
- **Decode and encode.** Otherwise the packet is decoded, resampled to 48 kHz if
  needed, mixed (stereo, soft-clipped) and encoded with libopus. This happens for:
  - any non-Opus input (MP3, AAC, FLAC, WAV);
  - any volume other than `1.0` (`set_volume(0.5)`, `set_volume(0.0)`);
  - Opus with 40 ms or 60 ms frames: Songbird passes the first three such packets
    through, then decodes the rest of the track;
  - more than one playing track (not reachable through the queue, which plays one).

The choice is made again every 20 ms: after `set_volume(0.5)` the next frames are
encoded, and once the volume is back at `1.0` passthrough resumes. A paused track
reads nothing.

On the bench machine (`crates/noctis-voice/benches/VOICE.md`) a passthrough frame
costs about 1 µs and a decoded-and-encoded frame about 140 µs, so passthrough is
roughly two orders of magnitude cheaper per call.

## The encoder bitrate

Passthrough frames keep the bitrate they were encoded at. For encoded frames:

- the encoder follows the joined channel's bitrate. Noctis reads it from the Discord
  cache when the bot's own voice state lands in a channel (resource `CHANNEL`, cached
  by default) and updates it on that channel's `CHANNEL_UPDATE`;
- an unknown channel (not cached, or `DiscordCacheConfig::none()`) uses 64 kbps,
  Discord's default channel bitrate;
- `VoiceConfig::max_bitrate(Some(bps))` caps it (8000..=384000; another value fails
  the build). The default `None` follows the channel.

```rust
use noctis::voice::VoiceConfig;

builder.voice(VoiceConfig::default().max_bitrate(Some(96_000)));
```

## Checking it: the frame metrics

Two counters, labelled `app` only:

| Metric | Counts |
| --- | --- |
| `noctis_voice_passthrough_frames_total` | 20 ms Opus frames sent by passthrough |
| `noctis_voice_encoded_frames_total` | 20 ms frames decoded for mixing and encoding (one track per call, so the frames the call encoded) |

A bot whose library is pre-encoded should see the passthrough counter grow at 50 per
second per playing call and the encoded counter stay nearly flat (it moves while the
volume is not `1.0`). If the encoded counter grows as fast, check the input format,
the volume and the Opus frame size (`ffprobe -show_packets` shows `duration_time`
`0.020000` for 20 ms frames).

## Pre-encoding a library

Encode each file once to Ogg Opus at 48 kHz with 20 ms frames:

```sh
ffmpeg -i in.flac -c:a libopus -b:a 128k -ar 48000 -frame_duration 20 out.opus
```

`-frame_duration 20` is libopus's default, but stating it keeps a later change from
silently moving the library to 40/60 ms frames, which are decoded. Play the result
with `Track::file("out.opus")` or from a URL with `Track::url(..)`.

## Streaming with yt-dlp

YouTube serves Opus audio as WebM Opus at 48 kHz, which plays by passthrough. Ask
`yt-dlp` for it and fall back to the best audio otherwise:

```rust
use noctis::voice::Track;

let track = Track::command(
    "yt-dlp",
    ["-f", "bestaudio[acodec=opus]/bestaudio", "-o", "-", "--", url.as_str()],
)?;
```

Keep the `--` before the user's URL: the program would otherwise parse a value such as
`--exec=...` as an option.
