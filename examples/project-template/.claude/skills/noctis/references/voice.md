# Voice

Join and leave voice channels from any handler. Noctis sends every Voice State Update
(op 4) through its own gateway and uses Songbird 0.6 only as the voice driver. Rule ids:
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
- Read the values back with `get_join_timeout()` and `is_self_deaf()`.

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

Audio playback is not part of the API yet. The call only holds the voice connection.

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
| `Interrupted { guild }` | `[VC-02]` A newer join, a leave, a disconnect or a `GUILD_DELETE` replaced the call, or `call.leave()` was called on a stale handle. |

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
- The gauge `noctis_voice_calls{app}` counts live and pending calls. It is published
  once the App runs.

## 6. Testing

`TestApp` runs on a fake gateway that has no voice server, so a `/join` there waits
for `join_timeout` and ends in `VoiceError::Timeout`: test the replies, not the
connection. A `/leave` sends op 4 with no channel and succeeds, and the intent check
is `app.intents().contains(Intents::GUILD_VOICE_STATES)` (the template's
`tests/voice.rs` does both).
