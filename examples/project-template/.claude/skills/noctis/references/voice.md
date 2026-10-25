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
  - `.max_bitrate(Option<u32>)`: caps the Opus encoder's bitrate in bits per second:
    `None` (default, follow the channel) or 8000..=384000 (§10).
  - Any value out of its range makes `build()` fail with a `BuildViolation` from the
    plugin that names the range.
- Read the values back with `get_join_timeout()`, `is_self_deaf()`, `get_max_queue()`,
  `get_idle_leave()`, `get_media_connect_timeout()`, `get_media_max_redirects()` and
  `get_max_bitrate()`.
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
- The counters `noctis_voice_passthrough_frames_total{app}` and
  `noctis_voice_encoded_frames_total{app}` count 20 ms frames sent by passthrough and
  frames decoded for encoding (§10).

## 6. Tracks

A `Track` describes audio; nothing is opened, fetched or spawned until it becomes the
queue's current track. Every constructor checks its input and returns
`Result<Track, VoiceError>` (`InvalidInput { reason }`).

| Constructor | Source | Refused |
| --- | --- | --- |
| `Track::file(path)` | a local file (`TrackSource::File`) | an empty path. A missing file is found only at play time: `TrackError`, then the next track. |
| `Track::url(url)` | an `http`/`https` media URL (`TrackSource::Url`), fetched with the App's media client (§9) | empty or malformed URLs, more than 4096 bytes, any other scheme (`ftp:`, `file:`, `data:`), credentials (`https://user:pass@host/..`; they are never sent), and Discord's API: host `discord.com` or `discordapp.com` or a subdomain (`ptb.discord.com`) with `api` as the first path segment (percent-decoded once, so `%61pi` counts), case-insensitive, trailing dot ignored. `https://cdn.discordapp.com/attachments/..` is fine. Any other host is fetched, private ones too (loopback, `10.x`, `169.254.169.254`): when users supply URLs, allow-list hosts (for example only `cdn.discordapp.com` attachments) before `Track::url`. |
| `Track::command(program, args)` | a child process whose stdout is the audio (`TrackSource::Command(program)`) | an empty program. It is spawned when the track becomes current (stdin and stderr closed) and killed and reaped when the track ends, is skipped or stopped, or the call is left. A program that does not start is a `TrackError` at play time. |

Builders: `.title(impl Into<String>)` (at most 256 chars, a longer one is cut at a
char boundary, `""` clears it) and `.requested_by(user_id)`. `track.source()` reads
the source back.

```rust
use noctis::voice::Track;

let a = Track::url("https://cdn.example.com/song.ogg")?.title("Song").requested_by(ctx.user_id());
let b = Track::file("assets/intro.wav")?;
let c = Track::command("yt-dlp", ["-f", "bestaudio[acodec=opus]", "-o", "-", "--", url.as_str()])?;
```

`Track::command` passes args verbatim with no shell, but the program still parses them
as options (a user value like `--exec=...` would be one), so put `--` before user input.

Tip: Ogg/WebM Opus at 48 kHz with 20 ms frames is the cheapest input: it is sent by
passthrough, without decode or re-encode (§10). With `yt-dlp`, prefer
`bestaudio[acodec=opus]`.

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
- connect timeout `media_connect_timeout` (default 10 s); no total or read timeout,
  because a track streams for minutes and a paused one stops reading. A connection
  that dies silently is detected by TCP keepalive and fails the track (`TrackError`);
  a live server that stalls is ended with `skip`;
- any public or private host is fetched (loopback, private ranges, `169.254.169.254`),
  directly or through redirects: a bot that takes URLs from users should allow-list
  hosts (for example only `cdn.discordapp.com` attachments) before `Track::url`;
- at most `media_max_redirects` redirects (default 5); one more fails the track
  (`TrackError`);
- a redirect to Discord's API (same rule as `Track::url`) is refused before
  connecting (`TrackError`);
- no proxy (environment proxies such as `HTTPS_PROXY` are ignored);
- no bot token, no `Authorization`, no user agent and no Noctis header: the request
  carries only `host`, `accept` and a `range` when resuming.

Never fetch media with your own HTTP client to bypass these rules.

## 10. Passthrough and encoding

Noctis parses each current track's input before the driver plays it. Then:

- **Passthrough.** An Opus packet goes to the wire as it is (no decode, no encode)
  when the track is the call's only playing track, its volume is exactly `1.0` and
  the packet holds 20 ms (960 samples at 48 kHz). This is the normal music-bot case
  with Ogg Opus or WebM/Matroska Opus input. It is checked every 20 ms: after
  `set_volume(0.5)` the next frames are decoded, and passthrough resumes when the
  volume is back at `1.0`.
- **Fallback.** Everything else is decoded, resampled to 48 kHz, mixed (stereo,
  soft-clipped) and encoded with libopus: any other format (MP3, AAC, FLAC, WAV),
  any volume other than `1.0`, and Opus with 40 or 60 ms frames (Songbird passes the
  first three such packets through, then decodes the rest of the track).
- **Encoder bitrate.** The encoder follows the joined channel's bitrate: read from
  the Discord cache when the bot lands in the channel (resource `CHANNEL`, cached by
  default) and updated on that channel's `CHANNEL_UPDATE`. Without a cached channel it
  uses 64 kbps, Discord's default. `VoiceConfig::max_bitrate(Some(96_000))` caps it.
  Passthrough frames keep the bitrate they were encoded at.
- **Metrics.** `noctis_voice_passthrough_frames_total{app}` counts frames sent by
  passthrough; `noctis_voice_encoded_frames_total{app}` counts 20 ms frames decoded
  for encoding (one track per call, so the frames the call encoded). On a music bot
  that pre-encodes its library, the encoded counter barely moves. If it grows as fast
  as tracks play, check the input format, the volume and the frame size.

Pre-encode a library once for passthrough:

```sh
ffmpeg -i in.flac -c:a libopus -b:a 128k -ar 48000 -frame_duration 20 out.opus
```

Stream YouTube's Opus (WebM Opus at 48 kHz) without re-encoding:

```rust
let track = Track::command(
    "yt-dlp",
    ["-f", "bestaudio[acodec=opus]/bestaudio", "-o", "-", "--", url.as_str()],
)?;
```

## 11. Music-bot patterns

The workspace example `examples/music-bot` (noctis-rs repo) is a complete music bot
built with these patterns; `docs/guides/voice.md` walks through it.

**The author's channel.** Noctis keeps no voice-state cache. Ask Discord once per
`/play` ("Get User Voice State"); a 404 means the user is in no voice channel:

```rust
use noctis::twilight::http::error::ErrorType;

async fn author_channel(ctx: &CommandCtx, guild: Id<GuildMarker>, user: Id<UserMarker>)
    -> HandlerResult<Option<Id<ChannelMarker>>> {
    match ctx.http().user_voice_state(guild, user).await {
        Ok(response) => Ok(response.model().await?.channel_id),
        Err(e) if matches!(e.kind(), ErrorType::Response { status, .. } if status.get() == 404) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
```

`/play` then reuses the guild's call (`ctx.voice().call(guild)`). It refuses to move a
call that plays in another channel and otherwise joins with `ctx.voice().join(guild, channel)`.

**The input.** Treat a query as a link only when `Track::url(query)` accepts it
(http/https, no credentials, not Discord's API). Anything else is the search
`ytsearch1:<query>`. The target always follows `--`, so a query like `--exec rm` is
never parsed as an option:

```rust
let target = if Track::url(q).is_ok() { q.to_owned() } else { format!("ytsearch1:{q}") };
let track = Track::command("yt-dlp", ["-f", "bestaudio[acodec=opus]/bestaudio", "--no-playlist",
    "--playlist-items", "1", "--quiet", "-o", "-", "--", target.as_str()])?
    .title(q)                 // cut to 256 chars
    .requested_by(ctx.user_id());
```

**The player View.** `render_player(&PlayerState, page, &Limits) -> V2Message` is pure.
`PlayerState::of(Option<&CallHandle>)` copies `now_playing`, `is_paused`, `volume`
and `list` into plain fields, so a unit test builds a state without a call. The View
holds the title, the requester, `Playing`/`Paused · Volume N%`, a controls row and one
page of the queue. Page size comes from `Budget`, and the empty state reads "Nothing
is playing". It has four Routes. Each is `#[derive(Button)]`, keeps its page in the
Payload and answers by rendering again in place:

```rust
#[derive(Button, Debug, Clone, Copy)]
#[button("music.toggle", defaults(style = Secondary, label = "Pause"))]
pub struct TogglePause { pub page: u16 }        // also SkipTrack { page }, StopPlayback, QueuePage { page }

impl TogglePause {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if let Some(call) = ctx.guild_id().and_then(|g| ctx.voice().call(g)) {
            let _ = if call.is_paused() { call.resume() } else { call.pause() };
        }
        let call = ctx.guild_id().and_then(|g| ctx.voice().call(g));
        ctx.update(render_player(&PlayerState::of(call.as_ref()), self.page, ctx.limits())).await
    }
}
```

A stale player (the bot left) re-renders the empty state; never collect or listen for
clicks.

**The control commands.** `/pause`, `/resume`, `/skip`, `/stop`, `/volume percent`
(`min_value = 0, max_value = 200`, divided by 100 for `set_volume`) and `/leave`
declare `response = ephemeral` and answer with one `ctx.reply_text` line. They use
"I'm not in a voice channel." when the guild has no call. `/play` and `/queue` answer
with the public player.

**Idle leave.** `.voice(VoiceConfig::default().idle_leave(Some(Duration::from_mins(5))))`
leaves a call that stays alone or silent; log failed tracks with a `#[derive(Hook)]`
on `TrackError` (the queue has already moved on).

## 12. Testing voice

`VoiceConfig::recording()` (Noctis feature `testkit` together with `voice`) swaps the
Songbird driver for a recorder. No UDP, no audio, no child process. A join still sends
op 4 through the gateway and resolves on Discord's answers (`join_gateway`). The queue
runs for real, and every driver command is recorded per guild.

| Item | What it does |
| --- | --- |
| `VoiceConfig::recording()` / `.recorder()` | the recording config; `recorder()` is its `VoiceRecorder` (`None` for a normal config); every other builder method still applies |
| `VoiceRecorder::commands(guild)` | `Vec<RecordedCommand>` in order: `Play { track: TrackInfo, volume: f32 }`, `Pause`, `Resume`, `Stop`, `SetVolume(f32)` |
| `VoiceRecorder::events(guild)` | `Vec<RecordedEvent>` in order: `Start(TrackStart)`, `End(TrackEnd)`, `Error(TrackError)`, `Empty(QueueEmpty)` (recorded with or without a hook) |
| `VoiceRecorder::start(guild)` | reports the current track playable: `TrackStart` fires; returns the track |
| `VoiceRecorder::finish(guild)` | reports it played to the end: `TrackEnd` (`Finished`), then the next track's `Play` or `QueueEmpty` |
| `VoiceRecorder::fail(guild, error)` | reports it failed: `TrackError`, then the next track or `QueueEmpty` |
| `TestApp::answer_voice(true)` | the gateway fake answers each op 4 with the bot's `VOICE_STATE_UPDATE` and, for a join, a `VOICE_SERVER_UPDATE` (session `FAKE_VOICE_SESSION`, token `FAKE_VOICE_TOKEN`, endpoint `FAKE_VOICE_ENDPOINT`, in `noctis::testkit::fake`) |
| `TestApp::voice_state_updates()` | the `d` of every op 4 the App sent (`channel_id` is `null` for a leave) |
| `TestApp::dispatch_bot_voice_state(guild, Some(channel) / None)` / `dispatch_voice_server(guild)` | answers op 4 by hand (with `answer_voice(false)`), e.g. concurrently with the command |

The recorder types live in `noctis::testkit::voice`. Without any answer, a join ends in
`VoiceError::Timeout` after `join_timeout`. The helpers work without the `voice`
feature too: they only touch the gateway.

```rust
use noctis::testkit::fake::{FakeResponse, Method, StatusCode};
use noctis::testkit::voice::RecordedCommand;
use noctis::voice::{AppBuilderVoiceExt as _, VoiceConfig};

#[tokio::test(flavor = "multi_thread")]
async fn play_joins_and_plays() -> Result<(), TestError> {
    let config = VoiceConfig::recording();
    let recorder = config.recorder().expect("a recording config");
    let mut builder = App::builder();
    configure(&mut builder).voice(config);                 // the last `.voice(..)` wins
    let app = TestApp::build(&mut builder).await?
        .answer_voice(true)
        .guild(TestGuild::new(G, "Guild").channel(V, "Lounge", ChannelType::GuildVoice))
        .user(TestUser::new(A));
    let state = serde_json::json!({ "guild_id": G.to_string(), "channel_id": V.to_string(),
        "user_id": A.to_string(), "session_id": "s", "deaf": false, "mute": false,
        "self_deaf": false, "self_mute": false, "self_video": false, "suppress": false,
        "request_to_speak_timestamp": null });
    app.http_fake().route(Method::GET, format!("/guilds/{G}/voice-states/{A}"),
        FakeResponse::json(StatusCode::OK, &state));       // A is in V

    app.command("play").option("query", "lofi").by(A).in_guild(G).run().await?;

    let joined: Vec<_> = app.voice_state_updates().iter().map(|d| d["channel_id"].clone()).collect();
    assert_eq!(joined, [serde_json::json!(V.to_string())]);
    assert!(matches!(recorder.commands(G).as_slice(), [RecordedCommand::Play { .. }]));
    recorder.start(G);
    assert!(recorder.finish(G).is_some());                 // TrackEnd, then QueueEmpty
    app.stop().await
}
```
