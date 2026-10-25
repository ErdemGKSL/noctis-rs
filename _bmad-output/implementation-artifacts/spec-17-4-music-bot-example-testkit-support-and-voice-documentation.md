---
title: 'Story 17.4: Music-bot example, testkit support and voice documentation'
type: 'feature'
created: '2026-10-25'
status: 'in-progress'
baseline_revision: 'e66a25e56da9771f572b7f3abfc0ae3ce7a2e216'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-17-3-optimized-opus-path-passthrough-encoder-tuning-and-voice-bench.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/views-and-recipes.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Voice works (17.1-17.3), but no example shows a complete music bot. A bot author cannot test a voice command without a real UDP voice connection, because a join only resolves once Songbird's driver has connected. The voice guide covers only passthrough (FR-80, FR-51 voice, FR-74 voice guide; AD-17, AD-27, AD-30).

**Approach:** The story has three parts.
- **Recording driver.** A test-only driver, `VoiceConfig::recording()` (`noctis-voice` feature `testkit`, which only the facade's `testkit` feature turns on). It joins through Songbird's gateway-only `join_gateway`, so op 4 still goes out through Noctis's gateway and the join resolves on the injected `VOICE_STATE_UPDATE`/`VOICE_SERVER_UPDATE`. It replaces the Songbird player with a recorder that a test reads and drives.
- **Voice-agnostic `TestApp` helpers.** They inject and auto-answer those events and read the op 4 frames.
- **Example and docs.** A new workspace example `examples/music-bot`, tested with both, plus the full `docs/guides/voice.md`, the template/AGENTS/Usage Skill patterns and a README feature row.

## Boundaries & Constraints

**Always:**
- The recording driver is compiled only with `noctis-voice/testkit`. The facade maps `testkit = ["dep:noctis-testkit", "noctis-voice?/testkit"]`, so a default or voice-only build has none of it.
- No `noctis-testkit → noctis-voice` edge, normal or dev. The `TestApp` helpers speak raw gateway JSON only.
- With the recording driver, op 4 still goes through `NoctisShard` → `GatewayHandle::update_voice_state`, with the owning shard, the Shard Range and the GW-04 budget (VC-01). The fake gateway records it.
- The recording driver never opens a file, fetches a URL or spawns a process. It never touches UDP or the voice WebSocket.
- The recorder never calls into a queue while it holds its own lock, because `Player` methods run under the queue lock and take the recorder lock.
- New per-guild maps in `noctis-voice/src` carry a `// keyed-map: <why bounded>` marker (deps-check AD-25).
- Every new public item has rustdoc and an example where it is non-trivial. The public API change re-blesses `xtask/public-api.txt` and updates the Usage Skill (every new item mentioned) and its `API hash`, the template and its `AGENTS.md`, all in the same commit.
- The music-bot follows the UI Doctrine. Views are pure `render_*` functions, state lives in Payload fields, `ctx.update` is used in place, and there are no collectors or listeners. `skill-check`'s doctrine lint covers `examples/music-bot/{src,tests}` automatically.

**Never:**
- Change the Songbird features, `docs/discord-rules.md` or `noctis-rules`. This story adds no rule id.
- Run the real Songbird driver against the auto-answered fake endpoint in a test. A test on that endpoint uses `VoiceConfig::recording()`.
- Run network fetches, `yt-dlp` or `ffmpeg` in tests. Run `cargo hack`.
- Fix the 17.3 deferred item (the input-preparation timeout). It stays deferred.
- Add a member-voice-state cache to Noctis. The author's channel comes from Discord's REST "Get User Voice State" (Design Notes).

## I/O & Edge-Case Matrix

**Recording driver** (`noctis-voice`, App on `FakeGateway` with `answer_voice: true` unless stated; `G` = a guild owned by shard 1 of 2, `V` = channel 10, `R` = the config's `VoiceRecorder`):

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| default config | `VoiceConfig::default().recorder()` | `None`; Songbird player and `Call::join` as before | — |
| recording config | `VoiceConfig::recording()` | `recorder()` is `Some`. The config equals itself and its clone (`PartialEq` by recorder identity). It is unequal to another `recording()` and to `default()`. Builder setters keep the recorder | — |
| join, auto-answered | `voice.join(G, V)` | `Ok(CallHandle)` with `channel() == Some(V)`. Exactly one op 4 frame with `d.channel_id == "10"`. `active_calls() == 1` | — |
| join, manual answer | `answer_voice: false`; join spawned; the test dispatches the bot's `VOICE_STATE_UPDATE` (V), then `VOICE_SERVER_UPDATE` | resolves `Ok` after both events, in either order | — |
| join, no answer | `answer_voice: false`, `join_timeout(1 s)` | `Err(VoiceError::Timeout { after: 1 s })`. The call is removed and op 4 leave is sent | — |
| join, null endpoint | `VOICE_SERVER_UPDATE` with `endpoint: null`, then a real one | ignored, then resolves | — |
| join, same channel again | connected in V, `join(G, V)` | `Ok`, same generation, no new op 4 | — |
| join, move | connected in V, `join(G, 11)` | op 4 to channel 11. `Ok` once answered, `channel() == Some(11)`. Commands recorded so far are kept | — |
| join before READY / outside range | as 17.1 | `NotReady` / `Gateway(..)` unchanged | — |
| enqueue on idle | `enqueue(a)` | `R.commands(G) == [Play { track: a, volume: 1.0 }]`. No `TrackStart` yet. `now_playing() == a` | — |
| start | `R.start(G)` | returns `Some(a)`. `R.events(G)` ends with `Start(TrackStart{G, a})`. Hooks get `TrackStart` once. A second `start` returns `Some(a)` with no second event | — |
| finish with next | queue `[a, b]`, `R.finish(G)` | `Some(a)`. Events gain `End(a, Finished)`, then commands gain `Play{b, 1.0}` | — |
| finish last | queue `[a]`, `R.finish(G)` | `Some(a)`. Events gain `End(a, Finished)` then `Empty(G)` | — |
| fail | current b, `R.fail(G, "boom")` | `Some(b)`. Events gain `Error(b, "boom")`. The next track plays or `Empty` follows. Never panics | — |
| nothing current | no call, or empty queue, or after `stop()` | `start`/`finish`/`fail` return `None`. Nothing changes and no event fires | — |
| pause/resume/volume/skip/stop | the `CallHandle` ops | commands gain `Pause`, `Resume`, `SetVolume(0.5)`, `Stop` in order. Events show skip (`End Skipped`) and stop (`End Stopped`, `Empty`) as in 17.2 | — |
| unicode / long title | title `"Gülümse 🎵"`; 300-char title | recorded as the queue's `TrackInfo` (the title is cut to 256 chars by `Track::title`) | — |
| leave | `call.leave()` | op 4 with `channel_id: null`, auto-answered. `call(G)` is `None`. Commands gain `Stop` when a track was current. No event | — |
| two guilds | G and G2 | `commands`/`events` per guild never mix | — |
| shutdown | App stops with a live call | shutdown leave confirmed by the auto-answer, so the run ends without waiting out the 5 s leave budget | — |
| unknown guild | `R.commands(G3)`, `R.events(G3)` | empty `Vec` | — |

**FakeGateway `answer_voice`** (`noctis-testkit`; bot id = `ready.user_id`):

| Op 4 received (authenticated) | `answer_voice: true` | `false` (default) |
|---|---|---|
| `{guild_id: "5", channel_id: "6", self_deaf: true, self_mute: false}` | dispatches `VOICE_STATE_UPDATE` (guild 5, channel 6, user = bot, `session_id: "fake-voice-session"`, `self_deaf`/`self_mute` echoed, `deaf`/`mute`/`self_video`/`suppress` false, `request_to_speak_timestamp: null`), then `VOICE_SERVER_UPDATE` (`guild_id: "5"`, `token: "fake-voice-token"`, `endpoint: "voice.fake.invalid:443"`), on that connection, in that order | nothing |
| `channel_id: null` | only `VOICE_STATE_UPDATE` with `channel_id: null` | nothing |
| `d` missing, `d` not an object, `guild_id` missing or not a snowflake string | nothing answered, connection kept, frame still recorded | nothing |
| op 4 before IDENTIFY | close 4003 (unchanged) | close 4003 |

The fixed values are documented `pub const`s on `noctis_testkit::fake`: `FAKE_VOICE_SESSION`, `FAKE_VOICE_TOKEN`, `FAKE_VOICE_ENDPOINT`.

**TestApp helpers:**

| Call | Expected |
|---|---|
| `app.answer_voice(true)` | sets the gateway fake's `answer_voice` (builder style, like `.timeout`) |
| `app.voice_state_updates()` | `d` of every op 4 frame the App sent so far, oldest first; empty before any |
| `app.dispatch_bot_voice_state(G, Some(V))` / `(G, None)` | the bot's own `VOICE_STATE_UPDATE` (same JSON as the auto-answer) |
| `app.dispatch_voice_server(G)` | the `VOICE_SERVER_UPDATE` (same JSON as the auto-answer) |

**`music_bot::input::query_target(query) -> Option<String>`** and **`track_for(query, user) -> Option<Track>`** (target = what follows `--` in yt-dlp's args):

| `query` | target / result |
|---|---|
| `""`, `"   "`, `"\t\n"` | `None` (the handler replies "Type a song name or a link.") |
| `"never gonna give you up"` | `ytsearch1:never gonna give you up` |
| `"  lofi beats  "` | `ytsearch1:lofi beats` (trimmed) |
| `"https://youtu.be/dQw4w9WgXcQ"`, `"http://example.com/a.ogg"`, `"HTTPS://Example.com/a"` | the trimmed URL as is (`Track::url(q).is_ok()`) |
| `"https://user:pw@host/a"`, `"https://discord.com/api/v10/x"`, `"https://"` | `ytsearch1:<q>` (`Track::url` refuses it, so it is a search, never fetched as a link) |
| `"file:///etc/passwd"`, `"ftp://h/a"` | `ytsearch1:file:///etc/passwd` and so on: never a local file |
| `"-x --exec rm"`, `"--version"` | `ytsearch1:-x --exec rm`: after `--`, never an option |
| `"Gülümse 🎵"` | `ytsearch1:Gülümse 🎵` |
| 512-char query (the option's `max_length`) | target kept whole; the track title is the first 256 chars |

`ytdlp_args(target)` == `["-f", "bestaudio[acodec=opus]/bestaudio", "--no-playlist", "--playlist-items", "1", "--quiet", "-o", "-", "--", target]`. The track is `Track::command("yt-dlp", args)`, with `.title(trimmed query)` and `.requested_by(user)`.

**`music_bot::view::render_player(state, page, limits)`** (pure; `PlayerState { now_playing: Option<TrackInfo>, paused: bool, volume: f32, pending: Vec<TrackInfo> }`; `PlayerState::of(Option<&CallHandle>)`; `None` → the empty state):

| State | Rendered (one Container) |
|---|---|
| nothing current, nothing pending | `## Music` and `-# Nothing is playing. Use /play to start.` No buttons (empty state) |
| current a, playing, volume 1.0, no pending | `## Now playing`, `**a**` (escaped with `escape_markdown`), `-# Requested by <@u>` (when set), `-# Playing · Volume 100%`, `-# Nothing queued.`, controls row [Pause (Secondary), Skip (Secondary), Stop (Danger)]. No pager |
| paused, volume 0.5 | status `-# Paused · Volume 50%`. The toggle shows `Resume` (Primary) |
| title `*_~` / 256 chars of `*` | escaped. The message validates under `Limits::default()` |
| pending 1..=per_page | rows `1. b`, `2. c`, …, `-# Page 1 of 1 · N queued`, pager [Previous disabled, Next disabled] |
| pending 1000 (max_queue) | every page validates. Page p shows rows from `p*per_page+1` |
| `page` past the end (`u16::MAX`) | clamped to the last page |
| page 0 / last page | Previous disabled / Next disabled. The two `QueuePage` ids differ on every page (CID-02) |
| volume 2.0 / 0.0 | `Volume 200%` / `Volume 0%` (rounded `volume * 100`) |

`per_page` is computed with `Budget::of(fixed at its largest).fits(&TextDisplay::new("x".repeat(ROW_MAX_CHARS))).max(1)`, as in the pagination recipe. `ROW_MAX_CHARS = 2 * 256 + 8`.

**Music-bot end to end** (`TestApp` + `VoiceConfig::recording()` + `answer_voice(true)`; guild G=10 with text channel C=20 and voice channel V=50 (`ChannelType::GuildVoice`); user A=40; route `GET /guilds/10/voice-states/40` → A's voice state in V):

| Run | Expected |
|---|---|
| `/play query:"never gonna"` by A | public reply = `render_player` (title `never gonna`, buttons Pause/Skip/Stop). `voice_state_updates()` has one frame, channel `"50"`. `R.commands(G) == [Play{title "never gonna", source Command("yt-dlp"), volume 1.0}]` |
| `/play` by A, no route (404) | ephemeral "Join a voice channel first.". No op 4. No command |
| `/play query:"   "` | ephemeral "Type a song name or a link." No op 4 |
| `/play` by A while the call is in channel 51 | ephemeral "I'm already playing in <#51>." No op 4. The queue is unchanged |
| `/play` twice | second reply's queue shows `1. <second>`. One op 4 only (same channel) |
| `/pause`, `/resume`, `/skip`, `/stop`, `/volume percent:50`, `/leave` | ephemeral one-liners ("Paused.", "Resumed.", "Skipped never gonna.", "Stopped; 2 tracks removed.", "Volume set to 50%.", "Left the voice channel."). Recorder commands `Pause`, `Resume`, `Stop`+`Play`, `Stop`, `SetVolume(0.5)`. `/leave` sends op 4 `channel_id: null` |
| any control without a call | ephemeral "I'm not in a voice channel." |
| `/pause` when already paused; `/resume` when playing; `/skip` with nothing current | "Already paused.", "Already playing.", "Nothing is playing." |
| `/volume` option bounds | `min_value = 0`, `max_value = 200` declared. `percent:0` → `SetVolume(0.0)`, `percent:200` → `SetVolume(2.0)` |
| click Pause, then Resume, on the player | `update` in place (`ResponseKind::Update`). The toggle flips. Commands `Pause`, `Resume` |
| click Skip | update. Events `End(a, Skipped)`. Next `Play` |
| click Stop | update to the empty state. Events `End Stopped`, `Empty` |
| 30 queued, `/queue`, click Next, click Previous | page 1 → page 2 → page 1. Previous is disabled on page 1 |
| click on a player after `/leave` | update to the empty state (no error) |
| `R.start`, `R.finish`, `R.fail` after `/play` ×2 | events `Start a`, `End a Finished`, (`Play b`), `Error b "boom"`, `Empty` |
| manual injection (`answer_voice(false)`) | `/play` run and an injector are joined with `tokio::join!`. The injector waits (≤ 15 s) for an op 4 in `voice_state_updates()`, then calls `dispatch_bot_voice_state(G, Some(V))` + `dispatch_voice_server(G)`. The run returns the player view |

</intent-contract>

## Code Map

- `crates/noctis-voice/src/config.rs` -- `VoiceConfig` (`#[non_exhaustive]`, derives `Debug, Clone, PartialEq, Eq`). Add the field `#[cfg(feature = "testkit")] recorder: Option<VoiceRecorder>` (default `None`), plus the `#[cfg(feature = "testkit")]` methods `pub fn recording() -> Self` and `pub fn recorder(&self) -> Option<VoiceRecorder>`. `VoiceRecorder` implements `PartialEq`/`Eq` by `Arc::ptr_eq`, so the derive keeps working.
- `crates/noctis-voice/src/recording.rs` (new, `#[cfg(feature = "testkit")]`) -- `VoiceRecorder` (Clone; Debug shows only the guild count), `RecordedCommand` and `RecordedEvent` (both `#[non_exhaustive]`, `Debug, Clone, PartialEq`), and the crate-private `RecordingPlayer` (implements `queue::Player`).
  - The per-guild state is `HashMap<Id<GuildMarker>, GuildRecord>` with `// keyed-map:` marker. `GuildRecord` holds `commands: Vec`, `events: Vec`, and `current: Option<(u64 token, Weak<QueueCell>)>`.
  - The guilds of one test App live as long as the recorder.
  - Exact shapes: `enum RecordedCommand { Play { track: TrackInfo, volume: f32 }, Pause, Resume, Stop, SetVolume(f32) }` and `enum RecordedEvent { Start(TrackStart), End(TrackEnd), Error(TrackError), Empty(QueueEmpty) }` (the matrix's `End(a, Finished)` is shorthand for `End(TrackEnd { guild, track: a, reason: EndReason::Finished })`).
  - `impl VoiceRecorder { pub fn commands(&self, guild: Id<GuildMarker>) -> Vec<RecordedCommand>; pub fn events(&self, guild) -> Vec<RecordedEvent>; pub fn start(&self, guild) -> Option<TrackInfo>; pub fn finish(&self, guild) -> Option<TrackInfo>; pub fn fail(&self, guild, error: impl Into<String>) -> Option<TrackInfo> }`. Use one recorder per App; two Apps sharing a recording config record into the same guild entries, which the docs say.
- `crates/noctis-voice/src/queue.rs` -- `Binding` (shared per App) gains `#[cfg(feature = "testkit")] recorder: OnceLock<VoiceRecorder>` or an equivalent. `QueueCell::flush` records every `Note` as a `RecordedEvent` before the emitter check, so events are recorded even with no hook. `Player` doc: methods run under the queue lock.
  - Model `RecordingPlayer` on `queue/fake.rs` (`FakePlayer`, test-only; keep it).
  - `start`/`finish`/`fail` clone `(token, weak)` out of the recorder lock. They then upgrade the weak and call `on_playable`/`on_end`/`on_error` and return `now_playing()` read before the call. They return `None` when the slot is empty or the queue is closed.
  - `RecordingPlayer::stop(token)` clears the slot when its token matches.
- `crates/noctis-voice/src/call.rs` `new_queue` (L177) -- when `self.config.recorder()` is `Some`, the player is `RecordingPlayer { recorder, guild, queue: Weak }`. The `#[cfg(test)] fake_player` branch stays first.
- `crates/noctis-voice/src/voice.rs` `join_inner` (L106-148) -- with a recorder, call `call.join_gateway(target)` instead of `call.join(target)`. Map `Ok(ConnectionInfo)` to the same success path. `JoinError` mapping, timeout, `remove_join` and the leave-after-failure stay identical (one helper over both futures, or `futures_util::future::Either`).
  - Songbird 0.6.1 facts (`handler.rs` L279-310, `join.rs` L118-136): `join_gateway` sends op 4 through the same `Shard::Generic` and resolves `JoinResult<ConnectionInfo>` once state and server arrive, without `driver.raw_connect`.
- `crates/noctis-voice/src/lib.rs` -- `#[cfg(feature = "testkit")] mod recording; pub use recording::{RecordedCommand, RecordedEvent, VoiceRecorder};` plus a crate-doc bullet.
- `crates/noctis-voice/Cargo.toml` -- `[features] testkit = []`. Its dev-dep `noctis` (features `testkit`, `voice`) turns it on in its own tests.
- `crates/noctis/Cargo.toml` -- `testkit = ["dep:noctis-testkit", "noctis-voice?/testkit"]`, with the comment updated.
- `crates/noctis/src/lib.rs`:
  - `pub mod voice` docs (L2807-2930): a **Testing** bullet.
  - `pub mod testkit` (L3825): add `#[cfg(feature = "voice")] pub mod voice { pub use noctis_voice::{RecordedCommand, RecordedEvent, VoiceRecorder}; }` with module docs and a `no_run` example. Add docs bullets for the four `TestApp` voice helpers.
- `crates/noctis-testkit/src/fake/gateway.rs`:
  - `FakeGatewayConfig` (L58) gains `pub answer_voice: bool` (default `false`).
  - `receive_text` op 4 arm (L913-919) answers per the matrix. Use the connection's own dispatch path with the session's next `seq`. Model it on how `dispatch` (L567) builds frames.
  - Add the three `pub const FAKE_VOICE_*`, re-exported from `fake.rs`.
- `crates/noctis-testkit/src/app.rs` -- `#[must_use] pub fn answer_voice(self, on: bool) -> Self`, `pub fn voice_state_updates(&self) -> Vec<serde_json::Value>`, `pub fn dispatch_bot_voice_state(&self, guild: Id<GuildMarker>, channel: Option<Id<ChannelMarker>>)`, `pub fn dispatch_voice_server(&self, guild: Id<GuildMarker>)` (bot id from `self.gateway.config().ready.user_id`). Reuse `RecordedFrame::{op, json}` (gateway.rs L253-271). The crate docs (lib.rs) gain a bullet.
- `crates/noctis-voice/src/tests.rs` -- the `Running` harness (L150-270), `bot_voice_state` JSON (L580) and `until`/`WAIT`. Add the recording-driver e2e tests in a new `src/recording/tests.rs` that reuses the harness (make the needed helpers `pub(crate)`). Start the `FakeGateway` with `FakeGatewayConfig { answer_voice: true, .. }` through `FakeGateway::start_with`.
- `examples/ui-recipes/{Cargo.toml,src/main.rs,src/pagination.rs,tests/recipes.rs}` -- the model for the new example: manifest shape, `configure`, `App::from_env`, the `Budget` page sizing and the TestApp fixture style.
- `examples/project-template/src/modules/voice/mod.rs`, `tests/voice.rs`, `locales/{en-US,tr}.yaml`, `AGENTS.md` L70, `.claude/skills/noctis/{SKILL.md,references/voice.md,references/testing.md}` -- template and skill surfaces to update.
- `docs/guides/voice.md` (the 17.3 passthrough guide, kept as sections) and `docs/guides/README.md` index line.
- `README.md` -- Features table (L69-81) and the "Voice and music bots" bullets (L378-385).
- `xtask/src/deps_check.rs` -- `check_gate_features`/`enables_testkit` (L982-1080) must accept the weak `noctis-voice?/testkit` in the facade's `testkit` (it activates no dependency). If the closure logic counts `?/` as activation, fix it there, with a test.
- twilight-http 0.17.1 `Client::user_voice_state(guild_id, user_id)` (`client/mod.rs` L2077) → `GET /guilds/{g}/voice-states/{u}` → `VoiceState`. Detect a 404 with `matches!(e.kind(), ErrorType::Response { status, .. } if status.get() == 404)`.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-voice/{Cargo.toml,src/recording.rs,src/recording/tests.rs,src/config.rs,src/queue.rs,src/call.rs,src/voice.rs,src/lib.rs}` -- the recording driver per the Code Map. Tests cover every row of the recording-driver matrix: unit tests for the config/recorder rows and e2e tests on the fakes for the rest. Every wait is ≤ 15 s.
- `crates/noctis/{Cargo.toml,src/lib.rs}` -- the feature mapping, `noctis::testkit::voice` and the docs.
- `crates/noctis-testkit/src/{fake/gateway.rs,fake.rs,app.rs,lib.rs}` -- `answer_voice`, the constants and the four helpers. Add FakeGateway tests for every `answer_voice` row in the existing gateway test module. Assert that the answers deserialize as `twilight_model` `VoiceStateUpdate`/`VoiceServerUpdate`. Add a `TestApp` test (no voice) that checks `voice_state_updates()` is empty and the dispatch helpers reach an `EventHandler` of `VoiceStateUpdate`.
- `examples/music-bot/` (new; root `Cargo.toml` `members` gains `"examples/music-bot"`):
  - `Cargo.toml`: `name = "music-bot"`, `publish = false`, workspace package fields.
    - deps: `noctis = { workspace = true, features = ["voice"] }`, `tokio` (`macros`, `rt-multi-thread`), `tracing.workspace = true`.
    - dev-deps: `noctis = { workspace = true, features = ["testkit", "voice"] }`, `serde_json.workspace = true`.
    - `[lints] workspace = true`.
  - `src/lib.rs`: crate docs (what it shows, the yt-dlp runtime requirement, the SSRF note) and `pub fn configure(builder) -> &mut AppBuilder`. It does `.voice(VoiceConfig::default().idle_leave(Some(Duration::from_mins(5))))` and `.module(music::module())`.
  - `src/main.rs`: as the ui-recipes main.
  - `src/input.rs`: `query_target`, `ytdlp_args`, `track_for`.
  - `src/view.rs`: `PlayerState`, `render_player`, the Routes `TogglePause { page: u16 }`, `SkipTrack { page: u16 }`, `StopPlayback`, `QueuePage { page: u16 }` (all `#[derive(Button)]`, default `response = update`, each with its `#[handler]` calling `ctx.update(render_player(..))`).
  - `src/music.rs`: the slash commands (all `contexts = [Guild]`). `Play { query: String }` takes `#[option(min_length = 1, max_length = 512)]` and responds public. `Queue` responds public. `Pause`, `Resume`, `Skip`, `Stop`, `Volume { percent: i64 }` (with `min_value = 0, max_value = 200`) and `Leave` declare `response = ephemeral` and use `ctx.reply_text`. Also a `#[derive(Hook)] TrackErrorLog` (`tracing::warn!`) and `module()`.
  - Unit tests: every `input` row and every `render_player` row, including `validate` of each page and the CID-02 distinct ids.
  - `tests/music.rs`: every music-bot e2e row (`#[tokio::test(flavor = "multi_thread")]`, waits ≤ 15 s).
- `examples/project-template/src/modules/voice/mod.rs` -- `/play` joins the author's voice channel when the guild has no call (same REST lookup and 404 handling as the music-bot), instead of replying "Use /join first.". `/skip` and `/stop` keep `not_in_call`. Add locale key `voice.join_a_channel` ("Join a voice channel first." / tr "Önce bir ses kanalına katıl.") and update `cmd.play.description` in both locale files.
- `examples/project-template/tests/voice.rs` -- update `queue_commands_exist` for the new `/play` text. Add `play_joins_the_authors_channel`: `.voice(VoiceConfig::recording())` after `configure`, `answer_voice(true)` and the voice-state route. It asserts one op 4 to the author's channel and one `RecordedCommand::Play`.
- `examples/project-template/AGENTS.md` (voice line) -- add:
  - the author's channel via `ctx.http().user_voice_state` (404 = not in voice);
  - the now-playing View pattern (pure `render_player`, Routed pause/resume/skip/stop and page buttons, `ctx.update`);
  - yt-dlp search/URL input;
  - testing with `VoiceConfig::recording()` + `TestApp::answer_voice`, plus a pointer to `references/voice.md` §"Music-bot patterns".
- `examples/project-template/.claude/skills/noctis/references/voice.md`:
  - new "Music-bot patterns" section: author's channel, input builder with the yt-dlp args and the `--` rule, condensed `render_player` with the four Routes, the control commands, the idle leave;
  - new "Testing voice" section: the recorder API, with every `RecordedCommand`/`RecordedEvent` variant and `start`/`finish`/`fail`, the TestApp helpers and a complete test.
- `examples/project-template/.claude/skills/noctis/references/testing.md` -- a short voice subsection pointing to it. `SKILL.md`: the references row and the `API hash`.
- `docs/guides/voice.md` -- rewrite as "Voice and music bots". Sections, in order:
  1. Enabling voice (feature `voice`, C compiler + cmake for libopus, `.voice(VoiceConfig)`, the config table with every setting, range and default);
  2. Joining and leaving (`join`, `leave`, `call`, VC-01/VC-02, the author's channel lookup);
  3. Tracks and the queue;
  4. Track events (the table from the facade docs);
  5. Passthrough and pre-encoding (the 17.3 content, kept);
  6. Idle leave;
  7. Cluster notes (a guild's call lives in the process that owns its shard; `INTERACTION_CREATE` for that guild reaches the same process; `voice().call(g)` elsewhere is `None`; the shutdown leave is per process);
  8. Limits (VC-01 GW-04 110 per shard per 60 s shared with presence and member requests; VC-02 one call per guild; `max_queue` 1..=10000; volume 0.0..=2.0; title 256 chars; URL 4096 bytes; `join_timeout` 1..=60 s; `idle_leave` 1 s..=24 h; `media_connect_timeout`; `media_max_redirects` 0..=10; `max_bitrate`);
  9. Testing voice;
  10. The music-bot example (`cargo run --release -p music-bot`, `NOCTIS_TOKEN`, yt-dlp on `PATH`).

  Update the index line in `docs/guides/README.md`.
- `README.md` -- Features row `| **Voice and music bots** | Opt-in voice (feature `voice`): join/leave through Noctis's gateway, a bounded per-guild queue, typed track events, Opus passthrough, and a recording driver for tests; see `examples/music-bot` |`. Add voice bullets for the testkit recording driver and the music-bot example. Do not change the progress counts (the story loop does).
- `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`. Put the printed hash in `SKILL.md`.
- `ARCHITECTURE-SPINE.md` AD-30 and `maintainer-decisions.md` §10 -- one "Amended (2026-10-25, Story 17.4)" line each: recording driver via `join_gateway` behind `noctis-voice/testkit` (enabled only by the facade's `testkit`), voice-agnostic TestApp helpers, and the author's channel via REST.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- implementation `(decision)` lines, if any (the planning ones are logged).

**Acceptance Criteria:**
- Given `examples/music-bot` (workspace member, `publish = false`, `noctis` with `voice`), when `cargo test --release -p music-bot` runs, then every music-bot e2e row passes through `TestApp`. This includes `/play` joining the author's channel (op 4 recorded), the public now-playing View with working Pause/Resume, Skip, Stop and pager buttons, and the track events asserted through `VoiceRecorder`.
- Given a bot test with the facade's `testkit` and `voice` features, when it builds with `VoiceConfig::recording()`, then joins resolve without UDP. `TestApp::voice_state_updates()` shows the op 4 frames, `dispatch_bot_voice_state`/`dispatch_voice_server` complete a pending join, and queue commands and track events are readable from the recorder.
- Given `cargo tree -p noctis -e normal -f '{p} {f}'` and `cargo tree -p noctis --features voice -e normal -f '{p} {f}'`, then no line shows `noctis-voice` with the `testkit` feature. Given `cargo tree -p noctis-testkit -e all`, then `noctis-voice` is absent.
- Given `cargo run --release -p xtask -- skill-check`, when it runs, then it passes. That means every new public item is mentioned, the hash matches, and the doctrine lint passes over `examples/music-bot`. `deps-check`, `rules-catalog` (unchanged), `api-snapshot` (re-blessed) and `cargo deny check` pass.
- Given `docs/guides/voice.md`, then it has the ten sections listed in Tasks. Given `README.md`, its Features table has the voice row.

## Spec Change Log

## Review Triage Log

## Design Notes

**Why `join_gateway` (decision).** A real join resolves only after Songbird's driver opens the voice WebSocket and UDP to the endpoint in `VOICE_SERVER_UPDATE`. A fake voice server would mean reimplementing Discord's voice protocol, including DAVE. Songbird's own gateway-only join keeps the outermost Noctis surfaces real: `Voice::join`, op 4 through the gateway and budget, the plugin's `update_state`/`update_server` feeding the `Call`, timeouts and VC-02 moves. Only the driver connect and the audio player are swapped. Tests keep the 17.2 queue and event code paths.

**Why the recorder lives in `noctis-voice` (decision).** AD-24/AD-30 forbid a testkit → voice edge. The testkit gets only gateway-JSON helpers, which also serve bots that answer op 4 with another driver. `answer_voice` is opt-in, because an existing test that counts dispatches must not see extra frames.

**Explicit `start` (decision).** The recording player does not auto-report a track as playable. A spawned report would race a test's assertions, and an explicit `start` mirrors `finish`/`fail`, so every event a test sees is one it caused.

**Author's channel via REST (decision).** Noctis's Discord cache has no voice-state resource (`noctis-ports` `DiscordCacheResources` maps GUILD, CHANNEL, ROLE, MEMBER, USER, MESSAGE). Tracking every member's voice state in `noctis-voice` would add a large per-guild map for one lookup. `GET /guilds/{g}/voice-states/{u}` costs one request per `/play`. It is Cluster-safe and is answered in tests by a registered fake route. The music-bot refuses to move a call another listener is using ("I'm already playing in <#X>.").

**yt-dlp input (decision).** URLs and searches both go through `yt-dlp -f bestaudio[acodec=opus]/bestaudio`, the 17.3 passthrough-friendly input: YouTube's Opus is WebM Opus at 48 kHz. A query is a link only when `Track::url` accepts it. Anything else becomes `ytsearch1:`, so `file://` or a credentialed URL is never fetched as a link. The lib docs warn that yt-dlp fetches any public or private host, and that a production bot should allow-list sites.

**Golden test shape:**
```rust
let config = VoiceConfig::recording();
let recorder = config.recorder().expect("recording config");
let mut builder = App::builder();
music_bot::configure(&mut builder).voice(config); // the last .voice(..) wins
let app = TestApp::build(&mut builder).await?.answer_voice(true)
    .guild(TestGuild::new(G, "G").text_channel(C, "music").channel(V, "Lounge", ChannelType::GuildVoice))
    .user(TestUser::new(A));
app.http_fake().route(Method::GET, "/guilds/10/voice-states/40", FakeResponse::json(StatusCode::OK, &voice_state(A, V)));
let reply = app.command("play").option("query", "never gonna").by(A).in_guild(G).run().await?;
assert!(matches!(recorder.commands(G)[..], [RecordedCommand::Play { .. }]));
```

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-voice`, `cargo test --release -p noctis-testkit`, `cargo test --release -p noctis`, `cargo test --release -p music-bot` -- pass
- `cargo test --release` and `cargo test --release --features voice` in `examples/project-template` -- pass
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo tree -p noctis --features voice -e normal -f '{p} {f}' | grep noctis-voice` -- no `testkit`; `cargo tree -p noctis-testkit -e all --prefix none | grep -c '^noctis-voice '` -- 0
- `cargo run --release -p xtask -- deps-check`, `tw-grammar --check`, `rules-catalog`, `skill-check` -- ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot` -- ok; the printed hash goes into `SKILL.md`
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run only by the gate script.
