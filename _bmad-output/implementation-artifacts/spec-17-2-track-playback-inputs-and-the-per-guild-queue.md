---
title: 'Story 17.2: Track playback, inputs and the per-guild queue'
type: 'feature'
created: '2026-10-25'
status: 'done'
baseline_revision: '3f8d1593c584695ae8e541c4caea8d3464217626'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-17-1-voice-connections-through-noctis-gateway.md'
warnings: ['oversized']
deferred:
  - summary: >-
      No test drives a join that resolves Ok, so the two `shared.connected(guild, joining.generation)` calls in `join_inner` (the idle-leave arm point) are unpinned.
    evidence: |-
      Verification-gap layer (pre-verified): `idle_leave_leaves` calls `running.shared.connected(..)` directly and every join test in `tests/voice_gateway.rs` stays pending, times out, is interrupted or fails in the driver. Deleting either call keeps every gate green while `idle_leave` never fires for real calls. A resolved join needs a fake voice server (UDP/WS) the repo does not have; close the gap when Story 17.4's `recording()` fake exists.
    location: >-
      crates/noctis-voice/src/voice.rs:107, crates/noctis-voice/src/voice.rs:132
    severity: medium
---

<intent-contract>

## Intent

**Problem:** Story 17.1 joins and leaves voice calls, but a bot cannot play audio. Music bots need tracks from files, HTTP media URLs and child-process stdout, a bounded per-guild queue (enqueue, skip, pause, resume, stop, clear, volume, now playing, list), and typed track events their Modules can handle. None of this may expose Songbird or touch Discord's API (FR-80, AD-30, AD-24, AD-25, AD-29).

**Approach:** Add a small core seam: `ExtensionEvent`, so a plugin's own typed events reach `#[derive(Hook)]` handlers, plus an `Emitter` that fires them. In `noctis-voice`, add `Track` inputs (lazy Songbird `Input`s: `File`, `HttpRequest` over a dedicated media `reqwest` client, and a lazy child-process composer) and a Noctis-owned per-call queue. The driver holds at most the current track. Driver-free tests cover the queue through a crate-private `Player` seam with a Songbird implementation and a test fake. Add the track events, the counter, `idle_leave`, `max_queue`, the symphonia format features, the deps-check amendment, the spine/decision amendments, the template and the Usage Skill.

## Boundaries & Constraints

**Always:**
- Songbird stays `0.6.1` with the 17.1 feature set (`driver`, `gateway`, `rustls`, `tws`). Do not enable `builtin-queue`. Add `symphonia = { version = "0.5.5", default-features = false }` to `[workspace.dependencies]`. `noctis-voice` uses it only to turn on formats; it is the same 0.5 line as Songbird's, with no second version (`cargo tree -d` shows a single `symphonia`). Add `reqwest = { version = "0.12.28", default-features = false, features = ["rustls-tls"] }` to `[workspace.dependencies]`. It is the same version Songbird resolves, adds no new crate to the graph, and stays ring-only.
- The `noctis-voice` features `ogg`, `mkv`, `mp3`, `aac`, `isomp4`, `flac` and `wav` map to `symphonia/<same>`; `wav` also turns on `symphonia/pcm`. `default` is all seven, so the facade's `voice` turns them all on. The crate compiles and its tests pass with `--no-default-features` (cargo-hack): any test that decodes is `#[cfg(feature = "wav")]`.
- No Songbird, symphonia or reqwest type appears in a public signature.
- The driver never holds more than the queue's current track: pending tracks are Noctis descriptions, and their inputs are created only when they become current. The queue lives inside the call's `Entry`, so it is bounded by the live calls (AD-25). Its length counts the current track plus the pending ones and is at most `max_queue`.
- Each `Entry`'s queue state sits behind a `std::sync::Mutex` that is never held across an `.await`. The Call's `tokio::sync::Mutex` is held only for one Songbird call (`play_only` or `play`), as in 17.1.
- Events come from Noctis's own queue state. A driver `End`/`Error`/`Playable` whose track token is not the current one is ignored, so every track that became current yields exactly one terminal outcome.
- Media HTTP uses one `reqwest::Client` per App, built in `VoicePlugin::install` (a build error becomes a `BuildViolation::plugin::<VoicePlugin>`). It sets `connect_timeout(media_connect_timeout)`, `no_proxy()`, and `redirect::Policy::custom` that refuses hop `max_redirects + 1` and any hop to the Discord API. It sets no default headers and no user agent. It is never the Discord edge client and never carries the token.
- Every public item has rustdoc. Any public API change re-blesses `xtask/public-api.txt` and updates the template (code, `AGENTS.md`, locales), the Usage Skill and its `API hash` in the same commit.

**Never:**
- Songbird's `TrackQueue`/`builtin-queue`, `YoutubeDl`, or `Driver::play_input` for pending tracks.
- Passthrough, encoder tuning, the bench, `noctis_voice_passthrough_frames_total`/`_encoded_frames_total`, the `unstable` module, `VoiceConfig::recording()` or the music-bot example. Those belong to Stories 17.3 and 17.4.
- A `noctis-testkit -> noctis-voice` edge; network access in tests (servers bind `127.0.0.1:0`); changes to lanes, the drain or `docs/discord-rules.md` (no Discord rule changes).
- Guild, channel, user or track ids, or URLs, in metric labels.

## I/O & Edge-Case Matrix

Track inputs (`Track::file|url|command`, checked at construction; `InvalidInput{reason}` names the rule):

| Scenario | Input | Expected | Error |
|---|---|---|---|
| file ok | `Track::file("a.wav")`, `"音楽/ç.ogg"` | `TrackSource::File(path)`; the file is not opened yet | — |
| file empty | `""` | — | `InvalidInput` |
| file missing at play | the path does not exist | `TrackError`, then the next track starts | (event, no panic) |
| url ok | `https://cdn.discordapp.com/attachments/1/2/a.ogg`, `http://127.0.0.1:8080/x.mp3`, `https://dïscord.com/api` (IDN, another host), `https://discord.com/apix` | `TrackSource::Url` | — |
| url discord api | `https://discord.com/api/v10/x`, `HTTPS://DISCORD.COM/API`, `https://discord.com./api`, `https://ptb.discord.com/api/x`, `https://discordapp.com/api` | — | `InvalidInput` (host `discord.com`/`discordapp.com` or a subdomain, first path segment `api`, case-insensitive, trailing dot ignored) |
| url scheme | `ftp://h/a.mp3`, `file:///a`, `data:..` | — | `InvalidInput` |
| url userinfo | `https://u:p@h/a.mp3`, `https://u@h/a` | — | `InvalidInput` (credentials are never sent) |
| url malformed | `""`, `"not a url"`, `"https://"` | — | `InvalidInput` |
| url length | 4096 bytes ok; 4097 | ok / — | `InvalidInput` |
| command ok | `Track::command("yt-dlp", ["-o","-","URL"])`, `("sh",["-c","cat f.wav"])` | `TrackSource::Command(program)`; nothing is spawned until the track becomes current | — |
| command empty | program `""` | — | `InvalidInput` |
| command spawn fail | program not found, at play | `TrackError` and the next track | (event) |
| command killed | the track is skipped or stopped, or the call is left | the child is killed and reaped (Songbird `ChildContainer` drop) | — |
| title | `.title(t)` with 256 chars; 257 chars (multi-byte) | kept; truncated to 256 `char`s at a char boundary | — |
| title empty | `.title("")` | `title()` is `None` | — |

Queue (one guild; `L` = current + pending; default `max_queue` = 1000):

| Scenario | State / Call | Expected | Error |
|---|---|---|---|
| enqueue idle | nothing current, `enqueue(a)` | `a` is current and handed to the player; `TrackStart{a}` when the player reports it playable; returns `TrackInfo` | — |
| enqueue busy | `a` current, `enqueue(b)`, `enqueue(c)` | `list()==[b,c]` (FIFO); nothing else plays | — |
| bound | `L == max_queue`; `L == max_queue-1` | refused / accepted | `QueueFull{guild,max}` and nothing changes |
| max_queue 1 | `enqueue(a)` ok; `enqueue(b)` | — | `QueueFull` |
| natural end | `a` ends | `TrackEnd{a, Finished}`; `b` current; `TrackStart{b}` | — |
| last ends | `a` ends, nothing pending | `TrackEnd{a,Finished}` then `QueueEmpty`; current `None` | — |
| track error | `a` errors (create, decode or IO) | `TrackError{a, error}` (no `TrackEnd`); next starts or `QueueEmpty` | never panics; the queue continues |
| error chain | `a`,`b`,`c` all fail | 3 `TrackError`, then 1 `QueueEmpty` | — |
| skip | `a` current, `[b]` | returns `Some(a)`; `TrackEnd{a,Skipped}`; `b` starts | — |
| skip last | `a` current, `[]` | `Some(a)`; `TrackEnd{a,Skipped}`; `QueueEmpty` | — |
| skip idle | nothing current | `Ok(None)`, no event | — |
| late driver end | the driver's `End` for skipped `a` arrives after `b` started | ignored (stale token) | — |
| pause/resume | playing → `pause()`; paused → `pause()`; paused → `resume()`; playing → `resume()`; idle → either | `true`; `false`; `true`; `false`; `false` | — |
| paused enqueue | `a` paused, `enqueue(b)` | `b` pending; `a` stays paused | — |
| skip paused | `a` paused, `[b]`, `skip()` | `b` starts playing (not paused) | — |
| stop | `a` current, `[b,c]` | returns 3; `TrackEnd{a,Stopped}`; `QueueEmpty`; `list()` empty | — |
| stop idle | empty | `0`, no event | — |
| clear | `a` current, `[b,c]` | returns 2; `a` keeps playing; no event | — |
| volume | `set_volume(0.0)`, `(1.0)`, `(2.0)` | ok; applied to the current track and every later one; `volume()` reads it back (default 1.0) | — |
| volume bad | `-0.01`, `2.0001`, `NaN`, `±inf` | unchanged | `InvalidVolume{volume}` |
| now_playing | idle / `a` current / `a` paused | `None` / `Some(a)` / `Some(a)` with `is_paused()==true` | — |
| stale handle | queue op on a handle whose call was left or replaced | nothing changes; getters return `None` / empty / 1.0 | mutators: `Interrupted{guild}` |
| leave with queue | `a` current, `[b]`, then `leave`, disconnect, `GUILD_DELETE` or shutdown | the player is stopped and the queue dropped; no track events; counter `stopped` +1 | — |
| counter | finished, skipped, stopped, error | `noctis_voice_tracks_total{app,result}` +1 per track that became current; pending tracks dropped by `clear`/`stop`/leave are not counted | — |
| unique ids | 3 enqueues in 2 guilds | `TrackInfo::id()` all distinct (per App) | — |

Config (`VoiceConfig`; out of range = `BuildViolation::plugin::<VoicePlugin>` naming the range):

| Setting | Values → result |
|---|---|
| `max_queue(usize)` default 1000 | 0 ✗, 1 ✓, 10_000 ✓, 10_001 ✗ (range `1..=10000`) |
| `idle_leave(Option<Duration>)` default `None` | `None` ✓, 999 ms ✗, 1 s ✓, 24 h ✓, 24 h + 1 ns ✗ (range `1 s..=24 h`) |
| `media_connect_timeout(Duration)` default 10 s | 0 ✗, 999 ms ✗, 1 s ✓, 60 s ✓, 60 s + 1 ns ✗ |
| `media_max_redirects(u8)` default 5 | 0 ✓ (no redirect followed), 10 ✓, 11 ✗ |

Idle leave (`idle_leave = Some(d)`; idle = connected and nothing playing, i.e. no current track or the current one paused):

| Scenario | Expected |
|---|---|
| a connected call idle for `d` | op 4 `null` sent, entry removed, gauge −1 (same path as `CallHandle::leave`) |
| a track starts or resumes before `d` | the timer is cancelled and re-armed at the next idle |
| a pending call (join not resolved) | no timer |
| `None` | never leaves |

Media HTTP (local server, `media_max_redirects` 5):

| Scenario | Expected |
|---|---|
| GET of a URL track | the request carries no `authorization`, no `user-agent`, no header whose name or value contains `noctis`, `discordbot` or the bot token (case-insensitive); only `host`, `accept`, optional `range`/`accept-encoding` |
| 5 redirects then 200 | succeeds |
| 6 redirects | fails → `TrackError` |
| redirect to `https://discord.com/api/v10/users/@me` | refused before connecting → `TrackError` |
| `max_redirects` 0, one 302 | fails |
| `HTTPS_PROXY` set in the environment | ignored (`no_proxy()`) |
| no listener (connection refused) | `TrackError` within `media_connect_timeout` |

Extension events (core):

| Scenario | Expected |
|---|---|
| `emitter.emit(e)` during a run with a hook for `E` | the hook runs once with `e`, in emit order, on the hook queue, with `HookCtx::module()` = its Module |
| no hook for `E`, or outside a run | dropped, nothing allocated |
| two extension types, hooks only for `A` | `B` never reaches `A`'s hook |
| hook returns `Err` or panics | reported to the Error Sinks with `event_type = E::NAME`, as for lifecycle hooks; later events still run |
| `Emitter` kept after the App is dropped | `emit` is a no-op (it holds a `Weak`) |

</intent-contract>

## Code Map

- `crates/noctis-core/src/lifecycle.rs` -- `sealed::{HookKind (L65), Fired (L90), Sealed (L104)}`, `HookEvent` (L191), the `hook_events!` macro, `LifecycleHook::__new` (L491: erased `from_fired`), `Hooks::by_kind` (L540), `Lifecycle::{wants, enqueue, emit}` (L630-670), `HookCtx` (L400). Add `HookKind::Extension` (COUNT 11), `Fired::Extension(ExtensionFired)`, where `ExtensionFired` = `TypeId` + `&'static str` name + `Arc<dyn Any + Send + Sync>`, with hand-written `Debug` and `PartialEq` (`Arc::ptr_eq`). Add the blanket `impl<E: ExtensionEvent> sealed::Sealed for E` (downcast in `from_fired`, `KIND = Extension`) and `impl<E: ExtensionEvent> HookEvent for E { NAME = E::NAME }`. A scratch `rustc` check confirmed that this blanket coexists with the concrete impls. `wants` for `Extension` must also match the `TypeId`: keep the set of registered extension `TypeId`s in `Hooks`. Find where `event_type`/NAME is read for error reports and make it use the fired event's name.
- `crates/noctis-core/src/kind.rs` (L503) -- `__Bind` for hooks is already generic over `E: HookEvent`, so `#[derive(Hook)]` accepts extension events with no macro change.
- `crates/noctis-core/src/app.rs` -- add `App::emitter()`. `AppShared` holds `lifecycle`.
- `crates/noctis-core/src/observe.rs` (L103, L250) -- the `METRICS` table and its doc table; add `noctis_voice_tracks_total` (counter, `app`, `result`) and update the count test.
- `crates/noctis/src/lib.rs` -- `pub mod lifecycle` (L1715): re-export `ExtensionEvent` and `Emitter`, and add a doc line. `pub mod voice` (L2793): re-export the new items and extend the docs. Prelude: nothing new.
- `crates/noctis-voice/src/call.rs` -- `Entry` gains `queue: Arc<QueueCell>` (or equivalent) and `connected: bool`. `VoiceShared` gains `media: reqwest::Client`, `emitter: OnceLock<Emitter>` (bound with `app` in `bind_app`/`on_app_started`), a track-id counter and the `#[cfg(test)]` player override. Every removal path (`remove*`, `take_all`) must shut the queue down (stop the player, record `stopped`, cancel the idle timer).
- `crates/noctis-voice/src/voice.rs` -- `CallHandle` gains the queue methods. `join_inner` marks `connected` on `Ok` and arms the idle timer. Its lookups of `(guild, generation)` are the stale check.
- `crates/noctis-voice/src/plugin.rs` -- `install` builds the media client and reports violations. `on_app_started` binds the emitter. `leave_all` stops the queues before leaving.
- `crates/noctis-voice/src/config.rs` -- the new settings use the `get_*`/`is_*` getter convention (memlog 17.1). Extend `violations()`.
- `crates/noctis-voice/src/error.rs` -- add `QueueFull{guild,max}`, `InvalidVolume{volume: f32}` and `InvalidInput{reason: String}`. Drop `Eq` from the derive (`f32`); keep `PartialEq`.
- Songbird 0.6.1 (verified in `~/.cargo/registry/src/*/songbird-0.6.1`):
  - `input::{File::new(path) -> Input::Lazy, HttpRequest::new(reqwest::Client, String), Compose (async_trait: create, create_async, should_create_async), ChildContainer (kills/reaps on drop), AudioStream{input, hint}, Input::make_playable_async(codecs, probe)}`, `input::codecs::{get_codec_registry, get_probe}` (they register the symphonia formats enabled by feature unification).
  - `tracks::{Track::new(input).volume(v), Track.events: EventStore (pub; add handlers before play), TrackHandle::{play, pause, stop, set_volume}}`, `Driver::play_only(track)`/`play(track)` through `Call: DerefMut<Target = Driver>`.
  - `events::{Event::Track(TrackEvent::{Playable, End, Error}), EventHandler (async_trait act), EventContext::Track}`. An errored track fires `Error` and then `End` (`PlayMode::also_fired_track_events`).
  - `Config::test_cfg` is `#[cfg(test)]` only, so Songbird has no public test driver (hence the `Player` seam).
- `xtask/src/deps_check.rs` -- `BANNED_HTTP` (L101) and `asset-http` (module docs L33-40; tests near L1675). Amend: `reqwest` is allowed only as a normal dependency of `noctis-voice`.
- `xtask/src/` api-snapshot path mapping -- the 17.1 entries map `noctis_voice::*` to `noctis::voice::*`. Add the new types, and map `noctis_core::lifecycle::{ExtensionEvent, Emitter}` to `noctis::lifecycle::*`.
- `crates/noctis-testkit` `fake::{FakeGateway, FakeDiscordHttp}` and `crates/noctis-voice/tests/voice_gateway.rs` -- patterns for an App on the fakes (2 shards, `ReadyData` user 1, a pending join creates the entry).
- `examples/project-template/src/modules/voice/mod.rs`, `locales/{en-US,tr}.yaml` (`voice:` L116), `.claude/skills/noctis/references/voice.md`, `SKILL.md` (`API hash` L23), `AGENTS.md`.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/lifecycle.rs`, `app.rs`, `observe.rs` -- add the public `trait ExtensionEvent: Clone + fmt::Debug + Send + Sync + 'static { const NAME: &'static str; }`, documented as "a plugin's own hook event; every `ExtensionEvent` is a `HookEvent`". Add the public `Emitter` (`Clone`, `Debug`, holds `Weak<AppShared>`) with `emit<E: ExtensionEvent>(&self, event: E)`. Add `App::emitter()` and `HookCtx::emitter()`. Wire up the `Extension` kind as in the Code Map. Unit tests cover every extension-event matrix row; the hook test uses `LifecycleHook::__new::<TestEvent,..>`. Add the `noctis_voice_tracks_total` METRICS row.
- `Cargo.toml` (root), `crates/noctis-voice/Cargo.toml` -- add the workspace deps `symphonia` and `reqwest` (Always lines), the `noctis-voice` features with `default` = all seven formats, and `tokio` features `process`, `fs`, `io-util` only if they are used. If `async-trait = "0.1"` (already in the graph through Songbird) is used for `Compose`/`EventHandler`, add it; the alternative is hand-expanding it like `shard.rs`. Dev-deps: whatever the local HTTP server test needs (`tokio` `net`).
- `crates/noctis-voice/src/track.rs` (new) -- `Track` (`Debug`, `Clone`), with the constructors `file(impl Into<PathBuf>)`, `url(impl AsRef<str>)` and `command(impl Into<OsString>, impl IntoIterator<Item: Into<OsString>>)`, each returning `Result<Track, VoiceError>` and validated as in the matrix. Builders: `title(impl Into<String>)` and `requested_by(Id<UserMarker>)`. `TrackInfo` (`Clone` over an `Arc`, `Debug`, `PartialEq` by id) with `id() -> TrackId`, `title() -> Option<&str>`, `source() -> &TrackSource` and `requested_by()`. `TrackId(u64)` (`Copy`, `Eq`, `Hash`, `Display`, `get()`). `#[non_exhaustive] enum TrackSource { File(PathBuf), Url(String), Command(OsString) }`. Also add the crate-private conversion to a lazy Songbird `Input` (`File`; `HttpRequest` with the App's media client; a `ChildCompose` that spawns `std::process::Command` with stdin null, stdout piped and stderr null inside `create`, returning `ReadOnlySource<ChildContainer>`, with `should_create_async() == false`). Unit tests cover every input matrix row.
- `crates/noctis-voice/src/media.rs` (new) -- `fn media_client(&VoiceConfig) -> Result<reqwest::Client, String>` (Always line) and `fn is_discord_api(&Url) -> bool`, shared by `Track::url` and the redirect policy. Tests in `media/tests.rs` (or `#[cfg(test)] mod`) use a raw `tokio::net::TcpListener` HTTP/1.1 server that records request heads, serves a tiny generated WAV and answers 302 chains. They drive `HttpRequest::create_async()` and, under `wav`, `Input::make_playable_async` with no driver. They cover every media matrix row, and set `HTTPS_PROXY` only inside a dedicated test that restores it.
- `crates/noctis-voice/src/queue.rs` (new) -- a pure state machine (`current: Option<Current{info, token, paused}>`, `pending: VecDeque<Track>`, `volume`, `max`) whose operations return the player commands and the events to emit. Add the crate-private `trait Player: Send + Sync` (`play(token, Track, volume)`, `pause(token)`, `resume(token)`, `stop(token)`, `set_volume(token, f32)`). The Songbird implementation builds `songbird::tracks::Track::new(input).volume(v)`, attaches `Playable`/`End`/`Error` handlers that report `(token, outcome)` back through a `Weak` to the queue cell, and plays it with `play_only` under the Call lock inside a spawned task (advancing never blocks a driver event task). `Error` maps to `TrackError` with the `PlayError` `Display` text; the `End` that follows is ignored. Add the idle timer (a `tokio::time::sleep` task per entry, only when `idle_leave` is `Some`, cancelled on activity and on removal). Unit tests cover every queue and idle matrix row with a `FakePlayer` and `tokio::time::pause()`.
- `crates/noctis-voice/src/events.rs` (new) -- `#[non_exhaustive]`, `Debug`, `Clone`, `PartialEq` events implementing `ExtensionEvent`: `TrackStart{guild, track: TrackInfo}` (`VOICE_TRACK_START`), `TrackEnd{guild, track, reason: EndReason}` (`VOICE_TRACK_END`), `TrackError{guild, track, error: String}` (`VOICE_TRACK_ERROR`), `QueueEmpty{guild}` (`VOICE_QUEUE_EMPTY`), and `#[non_exhaustive] enum EndReason { Finished, Skipped, Stopped }`.
- `crates/noctis-voice/src/{call,voice,plugin,config,error,lib}.rs` -- wire up the queue and the timer, and add the `CallHandle` API: async `enqueue(Track) -> Result<TrackInfo, VoiceError>`, `skip() -> Result<Option<TrackInfo>, _>`, `pause()`/`resume() -> Result<bool, _>`, `stop()`/`clear() -> Result<usize, _>`, `set_volume(f32) -> Result<(), _>`; sync `now_playing() -> Option<TrackInfo>`, `is_paused() -> bool`, `list() -> Vec<TrackInfo>`, `queue_len() -> usize`, `volume() -> f32`. Add the config settings, error variants and re-exports, and update the crate docs. Add an in-crate e2e test (`src/tests.rs`): an App on `FakeGateway` with a Module carrying `LifecycleHook::__new` hooks for all four events, a pending join, the player overridden by `FakePlayer`, then enqueue/end/error/skip/stop. It asserts the hook-observed event sequence, the counter (via `metrics-util` `DebuggingRecorder`), queue order and the stale-handle errors. Also add the idle-leave e2e (`idle_leave` 1 s, entry marked connected through a `#[cfg(test)]` helper: op 4 `null` recorded, `active_calls()==0`) and the leave-with-queue row. Every wait is bounded (≤ 15 s).
- `crates/noctis-voice/tests/track_events_derive.rs` -- compile-and-build check: a `#[derive(Hook)]` struct with `#[handler] async fn handle(self, ctx: HookCtx, e: noctis::voice::TrackStart)` added to a Module; `App::builder().voice(..)` builds.
- `crates/noctis/src/lib.rs` -- re-exports `noctis::voice::{Track, TrackInfo, TrackId, TrackSource, TrackStart, TrackEnd, TrackError, QueueEmpty, EndReason}` and `noctis::lifecycle::{ExtensionEvent, Emitter}`. Update the docs: the queue, the events table, formats, media HTTP and idle leave.
- `xtask/src/deps_check.rs` -- amend `asset-http`: `reqwest` is allowed only as a normal dependency of `noctis-voice` (AD-30 media inputs). Update the module docs. Add tests: reqwest on `noctis-voice` (normal) passes; on another member, or as a dev-dependency of `noctis-voice`, it fails with a message naming AD-30.
- `xtask` api-snapshot mapping, `xtask/public-api.txt` -- add the mappings, then `--bless`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` -- add "Amended (2026-10-25, Story 17.2)" lines. AD-24: the media client (reqwest 0.12 as Songbird's input client, connect timeout, redirect cap, `no_proxy`, no headers or token, Discord API refused in input and redirects; `asset-http` allows reqwest only for `noctis-voice`), and `ExtensionEvent`/`Emitter` as the plugin event seam. AD-30: the Noctis-owned queue (driver holds only the current track), `max_queue`, `idle_leave`, the four events and the symphonia format features. AD-21: extension events go on the hook queue. Add the AD-29 metric row `noctis_voice_tracks_total` (result). Also add the same media line to `_bmad-output/planning-artifacts/maintainer-decisions.md` §10 as "Amended (2026-10-25, Story 17.2)".
- `examples/project-template/{src/modules/voice/mod.rs, locales/en-US.yaml, locales/tr.yaml, AGENTS.md, tests/voice.rs}` -- add `/play url:String` (`Track::url`, `requested_by` invoker, enqueue on `ctx.voice().call(guild)`, else an ephemeral "join first" reply; reply with title/position or the `VoiceError`), `/skip` and `/stop`, plus a `#[derive(Hook)]` `TrackErrorLog` that logs `TrackError` with `tracing::warn!`. Add the locale keys in both files. The test asserts the new commands exist. `AGENTS.md` mentions the queue.
- `examples/project-template/.claude/skills/noctis/{references/voice.md, SKILL.md}` -- new sections: Tracks (the three inputs, validation, formats/features, pre-encoded tips deferred to 17.3), the Queue (every method with its semantics from the matrix), Track events (the `#[derive(Hook)]` example, the four events and reasons), Idle leave, `max_queue`, Media HTTP behavior, and Metrics. Also the new `API hash`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the `(decision)` lines from the Design Notes, plus any implementation decisions.

**Acceptance Criteria:**
- Given an App with `.voice(..)` and a Module with a `#[derive(Hook)]` handler for `TrackEnd`, when it builds, then the build succeeds and the hook is registered for the `VOICE_TRACK_END` extension event.
- Given a running App on the fakes with a call and the fake player, when three tracks are enqueued and the player reports end, error and end, then the Module's hooks observe exactly `TrackStart a, TrackEnd a Finished, TrackStart b, TrackError b, TrackStart c, TrackEnd c Finished, QueueEmpty`, and `noctis_voice_tracks_total{result="finished"} == 2`, `{result="error"} == 1`.
- Given `cargo tree -p noctis -e normal --prefix none` and the same with `--features voice`, then the first lists no `songbird`, `symphonia` or `reqwest`, and the second lists exactly one version each of `symphonia` and `reqwest` and no `aws-lc-rs`. `deps-check` and `cargo deny check` pass.
- Given a URL track whose server redirects to `discord.com/api`, when it is played, then no connection to Discord is attempted and the outcome is `TrackError`.
- Given the gates, when run, then `api-snapshot` matches the re-blessed file, `skill-check` passes, `cargo test --release -p noctis-voice --no-default-features` passes, and `rules-catalog` is unchanged and green.

## Spec Change Log

## Review Triage Log

### 2026-10-25 — Review pass
- verdicts: 36 findings — high 0, medium 14, low 7, false 15, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) Idle leave can fire after a track starts: the elapsed timer spawns `leave()` detached and `idle_leave` removes by generation without re-checking idleness; `Locked.timer` never cleared — confirmed at `queue.rs` `sync_idle` and `call.rs` `idle_leave`/`remove_generation`; patch: remove only when the queue is still idle (the stale-timer sub-claim is harmless, see the edge row below).
  - `[medium]` `[patch]` (blind) Media client has no read/response timeout, so a stalled or silently dead connection blocks the lazy `HttpRequest` forever — confirmed: only `connect_timeout` is set and Songbird 0.6.1 has no preparation timeout; `read_timeout` would kill paused tracks (the consumer stops reading), so patch: TCP keepalive on the client plus a doc sentence.
  - `[medium]` `[patch]` (blind) `is_discord_api` bypassable with percent-encoding (`/%61pi/`) — confirmed: the first segment is compared raw; patch: percent-decode it before comparing, plus test rows.
  - `[medium]` `[patch]` (blind) URL tracks allow SSRF into loopback/private/link-local hosts, and the template feeds any member's string to `Track::url` — confirmed; the spec deliberately accepts `127.0.0.1` (tests need it), so no code guard; patch: warnings in `Track::url` rustdoc, voice.md and the template's `/play`.
  - `[low]` `[reject]` (blind) Redirect policy allows https to http downgrade — real but no credentials or headers are ever sent, users will rarely meet it, and the fix adds a branch and a policy decision.
  - `[low]` `[reject]` (blind) Extension events ride the unbounded lifecycle channel — the channel is pre-existing; voice events are bounded by track transitions (at most `max_queue` per burst), and a bound/overflow policy is new design, not a direct correction.
  - `[medium]` `[patch]` (blind) `SongbirdPlayer` has no tests (slot mirror, token-mismatch drop, `stop` token reset, `Errored` extraction, event registration) — same root as the vgap `Report` row; patch: `Report`-to-queue mapping tests, plus slot tests on an unconnected `Call` if Songbird allows.
  - `[medium]` `[defer]` (blind) Real join path never calls `connected` in a test — same root as the vgap row; deferred: needs a fake voice server (Story 17.4).
  - `[low]` `[reject]` (blind) Child kill-and-reap promise untested — the kill is Songbird's `ChildContainer` drop (`ChildCompose` wraps the child in it); a process-liveness test is platform-specific and adds complexity for a regression developers are unlikely to meet.
  - `[medium]` `[patch]` (blind) `Track::command` yt-dlp examples pass user URLs without `--`, inviting option injection (`--exec`) — confirmed in voice.md L194 and the track.rs docs that agents copy; patch: `--` before user input plus one sentence.
  - `[low]` `[reject]` (blind) Template `/play`, `/skip`, `/stop` have no authorization — an example design choice like 17.1's `/join`/`/leave`; no defect in noctis, and access policy is the bot author's.
  - `[low]` `[reject]` (blind) `TrackError.error` can carry URL query secrets while the doc says "never a token" — the claim refers to the bot token, which is never sent; the URL is the bot's own input and is already in `TrackInfo::source`; low and not met in everyday use.
  - `[low]` `[patch]` (blind) `proxy_environment_is_ignored` passes vacuously if the child test path changes (0 tests, exit 0) — confirmed: only `status.success()` is checked; patch: assert `1 passed` in the child's stdout.
  - `[low]` `[reject]` (blind) Template `/play` shows `queue_len()` as position and `enqueue` burns ids on `QueueFull` — cosmetic: the position counts the current track as #1 and only a concurrent `/play` skews it; id gaps break no uniqueness.
  - `[false]` `[reject]` (blind) `ExtensionFired` equality is pointer identity — the spec's Code Map deliberately chose `Arc::ptr_eq`, and no caller compares extension payloads for equality; no named harm.
  - `[false]` `[reject]` (blind) Extension hooks silently disappear through `Hooks::of` — every current call site is correct: `of_fired` routes extensions through `by_extension`, `wants` is only used for `Fired::from_shard` (never extension), and `of` documents the empty slot.
  - `[medium]` `[patch]` (edge) Enqueue/resume after the idle sleep but before the spawned leave removes the entry still leaves the call — same root as the blind idle row; patched there.
  - `[false]` `[reject]` (edge) Fired timer kept in `locked.timer` can block re-arming when leave is a no-op — the leave only no-ops when the entry of that generation is gone (its queue was shut down and closed) or the App is dropped; with the idle re-check, a not-idle result means `sync_idle` already took the timer.
  - `[false]` `[reject]` (edge) `TrackEnd`/`TrackError` without a preceding `TrackStart` — the spec settled it: `TrackStart` fires on `Playable`, and the matrix rows "track error (create…)" and "error chain" expect `TrackError` without a start.
  - `[medium]` `[patch]` (edge) `is_discord_api` percent-encoding bypass — same root as the blind row; patched there.
  - `[false]` `[reject]` (edge) Enqueue on a pending call then a failed join discards the track with no event — the spec matrix "leave with queue" settles it: removal stops the player, counts `stopped`, emits no track events; enqueue on a pending call is the spec's tested path.
  - `[medium]` `[patch]` (edge) Aborting the timer after the sleep cannot stop the detached leave — same root as the blind idle row; patched there.
  - `[medium]` `[patch]` (vgap) `SongbirdPlayer`/`Report` driver-event mapping untested — pre-verified regression gap; patch: in-crate `Report` tests over a `QueueCell`.
  - `[medium]` `[defer]` (vgap) `join_inner` marking the call connected never exercised — pre-verified; filed disposition `defer` accepted: a resolved join needs a fake voice server the repo lacks.
  - `[medium]` `[patch]` (vgap) App shutdown with a non-empty queue: `take_all`'s `queue.shutdown()` unpinned — pre-verified; patch: e2e test asserting `Stop(token)`, `stopped` +1 and no hook event after `running.stop()`.
  - `[medium]` `[patch]` (intent D1) Driver-to-queue event path not exercised — same root as the `SongbirdPlayer` rows; patched there (Songbird's test driver is `#[cfg(test)]`-only, which the AC allows).
  - `[false]` `[reject]` (intent D2) Only WAV is decoded in tests — the spec's Design Notes deliberately prove decoding on a generated WAV; other formats are wired through features with no fixture encoder in the repo.
  - `[false]` `[reject]` (intent D3) Header test calls `media_client` directly — the plugin uses the same function; the auditor itself notes no behavioural gap.
  - `[false]` `[reject]` (intent D4) No event when a track ends by leave/shutdown — settled by the spec matrix "leave with queue" and the AD-30 amendment.
  - `[false]` `[reject]` (intent D5) Unpaired Start/End — settled by the spec (start on `Playable`) and the memlog decision.
  - `[false]` `[reject]` (intent D6) Events are hook payloads via a new core seam — the spec's intent-contract approach selects this seam; Modules handle them with `#[derive(Hook)]`.
  - `[false]` `[reject]` (intent D7) "Never panics a handler" only at the queue surface — `lifecycle/tests.rs::extension_events_run_in_order` covers erroring and panicking extension hooks reported to the Error Sinks.
  - `[false]` `[reject]` (intent D8) `set_volume` vs `volume` naming — the spec's task list names `set_volume(f32)` plus the `volume()` getter.
  - `[false]` `[reject]` (intent D9) Paused counts as idle — the spec's idle-leave matrix defines idle as "no current track or the current one paused".
  - `[false]` `[reject]` (intent D10) Template covers only `/play`, `/skip`, `/stop` — the spec's task list selects these; the Usage Skill documents the full queue API, satisfying "template and Usage Skill document".
  - `[false]` `[reject]` (intent D11) Maintainer decisions §10 amended by the diff — the spec directs the amendment and the maintainer delegated decisions; no violation.

## Design Notes

**Typed events through a core seam (decision).** `EventPayload` and `HookEvent` are sealed and gateway- or lifecycle-only, and core has no way for a plugin to deliver its own events to Modules. The lifecycle hook machinery is already type-erased (`from_fired`) and already reaches `#[derive(Hook)]` through `E: HookEvent`. One `Extension` kind with a `TypeId`-checked downcast therefore gives plugins typed, ordered, App-wide events. Error reporting, Module ownership, the end-of-run drain and the derive all come with it, and the macros do not change. `Emitter` holds a `Weak` because `VoiceShared` lives in the App's own state (an `Arc` would leak the App). Events fire on the hook queue (not the gateway lanes), so a slow track hook never delays gateway events.

**Queue outside Songbird (decision).** Songbird's `TrackQueue` registers every queued track with the mixer, ignores `Error`, and has no bound. It would put up to 1000 inputs in the mixer and break 17.3's "one track" passthrough condition. The Noctis queue holds descriptions and hands the driver only the current track. A per-track token makes late driver events harmless.

**Driver-free tests.** Songbird's `Config::test_cfg` is `#[cfg(test)]`, so the queue goes through a crate-private `Player`. Real decoding is still proven without Discord: `Input::make_playable_async` runs symphonia on a generated WAV served from a file, an HTTP server and `sh -c cat`. Story 17.4's `VoiceConfig::recording()` will make the fake public.

**Media client (decision).** `HttpRequest` takes a `reqwest::Client`, and Songbird does not re-export reqwest. `noctis-voice` therefore names reqwest directly (the version Songbird already resolves), and `asset-http` allows exactly that edge. `no_proxy()` keeps media off the Discord edge and environment proxies, and makes the header test deterministic. There is no total request timeout because tracks stream for minutes.

**Golden usage:**
```rust
let call = ctx.voice().call(guild).ok_or(VoiceError::Interrupted { guild })?;
let info = call.enqueue(Track::url(&url)?.title(name).requested_by(user)).await?;
#[derive(Hook)] struct Np;
impl Np { #[handler] async fn handle(self, ctx: HookCtx, e: TrackStart) -> HandlerResult<()> { /* render */ Ok(()) } }
```

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-core` -- pass (extension-event tests)
- `cargo test --release -p noctis-voice` and `cargo test --release -p noctis-voice --no-default-features` -- pass
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo tree -p noctis -e normal --prefix none | grep -cE '^(songbird|symphonia|reqwest) '` -- 0; `cargo tree -p noctis --features voice -d` -- no second `symphonia`/`reqwest`; `cargo tree -p noctis --features voice -e normal -i aws-lc-rs` -- no match
- `cargo run --release -p xtask -- deps-check`, `tw-grammar --check`, `rules-catalog`, `skill-check` -- ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot` -- ok; the printed hash goes into `SKILL.md`
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run only by the gate script.

## Auto Run Result

- **Summary:** `noctis-voice` gains track playback: `Track` inputs (URL over a dedicated no-proxy media client that refuses Discord API hosts, file, child command), a bounded per-guild Noctis-owned queue (enqueue/skip/stop/pause/resume/volume, `max_queue`) that hands Songbird only the current track via a crate-private `Player` with per-track tokens, idle leave, and typed `TrackStart`/`TrackEnd`/`TrackError` events delivered to Modules through a new core `Extension` lifecycle seam (`#[derive(Hook)]` works unchanged). Template gains `/play`, `/skip`, `/stop`; Usage Skill and public API snapshot updated.
- **Files changed:** `noctis-core` lifecycle/app/observe (extension event seam + tests); `noctis-voice` `track.rs`, `queue.rs` (+fake, tests), `player.rs`, `media.rs` (+tests), `events.rs`, `call.rs`, `voice.rs`, `config.rs`, `error.rs`, `plugin.rs`, `test_support.rs`, `tests.rs`, `tests/track_events_derive.rs`; `noctis` re-exports; `xtask/src/deps_check.rs` (reqwest edge for asset-http); `xtask/public-api.txt`; `examples/project-template/` (voice module, locales, tests, AGENTS.md, Usage Skill voice/app references, Cargo.lock).
- **Review findings:** 36 (high 0, medium 14, low 7, false 15). Patched (10 entries, all medium except 1 low; distinct fixes): idle leave re-checks idleness before removing the call; media client TCP keepalive + doc; `is_discord_api` percent-decodes the first path segment; SSRF warnings in `Track::url` docs, voice.md and template `/play`; `--` before user URLs in yt-dlp examples; `Report`-to-queue driver mapping tests; app-shutdown-with-queue e2e test; `proxy_environment_is_ignored` asserts `1 passed`.
- **Deferred (2, same root):** real join path marking the call connected is untested; needs a fake voice server (Story 17.4).
- **Rejected:** 7 low (https-to-http redirect, unbounded lifecycle channel, child reap test, template authorization, URL secrets in `TrackError`, cosmetic queue position/id gaps) and 15 false (all settled by the spec matrix, Design Notes or existing tests); reasons recorded in the Review Triage Log.
- **Follow-up review recommended: true** (first pass, 0 high / 9 medium / 1 low patched). Named risk: the idle-leave race fix (re-check idleness inside `idle_leave` against the detached timer) touches concurrency between `sync_idle`, `enqueue`/`resume` and removal by generation, and is only covered by unit-level queue tests.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, deps-check, tw-grammar, tw_crosscheck, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91); `cargo hack` left to the epic-last gate. CI paused by the maintainer.
- **Residual risks:** no test drives a real Songbird join/driver end to end (deferred to 17.4); URL tracks can reach private hosts by design (documented, no guard); redirect downgrade to http allowed.
