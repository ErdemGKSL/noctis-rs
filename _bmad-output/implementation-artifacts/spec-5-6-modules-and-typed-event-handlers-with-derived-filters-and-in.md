---
title: 'Story 5.6: Modules and typed Event Handlers with derived filters and intents'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: '5a343f04ef66f5d9d4dde950db6fbebae47db260'
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Template bootstrap writes a new .env and exits even when NOCTIS_TOKEN is already exported, contradicting its "env-only deployments never need a .env" doc; README says "next to itself" but the path is CWD-relative.
    evidence: |-
      load_or_create_env falls through to create_private on NotFound without checking the process environment; main.rs then returns early. From commit 9b82e43 (template single-binary work), not Story 5.6.
    location: >-
      examples/project-template/src/bootstrap.rs:51
    severity: medium
  - summary: >-
      Template loads .env (dotenvy set_var) inside #[tokio::main] after the multi-thread runtime started.
    evidence: |-
      main.rs calls load_or_create_env inside the async main; env mutation while other threads exist is racy. Move it into a plain fn main before building the runtime. From commit 9b82e43, not Story 5.6.
    location: >-
      examples/project-template/src/main.rs:12
    severity: low
  - summary: >-
      Template dependency versions base64 0.23, getrandom 0.4, zeroize 1.9 (and the getrandom::fill / STANDARD APIs) were not recorded as verified against crates.io.
    evidence: |-
      Unverified (maybe-false). Settle by checking crates.io/docs.rs for those versions and APIs; the template is outside workspace gates. From commit 9b82e43, not Story 5.6.
    location: >-
      examples/project-template/Cargo.toml:38
    severity: medium (unverified)
  - summary: >-
      Sub-queue and concurrent handler tasks are not tracked, so handlers keep running after App::run returns.
    evidence: |-
      Workers::new and dispatch use bare tokio::spawn; only the drain task is under AbortOnDrop. Shutdown draining belongs to Story 5.8 (excluded by the intent's Never list).
    location: >-
      crates/noctis-core/src/worker.rs:75
    severity: medium
  - summary: >-
      The biased select (queued interactions before a freed permit) has no test where both branches are ready.
    evidence: |-
      interaction_bypasses_permits only covers the permit-not-ready case. Interactions are traced and dropped until Epic 6, where the ordering becomes observable and should be pinned.
    location: >-
      crates/noctis-core/src/worker.rs:133
    severity: low
  - summary: >-
      Template bootstrap.rs unit tests never run in any gate.
    evidence: |-
      examples/project-template is excluded from the workspace and the template CI job only builds with continue-on-error; the template cannot compile until planned API lands (Story 14.2). Then template-build should run cargo test --release.
    location: >-
      examples/project-template/src/bootstrap.rs
    severity: medium
  - summary: >-
      HandlerError accepts only `E: Error + Send + Sync + 'static`, so `?` on `Box<dyn Error + Send + Sync>` or `anyhow::Error` does not compile and there is no constructor from a plain message.
    evidence: |-
      Box<dyn Error + Send + Sync> and anyhow::Error do not implement Error, and String/&str do not either; the workaround is HandlerError::new(io::Error::other(..)). The fix adds public API (e.g. HandlerError::msg or a From<Box<dyn Error + Send + Sync>> impl, coherence to be checked), which belongs to Story 6.8 (HandlerError and error-handling patterns, AD-13/AD-16).
    location: >-
      crates/noctis-core/src/handler.rs:73
    severity: medium
  - summary: >-
      Event handler failure logs use `error = %error`, which prints only the outermost error and drops the source() chain.
    evidence: |-
      HandlerError's Display delegates to the inner error's Display. Logging the full chain (or Debug) belongs with Story 6.8 ("the full error is logged through structured tracing") and Story 5.7 Error Sinks, both excluded from 5.6 by the intent's Never list.
    location: >-
      crates/noctis-core/src/worker.rs:235
    severity: low
  - summary: >-
      docs/discord-rules.md INT-13 and ARCHITECTURE-SPINE.md (AD-24 "Discord edge", test-fakes paragraph) describe the Discord edge as current behaviour although Story 5.9 has not implemented it.
    evidence: |-
      No code in the workspace implements the edge; the wording came from commit f9e3658 (Plan Story 5.9), not Story 5.6. Mark it [planned, Story 5.9] or let Story 5.9 make it true.
    location: >-
      docs/discord-rules.md:168
    severity: low
  - summary: >-
      research/http-warm-transport.md cites probe code at an ephemeral session scratchpad path, so its measurements cannot be reproduced.
    evidence: |-
      Line 7 points at /tmp/claude-0/.../scratchpad/http-probe ("scratch, not in the repo"). From commit 45140a7, not Story 5.6; commit the probe sources or inline them when Story 5.9 starts.
    location: >-
      _bmad-output/planning-artifacts/research/http-warm-transport.md:7
    severity: low
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
- `crates/noctis-core/src/registry.rs` (new): handler lists, `droppable` and the `handlers(EventType)` lookup are all keyed by the **dispatch key** (Design Notes), never by `KIND`. `GUILD_DELETE` holds both `GuildDelete` and `UnavailableGuild` handlers, merged in fold order; it is droppable only when every handler in that slot (of either payload) is `droppable`. `event_types` stays `INTERACTION_CREATE ∪ PARSE`, so every parsed type has a slot.
- `crates/noctis-core/src/events.rs` (new): the payload table; `UnavailableGuild`'s sealed impl is the only hand-written `extract` (see Design Notes "Dispatch key"); module docs carry the `UnavailableGuild` sentence and the DM-intents sentence (Design Notes).
- `crates/noctis-core/src/error.rs` L101-168: `BuildViolation` (add the three variants with rustdoc).
- `crates/noctis-core/src/lib.rs`: `mod`s, root re-exports (`Module`, `Descriptor`, `HandlerError`, `HandlerResult`, and `EventCtx`, `EventHandler`, `EventPayload`, which the macro names as `::noctis::__private::core::EventHandler`), `pub mod events` (the trait, ctx, handler and the 64 payload re-exports), crate docs.
- `crates/noctis-core/src/state.rs`: `StateMap::get` reused by `EventCtx::state`.
- `crates/noctis-gateway/src/lanes.rs`: `LaneReceiver::recv`/`try_recv`/Notify loop to mirror for `recv_interaction`; tests in `lanes/tests.rs`.
- `crates/noctis-gateway/src/shard.rs` L40-110 `classify`: warns `"gateway payload could not be parsed; ignored"` on parse failure (the E2E counter, read-only).
- `crates/noctis-macros/src/lib.rs`: add `#[proc_macro_attribute] pub fn event` + `mod event;`; follow `route.rs`/`common.rs` error style; unit tests on `proc_macro2` token streams in `event.rs`.
- `crates/noctis/src/lib.rs`: root re-exports L133-136, `__private` L386-391, prelude L357-384; add `pub mod events` (core's `EventCtx`, `EventHandler`, `EventPayload` + the 64 payload types re-exported from `twilight_model::gateway::payload::incoming`), `pub use noctis_macros::event` at the root and in the prelude with `Module`, `EventCtx`, `HandlerError`, `HandlerResult`; a `# Modules and events` crate-docs section.
- twilight (read-only, `~/.cargo/registry/src/*/`): model 0.17.1 `gateway/event/dispatch.rs` L294 (`GUILD_DELETE` always deserializes to `DispatchEvent::GuildDelete`; nothing in model or gateway 0.17.1 constructs `Event::UnavailableGuild`), `gateway/payload/incoming/{guild_delete.rs (id, unavailable: Option<bool>), unavailable_guild.rs (id only)}`, `gateway/event/mod.rs` (`Event` variants, `guild_id()` L189, `kind()`), `gateway/event/kind.rs` (`EventType: Hash`, `name()` L98), `gateway/intents.rs`; gateway 0.17.1 `event.rs` (intent-group flags L200-330, `From<EventType>` L338), `json.rs` L26-47 (`parse` skips unwanted types before deserializing).
- `crates/noctis-testkit/tests/sharding.rs` L42-130: `Fakes` helper (`builder()` with `http_proxy`/`gateway_url`, `spawn`); `lane_metrics.rs`: trace-capture pattern and the 5.5 `"no handler registered"` interaction trace that must keep passing.
- `xtask/rules-pending.toml` L361-364: the GW-05 entry to delete; `docs/discord-rules.md` L274: GW-05 row.
- `examples/project-template/src/modules/profile.rs` L24-34, L162-181 (`award_xp`); skill `references/modules-middleware-ratelimits.md` §1, §4; `references/app-and-sharding.md` L60-90 (builder table, `.module` row marked planned), L107-140 (errors), §4 L257-260 (intents), §9 L343; `SKILL.md` API hash line.

## Tasks & Acceptance

**Execution:**
1. `Cargo.toml` files: core `[dependencies]` `futures-util.workspace = true`, tokio features + `macros`, `time`; core `[dev-dependencies]` `serde_json`, `tracing-subscriber = { workspace = true, features = ["fmt", "std"] }`; testkit `[dev-dependencies]` `noctis.workspace = true`. No new crates.
2. `crates/noctis-gateway/src/lanes.rs` (+ tests): `recv_interaction`, Matrix 39.
3. `crates/noctis-core/src/{handler.rs, events.rs, module.rs, registry.rs, worker.rs}`, `app.rs`, `error.rs`, `lib.rs`: implement per Boundaries; unit tests for Matrix 1-41 (core-level handlers via `EventHandler::__new`; the GW-05 tests' names contain `gw_05`); keep every 5.4/5.5 test green. Also (Spec Change Log 1):
   - registry: handlers stored under their dispatch key (Design Notes "Dispatch key"). Test `unavailable_guild_keyed_on_guild_delete`: an `UnavailableGuild` handler is returned by `handlers(EventType::GuildDelete)` and nothing is stored under `EventType::UnavailableGuild`; with a `GuildDelete` handler added, both appear in fold order. Test `droppable_unavailable_guild`: a droppable `UnavailableGuild` handler next to a non-droppable `GuildDelete` handler leaves `GUILD_DELETE` non-droppable; the droppable `UnavailableGuild` handler alone makes `GUILD_DELETE` droppable; `UNAVAILABLE_GUILD` is never added to `droppable` by the fold. Test `every_parsed_type_has_a_slot`: for each of the 64 payloads alone, `handlers(<the single EventType of its PARSE>)` is non-empty.
   - events: unit test for `UnavailableGuild::extract` over `Event::GuildDelete` with `unavailable` `Some(true)` → `Some { id }`, `Some(false)` and `None` → `None`; `Event::UnavailableGuild(u)` → `Some(u)`.
   - worker: test that a `GuildDelete` event with `unavailable: Some(true)` runs both an `UnavailableGuild` and a `GuildDelete` handler (each once), and with `unavailable: None` only the `GuildDelete` handler; neither case logs an error. Test (ordered) with `EventHandler::__new` whose closure panics before returning its future: one `"event handler panicked"` log with the message, and a later same-guild event is still handled (proves the `catch_unwind` around the constructing call).
   - app: test through `App::run` (FakeGateway or the lane sender `App::run` actually builds) with `.event_lane(1, 0)` and one all-`droppable` handler blocked on a gate; flood the lane and assert at least one drop through the lane's dropped-event metric or its drop log. It must fail if `run` passes `LaneConfig::default()` instead of the builder's config.
4. `crates/noctis-macros/src/{event.rs, lib.rs}`: the attribute; unit tests for Matrix 43.
5. `crates/noctis/src/lib.rs` and `crates/noctis/tests/events.rs`: facade wiring, `compile_fail` doctests (Matrix 44), facade tests for Matrix 3, 11 (`gw_05_*`), 42 through `#[event]`. Add trybuild `compile_fail` cases in `crates/noctis/tests/ui` (the existing suite) for payload `String` and payload `InteractionCreate`, with `.stderr` snapshots that contain "is not a gateway event payload" (Matrix 44's message; doctests alone cannot assert it). Rustdoc of `pub use noctis_macros::event`: `ordered` states the hang hazard (Design Notes "Ordered hang hazard"); `droppable` states that `TYPING_START` and `PRESENCE_UPDATE` are always droppable regardless of the flag.
6. `crates/noctis-testkit/tests/events.rs`: Matrix 45-47 against `FakeDiscordHttp` + `FakeGateway` with `#[event]` handlers (multi-thread tokio, every wait ≤ 15 s). Plus `unavailable_guild_e2e`: FakeGateway sends a real `GUILD_DELETE` dispatch `{"id":"<snowflake>","unavailable":true}`, an `#[event]` `UnavailableGuild` handler is called exactly once with that id; a following `GUILD_DELETE` without `unavailable` does not call it.
7. `docs/discord-rules.md` GW-05 row: non-privileged intents are added automatically, a missing privileged one is `BuildViolation::MissingIntent`; delete GW-05 from `xtask/rules-pending.toml`.
8. Usage Skill + template: `modules-middleware-ratelimits.md` §1/§4 show the real `Module`/`#[event]`/`EventCtx`/`HandlerError` API (mark `.template`, `.locales`, `.before`, `.flags`, `kv()`, `cache()`, `modules()` `[planned]`), ordering (with the hang-hazard paragraph), droppable, intent derivation (with the DM-intents sentence and the `UnavailableGuild` sentence, Design Notes), and a table of the 64 payload types with their derived intents; `app-and-sharding.md`: `.module` (no longer planned), `.event_lane`, `.handler_concurrency`, the new violations, the worker set and interaction exemption; template `profile.rs` `award_xp` matches the real signature and states the derived intent. Re-bless the API snapshot and update `SKILL.md`'s API hash in the same commit.
9. `.memlog.md` (architecture): planning decisions are logged; append implementation decisions.

**Acceptance Criteria:**
- Given `Module::new("a/b")` with `#[event]` handlers mounted via `AppBuilder::module`, when the App is built and run, then the gateway deserializes only interactions and handled event types (Matrix 45) and IDENTIFY carries the derived intents (Matrix 46).
- Given a nested Module tree, when built, then registrations fold parent-before-child with ancestor paths (Matrix 4-5).
- Given a handler needing a privileged intent the App did not request, when `build()` runs, then the `BuildError` names GW-05, the handler and the intent (Matrix 11-15); with no handlers intents are `GUILDS`.
- Given an ordered and a concurrent handler for one event, when events arrive, then both run, the ordered one first, in per-guild order (Matrix 26-30, 47).
- Given full worker permits, when an interaction arrives, then it is taken without waiting for an event permit, and a panicking handler is logged while the shard keeps running (Matrix 32-35, 47).
- Given the new API, when the gates run, then `skill-check` passes with the updated skill and hash, and `rules-catalog` counts GW-05 as covered.

## Spec Change Log

### 1 — 2026-10-20 — Review pass 1 loopback (bad_spec)
- **Trigger:** `[high]` `UnavailableGuild` handlers never fire (blind 1, 15; edge 1; vgap 3; edge 3 grouped). twilight-model 0.17.1 turns every `GUILD_DELETE` into `Event::GuildDelete` (dispatch.rs:294) and never builds `Event::UnavailableGuild`; the Registry keyed handlers by `KIND` (`UnavailableGuild`) while the worker looked up `event.kind()` (`GuildDelete`). Side effects: `GUILD_DELETE` parsed and queued as a handlerless non-droppable event; the droppable fold marked `UNAVAILABLE_GUILD`, which the lane never sees; no test fed a real `GUILD_DELETE`.
- **Decision:** keep the intent contract (64 payloads, `UnavailableGuild` with `PARSE = GUILD_DELETE`, Matrix 16/19) and make it true by keying on the dispatched type. Dropping the payload would edit `<intent-contract>`.
- **Amended:** Code Map (`registry.rs`, `events.rs`, twilight refs); Tasks step 3 (registry/events/worker/app tests), step 5 (trybuild cases, `#[event]` rustdoc), step 6 (`unavailable_guild_e2e`), step 8 (skill paragraphs); Design Notes "Dispatch key", "DM intents", "Ordered hang hazard". Folded-in review patches: hang-hazard docs; `App::run` lane-config test; sync-panic `catch_unwind` test; trybuild `on_unimplemented` cases; DM-intents sentence; `droppable` rustdoc clause on typing/presence. Correction to the triage proposal: twilight's `UnavailableGuild` has only `id`, so `extract` builds `UnavailableGuild { id: d.id }`.
- **Known-bad state avoided:** any registry, droppable fold or lookup keyed by `KIND` where `KIND != dispatch`; an `UnavailableGuild` handler that compiles, builds and derives `GUILD_DELETE` but is never called; a parsed type with no handler slot; `UNAVAILABLE_GUILD` in the droppable set; `LaneConfig::default()` in `run` going unnoticed by tests.
- **KEEP (the code of commit 23afd8e is the base to rebuild from; restore it with `git show 23afd8e:<path>` and change only what this entry amends):** all 64 payload impls and intents; sealed `EventPayload` with the `on_unimplemented` diagnostic; `HandlerError`/`HandlerResult`/`EventCtx` design; `Module` pre-order fold with ancestor paths and call-order independence (Matrix 4-10); privileged-only GW-05 `MissingIntent` with the exact Display text; `event_lane`/`handler_concurrency` bounds and violations; the worker set (drain with `select! { biased; recv_interaction, acquire_owned }`, interactions never taking a permit, 64 hashed sub-queues spawned only with ordered handlers, clone-except-last, `catch_unwind` on both the constructing call and the future, `panic_message` fallback); `LaneReceiver::recv_interaction` and its Matrix 39 tests; the `#[event]` macro's token-level argument parsing, error messages, expansion shape and facade `__private::core`; testkit E2Es for Matrix 45-47; the docs/rules GW-05 row update; the skill/template/API-hash updates. Do not touch commits 9b82e43 (template bootstrap) and dfdf9ca (CV2 placement matrix): the loopback revert restores story paths to dfdf9ca, not to `baseline_revision`.

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 39 findings — high 3, medium 8, low 15, false 12, maybe-false 1
- findings:
  - `[high]` `[bad_spec]` (blind) `UnavailableGuild` handlers never fire — verified: twilight-model 0.17.1 deserializes `GUILD_DELETE` only to `Event::GuildDelete` (dispatch.rs:294) and nothing constructs `Event::UnavailableGuild`; the handler is keyed under `EventType::UnavailableGuild`, so `registry.handlers(GuildDelete)` never finds it. Amendment: Design Note "Dispatch key" (handlers keyed by the dispatched type `PARSE`; `UnavailableGuild::extract` takes `Event::GuildDelete` with `unavailable == Some(true)`), droppable fold by dispatch key, new tests.
  - `[low]` `[reject]` (blind) Drain takes a permit before checking for handlers — only reachable for handlerless parsed types, which exist solely through the UnavailableGuild bug (twilight `parse` skips unwanted dispatches); unreachable after the amendment.
  - `[medium]` `[patch]` (blind) One hung ordered handler can freeze all event handling — real: queued ordered jobs hold permits, so a stuck sub-queue absorbs every permit. No AD prescribes handler timeouts; smallest fix is documenting the hazard (rustdoc of `#[event]` `ordered` and the skill's ordering section).
  - `[low]` `[reject]` (blind) No handler timeout on concurrent handlers — a hung user future holding its permit is the normal semantics of a bounded pool; a timeout adds config surface no AD asks for.
  - `[low]` `[reject]` (blind) `#[event]` attributes land on the descriptor fn — spec-prescribed; lint attrs still reach the nested fn lexically, only `#[tracing::instrument]` misses (5.7 adds `noctis.event` spans); partitioning attributes adds macro logic.
  - `[low]` `[reject]` (blind) `#[event]` diagnostics lack `[RULE-ID]`/help, type errors left to rustc — messages are spec-prescribed (no rule id governs fn shape); type errors are deliberately routed through `on_unimplemented` (Matrix 44).
  - `[low]` `[patch]` (blind) Negative type cases only covered by `compile_fail` doctests — Matrix 44 expects the `on_unimplemented` message, which a doctest cannot assert; the repo has a trybuild suite (`crates/noctis/tests/ui`). Add trybuild cases with `.stderr` for payload `String` and `InteractionCreate`.
  - `[false]` `[reject]` (blind) Unrelated work bundled into the 5.6 diff — the bootstrap and placement-matrix work are separate commits (9b82e43, dfdf9ca) with their own messages; they only fall inside the baseline range.
  - `[medium]` `[defer]` (blind) Template bootstrap writes `.env` and exits even when variables are exported — real, but from commit 9b82e43, not this story.
  - `[low]` `[defer]` (blind) `.env` loaded after the tokio runtime started — real race window; from commit 9b82e43, not this story.
  - `[maybe-false]` `[defer]` (blind) Template dependency versions unverified — settle by checking crates.io/docs.rs for base64 0.23, getrandom 0.4, zeroize 1.9 and their APIs; from commit 9b82e43; medium if true.
  - `[low]` `[patch]` (blind) DM reaction/typing intents never derived and undocumented — the design (Design Notes) intends explicit DM intents, but only `MessageCreate` says so; add one sentence to the core `events` module docs and the skill naming `DIRECT_MESSAGE_REACTIONS`/`DIRECT_MESSAGE_TYPING`. (The `MESSAGE_CONTENT` part of `PRIVILEGED` is harmless.)
  - `[false]` `[reject]` (blind) Common events (Ready, Resumed, …) missing — the 64-type set is a deliberate closed table in the intent; READY/RESUMED are consumed by the shard as status events.
  - `[false]` `[reject]` (blind) `Ok(None)` handler mismatches dropped silently — `run_handler` only receives handlers looked up by `event.kind()`, and each `extract` matches its own `KIND` variant, so `None` is unreachable today (the UnavailableGuild handler is never looked up at all).
  - `[high]` `[bad_spec]` (blind) Missing tests: saturation interactions and a real `GUILD_DELETE` parse/dispatch — the missing end-to-end `GUILD_DELETE` test is what let the UnavailableGuild defect through; grouped with it, amendment adds that test.
  - `[high]` `[bad_spec]` (edge) UnavailableGuild handler never runs — same root as the first row; same amendment.
  - `[false]` `[reject]` (edge) Typing/presence stay droppable despite a non-droppable handler — intent Matrix 22 mandates `DROPPABLE_EVENTS ∪ …`.
  - `[medium]` `[bad_spec]` (edge) Handlerless parsed events (GUILD_DELETE for UnavailableGuild) occupy the lane as non-droppable — real only through the UnavailableGuild key mismatch; grouped, fixed by the dispatch-key amendment.
  - `[medium]` `[patch]` (edge) Hung ordered handler stalls its sub-queue and exhausts permits — same root as the blind hang row; same doc patch.
  - `[low]` `[reject]` (edge) Hung concurrent handler leaks its permit — see the blind timeout row.
  - `[medium]` `[defer]` (edge) Spawned handler tasks outlive `App::run` — real (bare `tokio::spawn`), but shutdown draining is Story 5.8 per the intent's Never list.
  - `[low]` `[reject]` (edge) Guildless ordered events share sub-queue 0 with some guilds — intent settles `subqueue(None) = 0` and AD-10 hashes guild ids; harm is throughput only and the fix adds a keying branch.
  - `[low]` `[reject]` (edge) Instrument/lint attrs on the descriptor fn — see the blind attribute row.
  - `[low]` `[reject]` (edge) `#[event(ordered,,droppable)]` yields an empty-name error — real but rare, and still a spanned error at the bad token.
  - `[low]` `[reject]` (edge) `#[event]` on an associated fn using `Self` — rare; rustc still errors; a check adds macro logic.
  - `[low]` `[patch]` (edge) Facade rustdoc for `droppable` omits that typing/presence are always droppable — direct doc correction in `crates/noctis/src/lib.rs` (the skill already says it).
  - `[medium]` `[patch]` (vgap) `App::run` lane config unverified at the run boundary — pre-verified; add a test that goes through `App::run` with `.event_lane(1, 0)` and an all-droppable blocked handler and observes a drop.
  - `[low]` `[patch]` (vgap) Synchronous panic in the handler-constructing call untested — pre-verified; add an ordered worker test with a closure that panics before returning its future.
  - `[medium]` `[bad_spec]` (vgap) Droppable marking by dispatched kind for UnavailableGuild unpinned — the kind/parse split it tests is the UnavailableGuild defect; the amendment folds by dispatch key and adds the mixed GuildDelete/UnavailableGuild droppable test.
  - `[low]` `[defer]` (vgap) Biased select order untested — pre-verified, filed defer; observable once Epic 6 adds interaction handlers.
  - `[medium]` `[defer]` (vgap) Template `bootstrap.rs` tests never run — pre-verified, filed defer; from commit 9b82e43, needs the template to compile (Story 14.2).
  - `[false]` `[reject]` (intent D1) GW-05 limited to privileged intents — deliberately settled (Design Notes "Privileged-only GW-05"; GW-05 rule text, FR-6 "derived").
  - `[false]` `[reject]` (intent D2) Filter test is a parse-failure-warning proxy — deliberately settled in Matrix 45; twilight `parse` skips unwanted types before deserializing (json.rs), so a failed-parse counter observes exactly the deserialization attempts.
  - `[false]` `[reject]` (intent D3) Interactions traced and dropped — the intent's Never list excludes interaction handlers; the bypass is what AC 5 asks.
  - `[false]` `[reject]` (intent D4) 64 hashed sub-queues, not strictly per-guild — AD-10 prescribes guild ids hashed to fixed sub-queues; per-guild order holds.
  - `[false]` `[reject]` (intent D5) Template still calls `ctx.kv()` — `kv()` is excluded by the intent and marked `[planned, Epic 7]` per the epics convention.
  - `[false]` `[reject]` (intent D6) Template bootstrap out of scope — separate commit 9b82e43, not part of the story's change.
  - `[false]` `[reject]` (intent D7) CV2 placement matrix out of scope — separate commit dfdf9ca.
  - `[false]` `[reject]` (intent D8) Public API beyond the ACs — the intent's Approach explicitly names `event_lane`, `handler_concurrency`, `droppable` and the payload re-exports.

### 2026-10-20 — Review pass
- verdicts: 30 findings — high 0, medium 3, low 14, false 13, maybe-false 0
- findings:
  - `[low]` `[reject]` (blind) `subqueue` shift `>> 58` not derived from `SUBQUEUES`; out-of-range index or failed `tx.send` drops a job silently — with `SUBQUEUES = 64` and the spec-prescribed formula every index is `< 64` (Matrix 38), so `ordered.get(..)` is always `Some`; `send` fails only after the sub-queue task ends at runtime shutdown. Only a future edit of the constant would expose it; guards and logs add code for an unreachable path.
  - `[low]` `[reject]` (blind) Handler failure/panic logs lack `shard` and `guild_id` — real, but the spine (L1074) puts `shard`/`guild_id` on the `noctis.event` span of Story 5.7, which the intent's Never list excludes; adding them to the event now would duplicate the span fields.
  - `[low]` `[defer]` (blind) `error = %error` drops the `source()` chain — real (Display delegates to the inner error only); full-error logging belongs to Story 6.8 / 5.7 Error Sinks, excluded by the intent's Never list. Added to `deferred`.
  - `[medium]` `[defer]` (blind) No `HandlerError` constructor from a plain message — real (`String`/`&str` are not `Error`; workaround `HandlerError::new(io::Error::other(..))`); the fix adds public API, and AD-13 plus Story 6.8 own `HandlerError`'s constructors and error-handling patterns. Grouped with edge 5; added to `deferred`.
  - `[false]` `[reject]` (blind) One `MissingIntent` per handler, not per intent — the intent contract prescribes "one `MissingIntent` per handler" and Matrix 14 expects two violations; each names a different handler, so they are not duplicates.
  - `[false]` `[reject]` (blind) `ordered` + `droppable` interaction undocumented — the `droppable` rustdoc (facade lib.rs L212-214) already says the event may be dropped; ordering then trivially covers delivered events only, no further outcome.
  - `[low]` `[patch]` (blind) Public doctests teach the hidden `EventHandler::__new` — real: `noctis::Module` re-exports core's rustdoc, so docs.rs shows `__new` and `noctis_core::` paths to bot authors (module.rs L51-63, L20-30; handler.rs L186-198). Patch: hide the `__new` construction behind rustdoc `# ` lines and point the prose at `#[event]`.
  - `[low]` `[patch]` (blind) Architecture memlog L171 states the rejected `from(KIND)` droppable design — real, contradicted by the loopback-1 line L175. Patch: mark L171's droppable/`PARSE` test sentences as superseded by L175.
  - `[low]` `[defer]` (blind) INT-13 and the spine describe the Story 5.9 Discord edge as current — real, but from commit f9e3658 (Plan Story 5.9), not this story. Added to `deferred`.
  - `[low]` `[defer]` (blind) Research doc cites probe code at an ephemeral scratchpad path — real, from commit 45140a7, not this story. Added to `deferred`.
  - `[low]` `[reject]` (blind) No test for interactions while permits are held by jobs parked behind a blocked ordered sub-queue — the bypass is structural (`select!` over `recv_interaction` regardless of where permits sit) and `interaction_bypasses_permits` pins it; the hazard is documented, a second test adds no protection.
  - `[false]` `[reject]` (blind) Unrelated planning changes (FR-21, Story 5.9, CV2 table) in the 5.6 diff — they are separate commits with their own messages (45140a7, f9e3658, dfdf9ca) that fall inside the baseline range; the proposed fix edits this spec.
  - `[low]` `[reject]` (edge) Hung concurrent handler leaks its permit — carried: a hung user future holding its permit is the normal semantics of a bounded pool; a timeout adds config surface no AD asks for.
  - `[low]` `[reject]` (edge) Drain acquires a permit before the handler lookup — carried: handlerless parsed types are unreachable (twilight `parse` skips unwanted dispatches; every parsed type has a slot after loopback 1).
  - `[medium]` `[defer]` (edge) Spawned handler tasks outlive `App::run` — carried: shutdown draining is Story 5.8 per the intent's Never list (already in `deferred`).
  - `[low]` `[reject]` (edge) `let _ = tx.send(job)` drops an event silently — `send` fails only once the sub-queue task has ended, which happens only at runtime shutdown (the code comment says so); a log there adds code for a shutdown-only path.
  - `[medium]` `[defer]` (edge) `?` on `Box<dyn Error + Send + Sync>` / `anyhow::Error` does not compile — real (neither implements `Error`); same root as the blind `HandlerError` constructor row; deferred to Story 6.8 with it.
  - `[low]` `[reject]` (edge) `#[event]` attributes land on the descriptor fn — carried: spec-prescribed; only `#[tracing::instrument]` misses and 5.7 adds `noctis.event` spans.
  - `[low]` `[reject]` (edge) Recursive self-call / `Self` inside an `#[event]` body — carried: rare, rustc still errors (the zero-arg descriptor fn or nested-fn `Self`); a check adds macro logic.
  - `[low]` `[reject]` (edge) `#[event(,)]` accepted as no flags — real (parse_args pops the trailing empty segment), but rare and the outcome equals an empty argument list; a guard adds a branch.
  - `[false]` `[reject]` (edge) Payload trybuild cases in `tests/types` skipped when a facade feature is on — they run in the same `ui()` test as `tests/ui` (ui.rs L47), under the same featureless gate the spec's "existing suite" has; the directory is required because `rules-catalog` admits only `<RULE-ID>_` fixtures in `tests/ui` (memlog L176).
  - `[false]` `[reject]` (intent 1) Out-of-story planning changes in the diff — separate commits 45140a7 and f9e3658 with their own messages, inside the baseline range only.
  - `[false]` `[reject]` (intent 2) Template bootstrap and CV2 placement matrix in the diff — carried: separate commits 9b82e43, dfdf9ca.
  - `[false]` `[reject]` (intent 3) AC1 counting via parse-failure warnings — carried: settled in Matrix 45; twilight `parse` skips unwanted types before deserializing.
  - `[false]` `[reject]` (intent 4) GW-05 privileged-only — carried: Design Notes "Privileged-only GW-05", FR-6 "derived".
  - `[false]` `[reject]` (intent 5) 64 hashed sub-queues — carried: AD-10 prescribes guild ids hashed to fixed sub-queues.
  - `[false]` `[reject]` (intent 6) Interactions bypass permits and are traced and dropped — carried: Never list excludes interaction handlers; the biased-select test is already deferred.
  - `[false]` `[reject]` (intent 7) Ancestor paths stored but unused — the AC says "records its ancestor path"; Middleware ordering (7.6) is excluded by the Never list, and Matrix 4-5 pin the recorded paths.
  - `[false]` `[reject]` (intent 8) Template still calls `ctx.kv()` and is not compiled — carried: `kv()` excluded by the intent, marked `[planned, Epic 7]`.
  - `[false]` `[reject]` (intent 9) `UnavailableGuild` dispatched from `GUILD_DELETE` — deliberately settled by Spec Change Log 1 and Design Note "Dispatch key", pinned by registry, worker and `unavailable_guild_e2e` tests.

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
- **Dispatch key (Spec Change Log 1):** a handler is dispatched on the single `EventType` whose flag equals its `PARSE`; that type is its *dispatch key*. It equals `KIND` for 63 payloads; for `UnavailableGuild` it is `EventType::GuildDelete`, because twilight-model 0.17.1 deserializes every `GUILD_DELETE` into `Event::GuildDelete` and never constructs `Event::UnavailableGuild`. The sealed trait carries it explicitly as `const DISPATCH: EventType` (set in the `macro_rules!` table next to `KIND`/`PARSE`; a unit test asserts `EventTypeFlags::from(DISPATCH) == PARSE` for all 64). `EventHandler` stores `dispatch` next to `kind`. The Registry keys handler lists, the droppable fold and `handlers(..)` by `dispatch`; the worker keeps looking up `event.kind()`, which now always matches. `KIND` stays the user-facing name: `MissingIntent.event` and the `event_type` log field use `KIND.name()` (an `UnavailableGuild` handler failure logs `event_type = "UNAVAILABLE_GUILD"`).
  - `UnavailableGuild`'s sealed `extract` is hand-written: `Event::GuildDelete(d)` with `d.unavailable == Some(true)` → `Some(UnavailableGuild { id: d.id })`; any other `GuildDelete` → `None`; `Event::UnavailableGuild(u)` → `Some(u)` (kept for completeness). Every other payload's `extract` matches only its own `KIND` variant.
  - `None` from `extract` is a normal, silent skip of that handler for that event (no log, no error) — it is now reachable for `UnavailableGuild` on a guild the bot left. A handler slot with every handler skipped still releases its permit normally.
  - Doc sentence (core `events` module docs, `noctis::events`, the skill's intents/payload section): "`UnavailableGuild` handlers run on a `GUILD_DELETE` dispatch with `unavailable: true` (an outage); a guild the bot left or was removed from reaches only `GuildDelete` handlers."
- **DM intents:** `DIRECT_MESSAGES`, `DIRECT_MESSAGE_REACTIONS` and `DIRECT_MESSAGE_TYPING` are never derived (the table derives only guild intents) and must be requested with `.intents(..)`; one sentence saying so goes in the core `events` module docs and the skill's intents section.
- **Ordered hang hazard:** a hung `ordered` handler blocks its sub-queue (every guild that hashes to it, and every guildless event in sub-queue 0), and the jobs queued behind it keep their permits, so it can eventually hold all `handler_concurrency` permits and stop event handling. No timeout is added (no AD asks for one); one short paragraph states this in the `#[event]` rustdoc (`ordered`) and in the skill's ordering section, advising bot authors to bound slow work in ordered handlers themselves (e.g. `tokio::time::timeout`).
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

## Auto Run Result

- **Summary:** Story 5.6 delivers `Module` trees (pre-order fold with recorded ancestor paths), typed event handlers over a sealed `EventPayload` set of 64 Discord event types via `#[event]` (flags `ordered`, `droppable`), gateway event filters and intents derived from registered handlers, privileged-only GW-05 `MissingIntent` startup violations, `event_lane`/`handler_concurrency` builder bounds, and a worker set: bounded permits, interactions bypassing permits through a biased select, 64 hashed per-guild ordered sub-queues, and `catch_unwind` panic isolation. Handlers are keyed by the dispatched type (`UnavailableGuild` is extracted from `GUILD_DELETE` with `unavailable == Some(true)`, decided in bad_spec loopback 1).
- **Files changed:** `crates/noctis-core/src/{events.rs,handler.rs,module.rs,registry.rs,app.rs,lane.rs}` (payloads, handlers, modules, registry, workers); `crates/noctis-macros` (`#[event]`); `crates/noctis/src/lib.rs` (facade re-exports and docs); `crates/noctis/tests/{ui,types}` (trybuild cases); `crates/noctis-testkit` E2Es (Matrix 45-47, `unavailable_guild_e2e`); `docs/discord-rules.md` and noctis-rules GW-05; `examples/project-template/` (code, AGENTS.md, usage skill, API hash); `xtask/public-api.txt`, `xtask/rules-pending.toml`, `xtask/src/api_snapshot.rs`; architecture memlog.
- **Review findings, pass 1 (39):** 3 high, 8 medium, 15 low, 12 false, 1 maybe-false. Bad_spec loopback 1 for the `UnavailableGuild` dispatch-key defect (it covered the 3 high and 2 medium rows). Patches: ordered-hang hazard docs, `App::run` lane-config test, sync-panic `catch_unwind` test, trybuild `on_unimplemented` cases, DM-intents sentence, `droppable` rustdoc clause. Defers: template bootstrap `.env` issues, unverified template dependency versions and untested `bootstrap.rs` (all from commit 9b82e43), handler tasks outliving `App::run` (Story 5.8), biased-select order test (Epic 6). Rejects: the reason for each one is recorded in the triage log (spec-prescribed attrs/diagnostics, unreachable paths, deliberate closed event set, separate commits, settled intents).
- **Review findings, pass 2 (30):** 0 high, 3 medium, 14 low, 13 false. Patches (2 low): hid `EventHandler::__new` in public doctests, marked the superseded memlog line. Defers: `HandlerError` message constructor / `anyhow` interop (Story 6.8), `source()` chain logging (5.7/6.8), the INT-13/spine wording for Story 5.9, the research doc's scratchpad path. Rejects: the reason for each one is recorded in the triage log.
- **Follow-up review recommended:** false. This pass patched 0 high, 0 medium and 2 low entries.
- **Verification:** all local gates in `gates.sh` pass: fmt, clippy `-D warnings`, the release workspace tests, `cargo hack --each-feature`, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, and the MSRV 1.91 check. CI is paused by the maintainer.
- **Residual risks:** spawned handler tasks are not drained at shutdown (5.8). A hung ordered handler can absorb every permit (documented, no timeout). No ergonomic `HandlerError` from a message (6.8). The biased-select order is untested until Epic 6.
