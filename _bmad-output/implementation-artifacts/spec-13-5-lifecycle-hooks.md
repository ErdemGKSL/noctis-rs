---
title: 'Story 13.5: Lifecycle Hooks'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-54 has no public surface: core already turns `ShardEvent`s and its own shutdown into a crate-private `LifecycleEvent` broadcast (`crates/noctis-core/src/lifecycle.rs`), but nothing lets a bot author react to App start, shard state, Publish, Module toggles or shutdown, and the Usage Skill drafts `#[derive(Hook)]` as if it existed.

**Approach:** Add a sixth kind derive, `#[derive(Hook)]` (optional `#[hook(flags = [..])]`) on a unit struct whose `#[handler]` method is `async fn handle(self, ctx: HookCtx, event: E) -> HandlerResult<()>`; the payload type `E` (one of ten sealed `noctis::lifecycle` types) selects the event. Modules register hooks with `.add::<T>()`; the Registry folds them; core fires every occurrence into one per-App ordered queue drained by a single hook-runner task while the App runs (inline when it does not), each hook under `catch_unwind`, failures to a new `ErrorOrigin::Hook` Error Sink.

## Boundaries & Constraints

**Always:**
- Events and payloads (all in `noctis_core::lifecycle`, re-exported as `noctis::lifecycle`; each a `#[non_exhaustive]` struct with `pub` fields, `Debug, Clone, PartialEq, Eq`, plus a `const NAME: &'static str` through the sealed trait `HookEvent: Sealed + Clone + Send + Sync + 'static`):
  `AppStarted { publisher: bool }` (`APP_STARTED`), `ShardReady { shard: u32 }` (`SHARD_READY`), `ShardResumed { shard }` (`SHARD_RESUMED`), `ShardDisconnected { shard, close_code: Option<u16> }` (`SHARD_DISCONNECTED`), `ShardFatal { shard, close_code: Option<u16> }` (`SHARD_FATAL`), `AllShardsReady { shards: Range<u32>, total: u32 }` (`ALL_SHARDS_READY`), `PublishCompleted { scope: PublishScope, clear: bool, report: PublishReport }` (`PUBLISH_COMPLETED`; the diff is `report.targets()[i].diff()`), `PublishFailed { scope, dry_run: bool, clear: bool, error: PublishError }` (`PUBLISH_FAILED`), `ModuleToggled { module: &'static str, scope: ModuleScope, enabled: bool, reset: bool }` (`MODULE_TOGGLED`; `enabled` = the Module's own state in `scope` after the change), `ShutdownStarted { reason: ShutdownReason }` (`SHUTDOWN_STARTED`), with `#[non_exhaustive] enum ShutdownReason { Signal, Requested, AllShardsEnded, StartupFailed }` (`as_str`: `"signal"`, `"requested"`, `"all shards ended"`, `"startup failed"`; `Display`).
- Firing points (only core fires; the gateway never does, AD-21): `AppStarted` in `run_with` after the locale overlays and the startup Module-state load, before the startup Publish; `PublishCompleted`/`PublishFailed` when any `App::publish(..)` future resolves (startup, CLI modes, author code; dry runs and clears included); the four shard events from `Lifecycle::shard` (where `LifecycleEvent` already maps them; `Closed` maps to nothing); `AllShardsReady` once per run when `await_startup` returns `Ok`, before `supervise`; `ModuleToggled` from `Modules::set_enabled`/`reset` only when the record actually changed, after the snapshot update and after the write mutex is released; `ShutdownStarted` exactly once per run in which `AppStarted` fired: in `shut_down` (reason from `Stop`/`reason`: `"signal"` → `Signal`, `"requested"` → `Requested`, `Stop::AllEnded` → `AllShardsEnded`), in the early-shutdown branch of `run_with` (`Requested`/`Signal`) only if `AppStarted` already fired, and on every `run_with` error return after `AppStarted` (`StartupFailed`).
- Delivery: while `run_with` runs, `fire` enqueues into an unbounded `tokio::sync::mpsc` (never blocks, never drops, never fails) read by one runner task; the runner runs, per event in arrival order, every hook registered for that event in Registry fold order (pre-order, `add` order, plugins included), one at a time, and starts the next event only after the previous event's hooks finished. An event with no registered hook enqueues nothing. Outside a run (no runner attached), `fire` runs the same hooks inline and the caller (`Publish`'s future, `Modules::set_enabled`/`reset`) awaits them before returning; the inline path returns a boxed future so a hook that toggles a Module recurses safely.
- Each hook run: span `noctis.hook` (`app`, `module`, `handler`, `hook_event` = NAME, `shard` for shard events); start and poll under `AssertUnwindSafe(..).catch_unwind()` (AD-13); records `noctis_handler_duration_seconds{app, kind="hook"}`; an `Err` (user conditions included) or a panic increments `noctis_handler_errors_total{app, kind="hook"}` and goes to the `ErrorOrigin::Hook` sink as `ErrorReport { origin: Hook, kind: Error|Panic, app, shard: Some(shard) for shard events else None, guild_id: None, module: Some(module), handler: Some(T::NAME), event_type: Some(E::NAME), extensions: None, middleware: None }`; later hooks and the App keep running.
- `ErrorOrigin::Hook` (`as_str` `"hook"`, appended last to `ALL`, now 6); `LogSink` logs it at `error` with target `noctis::hook` and messages `"hook handler failed"`, `"hook handler panicked"`, `"hook handler cancelled at shutdown"`.
- End of a run (every `run_with` return after the runner started): the queue closes after `watch.flush()` (or at the error return), and `run_with` awaits the runner until `max(ShutdownStarted fire time + shutdown_grace, now + 1 s)`; then the hook still running is cancelled and reported once as `ReportKind::Cancelled`, and queued events are discarded with one `warn` on `noctis::hook` (`"lifecycle hooks discarded at shutdown"`, field `events` = count). Dropping the `run_until` future aborts the runner (no `ShutdownStarted`).
- `HookCtx` (cheap clone, one `Arc`): `module() -> &'static str`, `app_id() -> &str`, `state::<T>() -> Option<State<T>>`, `http()`, `gateway()`, `kv()`, `cache()`, `modules()`, `webhooks()`; `Debug` prints module and states, never the HTTP client.
- Hooks run no Middleware, no locale stage, no rate limit; they run whether or not their Module is enabled at runtime (Module enablement gates interactions and events only); Boot Flags filter them like every registration (an unselected hook is never registered and never fires).
- Every public API change updates the Usage Skill, `examples/project-template/` (code, `AGENTS.md`), re-blesses `xtask/public-api.txt` and sets `SKILL.md`'s `API hash` in the same change (AGENTS.md policy).

**Never:**
- No hook fired by `noctis-gateway`; no new dependency; no change to the startup order (Publish stays before `GET /gateway/bot`); no `broadcast` (lag would lose events) on the hook path; no per-hook timeout; no concurrent hook runs; no `ModuleToggled` for changes another process wrote (seen only through the version poll); no Discord rule id (no `docs/discord-rules.md` row).
- No `ordered`/`droppable`/`rate_limit`/`auto_defer`/`response` on `#[hook]`; no free-function hooks; no `Ext<T>` parameters on hooks.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| H1 bare derive | `#[derive(Hook)] struct Up;` + `#[handler] async fn handle(self, ctx: HookCtx, ev: AppStarted) -> HandlerResult<()>` | registers unflagged; `descriptor()` Debug names handler and event `APP_STARTED` | — |
| H2 empty attr | `#[hook]` or `#[hook()]` | same as H1 | — |
| H3 flags | `#[hook(flags = ["beta"])]` | `Module::__add_flagged(&["beta"], ..)`; selection `none` drops it | — |
| H4 flag grammar | `flags = ["Beta"]`, `["all"]`, 33-char flag, `["a","a"]`, unicode `["bêta"]` | compile errors identical to `#[event(flags = ..)]` (Story 13.2) | compile error |
| H5 unknown arg | `#[hook(ordered)]`, `#[hook(x = 1)]` | ``unknown `#[hook]` argument `ordered`: expected `flags = ["beta"]` `` | compile error |
| H6 duplicate | `#[hook(flags = ["a"], flags = ["b"])]` | ``duplicate `#[hook]` argument `flags` `` | compile error |
| H7 two attrs | `#[hook] #[hook]` | `a struct takes one #[hook(..)] attribute` | compile error |
| H8 name-value | `#[hook = "x"]` | ``#[hook]` takes `flags = [..]`: `#[hook(flags = ["beta"])]` `` | compile error |
| H9 not unit | tuple/named struct, enum, generic struct | ``#[derive(Hook)]` needs a unit struct`` | compile error |
| H10 no handler | derive without `#[handler]` | ``Up` has no #[handler] method, so it cannot be registered` + help `#[handler] async fn handle(self, ctx: HookCtx, event: AppStarted) -> HandlerResult<()>` | compile error (E0080) |
| H11 wrong payload | `ev: MessageCreate` or `ev: String` | `` `MessageCreate` is not a lifecycle event`` (note: use a type from `noctis::lifecycle`) | compile error |
| H12 wrong shape | `(self, ctx: HookCtx)`, `(self, ctx, ev, r: CommandResponder)` | `lifecycle hooks take (self, ctx: HookCtx, event: <lifecycle event>)` | compile error |
| H13 Ext param | `(self, ctx: HookCtx, ev: AppStarted, user: Ext<U>)` | error text contains `lifecycle hooks run no Middleware` | compile error |
| H14 return type | `-> HandlerResult<u8>` or `-> ()` | type error | compile error |
| H15 other hooks | `#[on_rate_limited]` / `#[on_stale]` method on a Hook struct | INT-14 / not-a-hook-struct error (as on `EventHandler`) | compile error |
| R1 no hooks | App without hooks runs | no runner work, no queue item, no allocation per event | — |
| R2 fold order | hooks for `ShardReady` in `ops`, `ops/alerts` (child), plugin Module | run ops → ops/alerts → plugin, sequentially, each once | — |
| R3 ordering | `ShardReady` hooks still running when `AllShardsReady` fires | `AllShardsReady` hooks start after every `ShardReady` hook returned | — |
| R4 hook error | hook returns `Err(HandlerError::new(io::Error))` (or `HandlerError::user(k)`) | report as in Always, next hook runs, App keeps running, errors counter +1 | Error Sink `Hook` |
| R5 panic on poll | hook panics after an `.await` | `ReportKind::Panic(msg)`; runner survives | Error Sink |
| R6 panic at start | `handle` body panics before its first `.await` | same as R5 | Error Sink |
| R7 sink choice | `error_sink(ErrorOrigin::Hook, s)` set / unset | `s` receives / `LogSink` logs on `noctis::hook` | panicking sink caught (existing) |
| R8 AppStarted | `run_until` with a token, overlays and Module state OK | `AppStarted { publisher }` first, once | — |
| R9 no start | missing token, bad overlay, overlay `Load` error | no hook fires at all | `run` returns the existing error |
| R10 startup Publish | `publisher(true)`, fake answers OK | `AppStarted`, then `PublishCompleted { scope: Global, clear: false, report }` (diff lines inside), then shard events | — |
| R11 Publish fails | fake answers 500 on `GET /applications/@me` | `PublishFailed { scope: Global, dry_run: false, clear: false, error: Rejected{..} }`; App keeps starting | logged as today |
| R12 shard events | READY, RESUMED, close 4000 (Disconnected `Some(4000)`), no close frame (`None`), close 4014 after startup | `ShardReady`, `ShardResumed`, `ShardDisconnected`, `ShardFatal { close_code: Some(4014) }`; requested `Closed` fires nothing | — |
| R13 all ready | 2-shard range, READY for 0 then 1, later reconnect + RESUMED | `AllShardsReady { shards: 0..2, total: 2 }` once, after the last `ShardReady`; not again | — |
| R14 fatal at startup | close 4014 before every shard is ready | `ShardFatal`, then `ShutdownStarted { StartupFailed }`; `run` returns `StartError::DisallowedIntents` after the hooks drained (bounded) | as today |
| R15 toggle | `set_enabled("profile", Guild(g), false)` on an enabled Module | `ModuleToggled { module: "profile", scope: Guild(g), enabled: false, reset: false }` once | — |
| R16 no-op toggle | same call again (record unchanged) | nothing fires | — |
| R17 reset | `reset("profile", Guild(g))` removing that override (global enabled) | `ModuleToggled { enabled: true, reset: true }`; reset with nothing to remove fires nothing | — |
| R18 bad toggle | unknown id, port error | nothing fires | `ModuleStateError` as today |
| R19 remote toggle | another App writes the store; this one polls it | nothing fires here | — |
| R20 shutdown | `run_until` stop / SIGTERM / last shard ended | `ShutdownStarted { Requested / Signal / AllShardsEnded }` once, last hook of the run (shard events seen during closing may follow it) | — |
| R21 early stop | shutdown resolves before `AppStarted` / after `AppStarted` but before `GET /gateway/bot` | nothing / `ShutdownStarted { Requested }` | — |
| R22 slow hook at end | a `ShutdownStarted` hook sleeps past the deadline; grace 0 | cut after `max(deadline, now + 1 s)`: one `Cancelled` report; queued rest discarded with one warn | Error Sink |
| R23 dropped run | the `run_until` future is dropped | runner aborted; no further hook | — |
| R24 outside a run | `app.publish(..).dry_run(true).await` or `app.modules().set_enabled(..)` before `run` | hooks run inline before the call returns (`report.is_dry_run()` true) | — |
| R25 re-entrant | a `ModuleToggled` hook calls `ctx.modules().set_enabled(..)` on another Module | in a run: enqueued, runs after; inline: nested run; no deadlock | — |
| R26 disabled Module | hook's Module disabled globally | hook still runs | — |

</intent-contract>

## Code Map

- `crates/noctis-core/src/lifecycle.rs` -- crate-private `LifecycleEvent` (Copy enum), `Lifecycle` (broadcast, `send`, `shard`, `#[cfg(test)] subscribe`), `LIFECYCLE_CAPACITY`. Becomes the public module: payload types, `HookEvent` (sealed), `ShutdownReason`, `HookCtx`, `LifecycleHook` descriptor (erased `Fn(HookCtx, &Fired) -> Option<BoxFuture<'static, HandlerResult<()>>>`, `Debug` = handler + event NAME), runner + inline `run_hooks`. Keep the broadcast tap so `app/tests.rs` (L2000-2530, `supervised`, `ShardFatal` asserts) and `worker/tests.rs` L1266 keep passing; the internal enum may change from `Copy` to carry payloads (update those tests if so). `lib.rs` L232 `mod lifecycle;` → `pub mod lifecycle;`.
- `crates/noctis-core/src/app.rs` -- L178 `lifecycle` field on `App` (move into `AppShared`), L745 `subscribe_lifecycle`; `run_with` L1022-1120 (early-shutdown branch L1035-1045, the `info` future L1047-1054: insert `AppStarted` after `start_module_state`, before `publish_at_startup`; `await_startup` result L1085, `supervise`, `shut_down` L1140-1176 with its `ShutdownStarted` send L1155); `await_startup` L1443, `supervise` L1497, `on_shard_event` L1543, `Watch` L1573 (all take `&Lifecycle`); `publish_at_startup` L1241; `AppShared` construction ~L3063 (`lifecycle: Lifecycle::new()` L3110 moves there). `Stop` enum and `reason: &'static str` map to `ShutdownReason`.
- `crates/noctis-core/src/handler.rs` L203 `AppShared` -- add `lifecycle: crate::lifecycle::Lifecycle`; `EventCtx` L249-396 is the model for `HookCtx`; `EventHandler` L398-560 the model for `LifecycleHook`. `crates/noctis-core/src/test_support.rs` also builds `AppShared`.
- `crates/noctis-core/src/publish.rs` L392 `IntoFuture for Publish` / L420 `execute` -- fire `PublishCompleted`/`PublishFailed` after `execute` resolves, inside the returned future (needs `Arc<AppShared>` via `app.shared()`).
- `crates/noctis-core/src/module_state.rs` L376 `State::write` (returns whether the record changed and the own state; the `info!` at L400 stays), L477 `Modules { state }` → hold the App's `Arc<AppShared>` (or the state plus a lifecycle handle) so `set_enabled` L512 / `reset` L531 fire after `write` returns; detached `Modules` fires nothing.
- `crates/noctis-core/src/kind.rs` -- L44 `__Kind` (doc/note lists the derives: add `Hook`), L65 marker pattern (add `__LifecycleKind`; the name `__HookKind` is taken by the stale/expired machinery L665), L387-451 events `__EventHandler`/`__EventParams`/`__Bind` impls (copy for `__LifecycleHook` + `__HookParams`, payload bound `E: HookEvent`), L809 `no_hooks!` (add `__LifecycleKind`), L863 `attach_nothing!` (add `LifecycleHook`), L958 rate-limited `EventHandler` impl (same for `LifecycleHook`), L1099 `require_extensions!` (do NOT add `LifecycleHook`; give the failure the H13 text, e.g. an `on_unimplemented` on a hook-specific `__Extensions`-style bound).
- `crates/noctis-core/src/module.rs` -- L22-71 sealed `Descriptor` impls, L186 `Module` fields (add `hooks: Flagged<LifecycleHook>`), `selected` L651, `registration_flags` ~L631, `Debug` L698, accessor list L565-625.
- `crates/noctis-core/src/registry.rs` -- `fold` L164 (collect hook entries `{ module, hook }` in the same pre-order pass L180-230; store by event NAME/kind for O(1) lookup), add `hooks_for(..)`.
- `crates/noctis-core/src/worker.rs` L456 `run_handler` (catch_unwind/report/metrics pattern), L622 `record_duration`, L630 `count_error`, L640 `panic_message`.
- `crates/noctis-core/src/sink.rs` -- `ErrorOrigin` L27 (+`Hook`, `ALL`, `as_str`, `index`, doctest `ALL.len() == 6`), `log_report!`/`LogSink` L287.
- `crates/noctis-core/src/observe.rs` L83-84 table rows (kind values now include `hook`).
- `crates/noctis-core/src/lib.rs` L266-360 re-exports (`HookCtx`, `LifecycleHook` at crate root like `EventHandler`; hidden `__LifecycleKind`, `__LifecycleHook`, `__HookParams` via `kind`), `crates/noctis/src/lib.rs` L1261 `EventHandler` derive re-export pattern (add `pub use noctis_macros::Hook;` with a doctest), L1519 `pub mod events` pattern (add `pub mod lifecycle`), L2311 prelude (+`Hook`, `HookCtx`).
- `crates/noctis-macros/src/event.rs` -- the template for new `crates/noctis-macros/src/lifecycle.rs` (args: only `flags`); `crates/noctis-macros/src/kind.rs` `Family` (+`Hook`: marker, descriptor `LifecycleHook`, help signature, missing-handler text); `crates/noctis-macros/src/lib.rs` L222 (`#[proc_macro_derive(Hook, attributes(hook))]`); `crates/noctis-macros/src/handler.rs` L109 `struct_form` (add the `HookCtx` form), L38 `PARAMS`.
- Tests to copy: `crates/noctis-testkit/tests/shutdown.rs` (FakeGateway + FakeDiscordHttp + `globals()`/`serial()` capture harness), `gateway_close.rs`, `module_toggle.rs`, `publish.rs`; `FakeGateway::close(code, reason)` closes every live connection (4000 → Disconnected + Resumed; 4014 → Ended). UI: `crates/noctis/tests/types/*.rs` (+`.stderr`) holds non-rule compile-fail cases (`event_payload_string.rs` pattern); `crates/noctis/tests/events.rs`, `error_sinks.rs` facade tests.
- Skill/template: `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` §7 L783-807 (draft API: replace) and §8 L816 (`[planned, Story 13.5 ..]`), `modules-middleware-ratelimits.md` L47 (`#[hook]` joins later [planned]) and L274-275 (Hook [planned]), `SKILL.md` L23 `API hash` and L487 index row, `testing.md` (add a hook test recipe); `examples/project-template/src/modules/mod.rs`, `src/lib.rs` L70-72 (`.module(..)` chain), `AGENTS.md` (bullet list near L64).
- Spine `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` AD-21 (L955-1000) and AD-13 (L630-640); memlog `.memlog.md` in the same folder.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/sink.rs` -- add `ErrorOrigin::Hook` per Always; update doctests and `sink/tests.rs` -- hook failures need their own sink.
- `crates/noctis-core/src/lifecycle.rs` (+ new `lifecycle/tests.rs`) -- public payloads, `HookEvent`, `ShutdownReason`, `HookCtx`, `LifecycleHook` (`#[doc(hidden)] pub fn __new::<E, F, Fut>(handler: &'static str, f: F)`), `Lifecycle` (attach/detach runner sender, `fire` enqueue-or-inline, broadcast tap), runner loop and end-of-run drain per Always -- the FR-54 engine; unit-test R1-R7, R22-R25 with hand-built descriptors.
- `crates/noctis-core/src/kind.rs`, `module.rs`, `registry.rs`, `lib.rs` -- `__LifecycleKind`, `__LifecycleHook { fn __new() -> Self }`, `__HookParams`, `__Bind<__CX<E>>` + rejecting shapes, attach/rate-limit no-op impls, `Module` storage + Boot Flag filtering + Debug, Registry hook index in fold order -- registration surface.
- `crates/noctis-core/src/handler.rs`, `app.rs`, `test_support.rs` -- `AppShared.lifecycle`; `run_with` firing points (`AppStarted`, `AllShardsReady`, `ShutdownStarted` with reason on every path, runner start/stop/drain, `AbortOnDrop` on the runner) -- run integration (R8, R9, R13, R14, R20-R23).
- `crates/noctis-core/src/publish.rs` -- fire Publish hooks on every resolved Publish -- R10, R11, R24.
- `crates/noctis-core/src/module_state.rs` -- `write` reports the change; `Modules` fires `ModuleToggled` -- R15-R19.
- `crates/noctis-core/src/observe.rs` -- doc table: `kind` includes `hook` for the two handler metrics.
- `crates/noctis-macros/src/lifecycle.rs` (new), `kind.rs`, `handler.rs`, `lib.rs` -- `#[derive(Hook)]` + `#[hook(flags = [..])]` with H1-H10 errors (unit tests in the new file, mirroring `event.rs` tests) -- compile-time tier.
- `crates/noctis/src/lib.rs` -- `pub use noctis_macros::Hook` (doctest with a real hook), `pub mod lifecycle` (re-export every payload, `HookEvent`, `ShutdownReason`, `HookCtx`, `LifecycleHook`; module doc lists each event, when it fires and its data), prelude `Hook`, `HookCtx` -- public API.
- `crates/noctis/tests/types/hook_*.rs` + `.stderr` -- H9, H11, H12, H13, H15 (bless with `TRYBUILD=overwrite` once, review); `crates/noctis/tests/hooks.rs` (new) -- real derives: Debug of descriptors, Boot Flag selection keeps/drops a flagged hook, `Module` Debug lists hooks.
- `crates/noctis-testkit/tests/lifecycle.rs` (new; own binary, copy the `shutdown.rs` harness) -- end to end over FakeGateway/FakeDiscordHttp with real `#[derive(Hook)]` structs pushing `(NAME, data)` into a `State` probe: (a) `publisher(true)`, 1 shard: close 4000 then toggle via a pre-cloned `app.modules()` then stop → exact sequence `APP_STARTED, PUBLISH_COMPLETED, SHARD_READY{0}, ALL_SHARDS_READY{0..1,1}, SHARD_DISCONNECTED{0,Some(4000)}, SHARD_RESUMED{0}, MODULE_TOGGLED{..}, SHUTDOWN_STARTED{Requested}` (each once; `PUBLISH_COMPLETED.report.targets()[0].diff()` contains `+ create chat_input`); (b) close 4014 after startup → `SHARD_FATAL{0,Some(4014)}` then `SHUTDOWN_STARTED{AllShardsEnded}`; (c) failing Publish → `PUBLISH_FAILED`; (d) a hook that errors and one that panics → two `ErrorOrigin::Hook` reports, later hooks still observed, App still answers a dispatched event; (e) R14 and R22.
- Spine + memlog -- append "Amended (2026-10-24, Story 13.5)" to AD-21 (hook queue: one ordered runner per run, inline outside a run, `ShutdownStarted` pairing, end-of-run drain) and AD-13 (`ErrorOrigin::Hook`); add `(decision)` lines for the Design Notes decisions.
- Template + Skill -- new `examples/project-template/src/modules/ops/mod.rs` (`module()` = `Module::new("ops")` with hooks `Online` on `AllShardsReady` → `tracing::info!`, `ShardDown` on `ShardFatal` → `tracing::error!`, `ModuleAudit` on `ModuleToggled` → `tracing::info!`), mounted in `src/modules/mod.rs` and `src/lib.rs`; rewrite `app-and-sharding.md` §7 for the shipped API (every event, when it fires, its data, ordering, inline-outside-a-run, runtime enablement ignored, Boot Flags, Error Sink `Hook`, end-of-run drain), drop the §8 `[planned]` note, fix `modules-middleware-ratelimits.md` L47/L274, `SKILL.md` index row, add a hook test recipe to `testing.md`, add one `AGENTS.md` bullet ("react to App start, shards, Publish, Module toggles and shutdown with `#[derive(Hook)]` …, never poll"), re-bless `xtask/public-api.txt` and set `API hash` -- AGENTS.md policy.

**Acceptance Criteria:**
- Given `#[derive(Hook)]` structs added to a Module of a running test App with `publisher(true)`, when it starts, publishes, has its shard disconnected and resumed and is stopped, then the hooks observe, in that order and exactly once each, `AppStarted`, `PublishCompleted` (with the diff), `ShardReady`, `AllShardsReady`, `ShardDisconnected`, `ShardResumed`, `ShutdownStarted { reason: Requested }`, each with typed data.
- Given a Module enabled or disabled globally or in one guild through `Modules`, when the change is applied, then the `ModuleToggled` hook fires once with that scope, and an unchanged write fires nothing.
- Given a shard closing fatally after startup, when core receives `ShardEvent::Ended`, then `ShardFatal` fires with its close code (noctis-gateway contains no hook code).
- Given a hook that returns an error and another that panics, when they run, then each failure reaches the `ErrorOrigin::Hook` Error Sink with module, handler and event name, and the App keeps dispatching.
- Given the new public API, when the story is done, then the Usage Skill documents every hook event and its data, the template's `ops` Module compiles, and `api-snapshot`, `skill-check` and `rules-catalog` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Order (decision):** the epic lists Publish after "all Shards ready", but the startup Publish runs before `GET /gateway/bot` (Story 6.x, documented). Hooks fire in occurrence order, so a publisher's run reads `AppStarted, PublishCompleted, ShardReady.., AllShardsReady, .., ShutdownStarted`; moving the Publish would change shipped behavior for no FR.
- **Queue, not broadcast (decision):** the existing broadcast lags at 256 and would break exactly-once; one unbounded mpsc and one runner give total order with no back-pressure on shard tasks. Sequential hooks mean a slow hook delays later ones; the Skill tells authors to `tokio::spawn` long work.
- **Inline outside a run (decision):** `run_until(self)` consumes the App, so in-run Publishes are only the startup one; CLI `--publish*` modes and `App::publish` in author code run without a runner, and audit hooks must still see them.
- **Local toggles only (decision):** "once per occurrence" is per write through this App's handle; diffing polled snapshots would fire N times across a Cluster.
- **Runtime enablement ignored (decision):** an `ops` Module's alert hooks must keep working, and a toggled Module must be able to observe its own re-enable.
- **Example:**
  ```rust
  use noctis::lifecycle::{AllShardsReady, ModuleToggled};
  #[derive(Hook)]
  pub struct Online;
  impl Online {
      #[handler]
      async fn handle(self, ctx: HookCtx, ev: AllShardsReady) -> HandlerResult<()> {
          tracing::info!(module = ctx.module(), shards = ev.shards.len(), total = ev.total, "online");
          Ok(())
      }
  }
  // Module::new("ops").add::<Online>()
  ```

## Verification

Run from `/home/claude/lanes/b/noctis-rs`, release only (wrap workspace-wide runs in `tools/story-loop/bg.sh`):

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo test --release -p noctis-macros` and `cargo test --release -p noctis-core` -- expected: pass (new lifecycle, sink, registry, module_state tests)
- `cargo test --release -p noctis --test ui` -- expected: pass with the new `tests/types/hook_*` fixtures
- `cargo test --release -p noctis --test hooks` and `cargo test --release -p noctis-testkit --test lifecycle` -- expected: pass
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, hash equals `SKILL.md` `API hash`
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- expected: exit 0
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` and `cargo +1.91 check --release --workspace` -- expected: exit 0
- `cargo check --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template` -- expected: template compiles (delete `target/template` if disk is tight)
- `cargo hack` -- not run by hand: this is Epic 13's last story, so `tools/story-loop/gates.sh` runs it
