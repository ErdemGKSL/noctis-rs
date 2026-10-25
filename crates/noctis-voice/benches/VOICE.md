# Voice benchmarks

The CPU cost of one voice call's 20 ms frame on Songbird's real mixer, measured by
`crates/noctis-voice/benches/voice.rs` (Story 17.3, NFR-3). Allocation counts are
pinned separately by `tests/passthrough_alloc.rs` (NFR-9).

**Not a gate.** No number here gates CI or the story loop: no gate runs this bench
and `crates/noctis-core/benches/budgets.toml` has no voice table.

## Machine

- CPUs: 4 (`nproc`), Intel(R) Xeon(R) Processor (`/proc/cpuinfo`)
- rustc: `rustc 1.97.0 (2d8144b78 2026-07-07)`
- OS: Ubuntu 24.04.5 LTS, Linux 6.18.44 (cloud container, shared host)

## Commit

`27a34113ba520fa1d0a403b174db73018e3a5e4b`, plus Story 17.3's changes (the frame
counters, the driver config and the bench itself).

## Command

```sh
cargo bench -p noctis-voice --bench voice
```

(`cargo bench` has no `--release`; the bench profile inherits release.)
`cargo bench -p noctis-voice --bench voice -- --test` runs every scenario once and
times 10 frames for the summary.

## Method

- Songbird 0.6.1's mock mixer (`internals` feature, a `noctis-voice`
  dev-dependency only): `Mixer::mock` with soft clipping, and per frame
  `Mixer::mix_and_build_packet`, which mixes (or passes through), encodes with
  libopus and seals the RTP packet with AES-256-GCM. No UDP send, no scheduler.
- Encoder bitrate: Songbird's mock default, 128 kbps (`DEFAULT_BITRATE`), stereo.
- Inputs: the committed 4 s fixtures in `tests/fixtures/`, parsed before timing and
  wrapped with the frame counters every played track gets
  (`__testing::instrument`), with no metrics recorder installed.
- Each criterion iteration batch uses `iter_custom`: an untimed setup builds a mixer
  with fresh inputs primed by 5 warm-up ticks, rebuilt whenever 180 of the fixture's
  200 frames are used; only `mix_and_build_packet` is timed.
- The summary table is the bench's own 2 s timing per scenario after criterion;
  calls/core = floor(20 ms / ns per frame), the calls one core could keep fed.

## Results

| Scenario | criterion time per frame (95 % CI) | summary ns/frame | calls/core |
| --- | ---: | ---: | ---: |
| (a) `passthrough_ogg_opus`: one Ogg Opus 20 ms stereo track at volume 1.0 | 1.24 / 1.29 / 1.35 µs | 1227 | 16299 |
| (b) `mp3_decode_encode`: one MP3 44.1 kHz stereo track (decode, resample, mix, encode) | 137.6 / 138.9 / 141.1 µs | 138420 | 144 |
| (c) `two_mixed_tracks`: two Ogg Opus tracks (decode both, mix, encode) | 159.3 / 161.4 / 163.9 µs | 157690 | 126 |

Passthrough costs about 1/110 of a decode-and-encode frame: the packet is copied and
encrypted, nothing else. The decode path is dominated by the libopus encode (and, for
(b), the 44.1 to 48 kHz resample); (c) decodes two Opus streams instead of one MP3.
The numbers exclude the UDP send and Songbird's scheduler, so a real call costs
somewhat more.

## Allocations

`cargo test --release -p noctis-voice --test passthrough_alloc` (dhat, 100 ticks
after 50 warm-up ticks, smallest of 7 windows):

| Case | Blocks per 100 frames | Bytes |
| --- | ---: | ---: |
| passthrough, plain Songbird input | 100 | 11800 |
| passthrough, with Noctis's frame counters | 100 | 11800 |
| MP3 decode, plain | 202 | 15944 |
| MP3 decode, with Noctis's frame counters | 202 | 15944 |

The remaining blocks are symphonia's (the Ogg demuxer boxes each packet); Noctis adds
none per frame.
