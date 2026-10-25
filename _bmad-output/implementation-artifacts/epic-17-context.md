# Epic 17 Context: Voice and music bots

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author opt into a `voice` feature to join and leave voice channels from any handler through Noctis's own gateway, play a bounded per-guild queue of tracks (files, HTTP URLs, child-process stdout) demuxed and decoded by symphonia, and send pre-encoded Opus to the wire without decode or re-encode. This reverses the original "no voice" non-goal (Lavalink stays out) so music bots can be built on Noctis cheaply in CPU and memory, demonstrated end to end by a music-bot example.

## Stories

- Story 17.1: Voice connections through Noctis's gateway
- Story 17.2: Track playback, inputs and the per-guild queue
- Story 17.3: Optimized Opus path: passthrough, encoder tuning and the voice bench
- Story 17.4: Music-bot example, testkit support and voice documentation

## Requirements & Constraints

- `.voice(VoiceConfig)` on the AppBuilder installs voice and adds the `GUILD_VOICE_STATES` intent; without the feature nothing voice-related compiles in, and the default facade dependency graph contains no `songbird`.
- Join resolves to a `CallHandle` or typed `VoiceError` (join timeout default 10 s, gateway error, driver error); leave sends op 4 with no channel and removes the call. Moves reconnect; disconnects and guild deletes remove the call.
- Queue: enqueue, skip, pause, resume, stop, clear, volume (0.0-2.0, checked), now_playing, list; bounded by `VoiceConfig::max_queue` (default 1000, overflow is a typed error). Optional `idle_leave` (default off).
- Track events (`TrackStart`, `TrackEnd`, `TrackError`, `QueueEmpty`) reach Modules as typed events; a track error never panics or stops the queue.
- Media HTTP fetches: connect timeout, redirect cap, no bot token or Noctis headers, never Discord's API.
- Performance (NFR-3, NFR-9): passthrough for Ogg/WebM Opus at 48 kHz, one track at unity volume; steady-state passthrough frame allocates nothing in `noctis-voice` (dhat test); criterion bench `benches/voice.rs` with results in `benches/VOICE.md`, not gating CI.
- Discord rules: new "Voice" section in `docs/discord-rules.md` (op 4 owning shard and budget; one voice connection per guild per bot), enforced in `noctis-rules` with the same rule ids.
- Every public API change re-blesses the API snapshot and updates the Project Template, its `AGENTS.md` and the Usage Skill (template checked with `--features voice`).

## Technical Decisions

- New layer-5 crate `crates/noctis-voice`, depending only on `noctis-core` and `noctis-gateway` among workspace crates, plus `songbird` 0.6 (`default-features = false`; features `driver`, `gateway` (only for `Call`/`Shard::Generic`), `rustls`, `tws`; ring-only rustls, no `aws-lc-rs`). Dev-only `noctis-voice -> noctis-testkit` edge allowed; deps-check rule `songbird-gated` allows `songbird` only as a normal dependency of `noctis-voice`. Facade feature `voice`, off by default, documents its C compiler + cmake requirement (libopus via `opus2`). `xtask deps-check` allows exactly these edges; `cargo deny` stays clean (justify new licenses/duplicates in `deny.toml`).
- Songbird's `twilight` and `serenity` features and its `Songbird` manager are NOT used (they cap twilight at <=0.17 and bypass the send budget); `gateway` adds no twilight and no send path. Songbird types stay behind Noctis types, except a documented semver-exempt `unstable` module.
- `noctis-gateway` owns op 4 (`UpdateVoiceState`) next to presence and Request Guild Members: owning-shard routing, Shard Range check, GW-04 budget, same `GatewayError`s, reachable via `GatewayHandle`.
- One `songbird::Call` per guild with a live or pending connection, created with `Shard::Generic` over the op 4 path; map never holds idle guilds. `VoicePlugin::install` (core `Plugin` seam, extension trait `AppBuilderVoiceExt`) mounts handlers for the bot's own `VOICE_STATE_UPDATE` and `VOICE_SERVER_UPDATE` feeding `update_state`/`update_server`. `ctx.voice()` (also from `App` and lifecycle contexts) returns a cheap clonable `Voice` handle.
- Codec: symphonia 0.5 (Songbird's line, no second version) for demux/decode with formats via `noctis-voice` features (ogg, mkv/webm, mp3, aac/isomp4, flac, wav); libopus encodes. Encode bitrate follows channel bitrate capped by `VoiceConfig::max_bitrate`; music settings (stereo, signal type music, soft-clip only while mixing); mixer buffers reused.
- Shutdown: the `shutdown` phase stops tracks and leaves every channel before shards close; `cancel` drops calls.
- Metrics (no guild/channel labels): `noctis_voice_calls` (gauge), `noctis_voice_tracks_total{result}`, `noctis_voice_passthrough_frames_total`, `noctis_voice_encoded_frames_total`.
- Testkit: `VoiceConfig::recording()` driver fake exposed only with the facade's `testkit` feature; no `noctis-testkit -> noctis-voice` edge.
- Verify Songbird 0.6.1 APIs (`Call::from_config`, `update_state`, `update_server`, `shards::VoiceUpdate`/`GenericSharder`) against the published crate before relying on them. Unresolvable driver limits are recorded as `(decision)` lines.

## UX & Interaction Patterns

- Music-bot example: `/play`, `/skip`, `/pause`, `/resume`, `/stop`, `/queue`, `/volume`, `/leave`; joins the author's channel; Components V2 now-playing View with Routed buttons (pause/resume, skip, stop) and a paginated queue following the UI Doctrine.
- Docs explain pre-encoding for passthrough (`ffmpeg ... -c:a libopus -ar 48000 -frame_duration 20`) and `yt-dlp` `bestaudio[acodec=opus]`.

## Cross-Story Dependencies

- 17.1 (op 4, crate, plugin, call map) underpins all others; 17.2 builds the queue/events on `CallHandle`; 17.3 tunes 17.2's playback path; 17.4 uses all three plus testkit.
- Epic 17 runs on its own lane after Story 16.1, concurrent with 16.2/16.3; 17.1 touches `noctis-gateway/src/command.rs` and the facade only, not lanes or the drain. Merges serialize through the merge lock. Epic 14's template/skill/docs stories later treat `voice` like any feature.
