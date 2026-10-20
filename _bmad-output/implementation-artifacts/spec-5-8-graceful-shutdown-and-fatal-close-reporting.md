---
title: 'Story 5.8: Graceful shutdown and fatal-close reporting'
type: 'feature'
created: '2026-10-20'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-7-observability-contract-and-error-sinks.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
  - '{project-root}/examples/project-template/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-1 and AD-21 require a graceful shutdown and loud fatal closes, and neither exists. `App::run` never watches Ctrl-C/SIGTERM. The gateway cannot close its shards on request. Handler tasks are detached, so nothing waits for them or cancels them. A shard that closes fatally after startup only gets one `error!` line (`"shard ended"`) and no lifecycle signal. When every shard has died, `run` returns `Ok(())`.

**Approach:** Add `Gateway::close_all(CloseMode)` and `Gateway::closed()`. Track every handler task with a `TaskTracker` and a `CancellationToken`. Implement the AD-21 order in a new `App::run_until(shutdown)`; `App::run` becomes `run_until(<Ctrl-C or SIGTERM>)`. The grace period is set by `AppBuilder::shutdown_grace` / `NOCTIS_SHUTDOWN_GRACE`. Handlers still running at the deadline are cancelled and reported as the new `ReportKind::Cancelled`. A fatal close after startup is logged with its typed cause, sets the shard gauge to 0 and is broadcast as a crate-internal `LifecycleEvent::ShardFatal`, which Story 13.5 turns into a hook. When every shard has ended, `run` returns `RunError::AllShardsEnded`.

## Boundaries & Constraints

**Always:**
- **Gateway (`noctis-gateway`):**
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum CloseMode { #[default] Resumable, Fresh }` maps to twilight `CloseFrame::RESUME` (4000) and `CloseFrame::NORMAL` (1000). It is exported from `lib.rs`.
  - `GatewayOptions.close_timeout: Duration` is new. Update all 10 struct literals and the `Debug`.
  - `Gateway` holds a `tokio::sync::watch::Sender<Option<CloseMode>>`.
  - `pub fn close_all(&self, mode: CloseMode)` sets the mode only while it is `None`, so the first call wins. It never blocks or awaits.
  - `pub async fn closed(&mut self)` joins every shard task and is cancel-safe. A second call returns at once.
  - Shard `drive` selects (`biased`) on the close watch before `shard.next()`. On a close request:
    - If the shard is identified (`shard.state().is_identified()`), it calls `shard.close(mode frame)` and keeps reading until the first `Message::Close`, the end of the stream, or `close_timeout`, whichever comes first.
    - Otherwise it stops at once and sends no frame and no new IDENTIFY/RESUME.
    - From the request on, it pushes no `Inbound` into the sink (`debug!` with the discarded count).
    - A requested stop sends the new `ShardEvent::Closed { shard }`. It never sends `Ended`, and it sends no `Disconnected` for the requested close.
    - A dropped watch sender counts as a `Resumable` request.
  - Update the crate docs, the `close_all`/`closed` doctests (`no_run`) and the `ShardEvent` docs.
- **Grace setting:**
  - `AppBuilder::shutdown_grace(&mut self, Duration) -> &mut Self` defaults to 10 s and accepts `Duration::ZERO..=Duration::from_mins(15)`. The last call wins, and `build()` resets it.
  - Out of range is `BuildViolation::InvalidShutdownGrace { actual: Duration }`, with the message `"shutdown grace {actual:?} is out of range: use a value in 0 s..=900 s"`.
  - `App::shutdown_grace(&self) -> Duration` returns it. Both `Debug` impls show it.
  - `from_env` reads `NOCTIS_SHUTDOWN_GRACE` after `NOCTIS_SHARD_TOTAL`. It is whole seconds: 1-4 ASCII digits, trimmed, and empty means unset.
  - Anything else is `EnvError::InvalidShutdownGrace { var }`; the message never shows the value. The value then goes to the setter, so 901-9999 fail at `build()`.
- **Signals and the error types:**
  - `StartError::SignalHandler` (`Copy`) uses the message `"the Ctrl-C/SIGTERM handler could not be installed: build the tokio runtime with enable_all() (as #[tokio::main] does)"`.
  - `App::run` checks the runtime, then the crypto provider, then installs the handlers, then checks the token.
  - The handlers are SIGINT and SIGTERM through `tokio::signal::unix::signal` on unix, and `tokio::signal::ctrl_c()` elsewhere.
  - Registration runs inside `catch_unwind`, because tokio panics when IO is disabled. An `Err` or a panic becomes `SignalHandler`.
  - New `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)] pub enum RunError { AllShardsEnded }` uses the message `"every shard ended without a shutdown request (see the \"shard closed fatally\" logs): fix the cause, then restart"`. `Error` gains a `Run(#[from] RunError)` variant. Both are re-exported at the core and facade roots.
- **`App::run_until(self, shutdown: impl Future<Output = ()> + Send) -> Result<(), Error>`** runs the same startup without installing signals. `run()` is `run_until(signals)`. The `run` future stays `Send`.
  - Startup selects on `shutdown`. If `shutdown` resolves before every shard is READY, the shutdown sequence runs and `run_until` returns `Ok(())`.
  - `supervise` returns `Stop::Requested` or `Stop::AllEnded`.
- **Shutdown sequence (core, in this order):**
  1. Log `info!(target: "noctis::app", reason = "signal" | "all shards ended", grace = ?grace, "shutdown started")` and broadcast `LifecycleEvent::ShutdownStarted` once.
  2. `gateway.close_all(CloseMode::Resumable)`.
  3. Fire the drain's `shutdown` token. The drain processes everything queued (interactions first), stops once both lanes are empty, then drops `Workers` so the sub-queue tasks finish. Await the drain handle, then `tracker.close(); tracker.wait()`. Both run under `timeout_at(start + grace)`. HTTP and the App stay alive.
  4. At the deadline, fire `cancel`. Wait up to `CANCEL_WAIT` (1 s) for the drain and the tracker. If tasks remain, log `warn!(running = tracker.len(), "handlers still running after cancellation")`.
  5. Await `gateway.closed()` bounded by `close_timeout + 1 s` (`SHARD_CLOSE_TIMEOUT` = 5 s). On timeout, log `warn!("shards did not close in time; aborting")` and drop the gateway.
     - From step 1 until here, a select loop keeps consuming the `ShardEvent` channel through `ShardStatus::update` and the lifecycle mapping, so `Closed` sets the gauge to 0 while the shards close.
  6. Log `info!("shutdown complete")`. Return `Ok(())` for `Requested` and `Err(RunError::AllShardsEnded)` for `AllEnded`. The ports drop with `self`.
  - A guard cancels `cancel` on every return path, including startup errors and a dropped `run` future, so no handler outlives `run`.
- **Workers (`worker.rs`):**
  - `Workers::new` takes a crate-internal `Shutdown { tracker: TaskTracker, shutdown: CancellationToken, cancel: CancellationToken }`. Every `tokio::spawn` there (the sub-queues and `run_concurrent`) becomes `tracker.spawn`.
  - In `drain`:
    - Each wait selects `biased` on `cancel`, and the permit wait does too.
    - On `cancel`, it logs `warn!(target: "noctis::app", interactions, events, "lanes discarded at shutdown")` when anything is left. `events` counts the lane, the overflow and an event that was waiting for a permit. Then it returns.
    - After `shutdown`, it pops with `try_recv` until the lanes are empty and returns.
  - In `run_handler`:
    - If `cancel` is already set, the handler is not called.
    - Otherwise the future races `cancel` (`biased`, cancel first).
    - Each run ends in exactly one outcome: Ok, Error, Panic or Cancelled.
    - Cancelled increments `noctis_handler_errors_total{kind="event"}` and records the duration only if the handler started. It reports `ErrorOrigin::Event` with `ReportKind::Cancelled` and every context field, inside `noctis.event`.
- **Sinks:**
  - `ReportKind::Cancelled` (unit variant) is documented as "the handler was still running, or still queued, when the shutdown grace period ended".
  - `LogSink` logs it at `error` with the message `"<origin> handler cancelled at shutdown"` and no `error`/`panic` field.
- **Fatal close and lifecycle:**
  - `crates/noctis-core/src/lifecycle.rs` (crate-private) defines `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub(crate) enum LifecycleEvent { ShardReady { shard }, ShardResumed { shard }, ShardDisconnected { shard, close_code: Option<u16> }, ShardFatal { shard, close_code: Option<u16> }, ShutdownStarted }`.
  - `App` holds a `tokio::sync::broadcast::Sender<LifecycleEvent>` (capacity 256) created in `build`. A send without receivers is ignored, and a lagging receiver never blocks. `App::subscribe_lifecycle` is `pub(crate)` and `#[cfg(test)]`.
  - `await_startup` and `supervise` broadcast the shard events.
  - On `Ended { shard, close_code }` after startup, `supervise` does three things. It logs `error!(target: "noctis::app", shard, close_code, error = %start_error(shard, close_code), "shard closed fatally")`, which replaces `"shard ended"`. It sets the gauge through `ShardStatus::update`. It broadcasts `ShardFatal`.
  - During startup the 5.4 `StartError` path is unchanged, and it also broadcasts `ShardFatal`.
  - `ShardEvent::Closed` sets the gauge to 0 and is logged at `debug`; it is never fatal.
- `tokio-util = { version = "0.7.20", default-features = false }` goes into the workspace, and core uses `features = ["rt"]`. Core's tokio gains the `signal` feature.
- Never log payloads or tokens. Do not use `unwrap`/`expect` outside tests. Run everything with `--release`.

**Never:**
- These belong to other stories:
  - Session writes to the `SessionStore`, `--no-resume` and `CloseMode::Fresh` at shutdown (12.3).
  - Public Lifecycle Hooks (13.5).
  - Interaction handlers and the Responder (Epic 6).
  - The HTTP edge (5.9).
- No second-signal force-quit, no `process::exit`, no `JoinHandle::abort` on handler tasks (cooperative cancel only), no new metric, no per-item sink reports for discarded lane items.

## I/O & Edge-Case Matrix

Abbreviations:
- `S` is a recording `Event` sink.
- `B(h)` is a `MessageDelete` handler `h` in Module `ops`, blocked on a gate, guild 3, shard 0.
- "E2E" is the FakeDiscordHttp + FakeGateway App in `noctis-testkit/tests/shutdown.rs` with `run_until(oneshot)`.
- Every wait is bounded at 15 s.

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Default grace | no setter | `app.shutdown_grace() == 10 s` |
| 2 | Bounds ok | `ZERO`; `from_mins(15)`; `1 ns` | accepted |
| 3 | Bounds bad | `from_mins(15) + 1 ns`; `Duration::MAX` | `InvalidShutdownGrace{actual}`; listed in order with other violations |
| 4 | Last wins / reset | `shutdown_grace(1s).shutdown_grace(2s)`; build twice | 2 s; the second App is back to 10 s |
| 5 | Env ok | `"30"`, `" 0 "`, `"0900"`, `"9999"` | 30 s, 0, 900 s, 9999 s (the last fails `build()` with `InvalidShutdownGrace`) |
| 6 | Env unset | missing, `""`, `"   "` | the default 10 s |
| 7 | Env bad | `"10000"`, `"1.5"`, `"-1"`, `"+1"`, `"10s"`, `"1e3"`, `"1_0"`, `"３０"`, `"٣"` | `EnvError::InvalidShutdownGrace{var:"NOCTIS_SHUTDOWN_GRACE"}`; message has no value |
| 8 | Env non-UTF-8 / override | invalid bytes; env 30 then setter 5 s | `NotUnicode`; 5 s |
| 9 | Close resumable | 2 shards READY, `close_all(Resumable)` | the fake records client close 4000 per connection; `Closed{0}`, `Closed{1}`; no `Ended`/`Disconnected`; `closed()` returns; channel ends; connection count unchanged afterwards |
| 10 | Close fresh | `close_all(Fresh)` | close 1000 (the fake forgets the session) |
| 11 | First wins | `close_all(Resumable)` then `(Fresh)` | only 4000 frames |
| 12 | Not identified | `IdentifyReply::Ignore`, `close_all` | the task ends ≤ 1 s, `Closed`, no close frame, one IDENTIFY total |
| 13 | Close unacked | silent server (HELLO, READY, then never reads), `close_timeout` 200 ms | `Closed` within 200 ms + 2 s |
| 14 | No enqueue after close | silent server sends `MESSAGE_DELETE` after `close_all` | the sink gets 0 items from it |
| 15 | Fatal, no close | `IdentifyReply::Close(4004)` | `Ended{close_code:Some(4004)}`; `closed()` returns; a second `closed()` returns at once |
| 16 | Empty drain | no lanes items; shutdown | drain returns at once; tracker empty |
| 17 | Finish in grace | 3 queued events, 50 ms handler, grace 1 s | 3 runs Ok; 0 `Cancelled` |
| 18 | Deadline | `B` never opened, grace 100 ms | one `Cancelled` with `shard 0, guild 3, module "ops", handler, event_type "MESSAGE_DELETE"`; `errors_total{kind=event}` +1; 1 duration sample |
| 19 | Ordered queued | ordered `B`, 2 events same guild, grace 100 ms | 2 `Cancelled`; the 2nd handler body never ran |
| 20 | Concurrent mix | 2 concurrent handlers, one Ok fast, one blocked | only the blocked one is `Cancelled` |
| 21 | One outcome | 200 runs finishing ±1 ms around the deadline | each run reports ≤ 1 time; never Ok + Cancelled |
| 22 | Zero grace | 2 queued events + 1 interaction, grace 0 | no handler runs; `"lanes discarded at shutdown"` with `interactions=1 events=2`; 0 reports |
| 23 | Permit wait | `handler_concurrency(1)`, `B` holds the permit, a 2nd event waits, grace 100 ms | `B` → `Cancelled`; the 2nd event is counted in `events=1`, not reported |
| 24 | Err/panic in grace | handlers `Err` and `panic!("x")` during grace | `Error`, `Panic("x")`, not `Cancelled` |
| 25 | Sink panics | `S` panics on `Cancelled` | `"error sink panicked"`; the sequence completes |
| 26 | Stuck handler | a handler in `std::thread::sleep(2 s)`, grace 100 ms | the warning `running=1` after ~1 s; `run_until` still returns |
| 27 | LogSink Cancelled | each origin | target `noctis::<origin>`, `"<origin> handler cancelled at shutdown"`, no `error`/`panic` |
| 28 | Fatal after start | supervise, total 2, `Ready` ×2, then `Ended{1, Some(4014)}` | `"shard closed fatally" shard=1 close_code=4014`, `error` contains `"4014 (disallowed intents)"`; gauge shard 1 = 0, shard 0 = 1; one `ShardFatal{1,Some(4014)}`; supervise keeps running |
| 29 | Fatal causes | `Ended` with `None`, `4004`, `4010`, `4012` | `ShardEnded`, `InvalidToken`, `FatalClose{code}` text respectively; `ShardFatal{.., close_code}` each |
| 30 | All ended | every shard `Ended` | `Stop::AllEnded`; `run_until` → `Err(Error::Run(RunError::AllShardsEnded))` after the sequence |
| 31 | Out of range | `Ended{shard 9}` with total 2 | logged and broadcast; the gauge is untouched |
| 32 | Lifecycle mapping | `Ready`, `Resumed`, `Disconnected{4000}`, `Closed` | `ShardReady`, `ShardResumed`, `ShardDisconnected{Some(4000)}`; `Closed` broadcasts nothing, gauge 0, no fatal log |
| 33 | No subscriber / lag | 0 receivers; 300 events to one unread receiver | no error or block; the receiver sees `Lagged` |
| 34 | Shutdown event | shutdown requested | exactly one `ShutdownStarted`, before any `Closed` |
| 35 | E2E graceful | `B` started, trigger shutdown; when the fake logs close 4000 the test opens the gate and `B` calls `ctx.http()` `GET /users/@me` (routed 200) | the HTTP call succeeds after the close; B completes; `Ok(())`; logs `shutdown started` < `shutdown complete`; no `Cancelled` |
| 36 | E2E deadline | `shutdown_grace(300ms)`, `B` never opened | `S` gets 1 `Cancelled`; `Ok(())` within 300 ms + 7 s; gauge 0 |
| 37 | E2E during startup | `IdentifyReply::Ignore`, trigger shutdown | `Ok(())` ≤ 7 s; no `"all 1 shards ready"` |
| 38 | E2E fatal | 2 shards READY, `gw.close(4014, "")` | 2 `"shard closed fatally"` lines `close_code=4014`; `Err(Error::Run(RunError::AllShardsEnded))`; one IDENTIFY per shard |
| 39 | Signals | own binary `tests/shutdown_signal.rs` (`cfg(unix)`): App 1 `run()` + `kill -TERM <pid>`, then App 2 + `kill -INT` | each `Ok(())` and a 4000 close frame |
| 40 | No IO driver | `new_multi_thread().enable_time()` runtime, `run()` | `Err(Start(SignalHandler))`, no panic escapes |
| 41 | Aborted run | the `run_until` task is aborted while `B` runs | `B`'s future is dropped (cancel guard); its gate permit is released |

</intent-contract>

## Code Map

- `crates/noctis-gateway/src/gateway.rs`:
  - `GatewayOptions` L39-70 (add `close_timeout`).
  - `Gateway` L104-208 (the watch sender, `close_all`, `closed`; `start` passes a watch receiver and `close_timeout` to `shard::run`).
  - `Debug` (add `closing`).
- `crates/noctis-gateway/src/shard.rs`:
  - `run`/`drive` L322-377 (the select, close handling, `Closed`).
  - twilight 0.17.1: `Shard::close` queues the frame and is only sent while the state is not disconnected. `state().is_identified()` is the check. After the close, keep polling until `Message::Close`, but never past it, because twilight would reconnect.
- `crates/noctis-gateway/src/inbound.rs` L148-185: `ShardEvent` (add `Closed`). `lib.rs`: export `CloseMode`, update the docs.
- `crates/noctis-core/src/app.rs`:
  - The constants L41-63 (`SHUTDOWN_GRACE_VAR`, default and range).
  - The `App` fields L82-98, `run` L332-383 (split into `run` and `run_until`), `start_error` L412 (reuse it for the fatal cause), `ShardStatus::update` L438 (`Closed` → 0).
  - `await_startup` L459 and `supervise` L509 (lifecycle, `Stop`, shutdown select).
  - Builder fields L575, `new` L624, `from_lookup` L647 (env, with a `parse_grace_secs` helper next to `parse_shard_total` L1151), setters, `build` L1013, and both `Debug` impls.
  - Tests in `app/tests.rs`.
- `crates/noctis-core/src/worker.rs`:
  - `AbortOnDrop` L40, `Workers::new` L71, the spawns L80/L118/L208, `drain` L130, `run_handler` L246.
  - Tests in `worker/tests.rs` (`Harness` L53 builds `Workers` via `app.workers()`, so extend it with a `Shutdown`).
- `crates/noctis-core/src/sink.rs`: `ReportKind` L102, `LogSink` messages (+ `sink/tests.rs`).
- `crates/noctis-core/src/error.rs`: `Error` L24, `BuildViolation` L105, `StartError` L238, `EnvError` L397; add `RunError`.
- `crates/noctis-core/src/startup.rs`: put the signal install (`Signals` with `recv()`) here. `lib.rs`: `mod lifecycle;`, re-export `RunError`.
- `crates/noctis-core/Cargo.toml` and the root `Cargo.toml` (`tokio-util`, tokio `signal`).
- `crates/noctis/src/lib.rs`: root re-exports L232-235 (add `RunError`) and the crate docs (shutdown section).
- `crates/noctis-testkit/tests/events.rs` L164-235: the `Running` harness and the `TYPING_START` session-wait pattern to copy. `observability.rs`: the global recorder and serial lock pattern. FakeGateway `wait_for_frame` and `FrameKind::Close{code}` show client closes, and `IdentifyReply::Ignore` holds startup. FakeDiscordHttp `route(Method::GET, "/users/@me", FakeResponse::json(..))`.
- Skill:
  - `references/app-and-sharding.md`: L43-54 run paragraph, L96 move `.shutdown_grace` to the implemented table, §2 env table (`NOCTIS_SHUTDOWN_GRACE`), Errors §, §8 Shutdown (real order; `ShutdownStarted` hook and session writes stay [planned 13.5/12.3]).
  - `modules-middleware-ratelimits.md` L270-290: `ReportKind::Cancelled`; drop "[planned, Story 5.8]".
  - `troubleshooting.md` near L121: `"shard closed fatally"` and `RunError::AllShardsEnded` rows.
  - `SKILL.md` L21 API hash.
- Template: `src/lib.rs` `configure` (`.shutdown_grace(Duration::from_secs(15))` with a comment), `src/observability.rs` (`ReportKind::Cancelled => "cancelled"`), `src/main.rs` comment (`run` stops on Ctrl-C/SIGTERM after the grace period), and `AGENTS.md` (one shutdown bullet).

## Tasks & Acceptance

**Execution:**
1. Root `Cargo.toml`, `crates/noctis-core/Cargo.toml`: add `tokio-util`, and add `signal` to core's tokio. Check `cargo deny check`.
2. `crates/noctis-gateway/src/{gateway.rs, shard.rs, inbound.rs, lib.rs}`:
   - `CloseMode`, `close_timeout`, `close_all`, `closed`, `ShardEvent::Closed`. Update every `GatewayOptions` literal.
   - E2E tests `crates/noctis-testkit/tests/gateway_close.rs`, Matrix 9-15. The silent server uses tokio-websockets `ServerBuilder` and a `oneshot` to trigger its dispatch.
3. `crates/noctis-core/src/{error.rs, sink.rs, startup.rs, lifecycle.rs, lib.rs}`:
   - `RunError`, `Error::Run`, `StartError::SignalHandler`, `BuildViolation::InvalidShutdownGrace`, `EnvError::InvalidShutdownGrace`, `ReportKind::Cancelled`, `Signals`, `LifecycleEvent`.
   - Unit tests: Matrix 27 and 40.
4. `crates/noctis-core/src/worker.rs` (+ tests): `Shutdown`, tracker spawns, the cancel-aware `drain` and `run_handler`. Tests: Matrix 16-26 (multi-thread or current-thread tokio as needed, `tokio::time::pause` where useful).
5. `crates/noctis-core/src/app.rs` (+ tests): the setter, env, getter, `run`/`run_until`, the sequence, `supervise`/`await_startup` with lifecycle and `Stop`, and the cancel guard. Tests: Matrix 1-8 and 28-34 (`supervise` driven by a hand-fed `mpsc` and a local `DebuggingRecorder`).
6. `crates/noctis/src/lib.rs`: re-exports and docs. `crates/noctis/tests/error_sinks.rs`: assert that `ReportKind::Cancelled` is reachable through the facade.
7. `crates/noctis-testkit/tests/shutdown.rs` (Matrix 35-38, 41, serial lock) and `tests/shutdown_signal.rs` (Matrix 39, `#![cfg(unix)]`, sends the signal with `std::process::Command::new("kill")`).
8. Skill, template, `AGENTS.md`, `xtask/public-api.txt` (`--bless`), the `SKILL.md` API hash, and every new public name mentioned in the skill.
9. Architecture `.memlog.md`: the planning decisions are logged; append implementation decisions.

**Acceptance Criteria:**
- Given a running App, when Ctrl-C or SIGTERM arrives (or the `run_until` future resolves), then core logs `shutdown started`, broadcasts `ShutdownStarted`, closes every shard with 4000, drains both lanes and waits for in-flight handlers within `shutdown_grace` while HTTP still answers, awaits `gateway.closed()` and returns `Ok(())` (Matrix 9, 34-35, 39).
- Given handlers still running or queued at the grace deadline, when the deadline passes, then each is cancelled once and reported to the `Event` Error Sink as `ReportKind::Cancelled` with its context (Matrix 18-23, 36).
- Given a shard closing fatally after startup, when the gateway reports `Ended`, then core logs `shard closed fatally` with the typed cause, sets `noctis_shard_status{shard}` to 0 and broadcasts `LifecycleEvent::ShardFatal`; when every shard has ended, `run` returns `RunError::AllShardsEnded` instead of `Ok(())` (Matrix 28-31, 38).
- Given the new public API, when the gates run, then the skill and template document it and every gate in Verification passes.

## Spec Change Log

## Review Triage Log

## Design Notes

- **`run_until`:** tests and embedders need a shutdown trigger that is not a process signal; `run` stays the one-liner for bots. Signals are installed by `run` only, so tests never touch process signal state (except Matrix 39's own binary).
- **`RunError::AllShardsEnded`:** with every shard dead, `Ok(())` exits 0 and looks like a clean stop. The error makes it loud (FR-1 "never a silent stop") while a single fatal shard keeps the App running in a degraded state, visible in logs, the gauge and the lifecycle event.
- **Stop enqueuing at close:** AD-21 says shards stop enqueuing once `close_all` is called. Frames after that are not pushed (at-most-once, as for back-pressure drops), so the drain can reach "empty".
- **Unstarted jobs count as cancelled:** an ordered job queued behind a blocked handler is accepted in-flight work; reporting it keeps "every dispatched handler ends in exactly one outcome". Events still in the lanes were never dispatched, so they get one summary warning instead of per-item sink calls.
- **Default 10 s:** shutdown takes at most the grace period + 1 s (cancel wait) + 6 s (the close bound), under Kubernetes' 30 s default `terminationGracePeriodSeconds`.
- **Lifecycle seam:** a crate-internal `broadcast` (not a public API) lets 13.5 attach hooks without changing the gateway. The gateway still only emits `ShardEvent`s (AD-21).
- Sketch:
  ```rust
  let (tx, rx) = tokio::sync::oneshot::channel::<()>();
  let run = tokio::spawn(app.run_until(async { let _ = rx.await; }));
  // ... later
  let _ = tx.send(());
  run.await??;
  ```

## Verification

**Commands** (repo root; local gates only; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- expected: exit 0. Also run `-p noctis-testkit --test shutdown --test gateway_close --test shutdown_signal` 3 times, with no flakes.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo +1.91 check --release --workspace --all-targets` and `cargo deny check` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog` and `-- skill-check` -- expected: each exits 0 (needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0).
- `test ! -d target/debug` -- expected: true.
