---
title: 'Story 17.3: Optimized Opus path: passthrough, encoder tuning and the voice bench'
type: 'feature'
created: '2026-10-25'
status: 'in-progress'
baseline_revision: '7836d36323d6295ded8ef541a903eb22d20b7857'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-17-2-track-playback-inputs-and-the-per-guild-queue.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 17.2 plays tracks, but nothing proves that pre-encoded Opus skips decode and encode, the encoder runs at Songbird's fixed 128 kbps whatever the channel allows, and there is no measurement of what one call costs in CPU or allocations (FR-80 codec, NFR-3, NFR-9, AD-25, AD-29, AD-30).

**Approach:** Songbird 0.6.1 already sends Opus by passthrough. Its mixer passes a packet through when exactly one track is playing at volume 1.0, the codec is Opus and the packet holds 960 samples (`mixer/mod.rs` `mix_tracks`, `mix_logic.rs`). It re-checks this every 20 ms, so a return to unity volume resumes passthrough. Noctis makes this observable and tuned:
- **Input preparation.** Noctis parses each current track's input itself (`Input::make_playable_async`) and wraps the parsed symphonia `FormatReader` and `Decoder` in allocation-free counting wrappers that feed `noctis_voice_passthrough_frames_total` and `noctis_voice_encoded_frames_total`.
- **Driver config.** The driver config is set explicitly.
- **Encoder bitrate.** The bitrate follows the joined channel's bitrate, capped by the new `VoiceConfig::max_bitrate`.
- **Measurement.** A criterion bench and a dhat test drive Songbird's real mixer through its `internals` feature, which is enabled for tests and benches only.
- **Docs.** A first `docs/guides/voice.md` and the Usage Skill explain pre-encoding.

## Boundaries & Constraints

**Always:**
- Songbird stays `0.6.1` with the 17.1 normal-dependency features (`driver`, `gateway`, `rustls`, `tws`). Only `noctis-voice`'s `[dev-dependencies]` adds `songbird = { workspace = true, features = ["internals"] }`. `internals` adds `byteorder`, which is already in the graph, and no new crate. `cargo tree -p noctis --features voice -e normal -f '{p} {f}'` never shows `internals`.
- Every current track's input is made playable by Noctis (`make_playable_async(get_codec_registry(), get_probe())`) before `play_only`. It is then wrapped (Design Notes) and handed to the driver as `Input::Live(LiveInput::Parsed(..), lazy)`. Preparation runs in the player's spawned task, never under the Call lock. The slot token is checked after preparation and again under the Call lock, so a stale preparation drops its input. Dropping a command input kills its child.
- Steady-state passthrough frames allocate nothing in `noctis-voice` code:
  - counters are `metrics::Counter` handles resolved once per track (`Counter::noop()` before the `app` label is bound);
  - the wrappers only delegate and touch atomics or plain fields.
- The new counters carry only the `app` label. No guild, channel, track or URL label.
- No Songbird, symphonia or reqwest type appears in a facade-visible signature. The only exception is the `#[doc(hidden)] pub mod __testing` in `noctis-voice`, which the facade never re-exports (precedent: `noctis_core::__testing`).
- Every public item has rustdoc. The public API change (`max_bitrate`) re-blesses `xtask/public-api.txt` and updates the template, its `AGENTS.md`, the Usage Skill and its `API hash` in the same commit.

**Never:**
- Fork or patch Songbird. Never reach mixer internals in production code, and never enable `internals`, `builtin-queue`, `receive`, `twilight` or `serenity` on the normal dependency.
- Play two tracks at once from the queue. The queue stays one-track (`play_only`). The "second track" fallback exists only at driver level, in tests and the bench.
- Add a Discord rule id. The encoder bitrate is a sender setting that Discord does not reject (decision), so `max_bitrate` is a config range like `media_max_redirects`. `docs/discord-rules.md` and `noctis-rules` stay unchanged.
- Gate CI or the story loop on a bench number. Never run `cargo hack`. Never fetch from the network in tests: the fixtures are committed files.
- `VoiceConfig::recording()`, the music-bot example, or the full voice guide. Those belong to Story 17.4.

## I/O & Edge-Case Matrix

Frame counting. Fixtures: `F20s` = Ogg Opus stereo 20 ms, `F20m` = Ogg Opus mono 20 ms, `W20` = WebM Opus stereo 20 ms, `F60` = Ogg Opus stereo 60 ms, `M44` = MP3 44.1 kHz. Each lasts 4 s. "Drive" means Songbird's mock mixer runs ticks until the track ends, then the track is dropped (which flushes the counters). `P` = the fixture's 960-sample Opus packets and `S` = its other packets, both counted by demuxing the fixture in the test.

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| passthrough stereo | `F20s`, one track, volume 1.0 | passthrough == `P`; encoded == ceil(S's samples / 960) (0 when `S` is empty) | — |
| passthrough mono | `F20m`, one track, 1.0 | same as stereo (Songbird passes packets through whatever the channel count) | — |
| passthrough webm | `W20`, one track, 1.0 | same | — |
| volume ≠ 1 | `F20s` at 0.5; at 0.0; at 2.0 | passthrough == 0; encoded == `P` + ceil(S samples/960) | — |
| volume 1.0 − ε | `F20s` at `1.0 - 2.0 * f32::EPSILON` | passthrough == 0 (Songbird's `abs(v - 1.0) < EPSILON` test) | — |
| switch and back | `F20s`: 30 ticks at 1.0, `set_volume(0.5)`, 30 ticks, `set_volume(1.0)`, then to the end | passthrough == `P` − 30; encoded == 30 (+ the short-packet frames) | — |
| two tracks | two `F20s` tracks playing in one mixer at 1.0 | passthrough == 0 on both; each counts its own decoded frames | — |
| second paused | two tracks, the second paused | passthrough as with one track (Songbird counts only playing tracks) | — |
| non-Opus | `M44` at 1.0 | passthrough == 0; encoded == ceil(decoded frames × 48000 / 44100 / 960) | — |
| 60 ms frames | `F60` at 1.0 | passthrough == 0 (3 strikes, then Songbird blocks passthrough for the track); encoded == ceil(total samples / 960) | — |
| paused track | `F20s` paused for 20 ticks | no counts while paused | — |
| no recorder / unbound label | any | counters are no-ops; nothing allocates for them | — |
| probe failure | file of 1 KiB of zero bytes; missing file; HTTP 404; command that prints nothing | `TrackError{error}` from Noctis (the `MakePlayableError` text); the next track starts; nothing is handed to the driver | never panics |
| stale preparation | `skip()` or `stop()` while the input is still being prepared | the prepared input is dropped and `play_only` is never called for that token; no `TrackStart` | — |
| start event | preparation succeeds | `play_only`, then Noctis reports `on_playable(token)`, so `TrackStart` fires once (the driver fires no `Playable` for an already-parsed input) | — |

Encoder bitrate (`effective_bitrate(channel: Option<u32>, max: Option<u32>) -> u32`). The channel value is clamped to `8_000..=384_000`. An unknown channel falls back to 64 000 (Discord's default channel bitrate). The result is `min(channel_or_fallback, max)`.

| channel / max | → |
|---|---|
| `Some(96_000)` / `None` | 96 000 |
| `Some(384_000)` / `Some(128_000)` | 128 000 |
| `Some(64_000)` / `Some(128_000)` | 64 000 |
| `Some(8_000)` / `None` | 8 000 |
| `Some(0)` / `None`; `Some(7_999)` / `None` | 8 000 (clamped) |
| `Some(1_000_000)` / `None` | 384 000 (clamped) |
| `None` / `None` | 64 000 |
| `None` / `Some(32_000)` | 32 000 |
| `None` / `Some(128_000)` | 64 000 |

Bitrate events. The App runs on the fakes with a pending call in guild G, and the Discord cache holds G's voice channel V with bitrate 96 000.

| Scenario | Expected applied bitrate (crate-private `VoiceShared::bitrate(G)`) |
|---|---|
| the bot's `VOICE_STATE_UPDATE` puts it in V | 96 000 (`Driver::set_bitrate(Bitrate::Bits(..))` on the call) |
| the same with `max_bitrate(Some(64_000))` | 64 000 |
| V is not in the cache, or the cache is `DiscordCacheConfig::none()` | 64 000 |
| `CHANNEL_UPDATE` of V with bitrate 128 000 | 128 000 |
| `CHANNEL_UPDATE` of another channel of G; of a channel of a guild with no call | unchanged; nothing happens |
| the call moves to channel V2 (bitrate 32 000, cached) | 32 000 |
| others' voice states | ignored |

Config (`VoiceConfig::max_bitrate(Option<u32>)`, default `None`. Out of range is a `BuildViolation::plugin::<VoicePlugin>` naming the range):

| Value | Result |
|---|---|
| `None` | ✓ (follows the channel) |
| `Some(8_000)`, `Some(96_000)`, `Some(384_000)` | ✓ |
| `Some(0)`, `Some(7_999)`, `Some(384_001)`, `Some(u32::MAX)` | ✗ (range `8000..=384000`) |

Allocation (`tests/passthrough_alloc.rs`, harness-less dhat). Each case runs 50 warm-up ticks (the first second), then measures the next 100 ticks of `Mixer::mix_and_build_packet`.

| Scenario | Expected |
|---|---|
| `F20s` plain Songbird input vs the same input through `__testing::instrument` | the block and byte counts are equal (Noctis adds 0 per frame) |
| `F20s` instrumented | the block count == recorded constant `PASSTHROUGH_BLOCKS_PER_100` (measured at implementation; a later added allocation fails) |
| `M44` instrumented | the block count == recorded `DECODE_BLOCKS_PER_100`, and equal to the plain input's count |

</intent-contract>

## Code Map

- `crates/noctis-voice/src/player.rs` -- `SongbirdPlayer::play` (L84) spawns a task that locks the Call and builds `SongbirdTrack::new(input)`. Change it so that it:
  1. builds `track.input(&media)`;
  2. awaits `frames::prepare(input, counters)`, which returns `Result<Input, String>`;
  3. on `Err`, calls `queue.on_error(token, e)` if `slot.token == token`;
  4. on success, locks the Call, re-checks the token, then `play_only` and records the handle;
  5. calls `queue.on_playable(token)` (still idempotent: `queue.rs` L181 `started` flag).

  Keep the `End`/`Error` driver reports. `Playable` from the driver may stay registered because it is harmless and never fires for parsed inputs. Update `player/tests.rs` (`play_then_stop_clears_the_slot`, `pause_before_the_handle_is_recorded`) for the async preparation.
- `crates/noctis-voice/src/track.rs` L265 `TrackInfo::input` -- unchanged. It still yields lazy inputs, and `prepare` makes them live.
- `crates/noctis-voice/src/call.rs` -- `Call::from_config(.., Config::default().gateway_timeout(None))` (L131-136) becomes `crate::frames::driver_config()`. `Entry` gains the applied `bitrate: u32` (initial 64 000 capped by `max_bitrate`, set on the Call at creation). `VoiceInner.binding` (`app` label, L83) is the source of the counters' label. Add `set_bitrate(guild, channel: Id<ChannelMarker>, channel_bitrate: Option<u32>)`, which applies only when the entry's channel is `channel` and the value changed, under the Call lock in a spawned task (never while holding the map mutex). Add `#[cfg(test)] bitrate(guild) -> Option<u32>`.
- `crates/noctis-voice/src/plugin.rs` -- `on_voice_state` `Some(channel)` arm (L120-128): after `moved`, read `ctx.cache().channel(channel).await` (`Ok(Some(c))` → `c.bitrate`; anything else → `None`) and call `shared.set_bitrate`. Mount a `ChannelUpdate` handler (`EventHandler::__new::<ChannelUpdate, _, _>("noctis_voice::on_channel_update", true, false, ..)`): with `guild_id` `Some(g)`, call `set_bitrate(g, channel.id, channel.bitrate)`.
- `crates/noctis-voice/src/config.rs` -- add `max_bitrate`, `MAX_BITRATE_RANGE: RangeInclusive<u32> = 8_000..=384_000`, the getter `get_max_bitrate() -> Option<u32>` (getter convention, memlog 17.1), and a `violations()` row. Extend the doc example.
- `crates/noctis-voice/src/frames.rs` (new) -- the counting wrappers, `prepare`, `driver_config`, `effective_bitrate` and the counter names (Design Notes).
- `crates/noctis-voice/src/lib.rs` -- `mod frames;`, plus the `#[doc(hidden)] pub mod __testing { pub fn instrument(input: songbird::input::Input, app: &str) -> Result<songbird::input::Input, songbird::input::Input> }`. It wraps an already-parsed input (`Err(input)` when it is not `Input::Live(LiveInput::Parsed(_), _)`) with counters labelled `app`. Add crate-doc bullets for passthrough, the metrics and `max_bitrate`.
- `crates/noctis-core/src/observe.rs` L103-104, L250-262 and `observe/tests.rs` L63-65 -- add the two counter rows (`app`) and change the count from 25 to 27.
- `crates/noctis/src/lib.rs` `pub mod voice` docs (around L2876) -- passthrough, the two metrics and `max_bitrate`. No new re-export: `VoiceConfig` is already exported.
- `xtask/src/deps_check.rs` `check_songbird` (L718) and tests (L1389-1482) -- allow `songbird` as a normal **or dev** dependency of `noctis-voice`. A build dependency, or any other crate, still fails. Update the module docs (L45-46).
- Songbird 0.6.1 facts (verified in `~/.cargo/registry/src/*/songbird-0.6.1`):
  - `Mixer::mock(handle, softclip) -> (Mixer, Listeners)` and `MockScheduler::{new(None), add_mixer_direct, core.run_once()}` (`driver/test_impls.rs`, `driver/scheduler/live.rs` L193) need `internals`. With `internals`, `march_deadline` does not sleep.
  - `Mixer::mix_and_build_packet(&mut [u8])` mixes, encodes and encrypts without sending. `run_once` also sends to `127.0.0.1:5316`; the `Io` errors only call `do_rebuilds(false, false)`. Keep the `Listeners` alive.
  - `songbird::driver::bench_internals::track_context(Track) -> (TrackHandle, TrackContext)` and `Mixer::add_track(ctx)`.
  - `songbird::constants::VOICE_PACKET_MAX` (or a 1460-byte buffer).
  - `Config::{mix_mode, use_softclip, preallocated_tracks}`, with defaults `Stereo`/`true`/`1`.
  - `Driver::set_bitrate(Bitrate)`, where `songbird::driver::Bitrate` = `opus2::Bitrate`. It persists in the parked mixer.
  - The encoder is `Application::Audio`.
  - An already-parsed input fires no `Playable` (`tasks/events.rs`: only state changes fire).
- `crates/noctis-core/tests/pipeline_alloc.rs` -- the harness-less dhat pattern (recorded constants, warm-up, smallest of 7 windows). `crates/noctis-core/benches/PIPELINE.md` is the results-file layout.
- `crates/noctis-voice/src/tests.rs` -- the App-on-fakes e2e helpers (`start`, `pending_call`, `until`, DebuggingRecorder totals) for the bitrate rows.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-voice/tests/fixtures/` -- commit five fixtures generated once with ffmpeg 6.1 (`-map_metadata -1 -fflags +bitexact -flags:a +bitexact`, source `-f lavfi -i "sine=frequency=440:sample_rate=48000:duration=4"`):
  - `sine-stereo-20ms.opus`: `-ac 2 -c:a libopus -b:a 32k -ar 48000 -frame_duration 20`;
  - `sine-mono-20ms.opus`: `-ac 1`, otherwise the same;
  - `sine-stereo-20ms.webm`: the same as stereo, with `-f webm`;
  - `sine-stereo-60ms.opus`: `-frame_duration 60`;
  - `sine-44k.mp3`: `-ar 44100 -ac 2 -c:a libmp3lame -b:a 48k`.

  Put the exact commands in a comment at the top of `src/frames/tests.rs`. Each file stays under 32 KiB.
- `Cargo.toml` (`noctis-voice`) -- dev-deps `songbird` (`internals`), `criterion = { workspace = true, features = ["cargo_bench_support"] }`, `dhat.workspace = true` and tokio `rt-multi-thread`. Add `[[bench]] name = "voice", harness = false` and `[[test]] name = "passthrough_alloc", harness = false`.
- `crates/noctis-voice/src/frames.rs` + `src/frames/tests.rs` -- implement the Design Notes. Unit tests cover:
  - every frame-counting row, driven by `Mixer::mock` + `MockScheduler::run_once` with a `DebuggingRecorder` installed via `metrics::with_local_recorder` or the 17.2 snapshotter;
  - every `effective_bitrate` row;
  - `driver_config()` fields;
  - wrapper delegation (`codec_params`, `tracks`, `seek` resets the pending flag).

  Every loop is bounded (≤ 1000 ticks).
- `crates/noctis-voice/src/{player,call,plugin,config,lib}.rs` -- the Code Map changes. Add `player/tests.rs` rows for "probe failure" and "stale preparation", plus e2e tests in `src/tests.rs` for every bitrate-events row. The cache is seeded by dispatching `GUILD_CREATE`/`CHANNEL_CREATE` through the fake gateway, or with `App::feed_discord_cache`. Every wait is ≤ 15 s.
- `crates/noctis-voice/tests/passthrough_alloc.rs` -- harness-less dhat test for the allocation rows. It uses `#[global_allocator] static ALLOC: dhat::Alloc`, a one-worker runtime, no recorder and no tracing subscriber. The input is prepared before the window. It reports the smallest of 7 windows and prints the counts on stdout. The constants are recorded with a doc comment naming the story.
- `crates/noctis-voice/benches/voice.rs` -- criterion bench (`criterion_group!`/`criterion_main!`) with three functions in group `voice`: `passthrough_ogg_opus`, `mp3_decode_encode` and `two_mixed_tracks`. Each uses `iter_custom`: an untimed setup builds `Mixer::mock(handle, true)` with the instrumented input(s) primed by a few warm-up ticks, and the timed part runs `mix_and_build_packet`. A new mixer is built whenever the input nears its end. The result is per-frame time per call. After criterion, `main` prints a summary table (`scenario | ns/frame | calls/core = floor(20 ms / ns_per_frame)`) from its own 2 s timing per scenario, or 10 frames when the args contain `--test`.
- `crates/noctis-voice/benches/VOICE.md` -- machine (`nproc`, CPU model, rustc, OS), commit, command (`cargo bench -p noctis-voice --bench voice`), the encoder bitrate used (Songbird's mock default, 128 kbps), the numbers and calls per core for (a)/(b)/(c), the method (mock mixer: mix + encode + AEAD, no UDP send), and "Not a gate".
- `crates/noctis-core/src/observe.rs`, `observe/tests.rs` -- the METRICS rows: `noctis_voice_passthrough_frames_total` ("20 ms Opus frames sent by passthrough (feature `voice`)") and `noctis_voice_encoded_frames_total` ("20 ms frames decoded for mixing and encoding (feature `voice`)"), both counters labelled `app`.
- `xtask/src/deps_check.rs` -- the `songbird-gated` amendment, with tests. A dev dependency of `noctis-voice` passes; a build dependency of `noctis-voice`, or a normal or dev dependency of `noctis-core`, fails with an AD-30 message.
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`.
- `docs/guides/voice.md` (new) and `docs/guides/README.md` (one index line) -- "Voice: passthrough and encoding", covering:
  - when passthrough applies and when it falls back;
  - `max_bitrate` and channel bitrate following;
  - the two metrics and how to check them;
  - pre-encoding a library (`ffmpeg -i in -c:a libopus -b:a 128k -ar 48000 -frame_duration 20 out.opus`);
  - streaming with `yt-dlp` `bestaudio[acodec=opus]`: `Track::command("yt-dlp", ["-f", "bestaudio[acodec=opus]/bestaudio", "-o", "-", "--", url])` (YouTube's Opus is WebM Opus at 48 kHz);
  - that 40/60 ms Opus frames are decoded.

  Story 17.4 extends this file.
- `examples/project-template/.claude/skills/noctis/{references/voice.md, SKILL.md}` -- a "Passthrough and encoding" section with the same content, condensed, plus `max_bitrate` and the metrics. Update the `API hash`.
- `examples/project-template/{AGENTS.md, src/lib.rs, src/modules/voice/mod.rs}` -- the AGENTS voice line gains passthrough and pre-encoding plus `max_bitrate`. The `.voice(..)` call in `lib.rs` gains a comment showing `.max_bitrate(Some(96_000))`. The module doc says pre-encoded Opus plays by passthrough.
- `ARCHITECTURE-SPINE.md` AD-30 and `maintainer-decisions.md` §10 -- add "Amended (2026-10-25, Story 17.3)" lines covering the Design Notes decisions: input preparation and counters, the metric semantics, the driver config, bitrate following and `max_bitrate`, the dev-only `internals` edge and the `songbird-gated` dev allowance.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- record the implementation decisions as `(decision)` lines. The planning decisions are already logged.

**Acceptance Criteria:**
- Given the committed Ogg Opus fixture prepared through the player's own path (`frames::prepare`, the same function `SongbirdPlayer::play` calls) and played as the only track at volume 1.0 on Songbird's mock mixer (`internals`), when the mixer runs it to the end, then `noctis_voice_passthrough_frames_total{app}` equals the fixture's 960-sample packet count and `noctis_voice_encoded_frames_total{app}` counts only its short packets. At volume 0.5, or for the MP3 fixture, passthrough stays 0. After returning to 1.0, passthrough resumes. A live voice connection is out of reach without a fake voice server, the same limit as the deferred 17.2 item, so the mock mixer is the outermost driver surface available.
- Given `cargo test --release -p noctis-voice --test passthrough_alloc`, when it runs, then it passes, and the instrumented and plain per-frame block counts are equal and match the recorded constants.
- Given `cargo bench -p noctis-voice --bench voice -- --test`, when it runs, then all three scenarios execute and the summary prints ns/frame and calls/core. `benches/VOICE.md` records a full run.
- Given `VoiceConfig::default().max_bitrate(Some(384_001))`, when the App builds, then the build fails naming `8000..=384000`.
- Given `cargo tree -p noctis --features voice -e normal -f '{p} {f}'`, then no line shows Songbird's `internals`. `deps-check`, `cargo deny check`, `api-snapshot` (re-blessed), `skill-check` and `rules-catalog` (unchanged) pass.

## Spec Change Log

## Review Triage Log

## Design Notes

**Counting by wrapping the parsed input (decision).** Songbird decides between passthrough and encoding inside its private mixer and reports neither. Its codec registry is `&'static` (so it cannot carry an `app` label), and an already-parsed input skips the driver's probe. Noctis therefore prepares the input itself and wraps `Parsed.format`/`Parsed.decoder` (both `pub Box<dyn ..>`).

- **Shared state.** `CountingFormat { inner, track_id, opus: bool, pending: Arc<AtomicBool>, passthrough: Counter }` and `CountingDecoder { inner, pending, acc: u64, encoded: Counter }`.
- **`next_packet`.** On a packet of `track_id`, `if pending.swap(opus) { passthrough.increment(1) }`. On `Err`, it flushes the same way. Packets of other tracks do not touch `pending`.
- **`decode`.** It sets `pending = false`. On `Ok(buf)` it adds `buf.frames() * 48_000` to `acc`, and while `acc >= 960 * buf.spec().rate` it subtracts that amount and increments `encoded`. A rate of 0 is skipped.
- **`seek`.** It clears `pending`.
- **`Drop`.** The format flushes `pending`, and the decoder counts one more frame if `acc > 0`, because the mixer pads and encodes the last partial frame.
- **Static constructors.** `try_new` returns `Error::Unsupported("noctis frame counter")` and `supported_codecs` returns `&[]`. They are never called.

There are two entry points:
- `frames::instrument(input: Input, counters: FrameCounters) -> Result<Input, Input>` wraps an already-parsed input.
- `frames::prepare(input, counters) -> Result<Input, String>` is `make_playable_async` followed by `instrument`, with the error mapped to its `Display` text.

`FrameCounters` holds the two `Counter`s and is built from the `app` label, or as no-ops. Tests and `__testing::instrument` use the same functions.

**Tick timing.** A `TrackHandle` command takes effect at the end of the tick after it is sent, because `run_once` mixes first and processes commands last. For the "switch and back" row this means: ticks 1-31 pass through, ticks 32-61 decode (30), and passthrough resumes from tick 62.

`encoded` is therefore "20 ms frames this track supplied by decoding". Noctis plays one track per call, so this equals the frames the driver encoded for that call. The doc says so.

**Driver config (decision).** `driver_config()` is `Config::default().gateway_timeout(None).mix_mode(MixMode::Stereo).use_softclip(true).preallocated_tracks(1)`.
- Songbird soft-clips only mixed PCM frames, never passthrough ones, which satisfies "soft-clip only while mixing" with no runtime toggle.
- Songbird's encoder is `Application::Audio`, libopus's music mode. `OPUS_SET_SIGNAL` is not reachable through Songbird's public driver API, so signal detection stays automatic. Patching Songbird is out of scope.

**Bitrate (decision).** Discord does not enforce the sender's encode bitrate, so following the channel is a quality and bandwidth choice, not a rule. The channel's bitrate comes from the Discord cache (`CHANNEL` is cached by default) when the bot's own state lands in a channel, and from `CHANNEL_UPDATE` of that channel. An unknown channel uses 64 kbps (Discord's default), not Songbird's 128 kbps. Passthrough frames keep their source bitrate.

**Measuring the real driver (decision).** Songbird's `internals` feature exposes `Mixer::mock` and `MockScheduler`, which Songbird's own benches use: a fake connection with a cipher and no network send in `mix_and_build_packet`. It is a dev-dependency feature, so production graphs never see it. Workspace test builds unify it, but there `march_deadline` stops sleeping only for live mixers, and no test connects a real call. Story text says `cargo bench --release`, but `cargo bench` has no `--release` flag; the bench profile already inherits `release`.

**Golden test shape:**
```rust
let (mut mixer, _listeners) = Mixer::mock(rt.handle().clone(), true);
let input: Input = File::new(fixture("sine-stereo-20ms.opus")).into();
let input = rt.block_on(frames::prepare(input, counters)).unwrap();
let (handle, ctx) = track_context(Track::new(input).volume(1.0));
mixer.add_track(ctx).unwrap();
let mut buf = [0u8; VOICE_PACKET_MAX];
while ticks < 1000 && !ended { mixer.mix_and_build_packet(&mut buf).unwrap(); }
```

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-voice` (includes `passthrough_alloc`) and `cargo test --release -p noctis-voice --no-default-features` -- pass. Decoding tests that need a format are `#[cfg(feature = ..)]`-gated (`ogg`, `mkv`, `mp3`).
- `cargo test --release -p noctis-core` -- pass (METRICS count 27)
- `cargo bench -p noctis-voice --bench voice -- --test` -- runs; `cargo bench -p noctis-voice --bench voice` once for `VOICE.md`
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo tree -p noctis --features voice -e normal -f '{p} {f}' | grep -c internals` -- 0; `cargo tree -p noctis -e normal --prefix none | grep -c '^songbird '` -- 0
- `cargo run --release -p xtask -- deps-check`, `tw-grammar --check`, `rules-catalog`, `skill-check` -- ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot` -- ok; the printed hash goes into `SKILL.md`
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run only by the gate script.
