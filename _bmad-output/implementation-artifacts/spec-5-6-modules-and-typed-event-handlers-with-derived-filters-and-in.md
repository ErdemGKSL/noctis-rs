---
title: 'Story 5.6: Modules and typed Event Handlers with derived filters and intents'
type: 'feature'
created: '2026-10-20'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Core's drain task (5.5) traces and drops every item: there are no Modules, no event handlers, no worker set, and the gateway parses only `INTERACTION_CREATE`. FR-5/FR-6/FR-44/NFR-4, AD-7/9/10/13/21/23 and GW-05 need typed Event Handlers registered in nestable Modules, a Registry folded once at build, a gateway filter and intents derived from the handlers, and a semaphore-bounded worker set with per-guild ordered handlers and `catch_unwind`.

**Approach:** Add `Module`, the `#[event]` attribute macro, `EventCtx`, `EventHandler`, a sealed `EventPayload` trait over 64 twilight payload types, `HandlerError`/`HandlerResult`, and `AppBuilder::{module, event_lane, handler_concurrency}`. `build()` folds the Module trees into an immutable core `Registry` (handlers by `EventType`, parse flags, droppable set, derived intents, GW-05 violations). `App::run` passes the derived filter to the gateway and the lane config to `lanes`, and replaces the drain task with a worker set.

## Boundaries & Constraints

**Always:**
- **Event payloads (`crates/noctis-core/src/events.rs`, re-exported as `noctis::events`):** `pub trait EventPayload: sealed::Sealed + Send + 'static` (no public methods; `#[diagnostic::on_unimplemented(message = "`{Self}` is not a gateway event payload", note = "use a type from `noctis::events`; interactions are handled by commands and Routes")]`). The sealed supertrait carries `const KIND: EventType`, `const PARSE: EventTypeFlags`, `const INTENT: Intents` and `fn extract(Event) -> Option<Self>` (unboxes boxed variants). One `macro_rules!` table implements it for exactly these 64 `twilight_model::gateway::payload::incoming` types, with derived intent (`*` = privileged, `—` = none):
  - `AUTO_MODERATION_CONFIGURATION`: `AutoModerationRuleCreate`, `AutoModerationRuleDelete`, `AutoModerationRuleUpdate`; `AUTO_MODERATION_EXECUTION`: `AutoModerationActionExecution`.
  - `GUILD_MODERATION`: `BanAdd`, `BanRemove`, `GuildAuditLogEntryCreate`.
  - `GUILDS`: `ChannelCreate`, `ChannelDelete`, `ChannelPinsUpdate`, `ChannelUpdate`, `GuildCreate`, `GuildDelete`, `GuildUpdate`, `UnavailableGuild`, `RoleCreate`, `RoleDelete`, `RoleUpdate`, `StageInstanceCreate`, `StageInstanceDelete`, `StageInstanceUpdate`, `ThreadCreate`, `ThreadDelete`, `ThreadListSync`, `ThreadMemberUpdate`, `ThreadMembersUpdate`, `ThreadUpdate`.
  - `GUILD_EMOJIS_AND_STICKERS`: `GuildEmojisUpdate`, `GuildStickersUpdate`. `GUILD_INTEGRATIONS`: `GuildIntegrationsUpdate`, `IntegrationCreate`, `IntegrationDelete`, `IntegrationUpdate`. `GUILD_INVITES`: `InviteCreate`, `InviteDelete`. `GUILD_WEBHOOKS`: `WebhooksUpdate`. `GUILD_VOICE_STATES`: `VoiceStateUpdate`. `GUILD_SCHEDULED_EVENTS`: the five `GuildScheduledEvent{Create,Delete,Update,UserAdd,UserRemove}`.
  - `GUILD_MEMBERS*`: `MemberAdd`, `MemberRemove`, `MemberUpdate`. `GUILD_PRESENCES*`: `PresenceUpdate`.
  - `GUILD_MESSAGES`: `MessageCreate`, `MessageDelete`, `MessageDeleteBulk`, `MessageUpdate`. `GUILD_MESSAGE_POLLS`: `MessagePollVoteAdd`, `MessagePollVoteRemove`. `GUILD_MESSAGE_REACTIONS`: `ReactionAdd`, `ReactionRemove`, `ReactionRemoveAll`, `ReactionRemoveEmoji`. `GUILD_MESSAGE_TYPING`: `TypingStart`.
  - `—`: `CommandPermissionsUpdate`, `EntitlementCreate`, `EntitlementDelete`, `EntitlementUpdate`, `MemberChunk`, `UserUpdate`, `VoiceServerUpdate`.
  - `PARSE` is `EventTypeFlags::from(KIND)` except `UnavailableGuild`, whose `PARSE` is `GUILD_DELETE` (twilight parses it from the `GUILD_DELETE` header).
- **Handlers:** `pub type HandlerResult<T> = Result<T, HandlerError>`. `HandlerError` (`crates/noctis-core/src/handler.rs`): `new(impl Error + Send + Sync + 'static)`, blanket `impl<E: Error + Send + Sync + 'static> From<E>`, `as_error(&self) -> &(dyn Error + Send + Sync + 'static)`, `Display`/`Debug` delegate to the inner error; it does not implement `std::error::Error`. `EventCtx` (`Clone`, cheap: one `Arc`): `shard() -> u32`, `guild_id() -> Option<Id<GuildMarker>>` (`Event::guild_id()`), `module() -> &'static str`, `state::<T>() -> Option<State<T>>`, `http() -> &Arc<twilight_http::Client>` (the App's one client); manual `Debug` without the client.
- **`EventHandler`** (opaque, `Clone`, `Debug` = handler name + event name + flags): `#[doc(hidden)] pub fn __new<P: EventPayload, F, Fut>(handler: &'static str, ordered: bool, droppable: bool, f: F) -> Self` where `F: Fn(EventCtx, P) -> Fut + Send + Sync + 'static`, `Fut: Future<Output = HandlerResult<()>> + Send + 'static`; it stores an erased `Arc<dyn Fn(EventCtx, Event) -> Option<BoxFuture<'static, HandlerResult<()>>> + Send + Sync>`.
- **`Module`** (`crates/noctis-core/src/module.rs`, `Clone`, manual `Debug`): `new(id: &'static str)`, `add(self, impl Descriptor) -> Self`, `module(self, child: Module) -> Self`, `id(&self) -> &'static str`. `pub trait Descriptor: sealed` implemented by `EventHandler` only (commands etc. join later). Ids are not validated in 5.6 (Story 13.1).
- **`#[event]` (`crates/noctis-macros/src/event.rs`, `#[proc_macro_attribute]`):** args `ordered`, `droppable`, in any order, each at most once. Input must be a plain `async fn` (no `const`/`unsafe`/`extern`/receiver/generics/where clause) with exactly two typed params. Output: the original attributes and visibility on `fn name() -> ::noctis::__private::core::EventHandler`, whose body defines the original fn as an inner `async fn __noctis_event(..)` with the original body and returns `EventHandler::__new(concat!(module_path!(), "::", "<name>"), ordered, droppable, __noctis_event)`. Errors are spanned `syn::Error`s: "`#[event]` needs an `async fn`", "`#[event]` handlers take `(ctx: EventCtx, event: <payload>)`", "`#[event]` handlers cannot be generic", "`#[event]` handlers must be a plain `async fn`", "unknown `#[event]` argument `x`: expected `ordered` or `droppable`", "duplicate `#[event]` argument `x`". Facade `__private` gains `pub use noctis_core as core;`.
- **Builder (`app.rs`):** `module(&mut self, Module) -> &mut Self` (appended in call order, plugins included); `event_lane(&mut self, capacity: usize, overflow: usize)` (valid `1..=1_048_576` / `0..=4_194_304`, default 4096/16384, else `BuildViolation::InvalidEventLane { capacity, overflow }`); `handler_concurrency(&mut self, usize)` (valid `1..=65_536`, default 256, else `BuildViolation::InvalidHandlerConcurrency { actual }`). New `#[non_exhaustive]` variant `BuildViolation::MissingIntent { module: &'static str, handler: &'static str, event: &'static str, intent: Intents }`, Display `"[GW-05] event handler `{handler}` (Module `{module}`) handles {event}, which needs the privileged intent {NAME}: add `.intents(Intents::{NAME})` and enable it in the Discord Developer Portal"` (`event` = `EventType::name()`, `NAME` = `GUILD_MEMBERS` | `GUILD_PRESENCES`).
- **Fold (`crates/noctis-core/src/registry.rs`, once per `build()`, after plugin install and the shard-total check, before the crypto/runtime checks):** pre-order over the mounted roots in mount order: a Module's own registrations in `add` order, then each child (mount order), whatever order `add`/`module` were called. Each entry records `module`, `ancestors: Arc<[&'static str]>` (root first, self excluded) and the handler. Per `EventType`: `ordered` and `concurrent` handler lists in fold order. `event_types = INTERACTION_CREATE ∪ PARSE of every handler`; `droppable = DROPPABLE_EVENTS ∪ {types whose handlers are all droppable}`; derived intents = ∪ non-privileged `INTENT`; one `MissingIntent` per handler whose privileged `INTENT` is not in the requested set, in fold order. `App` stores `Arc<Registry>`; `App::intents()` = requested ∪ derived.
- **Worker set (`crates/noctis-core/src/worker.rs`; `App::run` passes `event_types` in `GatewayOptions` and `LaneConfig { event_capacity, overflow_capacity, droppable, ..default }`):** the drain loop `recv()`s. An interaction never takes a permit: in 5.6 it is traced `trace!(target: "noctis::app", shard, kind = ?kind, "no handler registered")` and dropped. An event acquires one permit of a `tokio::sync::Semaphore(handler_concurrency)` via `select! { biased; }` over the permit and `receiver.recv_interaction()` (interactions arriving meanwhile are handled first), then: no handlers → trace and release; ordered handlers → job to sub-queue `subqueue(guild_id)`, whose task runs them sequentially then spawns the concurrent part; else spawn the concurrent part. The concurrent part runs every concurrent handler at once (`join_all`); the permit is released when it ends. Each handler gets a clone of the `Event` (the last gets the original), runs as `AssertUnwindSafe(fut).catch_unwind()`, and `Err` → `error!(target: "noctis::event", module, handler, event_type, error = %e, "event handler failed")`, panic → `error!(.., panic = msg, "event handler panicked")` (`&str`/`String` payload, else `"<non-string panic>"`). `subqueue(None) = 0`, `subqueue(Some(id)) = (id.get().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize` (64 sub-queues, spawned only when the Registry has an ordered handler). The drain task keeps its `AbortOnDrop`.
- **Gateway (`lanes.rs`):** add `pub async fn recv_interaction(&mut self) -> Option<Inbound>`: cancel-safe, pops only from the interaction queue, `None` once no sender is left and that queue is empty; event items stay queued.
- Never log payloads, tokens or panic payloads of non-string type. No `unwrap`/`expect` outside tests. Every cargo command runs with `--release`.

**Never:** commands, Routes or any interaction handler (Epics 6-7); Middleware, Extensions, stages 3/4/7/10 (7.6); Error Sinks, `noctis.event` spans, handler metrics (5.7); shutdown draining (5.8); Module id format/uniqueness checks (13.1); cache-driven filters (12.5); injected extractor params; `ctx.kv()`/`cache()`/`modules()`; env variables for the new setters; `tokio::sync::Mutex` or blocking in `push`.

## I/O & Edge-Case Matrix

`M`=`MessageDelete` handler; guild ids `GA`, `GB` are fixtures with `subqueue(GA) != subqueue(GB)`; "paused" = `#[tokio::test(start_paused = true)]`.

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | No modules | `App::builder().build()` | intents `GUILDS`; `event_types == INTERACTION_CREATE`; droppable `DROPPABLE_EVENTS`; no sub-queues |
| 2 | Empty module | `.module(Module::new("a/b"))` | same as 1 |
| 3 | One handler | `MessageCreate` handler | intents `GUILDS \| GUILD_MESSAGES`; `event_types == INTERACTION_CREATE \| MESSAGE_CREATE` |
| 4 | Fold order | roots `p{h1, child c{h2,h3, child g{h4}}}`, `r{h5}` | order h1..h5; ancestors `[]`, `[p]`, `[p]`, `[p,c]`, `[]` |
| 5 | Call order irrelevant | `Module::new("p").module(c).add(h1)` | h1 before c's handlers |
| 6 | Same descriptor twice | `.add(h()).add(h())` | two entries; both run once per event |
| 7 | Tree on two Apps | clone mounted on two builders | both build; each folds independently |
| 8 | Ids unvalidated | `""`, `"A B"`, `"ü/🙂"` | build ok |
| 9 | Plugin mounts | plugin `install` calls `.module(x)` | x's handlers folded after previously mounted roots |
| 10 | Builder reset | build twice | second App has no handlers |
| 11 | GW-05 | `MemberAdd` handler, no `.intents` | one `MissingIntent{module, handler path, "GUILD_MEMBER_ADD", GUILD_MEMBERS}`; message has `[GW-05]`, the handler path and `GUILD_MEMBERS` |
| 12 | GW-05 satisfied | + `.intents(GUILD_MEMBERS)` | ok; intents `GUILDS \| GUILD_MEMBERS` |
| 13 | Presence | `PresenceUpdate` handler | `MissingIntent` with `GUILD_PRESENCES` |
| 14 | Several | `MemberAdd`, `MemberRemove` handlers | two violations, fold order |
| 15 | Mixed violations | 11 + `http_timeout(0)` | one `BuildError` holding both |
| 16 | Full table | each of the 64 payloads alone | derived intent and `PARSE` exactly as the table; `—` types keep `GUILDS` |
| 17 | No content intent | `MessageCreate` | `MESSAGE_CONTENT` and `DIRECT_MESSAGES` not added |
| 18 | Explicit extra | `.intents(DIRECT_MESSAGES)` + `MessageCreate` | `GUILDS \| GUILD_MESSAGES \| DIRECT_MESSAGES` |
| 19 | UnavailableGuild | its handler | `event_types` contains `GUILD_DELETE` |
| 20 | Droppable all | sole `M` is `droppable` | droppable `= DROPPABLE_EVENTS \| MESSAGE_DELETE` |
| 21 | Droppable mixed | two `M`, one droppable | `MESSAGE_DELETE` not droppable |
| 22 | Defaults stay | non-droppable `PresenceUpdate`/`TypingStart` handler | still droppable |
| 23 | Lane bounds | `event_lane` (1,0), (1048576,4194304) ok; (0,0), (1048577,0), (1,4194305) | violation `InvalidEventLane` with both values; default 4096/16384 reaches `LaneConfig` |
| 24 | Concurrency bounds | 1, 65536 ok; 0, 65537 | `InvalidHandlerConcurrency{actual}`; default 256 |
| 25 | Both run | concurrent h1, h2 on `M`; 1 event | each called once with the payload |
| 26 | Ordered first | ordered `o` sleeps 1 s, concurrent `c` (paused) | `c` starts after `o` ends |
| 27 | Per-guild order | ordered sleeps on event 1; events 1, 2 in `GA` | event 2's ordered call starts after event 1's ends; arrival order |
| 28 | Guild isolation | ordered blocked in `GA`; event in `GB` | `GB` handled while `GA` blocks |
| 29 | No guild | ordered `UserUpdate` handler, 2 events | sub-queue 0, sequential |
| 30 | Ordered sequence | ordered o1, o2 on one event | o1 ends before o2 starts (fold order) |
| 31 | Bound | `handler_concurrency(2)`, 5 events, handler awaits a release | at most 2 in flight; all 5 finish after release |
| 32 | Interaction bypass | `handler_concurrency(1)`, handler blocked on M1; push M2 then I1 | I1 traced while M1 still blocked and before M2's handler starts |
| 33 | Err | handler returns `Err(io::Error)` | one `error` log with module, handler, event_type, error; sibling handlers run |
| 34 | Panic | `panic!("boom")`, `panic!("{}", s)`, `panic_any(5)` | `"event handler panicked"` with `boom` / the string / `<non-string panic>`; next event still handled |
| 35 | Ordered panic | ordered panics | event's concurrent handlers still run; next same-guild event processed |
| 36 | No handler | event type without handler reaches dispatch | traced; permit released (next event handled with `handler_concurrency(1)`) |
| 37 | Close | drop every sender | drain returns; `AbortOnDrop` dropped while pending aborts |
| 38 | `subqueue` | `None`; same id twice; ids 0, 1, `u64::MAX` | 0; equal; all `< 64` |
| 39 | `recv_interaction` | queue [M1, I1]; closed with [M1]; pending then push I1; push M1 while pending | I1 (M1 stays); `None`; I1; stays pending, M1 still popped by `recv` |
| 40 | EventCtx | `MessageCreate` in guild via App | `shard()`, `guild_id() == Some`, `module()`, `state::<u32>()` Some / `state::<u64>()` None, `http()` `ptr_eq` App's |
| 41 | HandlerError | `?` on `io::Error`; `HandlerError::new(e)` | `Display` = inner; `as_error().downcast_ref::<io::Error>()` is `Some` |
| 42 | Macro forms | `#[event]`, `(ordered)`, `(droppable)`, `(ordered, droppable)`, `(droppable, ordered)`, `pub`, doc comments, `_` patterns, unicode fn name | compile; flags and handler path (`module_path!()::name`) correct |
| 43 | Macro errors | non-async; 0/1/3 params; `self`; generics; where; `const`/`unsafe`/`extern`; arg `x`; `ordered, ordered`; `ordered = true` | each the matching message above |
| 44 | Type errors | payload `String`; `InteractionCreate`; ctx `u8`; return `()` | `compile_fail` doctests; the first two show the `on_unimplemented` message |
| 45 | E2E filter | handler only for `MESSAGE_DELETE`; FakeGateway dispatches `MESSAGE_CREATE` with `d = {"bogus":true}`, then a valid `MESSAGE_DELETE` | 0 `"gateway payload could not be parsed"` warnings; delete handler called once. Control: with a `MESSAGE_CREATE` handler added, the same payload logs exactly 1 such warning |
| 46 | E2E intents | `MessageCreate` handler | IDENTIFY `d.intents == (GUILDS \| GUILD_MESSAGES).bits()` |
| 47 | E2E order + panic | ordered + concurrent on `MESSAGE_DELETE` in one guild; the first event panics in the ordered handler | both run per event (ordered first); 2nd event handled; 1 connection |

</intent-contract>

## Code Map

- `crates/noctis-core/src/app.rs`: `App` fields L67-79 (add `registry: Arc<Registry>`, lane/concurrency config; share `http` + `states` with `EventCtx` through an `Arc`); `run` L265-309 (`event_types` L292, `LaneConfig::default()` L298, drain L299); `AbortOnDrop`/`drain`/`dispatch` L398-425 → move into `worker.rs`; `AppBuilder` fields L488-507, `new` L529, `intents` L615, `install_plugins` L911, `build` L841-907 (fold after L875); `Debug` impls L448, L928.
- `crates/noctis-core/src/error.rs` L101-168: `BuildViolation` (add the three variants with rustdoc).
- `crates/noctis-core/src/lib.rs`: `mod`s, root re-exports (`Module`, `Descriptor`, `HandlerError`, `HandlerResult`, and `EventCtx`, `EventHandler`, `EventPayload`, which the macro names as `::noctis::__private::core::EventHandler`), `pub mod events` (the trait, ctx, handler and the 64 payload re-exports), crate docs.
- `crates/noctis-core/src/state.rs`: `StateMap::get` reused by `EventCtx::state`.
- `crates/noctis-gateway/src/lanes.rs`: `LaneReceiver::recv`/`try_recv`/Notify loop to mirror for `recv_interaction`; tests in `lanes/tests.rs`.
- `crates/noctis-gateway/src/shard.rs` L40-110 `classify`: warns `"gateway payload could not be parsed; ignored"` on parse failure (the E2E counter, read-only).
- `crates/noctis-macros/src/lib.rs`: add `#[proc_macro_attribute] pub fn event` + `mod event;`; follow `route.rs`/`common.rs` error style; unit tests on `proc_macro2` token streams in `event.rs`.
- `crates/noctis/src/lib.rs`: root re-exports L133-136, `__private` L386-391, prelude L357-384; add `pub mod events` (core's `EventCtx`, `EventHandler`, `EventPayload` + the 64 payload types re-exported from `twilight_model::gateway::payload::incoming`), `pub use noctis_macros::event` at the root and in the prelude with `Module`, `EventCtx`, `HandlerError`, `HandlerResult`; a `# Modules and events` crate-docs section.
- twilight (read-only, `~/.cargo/registry/src/*/`): model 0.17.1 `gateway/event/mod.rs` (`Event` variants, `guild_id()` L189, `kind()`), `gateway/event/kind.rs` (`EventType: Hash`, `name()` L98), `gateway/intents.rs`; gateway 0.17.1 `event.rs` (intent-group flags L200-330, `From<EventType>` L338), `json.rs` L26-47 (`parse` skips unwanted types before deserializing).
- `crates/noctis-testkit/tests/sharding.rs` L42-130: `Fakes` helper (`builder()` with `http_proxy`/`gateway_url`, `spawn`); `lane_metrics.rs`: trace-capture pattern and the 5.5 `"no handler registered"` interaction trace that must keep passing.
- `xtask/rules-pending.toml` L361-364: the GW-05 entry to delete; `docs/discord-rules.md` L274: GW-05 row.
- `examples/project-template/src/modules/profile.rs` L24-34, L162-181 (`award_xp`); skill `references/modules-middleware-ratelimits.md` §1, §4; `references/app-and-sharding.md` L60-90 (builder table, `.module` row marked planned), L107-140 (errors), §4 L257-260 (intents), §9 L343; `SKILL.md` API hash line.

## Tasks & Acceptance

**Execution:**
1. `Cargo.toml` files: core `[dependencies]` `futures-util.workspace = true`, tokio features + `macros`, `time`; core `[dev-dependencies]` `serde_json`, `tracing-subscriber = { workspace = true, features = ["fmt", "std"] }`; testkit `[dev-dependencies]` `noctis.workspace = true`. No new crates.
2. `crates/noctis-gateway/src/lanes.rs` (+ tests): `recv_interaction`, Matrix 39.
3. `crates/noctis-core/src/{handler.rs, events.rs, module.rs, registry.rs, worker.rs}`, `app.rs`, `error.rs`, `lib.rs`: implement per Boundaries; unit tests for Matrix 1-41 (core-level handlers via `EventHandler::__new`; the GW-05 tests' names contain `gw_05`); keep every 5.4/5.5 test green.
4. `crates/noctis-macros/src/{event.rs, lib.rs}`: the attribute; unit tests for Matrix 43.
5. `crates/noctis/src/lib.rs` and `crates/noctis/tests/events.rs`: facade wiring, `compile_fail` doctests (Matrix 44), facade tests for Matrix 3, 11 (`gw_05_*`), 42 through `#[event]`.
6. `crates/noctis-testkit/tests/events.rs`: Matrix 45-47 against `FakeDiscordHttp` + `FakeGateway` with `#[event]` handlers (multi-thread tokio, every wait ≤ 15 s).
7. `docs/discord-rules.md` GW-05 row: non-privileged intents are added automatically, a missing privileged one is `BuildViolation::MissingIntent`; delete GW-05 from `xtask/rules-pending.toml`.
8. Usage Skill + template: `modules-middleware-ratelimits.md` §1/§4 show the real `Module`/`#[event]`/`EventCtx`/`HandlerError` API (mark `.template`, `.locales`, `.before`, `.flags`, `kv()`, `cache()`, `modules()` `[planned]`), ordering, droppable, intent derivation, and a table of the 64 payload types with their derived intents; `app-and-sharding.md`: `.module` (no longer planned), `.event_lane`, `.handler_concurrency`, the new violations, the worker set and interaction exemption; template `profile.rs` `award_xp` matches the real signature and states the derived intent. Re-bless the API snapshot and update `SKILL.md`'s API hash in the same commit.
9. `.memlog.md` (architecture): planning decisions are logged; append implementation decisions.

**Acceptance Criteria:**
- Given `Module::new("a/b")` with `#[event]` handlers mounted via `AppBuilder::module`, when the App is built and run, then the gateway deserializes only interactions and handled event types (Matrix 45) and IDENTIFY carries the derived intents (Matrix 46).
- Given a nested Module tree, when built, then registrations fold parent-before-child with ancestor paths (Matrix 4-5).
- Given a handler needing a privileged intent the App did not request, when `build()` runs, then the `BuildError` names GW-05, the handler and the intent (Matrix 11-15); with no handlers intents are `GUILDS`.
- Given an ordered and a concurrent handler for one event, when events arrive, then both run, the ordered one first, in per-guild order (Matrix 26-30, 47).
- Given full worker permits, when an interaction arrives, then it is taken without waiting for an event permit, and a panicking handler is logged while the shard keeps running (Matrix 32-35, 47).
- Given the new API, when the gates run, then `skill-check` passes with the updated skill and hash, and `rules-catalog` counts GW-05 as covered.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Privileged-only GW-05:** auto-adding non-privileged intents is the "derived" half; GW-05 itself is about privileged intents that need Portal enablement, so only those fail the build. `MessageCreate` derives `GUILD_MESSAGES` (template comment); DMs need an explicit `.intents(DIRECT_MESSAGES)`.
- **Interactions bypass the semaphore:** a shared bound would let slow event handlers starve interactions past the 3 s deadline (FR-5/FR-21). The bound applies to events; `recv_interaction` lets the drain task serve interactions while it waits for a permit.
- **Expansion sketch:**
  ```rust
  pub fn award_xp() -> ::noctis::__private::core::EventHandler {
      async fn __noctis_event(ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> { /* body */ }
      ::noctis::__private::core::EventHandler::__new(
          concat!(module_path!(), "::", "award_xp"), false, false, __noctis_event)
  }
  ```
- **Snapshot size:** foreign re-exports appear as single `pub use` lines, so each payload name must appear in the skill (the table covers it).

## Verification

**Commands** (repo root; local gates only; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0; run `-p noctis-testkit --test events` 3 times (no flakes).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0. `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog` (GW-05 covered) and `-- skill-check` -- each exit 0 (needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0, see AGENTS.md pitfalls).
- `test ! -d target/debug` -- true.
