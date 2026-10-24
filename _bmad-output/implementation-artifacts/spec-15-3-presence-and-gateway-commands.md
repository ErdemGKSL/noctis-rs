---
title: 'Story 15.3: Presence and gateway commands'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
baseline_revision: 'c7bc02b36f584d72821538439cb5ce37fb0a63cb'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-12-3-resumable-shutdown.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** A Noctis bot cannot show a status: shards identify without a presence, nothing can update it at run time, there is no way to send a gateway command (Request Guild Members) to the shard that owns a guild, and `large_threshold` is not configurable. GW-04, GW-06 and GW-07 are still pending in `xtask/rules-pending.toml` (story 15.3).

**Approach:** `noctis-gateway` gains the presence and command types (`Presence`, `Activity`, `Status`, `LargeThreshold`, `MemberRequest`, `GuildMembers`, `GatewayError`) and a cloneable `GatewayHandle` that routes each command to the owning shard task over a Noctis-owned per-shard channel, enforces a per-shard send budget (GW-04) and correlates Request Guild Members answers (`GUILD_MEMBERS_CHUNK`, `RATE_LIMITED`) by nonce (GW-07). Core wires `AppBuilder::presence`/`large_threshold` into every shard's identify, exposes the handle as `App::gateway()`/`App::presence()` and `ctx.gateway()`, and the facade re-exports everything as `noctis::gateway`. FR-53, AD-21; GW-04, GW-06, GW-07.

## Boundaries & Constraints

**Always:**
- **No new cargo feature and no new dependency.** Everything compiles always (decision D1). Facade module `noctis::gateway` re-exports through `noctis_core::gateway` (core re-exports the `noctis_gateway` items; the facade gains no `noctis-gateway` edge).
- **Types (new file `crates/noctis-gateway/src/command.rs`, plus `crates/noctis-gateway/src/presence.rs`), all re-exported from `noctis_gateway`:**
  - `Status { Online, Idle, DoNotDisturb, Invisible }` (`#[non_exhaustive]`, `Copy`, `Default = Online`); maps to twilight `Status` (`online`, `idle`, `dnd`, `invisible`).
  - `Activity`: private fields `kind`, `name: String`, `state: Option<String>`, `url: Option<String>`; constructors `playing(name)`, `listening(name)`, `watching(name)`, `competing(name)`, `streaming(name, url)`, `custom(state)` (wire: type 4, `name = "Custom Status"`, `state`), each `impl Into<String>`; setter `.state(text)`. Only `name`, `state`, `type`, `url` are ever serialized (Discord: bots may set only those). No length validation (Discord documents none; decision D5).
  - `Presence`: private `status`, `activity: Option<Activity>`, `afk: bool`; `Presence::new(Status)`, shortcuts `Presence::online()`, `idle()`, `dnd()`, `invisible()`, `playing(name)`, `listening(name)`, `watching(name)`, `competing(name)`, `streaming(name, url)`, `custom(state)` (status `Online` plus that activity); builder setters `.status(Status)`, `.activity(Activity)`, `.afk(bool)`. Converts (crate-internal) to twilight `UpdatePresencePayload` built by struct literal (`activities` with 0 or 1 entry, `since: None`), never through `UpdatePresencePayload::new` (it rejects an empty list).
  - `LargeThreshold`: `Copy`, `Default` = 50; `pub const fn new<const N: u16>() -> Self where Threshold<N>: ValidLargeThreshold`; `pub const fn get(self) -> u16`. `ValidLargeThreshold` is a sealed trait implemented (via a `macro_rules!` over literal values) for exactly `Threshold<50>` ..= `Threshold<250>`, annotated `#[diagnostic::on_unimplemented(message = "[GW-06] `LargeThreshold::new::<N>()` takes 50..=250 (Discord's `large_threshold` range)", label = "outside 50..=250")]`. `Threshold`, `ValidLargeThreshold`, `UserIdCount`, `UserIds` live in `#[doc(hidden)] pub mod __bounds`.
  - `MemberRequest` (one `guild_id` by construction): `users<const N: usize>(guild, ids: [Id<UserMarker>; N]) -> Self where UserIds<N>: UserIdCount` (sealed trait implemented for `UserIds<1>` ..= `UserIds<100>`, `on_unimplemented` message starting `[GW-07]`); `user_list(guild, ids: impl IntoIterator<Item = Id<UserMarker>>) -> Result<Self, GatewayError>` (0 ids → `NoUserIds`, >100 → `TooManyUserIds { len }`; duplicates are kept as given); `prefix(guild, query: impl Into<String>, limit: u16) -> Result<Self, GatewayError>` (empty query → `EmptyQuery`; limit outside 1..=100 → `InvalidLimit { limit }`; decision D6); `all(guild) -> Self` (`query: ""`, `limit: 0`); `.presences(bool)` (default false). `guild()` accessor.
  - `GuildMembers { members: Vec<Member>, presences: Vec<twilight_model::gateway::presence::Presence>, not_found: Vec<Id<UserMarker>> }` (pub fields, `#[non_exhaustive]`), members in chunk-index order.
  - `GatewayError` (`thiserror`, `#[non_exhaustive]`, `Clone`, `PartialEq`): `NotRunning`; `ShardNotInRange { shard, range }`; `GuildNotInRange { guild, shard, range }`; `ShardClosed { shard }`; `SendLimit { shard, retry_after: Duration }` (message starts `[GW-04]`); `RateLimited { guild, retry_after: Duration }` (`[GW-07]`); `NoUserIds`, `TooManyUserIds { len }`, `EmptyQuery`, `InvalidLimit { limit }` (`[GW-07]`); `MissingIntent { intent: &'static str }` (`[GW-07]`, `"GUILD_MEMBERS"` or `"GUILD_PRESENCES"`); `Timeout { guild }`. Messages never contain the token, nonce or payloads.
- **Owning shard:** `shard = (guild_id >> 22) % total` (Discord's formula); outside `GatewayOptions::shards` → `GuildNotInRange`, no frame sent.
- **`GatewayHandle`** (noctis-gateway, `Clone`, `Debug`, `Send + Sync`): `GatewayHandle::detached()` (every call → `NotRunning`), `Gateway::handle(&self) -> GatewayHandle` (live). Core keeps one per App (`AppShared::gateway: GatewayHandle` whose inner `Arc<RwLock<Option<Arc<Commands>>>>` is attached by `run_with` right after `Gateway::start` and detached after `gateway.closed()` in `shut_down`; a CLI publish mode never attaches). Methods: `shard_for_guild(guild) -> Result<u32, GatewayError>`; `presence() -> PresenceHandle`; `request_guild_members(req) -> impl Future<Output = Result<GuildMembers, GatewayError>> + Send` (cancel-safe: dropping it removes its pending entry). `PresenceHandle` (`Clone`): `set(Presence) -> Result<(), GatewayError>` (every shard of this process; budget checked for all shards first, all-or-none: one shard over budget → `SendLimit` for the lowest such shard id and nothing sent) and `set_shard(shard: u32, Presence) -> Result<(), GatewayError>` (outside range → `ShardNotInRange`). Both are sync and never block.
- **Shard command path:** `Gateway::start` creates one `tokio::sync::mpsc::unbounded_channel::<String>()` per shard; the task's `drive` loop adds a `select!` branch (after the close branch, before `shard.next()`, `biased`) that calls `shard.send(json)`; twilight queues it until identified and paces it with its own limiter. A send whose receiver is gone (task ended) → `ShardClosed`. Commands requested before the shard connects wait in the channel.
- **GW-04 budget:** per shard, a sliding window of `tokio::time::Instant`s (`Mutex<VecDeque<Instant>>`, never held across `.await`) admits at most **110** user commands per 60 s (`SEND_BUDGET`, 120 minus twilight's maximum heartbeat reservation of 10; decision D2); the 111th → `SendLimit { shard, retry_after = oldest + 60 s − now }` and nothing is queued. Presence sets and member requests both count; Noctis's own presence re-send (below) does not.
- **Presence persistence:** the initial presence and `large_threshold` go into the twilight `Config` (`ConfigBuilder::from(config).presence(..).large_threshold(get())`) in `Gateway::start`, so every IDENTIFY carries them (twilight keeps them across resumes). The last presence set at run time per shard is remembered by its task; after a new session's READY (not RESUMED) the task re-sends it (op 3) so a re-identify never reverts to the initial presence.
- **Request Guild Members (op 8):** checks in order: request validity (already guaranteed by constructors), `all()` without `Intents::GUILD_MEMBERS` or `.presences(true)` without `Intents::GUILD_PRESENCES` → `MissingIntent` (decision D7), owning shard in range, budget. Noctis always sets the nonce: `format!("{seed:08x}{counter:x}")` (≤ 24 bytes < Discord's 32; `seed` = low 32 bits of `SystemTime` nanos at `Gateway::start`, `counter` an `AtomicU64`). The pending map (`Mutex<HashMap<String, mpsc::UnboundedSender<Answer>>>`) is shared by all shard tasks. Shard tasks parse with `wanted | GUILD_MEMBERS_CHUNK | RATE_LIMITED`; a chunk or a `RATE_LIMITED` (opcode 8, meta `RequestGuildMembers`) whose nonce is pending is forwarded to its waiter; an event goes to the sink only when `lane.wanted` contains its flag (cloned when both). A `RATE_LIMITED` with an unknown nonce is logged at `warn` (guild id and `retry_after` only). The waiter completes when it holds chunk indices `0..chunk_count`; it fails with `RateLimited` (`retry_after` from the event's seconds, `Duration::from_secs_f32`, negative/NaN → zero) or `Timeout` when no answer arrives for `MEMBER_CHUNK_TIMEOUT` = 10 s since the request or the last chunk (decision D4).
- **Builder/App/ctx surface (noctis-core):** `AppBuilder::presence(&mut self, Presence) -> &mut Self` and `AppBuilder::large_threshold(&mut self, LargeThreshold) -> &mut Self` (last call wins; `Debug` shows both); `App::gateway(&self) -> GatewayHandle`, `App::presence(&self) -> PresenceHandle`; `gateway(&self) -> GatewayHandle` on `EventCtx`, `CommandCtx` and the component/modal ctx macro in `component.rs` (detached seams return `GatewayHandle::detached()`). Not on `AutocompleteCtx`. `GatewayOptions` gains `pub presence: Option<Presence>` and `pub large_threshold: LargeThreshold`.

**Never:**
- No silent drop: every refused command is a typed `Err`, nothing is enqueued after an error.
- No twilight `MessageSender` or raw JSON send in the public API; no user-supplied nonce; no polling.
- No presence/command code in `noctis-prefix`, no change to Shard Range/session/close semantics of Stories 12.1–12.3, no change to the event lane drop policy.
- No debug builds; no new cargo feature; no hand-edit of `_bmad/`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior |
|---|----------|--------------|---------------------------|
| P1 | Initial presence | `.presence(Presence::watching("servers"))`, 2 shards | both IDENTIFY frames carry `d.presence = {status:"online", afk:false, since:null, activities:[{type:3,name:"servers"}]}` |
| P2 | No presence set | default builder | IDENTIFY carries no presence (`presence` absent or `null`, whatever twilight sends for `None`) |
| P3 | Status only | `Presence::dnd()` | `activities: []`, `status: "dnd"` |
| P4 | Custom status | `Presence::custom("☕ brewing — 𝔲𝔫𝔦𝔠𝔬𝔡𝔢")` | `{type:4,name:"Custom Status",state:"☕ brewing — 𝔲𝔫𝔦𝔠𝔬𝔡𝔢"}` byte-exact UTF-8 |
| P5 | Streaming | `Activity::streaming("live", "https://twitch.tv/x")` | `{type:1,name:"live",url:"https://twitch.tv/x"}` |
| P6 | Empty name | `Activity::playing("")` | sent as given (`name:""`); no client check (D5) |
| P7 | Runtime set, all | running shards 0..2, `app.presence().set(p)` | `Ok(())`; one op 3 per shard with `d` = p |
| P8 | Runtime set, one | `set_shard(1, p)` | `Ok`; op 3 on shard 1 only |
| P9 | Shard out of range | range 2..4, `set_shard(1, p)` / `set_shard(4, p)` / `set_shard(u32::MAX, p)` | `Err(ShardNotInRange { shard, range: 2..4 })`; no frame |
| P10 | Not running | before `run` / detached ctx / after `run_until` returned | `Err(NotRunning)` |
| P11 | Re-identify keeps runtime presence | `set(p2)`, then server invalidates (op 9 false) → new READY | after READY the shard sends op 3 with p2 |
| P12 | Resume | shard RESUMED | no extra op 3 |
| L1 | `LargeThreshold::new::<50>()`, `<250>()` | — | compiles; `get()` = 50 / 250 |
| L2 | `<49>()`, `<251>()`, `<0>()`, `<u16::MAX>()` | — | compile error containing `[GW-06]` |
| L3 | `.large_threshold(LargeThreshold::new::<100>())` | — | IDENTIFY `d.large_threshold = 100`; default 50 |
| M1 | Users | `MemberRequest::users(g, [u1])` | op 8 `{guild_id:g, user_ids:[u1], nonce}` (always an array: built with twilight's `.user_ids(vec)`); answer chunk → `Ok(GuildMembers)` with that member |
| M2 | 100 users | `users(g, [..; 100])` | compiles; `user_list` with 100 → `Ok` |
| M3 | 101 users / 0 users | `users(g, [..; 101])`, `users(g, [])` | compile error containing `[GW-07]` |
| M4 | Runtime list bounds | `user_list(g, [])` / 101 ids | `Err(NoUserIds)` / `Err(TooManyUserIds { len: 101 })` |
| M5 | Prefix | `prefix(g, "ab", 10)` | op 8 `{query:"ab", limit:10}` |
| M6 | Prefix bounds | `prefix(g, "", 5)` / `prefix(g, "a", 0)` / `prefix(g, "a", 101)` / `prefix(g, "ç", 100)` | `EmptyQuery` / `InvalidLimit{0}` / `InvalidLimit{101}` / `Ok` |
| M7 | All without intent | `all(g)` and App intents lack `GUILD_MEMBERS` | `Err(MissingIntent{intent:"GUILD_MEMBERS"})`; no frame |
| M8 | Presences without intent | `.presences(true)` without `GUILD_PRESENCES` | `Err(MissingIntent{intent:"GUILD_PRESENCES"})` |
| M9 | Multi-chunk | 3 chunks (indices 2,0,1, `chunk_count` 3) | `Ok` once all 3 arrived; members ordered by index; `not_found`/`presences` concatenated |
| M10 | Duplicate chunk index | index 0 twice | second ignored; still waits for the rest |
| M11 | Rate limited (GW-07) | server dispatches `RATE_LIMITED {opcode:8, retry_after:2.5, meta:{guild_id:g, nonce}}` | `Err(RateLimited { guild: g, retry_after: 2.5 s })` |
| M12 | Foreign nonce | chunk/`RATE_LIMITED` with an unknown nonce | not delivered to any waiter; chunk goes to the sink only if a handler wants `GUILD_MEMBERS_CHUNK`; `RATE_LIMITED` logs `warn` and goes to the sink only if a handler wants it |
| M13 | Timeout | no answer for 10 s | `Err(Timeout { guild })`; pending entry removed |
| M14 | Caller drops future | future dropped mid-wait | entry removed; later chunks with that nonce are treated as foreign |
| M15 | Guild outside range | range 0..2 of 4, guild whose shard is 3 | `Err(GuildNotInRange { guild, shard: 3, range: 0..2 })`; no frame |
| M16 | Owning shard | total 4, guild id `(5 << 22) | 7` | `shard_for_guild` = 1 |
| M17 | Chunk also wanted by a handler | `MemberChunk` Event Handler registered | waiter gets it and the handler runs too |
| B1 | GW-04 budget | 110 sends on shard 0 within 60 s | all `Ok` |
| B2 | 111th send | — | `Err(SendLimit { shard: 0, retry_after })`, `0 < retry_after ≤ 60 s`; nothing queued |
| B3 | Window slides | paused time, advance 60 s past the oldest | next send `Ok` |
| B4 | All-or-none | shard 1 at 110, `set(p)` on 0..2 | `Err(SendLimit{shard:1,..})`; shard 0 budget unchanged, no frame on any shard |
| B5 | Budgets independent | shard 0 exhausted | shard 1 sends `Ok` |
| C1 | Shard task ended | fatal close ended shard 0's task | `Err(ShardClosed { shard: 0 })` |

</intent-contract>

## Code Map

- `crates/noctis-gateway/src/gateway.rs` -- `GatewayOptions` (pub-field struct, `Debug` redacts token: add the 2 fields to `Debug`), `Gateway::start` (builds config via `shard_config`, spawns `shard::run` per id with `Lane`), `Gateway` struct (add `Arc<Commands>`), `close_all`/`closed`. Every doctest here builds a `GatewayOptions` literal: add `presence: None, large_threshold: Default::default()`.
- `crates/noctis-gateway/src/shard.rs` -- `classify(text, wanted, ..)` (parse with the extra flags; decide sink push by `lane.wanted`), `Lane` (add the command receiver, pending map, last-presence slot), `run`/`drive` (`tokio::select!` biased: close, commands, `shard.next()`; READY status → presence re-send). Existing `classify` tests at the bottom.
- `crates/noctis-gateway/src/shard_config.rs` -- `shard_config` stays as is; presence/large_threshold are applied in `Gateway::start` on top (twilight `ConfigBuilder::large_threshold` panics outside 50..=250; `LargeThreshold` makes that unreachable).
- `crates/noctis-gateway/src/lib.rs` -- module list and re-exports; crate doc gains a `# Presence and gateway commands` section.
- `crates/noctis-gateway/Cargo.toml` -- no new deps (`tokio` already has `sync`, `time`; dev `test-util`).
- `crates/noctis-core/src/app.rs` -- `App` fields (~L129), `AppBuilder` setters (pattern: `intents` L1799), `run_with` (`GatewayOptions` literal L878, `Gateway::start` L898), `shut_down` (detach after `closed`), `Debug` impls.
- `crates/noctis-core/src/handler.rs` -- `AppShared` (L203) gains `gateway: GatewayHandle`; `EventCtx::http` (L334) pattern for `gateway()`.
- `crates/noctis-core/src/command.rs` -- `CommandCtx::http` (L310) pattern; detached seam uses `GatewayHandle::detached()`.
- `crates/noctis-core/src/component.rs` -- ctx macro `http()` at ~L253 (`self.0.app` is `Option`): add `gateway()` there.
- `crates/noctis-core/src/lib.rs` -- add `pub mod gateway { pub use noctis_gateway::{...}; }` re-export block and doc.
- `crates/noctis/src/lib.rs` -- add `pub mod gateway` (pattern: `pub mod webhook` L1640) re-exporting `noctis_core::gateway::*` items, with a module doc example.
- `crates/noctis-core/src/app/tests.rs` L2556, `crates/noctis-testkit/tests/{session_resume,lanes,shard_log_redaction,sharding,gateway_close}.rs` -- `GatewayOptions` literals to extend.
- `crates/noctis-testkit/src/fake/gateway.rs` -- `FakeGateway::{dispatch, frames, wait_for_frame, close, set_config}`; `RecordedFrame::{op, json}` read client frames (op 2 IDENTIFY, op 3, op 8). Reuse; add nothing unless a helper is missing.
- `crates/noctis-testkit/tests/cluster.rs` -- pattern for an App against `FakeDiscordHttp` + `FakeGateway` with a Shard Range.
- `docs/discord-rules.md` L295, L297, L298 -- GW-04/06/07 rows ("How" column).
- `xtask/rules-pending.toml` L46-64 -- the four GW entries of story 15.3 (must be removed once evidence exists).
- `crates/noctis/tests/ui.rs` + `crates/noctis/tests/ui/<area>/` -- UI suite layout; new area `gw/`.
- `examples/project-template/src/lib.rs` `configure` (L34); `examples/project-template/AGENTS.md`; `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` §6 (L633, currently `[planned]` sketch with different names) and `SKILL.md` (API hash L23).
- twilight 0.17.1 facts (read from the crate source): `Shard::send(String)` queues on an unbounded channel and twilight's `CommandRatelimiter` paces sends (max 120 − heartbeat reserve, never below 110); `Config::presence` is sent on every IDENTIFY; `Event::RateLimited(RateLimited { opcode, retry_after: f32, meta: RateLimitMetadata::RequestGuildMembers { guild_id, nonce } })`, flag `EventTypeFlags::RATE_LIMITED`; `MemberChunk { chunk_count, chunk_index, guild_id, members, nonce, not_found, presences }`; `RequestGuildMembers::builder(g).nonce(..).presences(..).query(q, Some(limit))/.user_ids(..)`.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-gateway/src/presence.rs` -- `Status`, `Activity`, `Presence`, `LargeThreshold` + `__bounds` (sealed traits, macro-generated impls, `on_unimplemented`), conversion to `UpdatePresencePayload`; unit tests P3-P6 serialization (serde_json of the twilight payload) and L1.
- `crates/noctis-gateway/src/command.rs` -- `MemberRequest`, `GuildMembers`, `GatewayError`, `GatewayHandle`, `PresenceHandle`, `Commands` (per-shard senders + windows, pending map, nonce, intents, range, total), `SendWindow`, chunk collector; unit tests: M2/M4/M6/M7/M8/M9/M10/M13/M14/M16, B1-B5 (paused time; at least one test name contains `gw_04`), M11 via the collector (a test name containing `gw_07`), P9/P10/C1; a `compile_fail` doctest on `MemberRequest::users` with 101 ids whose failing line ends with `// [GW-07]`.
- `crates/noctis-gateway/src/gateway.rs`, `shard.rs`, `lib.rs` -- wire options, channels, interception, READY re-send, `Gateway::handle()`; extend `GatewayOptions` and all literals/doctests; crate docs.
- `crates/noctis-core/src/{app.rs,handler.rs,command.rs,component.rs,lib.rs}` -- builder setters, `App::gateway/presence`, attach/detach in `run_with`/`shut_down`, ctx `gateway()`, `noctis_core::gateway` re-export module; update every `GatewayOptions` literal in core tests.
- `crates/noctis/src/lib.rs` -- `pub mod gateway` with docs and a doctest building a `Presence` and `MemberRequest`.
- `crates/noctis/tests/ui/gw/GW-06_large_threshold_out_of_range.rs` (+ blessed `.stderr` containing `[GW-06]`, cases 49 and 251) and `crates/noctis/tests/ui/gw/GW-07_member_request_too_many_ids.rs` (+ `.stderr` containing `[GW-07]`, 101 ids and 0 ids).
- `crates/noctis-testkit/tests/presence_commands.rs` (new) -- App-level tests against `FakeGateway`: P1, P2, L3 (IDENTIFY frames), P7, P8, P11, P12, M1, M5, M11 (`RATE_LIMITED` dispatch, test fn name contains `gw_07`), M12, M15 (shard range 0..2 of 4), M17, P10 after `run_until` returns; plus the `GatewayOptions` literal updates in the other testkit tests.
- `docs/discord-rules.md` -- GW-04 How: Noctis per-shard budget of 110/60 s (`GatewayError::SendLimit`) in front of twilight's limiter; GW-06 How: `LargeThreshold::new::<N>()`, `AppBuilder::large_threshold`; GW-07 How: `MemberRequest` constructors (one guild, `users::<N>` 1..=100 at compile time, `user_list` at run time), intents checked, `RATE_LIMITED` → `GatewayError::RateLimited`, `Timeout`. Keep Enforcement cells as they are.
- `xtask/rules-pending.toml` -- remove the four 15.3 entries (GW-04 unit, GW-06 diagnosed, GW-07 by-construction, GW-07 unit).
- `examples/project-template/src/lib.rs` -- `configure` adds `.presence(Presence::watching("/help"))` (with a comment on `app.presence().set(..)`/`ctx.gateway()`); `examples/project-template/AGENTS.md` -- one line on presence and gateway commands; skill `references/app-and-sharding.md` §6 rewritten (drop `[planned]`, real names, errors table, budget, timeout, intents, no-feature note) and `SKILL.md` index line; every new snapshot item mentioned somewhere in the skill.
- `xtask/public-api.txt` -- re-bless (`cargo run --release -p xtask -- api-snapshot --bless`) and put the printed hash in `SKILL.md`'s `API hash` line.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- add `(decision)` lines for any implementation-time decision beyond D1-D7.

**Acceptance Criteria:**
- Given `AppBuilder::presence(p)` and `large_threshold(t)`, when the shards of a running App identify against `FakeGateway`, then every IDENTIFY frame's `d.presence` equals p's wire form and `d.large_threshold` equals `t.get()`.
- Given a running App, when `app.presence().set(p)` or `.set_shard(n, p)` is called, then the fake receives op 3 with p on every shard of the range or only on shard n, and a shard outside the range returns `ShardNotInRange` without a frame.
- Given a guild whose owning shard is outside this process's Shard Range, when `ctx.gateway().request_guild_members(..)` is awaited, then it returns `GuildNotInRange` and no op 8 is sent; for an in-range guild the op 8 goes to the owning shard's connection only.
- Given a shard that sent 110 commands in the last 60 s, when a 111th is attempted, then `GatewayError::SendLimit` is returned and nothing is queued (GW-04); given a `RATE_LIMITED` for the request's nonce, then the awaiting caller gets `GatewayError::RateLimited` (GW-07).
- Given `LargeThreshold::new::<49>()` or `<251>()`, when the UI suite compiles it, then compilation fails with a message containing `[GW-06]`; `<50>` and `<250>` compile.
- Given the finished story, when `cargo run --release -p xtask -- rules-catalog` runs, then GW-04, GW-06 and GW-07 count as covered with no pending entry; and `skill-check` and `api-snapshot` pass with the Usage Skill documenting presence and gateway commands.

## Spec Change Log

## Review Triage Log

## Design Notes

Decisions (log each as a `(decision)` line in the architecture memlog):
- **D1** No cargo feature: the code is small, adds no dependency and lives in `noctis-gateway` per AD-21; the epic allows "a feature or an extension", and an always-on module keeps the `cargo hack` matrix unchanged.
- **D2** GW-04 budget = 110 per shard per 60 s, measured at acceptance: twilight reserves up to 10 of Discord's 120 for heartbeats (its floor is 110 non-reserved), so an accepted command is never held by twilight's limiter for long, and refusing early is the only way to give the caller a typed error instead of an unbounded queue.
- **D3** API names reconcile the skill sketch with the story text: `app.presence().set(..)`/`.set_shard(..)` (story AC) and `ctx.gateway().request_guild_members(..)`; `GatewayError` (not `NotInRange`/`RateLimited` only).
- **D4** Request Guild Members returns the collected `GuildMembers` (the story's "fetch members on demand"), correlated by a Noctis nonce; a 10 s idle timeout makes a lost answer (reconnect mid-request) a typed `Timeout` rather than a hang.
- **D5** Presence text is not length-checked: Discord documents no limits for bot activity `name`/`state`; no new rule id.
- **D6** `MemberRequest::prefix` limit 1..=100 (Discord caps query results; `0` only means "all" with an empty query, which is `all()`).
- **D7** Missing `GUILD_MEMBERS` for `all()` and missing `GUILD_PRESENCES` for `presences(true)` are refused client-side (`MissingIntent`), since Discord answers them with nothing useful or an error close.

Wire example (P1): `{"op":2,"d":{..,"large_threshold":50,"presence":{"activities":[{"name":"servers","type":3}],"afk":false,"since":null,"status":"online"}}}`. Seal pattern:

```rust
pub struct Threshold<const N: u16>;
#[diagnostic::on_unimplemented(message = "[GW-06] `LargeThreshold::new::<N>()` takes 50..=250 ..")]
pub trait ValidLargeThreshold: sealed::Sealed {}
macro_rules! valid { ($($n:literal)*) => { $(impl sealed::Sealed for Threshold<$n> {} impl ValidLargeThreshold for Threshold<$n> {})* } }
valid!(50 51 /* … */ 250);
```

## Verification

**Commands** (run from the repository root, release only):
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (incl. `noctis` UI suite with `gw/` fixtures, testkit `presence_commands`)
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok; GW-04/06/07 covered, none pending
- `cargo run --release -p xtask -- api-snapshot` -- ok after `--bless`
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo run --release -p xtask -- tw-grammar --check` and `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- unchanged, pass
- `cargo deny check` -- ok; `cargo +1.91 check --release --workspace` -- ok (MSRV; `diagnostic::on_unimplemented` is stable since 1.78)
- `cargo hack` is not run (not the last story of Epic 15).
