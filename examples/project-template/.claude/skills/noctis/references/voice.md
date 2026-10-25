# Voice

Join and leave voice channels from any handler, and play tracks through a bounded
per-guild queue. Noctis sends every Voice State Update (op 4) through its own gateway
and uses Songbird 0.6 only as the voice driver; no Songbird type is part of the API. Rule ids:
`[VC-01]` (op 4 goes through the owning shard and the GW-04 budget) and `[VC-02]` (one
voice connection per guild). Both are in `limits-cheatsheet.md`.

## 1. Enable it

The cargo feature `voice` is **off by default**. It builds libopus from source, so the
build machine (and the CI or Docker build image) needs a C compiler and `cmake`.

```toml
# Cargo.toml of the bot (the template already has `voice = ["noctis/voice"]`)
[features]
voice = ["noctis/voice"]
```

```rust
use noctis::prelude::*;                       // brings AppBuilderVoiceExt and VoiceExt
use noctis::voice::VoiceConfig;

builder
    .voice(VoiceConfig::default())            // installs the VoicePlugin
    .module(modules::voice::module());
```

- `.voice(VoiceConfig)` (trait `AppBuilderVoiceExt`, sealed) installs the one
  `VoicePlugin`. The last call wins and there is no `DuplicatePlugin`.
  `.plugin(VoicePlugin::new(config))` does the same.
- It adds the `GUILD_VOICE_STATES` intent (not privileged) and the Module
  `noctis-voice` (its own handlers for the bot's voice state, the voice server and
  `GUILD_DELETE`). Do not mount a Module named `noctis-voice` yourself.
- `VoiceConfig` is `#[non_exhaustive]`. Start from `VoiceConfig::default()` or
  `VoiceConfig::new()`, then chain the setters:
  - `.join_timeout(Duration)`: 1 s..=60 s, default 10 s. Any other value makes
    `build()` return a `BuildViolation` from the plugin.
  - `.self_deaf(bool)`: default `true`, so the bot joins deafened and does not receive
    audio.
  - `.max_queue(usize)`: the most tracks a guild's queue holds, the current one
    included: 1..=10000, default 1000 (§7).
  - `.idle_leave(Option<Duration>)`: leave a connected call that stayed idle this long:
    `None` (default, never) or 1 s..=24 h (§9).
  - `.media_connect_timeout(Duration)`: 1 s..=60 s, default 10 s (§9).
  - `.media_max_redirects(u8)`: 0..=10, default 5; 0 follows none (§9).
  - Any value out of its range makes `build()` fail with a `BuildViolation` from the
    plugin that names the range.
- Read the values back with `get_join_timeout()`, `is_self_deaf()`, `get_max_queue()`,
  `get_idle_leave()`, `get_media_connect_timeout()` and `get_media_max_redirects()`.
- Demux formats are cargo features of the internal voice crate, all on with `voice`:
  Ogg, Matroska/WebM, MP3, AAC/MP4, FLAC and WAV (symphonia). Opus is always
  decodable.

## 2. Join and leave

`ctx.voice()` (trait `VoiceExt`, sealed) works on `EventCtx`, `CommandCtx`,
`ComponentCtx`, `ModalSubmitCtx` and `HookCtx`. `app.voice()` works on the `App`. Each
returns the App's `Voice`, which is a cheap clone.

```rust
use noctis::prelude::*;
use noctis::twilight::model::id::{Id, marker::ChannelMarker};
use noctis::voice::VoiceError;

#[derive(SlashCommand, Debug)]
#[command("join", contexts = [Guild], response = ephemeral)]
pub struct Join {
    #[option(channel_types = [GuildVoice, GuildStageVoice])]
    pub channel: Id<ChannelMarker>,
}

impl Join {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let Some(guild) = ctx.guild_id() else {
            return ctx.reply_text(keys::COMMON_GUILD_ONLY).await;
        };
        let s = Strings::of(ctx.response_locale());
        match ctx.voice().join(guild, self.channel).await {
            Ok(_call) => ctx.reply_text(s.voice.joined(format!("<#{}>", self.channel))).await,
            Err(error) => ctx.reply_text(s.voice.failed(error)).await, // a typed VoiceError
        }
    }
}
```

| Method | What it does |
| --- | --- |
| `voice.join(guild, channel)` | Sends op 4 from the guild's owning shard. It resolves to a `CallHandle` once Discord has answered and the voice driver has connected, within `join_timeout`. The future is `Send + 'static`, so it can be spawned. A join takes up to `join_timeout`, so use `response = ephemeral` or the auto-defer. |
| `voice.leave(guild)` | Removes the guild's call and sends op 4 with no channel. If there is no call, it still sends op 4, which clears a channel left over from before a restart. |
| `voice.call(guild)` | The guild's `CallHandle` (connected or still joining), or `None`. |
| `voice.active_calls()` | How many calls exist, live or pending. |
| `call.guild()` | The guild. |
| `call.channel()` | The channel the call is in or joining. It is `None` once the handle is stale (see VC-02). |
| `call.leave()` | Same as `voice.leave(guild)`, but a stale handle returns `Interrupted` and sends nothing. |

**One call per guild (VC-02).**
- Calling `join` for the channel the call is already connected to sends nothing and
  returns the same call.
- Calling `join` again for the same channel while the first join is still pending
  interrupts the first one (it returns `VoiceError::Interrupted`) and sends op 4 again.
- Calling `join` for another channel of the same guild moves the call there.
- A pending join that a newer join replaces returns `VoiceError::Interrupted`.
- When the bot is disconnected (a moderator kicks it, or Discord sends its own voice
  state with no channel), the call is removed. The same happens on `GUILD_DELETE`; no
  op 4 is sent in either case.
- A moderator moving the bot updates `call.channel()`.
- A `CallHandle` whose call was replaced or removed is stale.

Audio plays through the call's queue (§6-§8).

## 3. Errors

`VoiceError` is `#[non_exhaustive]`, `Clone + PartialEq`, and its messages carry no
token:

| Variant | When |
| --- | --- |
| `NotInstalled` | The App has no `.voice(..)`. |
| `NotReady` | The shards run but no READY has arrived yet, so the bot's user id is unknown. Retry later. |
| `Gateway(GatewayError)` | `[VC-01]` op 4 was refused before it was sent. The cause is one of: `NotRunning` (before `run` or during shutdown), `GuildNotInRange` (another Cluster process owns the guild), `ShardClosed`, or `SendLimit { retry_after }` (GW-04). Nothing is queued. |
| `Timeout { guild, after }` | Discord or the driver did not answer within `join_timeout`. The call was left and removed. |
| `Driver { guild, reason }` | The voice driver failed to connect. The call was left and removed. |
| `Interrupted { guild }` | `[VC-02]` A newer join, a leave, a disconnect or a `GUILD_DELETE` replaced the call, or `call.leave()` or a queue mutator was called on a stale handle. |
| `QueueFull { guild, max }` | `call.enqueue(..)` on a queue that already holds `max_queue` tracks (current included). Nothing changed. |
| `InvalidVolume { volume }` | `call.set_volume(v)` with `v` outside `0.0..=2.0` or not finite. Nothing changed. |
| `InvalidInput { reason }` | A `Track` constructor refused its input; `reason` names the rule (§6). |

`VoiceError` is `PartialEq` but not `Eq` (it carries an `f32`).

## 4. Gateway level: op 4 without voice

`noctis::gateway` (always compiled, no feature needed):

- `gateway.update_voice_state(VoiceStateRequest)` sends op 4 from the guild's owning
  shard.
  - It is synchronous and checks, in order: `NotRunning`, `GuildNotInRange`,
    `ShardClosed`, `SendLimit`.
  - It shares the per-shard GW-04 window of 110 commands per 60 s with presence sets
    and member requests.
  - The App needs the `GUILD_VOICE_STATES` intent to receive the answers
    (`VOICE_STATE_UPDATE`, `VOICE_SERVER_UPDATE`). Op 4 is sent without it, but the
    answers never arrive. The `voice` feature adds the intent.
- `VoiceStateRequest::join(guild, channel)` or `VoiceStateRequest::leave(guild)` build
  the request, then `.self_mute(bool)` and `.self_deaf(bool)` set the flags. Read it
  back with `.guild()` and `.channel()`.
- `gateway.current_user_id()` is the bot's user id from the first READY, or `None`
  before it.

Use `update_voice_state` only for a bot that has its own voice stack. With `voice`
installed, use `ctx.voice()`. A raw op 4 for a guild that has a call is seen by the
plugin as a move or a disconnect.

## 5. Shutdown and metrics

- The plugin registers a Shutdown Task (`AppBuilder::shutdown_task`, see
  `app-and-sharding.md` §8). After `ShutdownStarted` and before the shards close, it
  sends op 4 leave for every call and waits for Discord to confirm.
- It waits at most `min(shutdown_grace, 5 s)`. After that the remaining calls are
  dropped, and Discord removes the bot from the channel after its own timeout.
- With `shutdown_grace` set to 0 it does not run.
- Every queue is dropped with its call (leave, disconnect, `GUILD_DELETE`, idle leave,
  shutdown): the driver stops its track, a command track's child process is killed,
  and no track event fires.
- The gauge `noctis_voice_calls{app}` counts live and pending calls. It is published
  once the App runs.
- The counter `noctis_voice_tracks_total{app, result}` counts every track that became
  current once, by outcome: `finished`, `skipped`, `stopped` (also a current track
  dropped with its call) or `error`. Pending tracks dropped by `clear`, `stop` or a
  leave are not counted. No metric carries guild, channel, user or track ids, or URLs.

## 6. Tracks

A `Track` describes audio; nothing is opened, fetched or spawned until it becomes the
queue's current track. Every constructor checks its input and returns
`Result<Track, VoiceError>` (`InvalidInput { reason }`).

| Constructor | Source | Refused |
| --- | --- | --- |
| `Track::file(path)` | a local file (`TrackSource::File`) | an empty path. A missing file is found only at play time: `TrackError`, then the next track. |
| `Track::url(url)` | an `http`/`https` media URL (`TrackSource::Url`), fetched with the App's media client (§9) | empty or malformed URLs, more than 4096 bytes, any other scheme (`ftp:`, `file:`, `data:`), credentials (`https://user:pass@host/..`; they are never sent), and Discord's API: host `discord.com` or `discordapp.com` or a subdomain (`ptb.discord.com`) with `api` as the first path segment, case-insensitive, trailing dot ignored. `https://cdn.discordapp.com/attachments/..` is fine. |
| `Track::command(program, args)` | a child process whose stdout is the audio (`TrackSource::Command(program)`) | an empty program. It is spawned when the track becomes current (stdin and stderr closed) and killed and reaped when the track ends, is skipped or stopped, or the call is left. A program that does not start is a `TrackError` at play time. |

Builders: `.title(impl Into<String>)` (at most 256 chars, a longer one is cut at a
char boundary, `""` clears it) and `.requested_by(user_id)`. `track.source()` reads
the source back.

```rust
use noctis::voice::Track;

let a = Track::url("https://cdn.example.com/song.ogg")?.title("Song").requested_by(ctx.user_id());
let b = Track::file("assets/intro.wav")?;
let c = Track::command("yt-dlp", ["-f", "bestaudio[acodec=opus]", "-o", "-", url.as_str()])?;
```

Tip: Ogg/WebM Opus at 48 kHz is the cheapest input (decoding Opus is light; a
pass-through path that skips decode and re-encode arrives later). With `yt-dlp`,
prefer `bestaudio[acodec=opus]`.

## 7. The queue

Each call (pending or connected) has one queue, on its `CallHandle`. It holds the
current track plus the pending ones, at most `max_queue` (default 1000). The driver
only ever holds the current track; a pending track is a description until it becomes
current. Enqueueing works while the join is still pending.

| Method | What it does |
| --- | --- |
| `call.enqueue(track).await -> Result<TrackInfo, _>` | Appends the track and returns its `TrackInfo` (with a new `TrackId`, unique per App). With nothing current it becomes current at once; `TrackStart` fires when the driver reports it playable. A paused current track stays paused. `QueueFull` when the queue already holds `max_queue` (nothing changes). |
| `call.skip() -> Result<Option<TrackInfo>, _>` | Ends the current track (`TrackEnd` `Skipped`) and starts the next one, playing even if the skipped one was paused, or fires `QueueEmpty`. Returns the skipped track; `Ok(None)` and no event when nothing is current. |
| `call.pause() / call.resume() -> Result<bool, _>` | `true` when the state changed; `false` when already paused / already playing, or nothing is current. |
| `call.stop() -> Result<usize, _>` | Ends the current track (`TrackEnd` `Stopped`), drops the pending ones and fires `QueueEmpty`. Returns how many tracks the queue held (`0` and no event when empty). The call stays connected. |
| `call.clear() -> Result<usize, _>` | Drops the pending tracks; the current one keeps playing; no event. Returns how many were dropped. |
| `call.set_volume(v) -> Result<(), _>` | `0.0..=2.0` (default `1.0`), for the current track and every later one. `InvalidVolume` otherwise. |
| `call.now_playing() -> Option<TrackInfo>` | The current track, playing or paused. |
| `call.is_paused() -> bool` | Whether the current track is paused. |
| `call.list() -> Vec<TrackInfo>` | The pending tracks in play order (the current one excluded). |
| `call.queue_len() -> usize` | Current plus pending. |
| `call.volume() -> f32` | The queue's volume. |

`TrackInfo` is a cheap clone (equal when the ids are): `id() -> TrackId` (`get()`,
`Display`), `title() -> Option<&str>`, `source() -> &TrackSource`,
`requested_by() -> Option<Id<UserMarker>>`.

A stale handle (its call was left, disconnected, removed or replaced) changes nothing:
the mutators return `Interrupted`, the getters read `None`, empty, `false`, `0` or
`1.0`.

```rust
#[derive(SlashCommand, Debug)]
#[command("play", contexts = [Guild], response = ephemeral)]
pub struct Play {
    #[option(max_length = 4096)]
    pub url: String,
}

impl Play {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let Some(call) = ctx.guild_id().and_then(|g| ctx.voice().call(g)) else {
            return ctx.reply_text(s.voice.not_in_call()).await;
        };
        let queued = async {
            let track = Track::url(&self.url)?.title(&self.url).requested_by(ctx.user_id());
            call.enqueue(track).await
        };
        match queued.await {
            Ok(info) => {
                let title = info.title().unwrap_or_default().to_owned();
                ctx.reply_text(s.voice.queued(title, call.queue_len())).await
            }
            Err(error) => ctx.reply_text(s.voice.failed(error)).await,
        }
    }
}
```

## 8. Track events

The queue reports through four hook events (`noctis::voice`, each a
`#[non_exhaustive]` struct with `pub` fields, `Debug + Clone + PartialEq`). They are
`ExtensionEvent`s (`app-and-sharding.md` §7), so a `#[derive(Hook)]` handler takes
them; they are not gateway events and never use `#[derive(EventHandler)]`.

| Event (`NAME`) | Fires | Data |
| --- | --- | --- |
| `TrackStart` (`VOICE_TRACK_START`) | a track became current and the driver reported it playable | `guild`, `track: TrackInfo` |
| `TrackEnd` (`VOICE_TRACK_END`) | the current track finished, was skipped or stopped | `guild`, `track`, `reason: EndReason` (`Finished`, `Skipped`, `Stopped`; `as_str()`) |
| `TrackError` (`VOICE_TRACK_ERROR`) | the current track failed: its input could not be created (missing file, failed or refused fetch, command that did not start), parsed or decoded. No `TrackEnd` follows; the next track starts, or `QueueEmpty`. | `guild`, `track`, `error: String` (the driver's message, never a token) |
| `QueueEmpty` (`VOICE_QUEUE_EMPTY`) | the last track ended, failed or was skipped, or `stop()` ran | `guild` |

Every track that became current yields exactly one `TrackEnd` or `TrackError`; late
driver reports for a track that is no longer current are ignored. A track error never
panics and never stops the queue. The events go into the App's ordered hook queue,
like lifecycle hooks: a slow hook delays later hooks, never gateway events. They are
dropped when no hook takes them. Dropping a queue with its call fires none.

```rust
use noctis::lifecycle::HookCtx;
use noctis::voice::{TrackError, TrackStart};

#[derive(Hook)]
pub struct NowPlaying;

impl NowPlaying {
    #[handler]
    async fn handle(self, _ctx: HookCtx, ev: TrackStart) -> HandlerResult<()> {
        tracing::info!(guild = %ev.guild, track = %ev.track.id(), "now playing");
        Ok(())
    }
}

#[derive(Hook)]
pub struct TrackErrorLog;

impl TrackErrorLog {
    #[handler]
    async fn handle(self, _ctx: HookCtx, ev: TrackError) -> HandlerResult<()> {
        tracing::warn!(guild = %ev.guild, error = %ev.error, "track failed");
        Ok(())
    }
}

Module::new("voice").add::<NowPlaying>().add::<TrackErrorLog>();
```

## 9. Idle leave and media HTTP

**Idle leave.** With `VoiceConfig::idle_leave(Some(d))`, a connected call that had no
playing track (nothing current, or the current one paused) for `d` is left exactly
like `call.leave()`: op 4 with no channel, the call removed, the gauge decremented.
A track starting or resuming cancels the timer; it re-arms at the next idle moment. A
pending call (join not resolved) never arms it. Default `None`: never.

**Media HTTP.** URL tracks are fetched by the App's own media client, never by the
Discord HTTP client:
- connect timeout `media_connect_timeout` (default 10 s); no total timeout, because a
  track streams for minutes;
- at most `media_max_redirects` redirects (default 5); one more fails the track
  (`TrackError`);
- a redirect to Discord's API (same rule as `Track::url`) is refused before
  connecting (`TrackError`);
- no proxy (environment proxies such as `HTTPS_PROXY` are ignored);
- no bot token, no `Authorization`, no user agent and no Noctis header: the request
  carries only `host`, `accept` and a `range` when resuming.

Never fetch media with your own HTTP client to bypass these rules.

## 10. Testing

`TestApp` runs on a fake gateway that has no voice server, so a `/join` there waits
for `join_timeout` and ends in `VoiceError::Timeout`: test the replies, not the
connection. A `/leave` sends op 4 with no channel and succeeds, and the intent check
is `app.intents().contains(Intents::GUILD_VOICE_STATES)` (the template's
`tests/voice.rs` does both). Without a call, `/play`, `/skip` and `/stop` answer "join
first"; the queue and its events are covered by Noctis's own tests.
