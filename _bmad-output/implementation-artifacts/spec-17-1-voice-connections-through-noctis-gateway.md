---
title: 'Story 17.1: Voice connections through Noctis''s gateway'
type: 'feature'
created: '2026-10-25'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-17-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Noctis cannot hold voice connections. Songbird's own manager and its `twilight` feature would add a second op 4 send path outside GW-04 and pin twilight at `<=0.17`, so voice needs its own integration (maintainer decision §10, FR-80, AD-30).

**Approach:** Add op 4 (`VoiceStateRequest`) and the READY user id to `noctis-gateway`'s command path. Add a core shutdown-task seam that runs before the shards close. Add a new layer-5 crate `noctis-voice` that keeps one `songbird::Call` per guild, built with `Shard::Generic` over that op 4 path. Its `VoicePlugin` feeds the bot's own `VOICE_STATE_UPDATE` and every `VOICE_SERVER_UPDATE` to the Calls. Expose it through the facade feature `voice`, the docs, the rules catalog, the template and the Usage Skill.

## Boundaries & Constraints

**Always:**
- `songbird = { version = "0.6.1", default-features = false, features = ["driver", "gateway", "rustls", "tws"] }`. This is the only Songbird line in the repo: a `[workspace.dependencies]` entry used only by `noctis-voice`. The Design Notes explain why `gateway` is on.
- `noctis-voice` depends on `noctis-core` and `noctis-gateway` among workspace crates, plus `songbird`, `tokio`, `tracing`, `metrics`, `thiserror`, `twilight-model` and `futures-util` as needed. It has no `twilight-gateway` dependency. Songbird types never appear in a public signature.
- Every op 4 goes through `GatewayHandle::update_voice_state`: owning shard, Shard Range check, GW-04 window (the same `SendWindow`), the same errors, checked in this order: `NotRunning`, `GuildNotInRange`, `ShardClosed`, `SendLimit`.
- The call map is `std::sync::Mutex<HashMap<Id<GuildMarker>, Entry>>`, carrying a `// keyed-map: one entry per guild with a live or pending voice connection (AD-30)` marker. The lock is never held across an `.await`. Each `Call` sits behind `Arc<tokio::sync::Mutex<Call>>`. An entry is removed on leave, a failed or interrupted join, a disconnect (own VSU with `channel_id: null`), `GUILD_DELETE` and shutdown.
- The gauge `noctis_voice_calls` (label `app` only) equals the map length after every change.
- The plugin's handlers are mounted in Module `noctis-voice` with `EventHandler::__new(.., ordered = true, droppable = false, ..)`.
- Every public item has rustdoc. The public API change re-blesses `xtask/public-api.txt` and updates the template, its `AGENTS.md`, the Usage Skill and its `API hash` line in the same commit.

**Never:**
- Songbird's `Songbird` manager, its `twilight`/`serenity`/`tungstenite`/`native` features, or any `songbird::id` `From<twilight Id>` impl. Convert ids with `Id::into_nonzero()`.
- Queues, tracks, inputs, codec tuning, `idle_leave`, `max_queue`, the `unstable` module or `VoiceConfig::recording()`. Those belong to Stories 17.2 to 17.4.
- A `noctis-testkit -> noctis-voice` edge. Any change to lanes or the drain.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| op4 join frame | `update_voice_state(VoiceStateRequest::join(g, c))`, g on shard 1 of 0..2 | exactly one frame `{"op":4,"d":{"guild_id":g,"channel_id":c,"self_mute":false,"self_deaf":false}}` on shard 1's inbox, none on shard 0; window +1 | — |
| op4 leave frame | `VoiceStateRequest::leave(g)` | `channel_id: null` | — |
| op4 flags | `.self_mute(true).self_deaf(true)` | both `true` in `d` | — |
| op4 budget edge | 109 earlier sends then op 4 | 110th admitted | — |
| op4 over budget | 110 sends then op 4 | nothing sent | `SendLimit{shard, retry_after>0}` |
| op4 mixed budget | 60 presence + 50 op 4 | the next op 8, op 3 or op 4 is refused | `SendLimit` (one window per shard) |
| op4 not running | detached handle or close requested | nothing sent | `NotRunning` |
| op4 out of range | guild owned by shard 3, range 0..2 | nothing sent | `GuildNotInRange{guild, shard:3, range}` |
| op4 closed shard | inbox receiver dropped | nothing sent | `ShardClosed{shard}` |
| current user | before READY / after READY `user.id=1` / detached | `None` / `Some(1)` / `None` | READY without a readable `d.user.id`: stays `None` and logs a warn without the payload |
| config bounds | `join_timeout` 0 s, 999 ms, 1 s, 60 s, 60 s + 1 ns | violation, violation, ok, ok, violation | `BuildViolation::plugin::<VoicePlugin>` naming the range `1 s..=60 s` |
| build | `App::builder().voice(VoiceConfig::default())` | `intents()` ⊇ `GUILD_VOICE_STATES`; Module `noctis-voice` mounted; `.voice(..)` twice: last config wins, one plugin | — |
| not installed | `ctx.voice()` without the plugin | `join`/`leave` fail; `call()` is `None`; `active_calls()` is 0 | `NotInstalled` |
| join before run | `app.voice().join(g,c)` | no map entry | `Gateway(NotRunning)` |
| join out of range | guild outside the Shard Range | no frame, no entry | `Gateway(GuildNotInRange{..})` |
| join send refused | op 4 refused (`SendLimit`/`ShardClosed`) | entry removed if it was created by this join | `Gateway(err)` (the original `GatewayError`, not `NoSender`) |
| join pending | op 4 sent, no answer | `active_calls()==1`; `call(g)` is `Some`, `channel()==Some(c)` | — |
| join timeout | no VSU/VSERVU within `join_timeout` (test: 1 s) | op 4 leave sent best-effort; entry removed | `Timeout{guild, after: join_timeout}` |
| join driver error | own VSU(`session`, c) + VSERVU(endpoint `127.0.0.1:9`), `join_timeout` 30 s | op 4 leave sent; entry removed | `Driver{guild, reason}` (reason = songbird error `Display`, no token) |
| join ok | the driver connects | `CallHandle{guild, channel}` | — (needs a real voice server; see Design Notes) |
| same channel rejoin | already connected to c, `join(g,c)` | no op 4; same `CallHandle` | — |
| VC-02 two joins | `join(g,a)` pending, then `join(g,b)` | one entry; frames: op 4 `a`, then op 4 `b` (Songbird's own leave first when in progress, so op 4 `null` may come between) | first join returns `Interrupted{guild}`; second stays pending |
| moved | pending or live, own VSU with `channel_id: b` | `update_state` called; entry kept; `call(g).channel()==Some(b)` | — |
| disconnected | own VSU with `channel_id: null` | entry removed; gauge −1 | pending join returns `Interrupted` |
| foreign VSU | VSU of another user, or guild without an entry | ignored | — |
| VSERVU null endpoint | `endpoint: null` | ignored (Discord sends another) | — |
| guild delete | `GUILD_DELETE` (not unavailable) for g with an entry | entry removed, driver stopped, no op 4 | pending join returns `Interrupted` |
| leave with call | `leave(g)` | entry removed; op 4 `null` | op 4 refused: entry still removed, `Gateway(err)` returned |
| leave no call | `leave(g)` with no entry | op 4 `null` still sent (clears a channel left over from before a restart) | `Gateway(err)` |
| stale handle | `CallHandle` whose entry was removed or replaced (generation differs) | `channel()` is `None`; `leave()` sends nothing | `leave()` returns `Interrupted{guild}` |
| shutdown | 2 entries, then shutdown | before `close_all`: op 4 `null` per guild (FakeGateway records them before the close frame); map empty; gauge 0 | no confirmation within the budget: task dropped, calls dropped, shutdown continues |
| shutdown grace 0 | `shutdown_grace(0)` | tasks are not run; map dropped with the App | — |

</intent-contract>

## Code Map

- `crates/noctis-gateway/src/command.rs` -- `Commands::{set_presence, send_member_request}` (L578-635): the pattern for the op 4 send (window lock, `is_closed`, `admit`, `record`, `tx.send`); `shard_for_guild` (L556); `GatewayHandle` (L640) with `running()`; `encode()`; `GatewayError` (L49, add no variant); `tests::fixture`/`sent`/`guild_on` (L870+) for unit tests.
- `crates/noctis-gateway/src/shard.rs` -- `classify` (L69-): READY detection at L86. `Classified` gets `ready_user: Option<Id<UserMarker>>`, parsed only for READY by a tiny serde struct (`d.user.id`). `drive` (L424-) stores it into the shard inbox's shared `Arc<AtomicU64>` (0 = unknown).
- `crates/noctis-gateway/src/lib.rs` -- re-export `VoiceStateRequest`.
- `crates/noctis-core/src/app.rs` -- `App::shut_down` (L1183): run shutdown tasks after `hooks.shutdown_started(reason)` and before `gateway.close_all`, under `watch.during(..)`. `AppBuilder::plugin`/`plugin_mut` (L2760-2800). `install_plugins` (L3255). `build` (L2953) moves the tasks into `App`.
- `crates/noctis-core/src/plugin.rs` -- `Plugin` docs list the builder methods `install` may use; add `shutdown_task`.
- `crates/noctis-core/src/handler.rs` (`EventCtx::state/gateway` L358/369), `command.rs` (L319/370), `component.rs` (`ctx_accessors!` L232/278: `ComponentCtx`, `ModalSubmitCtx`), `lifecycle.rs` (`HookCtx::app_id/state/gateway` L418-433; `LifecycleHook::__new` L491; payload `AppStarted`) -- what `VoiceExt` impls call.
- `crates/noctis-core/src/events.rs` -- `VoiceStateUpdate` (boxed; `.0` is a twilight `VoiceState`: `user_id`, `guild_id: Option`, `channel_id: Option`, `session_id`), `VoiceServerUpdate` (`endpoint: Option<String>`, `guild_id`, `token`) and `GuildDelete` (removals only; outages go to `UnavailableGuild`).
- `crates/noctis-prefix/src/lib.rs` (L180-200) -- the model for a plugin: `BuildViolation::plugin::<Self>`, `Module::new(..).__add(EventHandler::__new::<P,_,_>(..))`. `crates/noctis-image/src/plugin.rs` (L417-450) is the model for the sealed `AppBuilder…Ext` with `plugin_mut`.
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` (`prefix` module at L2697, prelude at L2508, features list at L607) -- add the feature `voice`, the module `noctis::voice` and the prelude entries.
- `xtask/src/deps_check.rs` -- `CRATES` (L196, becomes 13), `GRAPH` (L212), `GATED` (L236); module docs list the rules.
- `crates/noctis-rules/src/rule.rs` (L75) -- the `RuleId` table; its test compares the table with `docs/discord-rules.md`.
- `docs/discord-rules.md` §15 Gateway (L289) -- add §17 "Voice" after §16; GW-04's "How" names op 4.
- `crates/noctis-testkit/tests/presence_commands.rs` -- the e2e pattern: an App on `FakeGateway` plus `FakeDiscordHttp`, `RecordedFrame`, `ReadyData` (default `user_id` 1), `gw.dispatch(type, json)`.
- `deny.toml` -- licenses: Songbird brings `webpki-roots` (CDLA-Permissive-2.0, through reqwest `rustls-tls`). `multiple-versions = "warn"`, so duplicates such as tokio-websockets 0.11/0.13, socket2 0.5/0.6 and rand 0.9/0.10 do not fail the check.
- Songbird 0.6.1 (verified from the published source): `Call` and `shards::{Shard::Generic, VoiceUpdate}` are compiled only under `gateway`. `Call::from_config(guild, Shard, user, Config)`, `join(channel) -> JoinResult<Join>` (send stage, then `Join: Future<Output = JoinResult<()>>`; release the Call lock before awaiting it), `leave()`, `update_state(session_id, Option<channel>)` (a `None` channel = local leave; a different channel = move), `update_server(endpoint, token)`, `current_channel()`, `Deref<Target = Driver>`. `JoinError::{Dropped, NoSender, TimedOut, Driver(_)}`. `Config::gateway_timeout(Option<Duration>)`, default driver retry 5× with 0.25 s to 4 s backoff. `build.rs` refuses `driver` without exactly one of `tws`/`tungstenite` plus `rustls`/`native`. libopus is built with cmake, and cmake 3.28 is present here.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-gateway/src/command.rs`, `shard.rs`, `lib.rs` -- add `pub struct VoiceStateRequest { guild, channel: Option<Id<ChannelMarker>>, self_mute, self_deaf }` (`join(guild, channel)`, `leave(guild)`, `const fn self_mute(bool)`, `const fn self_deaf(bool)`, `guild()`, `channel()`; `Debug, Clone, Copy, PartialEq, Eq`). Add `GatewayHandle::update_voice_state(&self, VoiceStateRequest) -> Result<(), GatewayError>` (serialized with twilight `UpdateVoiceState::new(guild, channel, self_deaf, self_mute)`) and `GatewayHandle::current_user_id(&self) -> Option<Id<UserMarker>>`. Wire READY's user id through `Classified`/`ShardInbox`. Update the module and `GatewayHandle` docs (op 4 listed). Unit tests: every op4/current-user matrix row; the names carry `vc_01`.
- `crates/noctis-core/src/app.rs`, `plugin.rs` -- add `AppBuilder::shutdown_task<F, Fut>(&mut self, name: &'static str, task: F) -> &mut Self` where `F: Fn(&App) -> Fut + Send + Sync + 'static, Fut: Future<Output = ()> + Send + 'static`. In `shut_down`, run every task concurrently with budget `min(shutdown_grace, 5 s)` (`SHUTDOWN_TASK_BUDGET`) and skip them when the grace is zero. At the budget, drop the futures and log one `noctis::app` warning naming the unfinished tasks. Unit tests: the order (a task observes `gateway().presence().set(..)` succeeding, i.e. before `close_all`), the budget cut, grace 0.
- `crates/noctis-rules/src/rule.rs`, `docs/discord-rules.md` -- add `VC_01 = "VC-01"`, `VC_02 = "VC-02"` and catalog §17 "Voice":
  - **VC-01** "Voice State Update (op 4) is sent by the guild's owning shard (`(guild_id >> 22) % shard_count`) and counts against the GW-04 budget". Runtime; enforced by `GatewayHandle::update_voice_state`.
  - **VC-02** "One voice connection per guild per bot; joining another channel in the same guild moves it". Runtime; enforced by `noctis-voice`'s call map.

  Extend GW-04's "How" to name op 4. Unit tests `vc_01_*` (gateway) and `vc_02_*` (voice) satisfy `rules-catalog`.
- `Cargo.toml` (root) -- add the member `crates/noctis-voice`, the `noctis-voice` path dependency and `songbird` (the Always line).
- `crates/noctis-voice/{Cargo.toml,src/lib.rs,src/config.rs,src/plugin.rs,src/voice.rs,src/call.rs,src/shard.rs,src/error.rs}` -- the crate:
  - The manifest uses the workspace version, license and lints, `description` "Voice for Noctis. Internal; use `noctis` (feature `voice`).". Dev-dependencies: `noctis = { workspace = true, features = ["testkit", "voice"] }`, `noctis-testkit`, `serde_json`, `tokio`.
  - Public items: `VoiceConfig` (`#[non_exhaustive]`, `Default`: `join_timeout` 10 s, `self_deaf` true; setters and getters), `VoicePlugin::new(VoiceConfig)`, the sealed `AppBuilderVoiceExt::voice(&mut self, VoiceConfig) -> &mut Self`, and the sealed `VoiceExt::voice(&self) -> Voice` for `App`, `EventCtx`, `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx` and `HookCtx`.
  - `Voice` (`Clone`): `join(guild, channel) -> impl Future<Output = Result<CallHandle, VoiceError>> + Send + 'static`, `leave(guild)` (async), `call(guild) -> Option<CallHandle>`, `active_calls() -> usize`.
  - `CallHandle` (`Clone`; `guild()`, `channel()`, `leave()`).
  - `VoiceError` (`#[non_exhaustive]`, thiserror): `NotInstalled`, `NotReady`, `Gateway(GatewayError)` (message starts `[VC-01]`), `Timeout{guild, after}`, `Driver{guild, reason}`, `Interrupted{guild}` (message starts `[VC-02]`).
  - `install` reports config violations, mounts the VSU, VSERVU, `GuildDelete` and `AppStarted` handlers, and registers the shutdown task.
  - Tests: the build, config and not-installed rows as unit tests; the gateway-facing rows as e2e tests.
- `crates/noctis-voice/tests/voice_gateway.rs` -- e2e on `FakeGateway`/`FakeDiscordHttp` (2 shards, `ReadyData` user 1) for every join, leave, moved, disconnected, foreign, guild-delete, VC-02, stale-handle and shutdown row. Every wait is bounded (≤ 15 s). The join-ok row is not tested (Design Notes).
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` -- add `voice = ["dep:noctis-voice"]`, with a comment that it is off by default and needs a C compiler and cmake (libopus via `opus2`). Add `#[cfg(feature = "voice")] pub mod voice` (docs, re-exports of the public items above) and prelude entries `AppBuilderVoiceExt`, `VoiceExt`. Re-export `VoiceStateRequest` in `noctis::gateway`. Update the features list in the crate docs.
- `xtask/src/deps_check.rs` -- add `"noctis-voice"` to `CRATES`, the edges `("noctis-voice","noctis-core")` and `("noctis-voice","noctis-gateway")` to `GRAPH`, and `(FACADE, "noctis-voice", "voice")` to `GATED`. Add a rule `songbird-gated` (AD-30): only `noctis-voice` may depend on `songbird`, as a normal dependency. Document it in the module docs and add unit tests like the existing rule tests.
- `deny.toml` -- add `{ crate = "webpki-roots", allow = ["CDLA-Permissive-2.0"] }` to `exceptions`, with a comment: reached only through the `voice` feature (Songbird's reqwest `rustls-tls`); the root certificate data license.
- `.github/workflows-disabled/ci.yml` -- in `template-build`, add `cargo build --release --features voice`. Add a step that fails when `cargo tree -p noctis -e normal --prefix none` lists `songbird`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md`, `_bmad-output/planning-artifacts/maintainer-decisions.md` -- add the "Amended (2026-10-25, Story 17.1)" lines from the Design Notes to AD-30, AD-21 (shutdown tasks in the order) and AD-24 (the `install` method list, plus `GatewayHandle::current_user_id`), and to §10 (the Songbird feature list).
- `examples/project-template/{Cargo.toml,src/lib.rs,src/modules/voice/mod.rs,tests/voice.rs,AGENTS.md,README.md}` -- add the feature `voice = ["noctis/voice"]` and a `#[cfg(feature = "voice")]` Module with `/join channel` (`#[option(.., channel_types = [GuildVoice, GuildStageVoice])] channel: Id<ChannelMarker>`, ephemeral reply on success or `VoiceError`) and `/leave`. `configure` calls `.voice(VoiceConfig::default())` under the cfg. A cfg'd test checks the intents and the commands. `AGENTS.md` names the feature.
- `examples/project-template/.claude/skills/noctis/{SKILL.md,references/voice.md}` -- the new reference covers enabling `voice`, the C toolchain, `ctx.voice().join/leave`, `CallHandle`, `VoiceError` with VC-01/VC-02, shutdown behavior and `GatewayHandle::update_voice_state`/`current_user_id`. Add the reference-table row, a mention of `AppBuilder::shutdown_task` (in `app-and-sharding.md`), and the new `API hash`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- add `(decision)` lines for any further implementation decisions.

**Acceptance Criteria:**
- Given the workspace, when `cargo tree -p noctis -e normal --prefix none` runs, then no `songbird` line appears; with `--features voice`, `songbird v0.6.1` and no `aws-lc-rs` appear; `deps-check` and `cargo deny check` pass.
- Given `App::builder().voice(VoiceConfig::default()).build()`, when inspected, then `GUILD_VOICE_STATES` is in `intents()` and `noctis::voice::VoiceExt::voice(&app)` returns a usable `Voice`.
- Given a running App on the fakes, when a handler calls `ctx.voice().join(g, c)`, then the FakeGateway connection of g's owning shard records the op 4 frame and the map holds g until the matrix outcome removes it.
- Given an App with an entry for g, when the App shuts down, then FakeGateway records op 4 `null` for g before the close frame and `noctis_voice_calls` ends at 0.
- Given the gates, when run, then `rules-catalog` finds evidence for VC-01 and VC-02, `api-snapshot` matches the re-blessed file, `skill-check` passes, and the template builds and tests with and without `--features voice`.

## Spec Change Log

## Review Triage Log

## Design Notes

**Why Songbird's `gateway` feature is on (decision).** Verification of songbird 0.6.1 found that `Call`, `shards::Shard::Generic` and `VoiceUpdate` exist only under `gateway`, so AD-30's literal plan (driver only) cannot build. `gateway` adds only async-trait, dashmap, flume, parking_lot and tokio. It does not add twilight or serenity, and it does not create a send path, so both reasons behind §10 still hold: no twilight pin and no op 4 outside GW-04. The `Songbird` manager is compiled but never constructed or re-exported. `tws` plus `rustls` is the websocket/TLS pair: tokio-websockets uses its `ring` feature, and reqwest 0.12 `rustls-tls` uses ring. A scratch resolve showed no `aws-lc-rs`. Amendment line for AD-30 and §10: "Songbird features are `driver`, `gateway` (only for `Call`/`Shard::Generic`; its `Songbird` manager is never used), `rustls`, `tws`."

**Bot user id.** `Call::from_config` needs the bot's user id, and "own VSU" means `user_id == bot`. Core keeps no READY payload, and token decoding fails on test tokens. The gateway therefore records READY's `d.user.id` (one `AtomicU64`, the same for every shard) and exposes it as `GatewayHandle::current_user_id()`. `join` without it returns `NotReady`.

**Op 4 errors through Songbird.** `VoiceUpdate::update_voice_state` can only return `JoinResult`. The `Shard::Generic` adapter (`NoctisShard { gateway, last_error: Mutex<Option<GatewayError>> }`, one per Call) stores the `GatewayError` and returns `JoinError::NoSender`. `Voice` maps `NoSender` to `VoiceError::Gateway(taken error)`. The adapter's `gateway` is the App's handle, cloned from the context that created the Call; every clone follows attach and detach.

**Join and the map.** `join`:
1. checks for the plugin's state (`NotInstalled`);
2. resolves the bot user id (`Gateway(NotRunning)` when the gateway is not running, else `NotReady`);
3. checks `shard_for_guild` (no entry is created on error);
4. gets or creates the entry (generation counter; `Config::default().gateway_timeout(None)`);
5. locks the Call and runs `call.join(channel)` (the send stage);
6. unlocks and awaits `Join` under `tokio::time::timeout(join_timeout)`.

`Dropped` maps to `Interrupted`, `TimedOut` or the elapsed timeout to `Timeout`, and `Driver(e)` to `Driver{reason: e.to_string()}`. After `Timeout` or `Driver`: if the entry still has this join's generation and no newer join took over, send op 4 leave best-effort (log a failure at debug) and remove the entry. `CallHandle` stores `(guild, generation)` and looks the entry up for each operation, so it never keeps a Call alive. `Entry` mirrors the current channel in a plain field, set by `join` and by own VSUs (a move), so `CallHandle::channel()` is synchronous and never locks the Call. Songbird's `Call` defaults `self_deaf` to false and offers only `deafen()`, which sends an extra op 4. Instead, the `NoctisShard` adapter sends `self_deaf = VoiceConfig::self_deaf` (default `true`, so the e2e frames carry `"self_deaf":true`) and passes `self_mute` through.

**Join success is not tested end to end.** Success needs a real Discord voice server (UDP plus a DAVE handshake). It is exercised manually and through Story 17.4's recording driver. 17.1 tests every path up to the driver, and the driver-error path proves that `update_state`/`update_server` reached the Call (Songbird calls `raw_connect` only once both arrive).

**Shutdown seam (decision).** AD-21 drops queued commands: `close_all` makes every command `NotRunning`, and the shard loop's `biased` select prefers close. AD-30's "leave every channel before the shards close" therefore needs a phase between `ShutdownStarted` and `close_all`. Hooks cannot do it because they run asynchronously. The voice shutdown task:
1. takes every entry out of the map (gauge 0);
2. for each guild, registers a confirmation waiter and calls `call.leave()` (op 4 `null`);
3. awaits the own-VSU `null` confirmations, so the frames are known to be on the wire before close.

If core drops the task at the budget, its drop guard drops the remaining Calls; that is the "`cancel` drops the calls" step. Amendment line for AD-21: "Order: fire `ShutdownStarted`; run Shutdown Tasks (`AppBuilder::shutdown_task`, concurrent, at most `min(shutdown_grace, 5 s)`, skipped at grace 0); `close_all`; …".

**App label binding.** `VoiceShared.app: OnceLock<Arc<str>>` is set by the `AppStarted` hook (`HookCtx::app_id`) and by `VoiceExt for App`. Gauge writes before binding are skipped; binding writes the current length.

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-gateway` and `cargo test --release -p noctis-voice` -- pass (vc_01 and vc_02 tests included)
- `cargo test --release -p noctis-core` -- pass (shutdown-task tests)
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo tree -p noctis -e normal --prefix none | grep -c '^songbird '` -- prints 0; `cargo tree -p noctis --features voice -e normal -i aws-lc-rs` -- no match
- `cargo run --release -p xtask -- deps-check`, `rules-catalog`, `tw-grammar --check`, `skill-check` -- ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot` -- ok; the printed hash goes into `SKILL.md`'s `API hash` line
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- In `examples/project-template`: `cargo build --release`, `cargo build --release --features voice`, `cargo test --release --features voice` -- ok
- `cargo hack` is run by the gate script only, never by the implementer.
