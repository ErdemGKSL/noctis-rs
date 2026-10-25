# Voice and music bots

Noctis can join voice channels and play audio (feature `voice`, FR-80). Every Voice
State Update (op 4) goes through Noctis's own gateway, and Songbird 0.6 is only the
voice driver (UDP, RTP, encryption). No Songbird type is part of the API. This guide
covers setup, joining and leaving, the queue, track events, the cheap Opus path,
running on a Cluster, the limits, testing, and the `examples/music-bot` bot that puts
it all together.

## 1. Enabling voice

The `voice` feature is off by default. It builds libopus from source, so the build
machine (and the CI or Docker build image) needs a C compiler and `cmake`.

```toml
[dependencies]
noctis = { version = "0.1", features = ["voice"] }
```

```rust
use noctis::prelude::*; // brings AppBuilderVoiceExt and VoiceExt
use noctis::voice::VoiceConfig;

builder
    .voice(VoiceConfig::default()) // installs the VoicePlugin, adds GUILD_VOICE_STATES
    .module(music::module());
```

`.voice(..)` installs the one `VoicePlugin` (the last call wins). It adds the
`GUILD_VOICE_STATES` intent (not privileged) and the internal Module `noctis-voice`,
which handles the bot's own `VOICE_STATE_UPDATE`, every `VOICE_SERVER_UPDATE`,
`CHANNEL_UPDATE` and `GUILD_DELETE`.

`VoiceConfig` is `#[non_exhaustive]`: start from `VoiceConfig::default()` and chain
the setters. A value out of its range makes `build()` fail with a `BuildViolation`
that names the range.

| Setting | Range | Default | What it does |
| --- | --- | --- | --- |
| `join_timeout(Duration)` | 1 s..=60 s | 10 s | How long `join` waits for Discord and the driver |
| `self_deaf(bool)` | — | `true` | Join deafened (the bot receives no audio) |
| `max_queue(usize)` | 1..=10000 | 1000 | Most tracks a guild's queue holds, the current one included |
| `idle_leave(Option<Duration>)` | `None` or 1 s..=24 h | `None` | Leave a connected call that stayed idle this long (§6) |
| `media_connect_timeout(Duration)` | 1 s..=60 s | 10 s | Connect timeout of a URL track's fetch |
| `media_max_redirects(u8)` | 0..=10 | 5 | Redirects a URL track's fetch follows |
| `max_bitrate(Option<u32>)` | `None` or 8000..=384000 | `None` | Caps the Opus encoder's bitrate, in bits per second (§5) |

Read them back with `get_join_timeout()`, `is_self_deaf()`, `get_max_queue()`,
`get_idle_leave()`, `get_media_connect_timeout()`, `get_media_max_redirects()` and
`get_max_bitrate()`.

## 2. Joining and leaving

`ctx.voice()` works on every handler context (and `app.voice()` on the `App`). It
returns the App's `Voice`, a cheap clone.

| Call | What it does |
| --- | --- |
| `voice.join(guild, channel).await` | Sends op 4 from the guild's owning shard and resolves to a `CallHandle` once Discord answered and the driver connected, within `join_timeout`. The future is `Send + 'static`. |
| `voice.leave(guild).await` | Removes the call and sends op 4 with no channel (also without a call, which clears a channel left over from a restart). |
| `voice.call(guild)` | The guild's live or pending `CallHandle`, or `None`. |
| `voice.active_calls()` | The number of live and pending calls. |
| `call.channel()`, `call.leave().await` | The call's channel (`None` once the handle is stale); leave through the handle. |

- **VC-01.** Every op 4 goes through `GatewayHandle::update_voice_state`: the guild's
  owning shard, the Shard Range check and the GW-04 budget. A refusal is
  `VoiceError::Gateway` and nothing is queued.
- **VC-02.** One call per guild. Joining another channel of the guild moves the call;
  joining the channel it is connected to sends nothing. A join replaced by a newer
  one, a leave, a disconnect or a `GUILD_DELETE` is `VoiceError::Interrupted`, and a
  `CallHandle` of a removed call is stale.
- **Errors.** `Timeout` and `Driver` leave and remove the call; `NotReady` before the
  first READY; `NotInstalled` without `.voice(..)`.

**The author's channel.** Noctis keeps no cache of members' voice states. To join the
channel of the user who ran a command, ask Discord with "Get User Voice State": one
request per command, correct on every Cluster process. A 404 means the user is in no
voice channel.

```rust
use noctis::twilight::http::error::ErrorType;

async fn author_channel(
    ctx: &CommandCtx,
    guild: Id<GuildMarker>,
) -> HandlerResult<Option<Id<ChannelMarker>>> {
    match ctx.http().user_voice_state(guild, ctx.user_id()).await {
        Ok(response) => Ok(response.model().await?.channel_id),
        Err(error) if matches!(error.kind(), ErrorType::Response { status, .. } if status.get() == 404) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
```

## 3. Tracks and the queue

A `Track` describes audio. Nothing is opened, fetched or spawned until it becomes
the queue's current track; then the driver holds only that one track.

| Constructor | Source | Refused |
| --- | --- | --- |
| `Track::file(path)` | a local file | an empty path (a missing file fails at play time) |
| `Track::url(url)` | an `http`/`https` media URL, fetched with the App's media client | malformed URLs, more than 4096 bytes, other schemes, credentials, Discord's API |
| `Track::command(program, args)` | a child process's stdout (no shell; killed when the track ends) | an empty program |

`.title(..)` (at most 256 chars, cut at a char boundary) and `.requested_by(user)`
add metadata. The media client fetches any public or private host: when users supply
URLs, allow-list hosts before `Track::url`. Its rules: connect timeout, redirect cap,
no proxy, no bot token, user agent or Noctis header, and no redirect to Discord's API.

Each call has one queue on its `CallHandle`, holding at most `max_queue` tracks:

| Method | What it does |
| --- | --- |
| `enqueue(track).await` | Appends; plays at once when nothing is current. `QueueFull` when full |
| `skip()` | Ends the current track (`Skipped`); the next one plays or the queue empties |
| `pause()` / `resume()` | `true` when the state changed |
| `stop()` | Ends the current track (`Stopped`) and drops the pending ones; returns how many tracks the queue held |
| `clear()` | Drops the pending tracks only |
| `set_volume(v)` | `0.0..=2.0` (default `1.0`), for the current and every later track |
| `now_playing()`, `is_paused()`, `list()`, `queue_len()`, `volume()` | Read the queue |

## 4. Track events

The queue reports through four hook events. They are `ExtensionEvent`s, so a
`#[derive(Hook)]` handler takes them; they go through the ordered hook queue, never
the gateway lanes, and are dropped when no hook takes them.

| Event (`NAME`) | Fires | Data |
| --- | --- | --- |
| `TrackStart` (`VOICE_TRACK_START`) | a track became current and is playable | `guild`, `track` |
| `TrackEnd` (`VOICE_TRACK_END`) | the current track finished, was skipped or stopped | `guild`, `track`, `reason` (`EndReason`) |
| `TrackError` (`VOICE_TRACK_ERROR`) | the current track failed (create, parse, decode); the next one starts | `guild`, `track`, `error` |
| `QueueEmpty` (`VOICE_QUEUE_EMPTY`) | the queue ran out of tracks | `guild` |

Every track that became current yields exactly one `TrackEnd` or `TrackError`. A
track error never panics and never stops the queue. A leave, a disconnect, a
`GUILD_DELETE` or the shutdown drops the queue without events.

```rust
#[derive(Hook)]
pub struct TrackErrorLog;

impl TrackErrorLog {
    #[handler]
    async fn handle(self, _ctx: HookCtx, event: TrackError) -> HandlerResult<()> {
        tracing::warn!(guild = %event.guild, error = %event.error, "track failed");
        Ok(())
    }
}
```

## 5. Passthrough and pre-encoding

Every 20 ms the driver builds one Opus packet per call. Noctis plays one track per
call at a time and parses that track's input itself before the driver gets it.

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

### The encoder bitrate

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

### Checking it: the frame metrics

Two counters, labelled `app` only:

| Metric | Counts |
| --- | --- |
| `noctis_voice_passthrough_frames_total` | 20 ms Opus frames sent by passthrough |
| `noctis_voice_encoded_frames_total` | 20 ms frames decoded for mixing and encoding (one track per call, so the frames the call encoded) |

A bot whose library is pre-encoded should see the passthrough counter grow at 50 per
second per playing call and the encoded counter stay nearly flat (it moves while the
volume is not `1.0`). If the encoded counter grows as fast, check the input format,
the volume and the Opus frame size (`ffprobe -show_packets` shows `duration_time`
`0.020000` for 20 ms frames). Two more metrics: the gauge `noctis_voice_calls{app}`
and the counter `noctis_voice_tracks_total{app, result}`.

### Pre-encoding a library

Encode each file once to Ogg Opus at 48 kHz with 20 ms frames:

```sh
ffmpeg -i in.flac -c:a libopus -b:a 128k -ar 48000 -frame_duration 20 out.opus
```

`-frame_duration 20` is libopus's default, but stating it keeps a later change from
silently moving the library to 40/60 ms frames, which are decoded. Play the result
with `Track::file("out.opus")` or from a URL with `Track::url(..)`.

### Streaming with yt-dlp

YouTube serves Opus audio as WebM Opus at 48 kHz, which plays by passthrough. Ask
`yt-dlp` for it and fall back to the best audio otherwise:

```rust
use noctis::voice::Track;

let track = Track::command(
    "yt-dlp",
    ["-f", "bestaudio[acodec=opus]/bestaudio", "--no-playlist", "-o", "-", "--", target.as_str()],
)?;
```

Keep the `--` before the user's input: the program would otherwise parse a value such
as `--exec=...` as an option. Pass a link only when `Track::url` accepts it and turn
anything else into a search (`ytsearch1:<query>`), so `file://` or a credentialed URL
is never fetched as a link. `yt-dlp` still fetches any host it is given: a public bot
should allow-list the sites it plays from.

## 6. Idle leave

`VoiceConfig::idle_leave(Some(d))` leaves a connected call that had no playing track
(nothing current, or the current one paused) for `d`, exactly like `call.leave()`:
op 4 with no channel and the call removed. A track starting or resuming cancels the
timer; it re-arms at the next idle moment. A pending call never arms it. The default
`None` never leaves.

## 7. Cluster notes

- A guild's call lives in the process whose Shard Range owns the guild's shard: op 4
  goes out on that shard, and Discord's answers come back on it.
- `INTERACTION_CREATE` for a guild arrives on the guild's shard too, so a `/play`
  handler runs in the process that owns the guild's call.
- `voice().call(guild)` in any other process is `None`, and `join` there is
  `VoiceError::Gateway(GuildNotInRange)`.
- The shutdown leave is per process: each process leaves the calls it owns before its
  shards close (at most `min(shutdown_grace, 5 s)`).
- "Get User Voice State" (§2) is a REST call, so the author's channel lookup works in
  every process without a shared cache.

## 8. Limits

| Limit | Value | Where |
| --- | --- | --- |
| VC-01: op 4 budget | 110 gateway commands per shard per 60 s (GW-04), shared with presence updates and Request Guild Members | `VoiceError::Gateway(SendLimit)` |
| VC-02: calls per guild | one; another channel moves the call | `VoiceError::Interrupted` for the replaced join |
| `max_queue` | 1..=10000 (default 1000) | `VoiceError::QueueFull` |
| Volume | 0.0..=2.0 | `VoiceError::InvalidVolume` |
| Track title | 256 chars (longer titles are cut) | `Track::title` |
| Media URL | 4096 bytes | `VoiceError::InvalidInput` |
| `join_timeout` | 1 s..=60 s | build violation |
| `idle_leave` | 1 s..=24 h | build violation |
| `media_connect_timeout` | 1 s..=60 s | build violation |
| `media_max_redirects` | 0..=10 | build violation |
| `max_bitrate` | 8000..=384000 bit/s | build violation |

VC-01 and VC-02 are Discord rules (`docs/discord-rules.md`); the others are Noctis's
own config ranges.

## 9. Testing voice

A real join resolves only after the voice driver connected over UDP, which no test
can do. With the facade's `testkit` feature (in `[dev-dependencies]`) next to
`voice`, `VoiceConfig::recording()` swaps in a recording driver:

- a join still sends op 4 through the gateway (owning shard, budget) and resolves on
  Discord's `VOICE_STATE_UPDATE` and `VOICE_SERVER_UPDATE`, without the driver;
- every queue plays on the config's `VoiceRecorder` (`config.recorder()`), which
  records each `RecordedCommand` (`Play { track, volume }`, `Pause`, `Resume`,
  `Stop`, `SetVolume`) and `RecordedEvent` (`Start`, `End`, `Error`, `Empty`) per
  guild, and never opens a file, fetches a URL or spawns a process;
- nothing plays by itself: `recorder.start(guild)` reports the current track
  playable (`TrackStart`), `finish(guild)` ends it (`TrackEnd` `Finished`, then the
  next track or `QueueEmpty`) and `fail(guild, "boom")` fails it (`TrackError`).

`TestApp` speaks the gateway side: `answer_voice(true)` makes the fake gateway answer
each op 4 like Discord; `voice_state_updates()` returns the `d` of every op 4 the App
sent; `dispatch_bot_voice_state(guild, Some(channel))` and
`dispatch_voice_server(guild)` send the answers by hand (for a test that checks a
pending join). Register the author's voice state with `http_fake().route(..)`.

```rust
let config = VoiceConfig::recording();
let recorder = config.recorder().expect("a recording config");
let mut builder = App::builder();
music_bot::configure(&mut builder).voice(config); // the last .voice(..) wins
let app = TestApp::build(&mut builder).await?.answer_voice(true)
    .guild(TestGuild::new(G, "G").channel(V, "Lounge", ChannelType::GuildVoice))
    .user(TestUser::new(A));
app.http_fake().route(Method::GET, "/guilds/10/voice-states/40",
    FakeResponse::json(StatusCode::OK, &voice_state(A, V)));
let reply = app.command("play").option("query", "never gonna").by(A).in_guild(G).run().await?;
assert_eq!(app.voice_state_updates()[0]["channel_id"], "50");
assert!(matches!(recorder.commands(G)[..], [RecordedCommand::Play { .. }]));
recorder.start(G); // TrackStart
recorder.finish(G); // TrackEnd (Finished), then QueueEmpty
```

Use one recording config per App: two Apps built from the same config record into
the same guild entries.

## 10. The music-bot example

`examples/music-bot` is a complete music bot:

- `/play query` joins the author's voice channel (§2) when the bot is in none,
  refuses to move a call someone is listening to ("I'm already playing in #X"), and
  queues a `yt-dlp` track: a link `Track::url` accepts, else a `ytsearch1:` search;
- the public now-playing View: one pure `render_player(state, page, limits)` with
  Routed Pause/Resume, Skip and Stop buttons and a paginated queue (the page is a
  Payload field; pages are sized with `Budget`), updated in place with `ctx.update`;
- `/queue`, `/pause`, `/resume`, `/skip`, `/stop`, `/volume percent` (0-200) and
  `/leave` with one private line each; a call idle for 5 minutes is left;
- `tests/music.rs` runs every command and button through `TestApp` with the
  recording driver.

Run it with `yt-dlp` on `PATH`:

```sh
NOCTIS_TOKEN=... cargo run --release -p music-bot
```
