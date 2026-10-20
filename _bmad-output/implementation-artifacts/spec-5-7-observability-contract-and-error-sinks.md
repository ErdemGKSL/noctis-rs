---
title: 'Story 5.7: Observability contract and Error Sinks'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: '28bfb398f6e0b1750ea1a016bdd31f14e97b63d6'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-5-6-modules-and-typed-event-handlers-with-derived-filters-and-in.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Sustained overflow drops produce one ERROR line or one ErrorSink call per dropped event on the shard task, with no coalescing or rate limit.
    evidence: |-
      lanes.rs OverflowFull calls the observer once per drop; LogSink logs each at error. The one-line-per-drop log already existed in Story 5.5, and the spec makes sinks synchronous and documents that they must not block, so throttling is a cross-cutting design change, not a 5.7 defect.
    location: >-
      crates/noctis-gateway/src/lanes.rs (OverflowFull branch); crates/noctis-core/src/sink.rs (EventDropObserver)
    severity: low
---

<intent-contract>

## Intent

**Problem:** FR-22, NFR-5, NFR-6, AD-13 and AD-29 need a consistent observability contract and per-origin Error Sinks, and none exist yet. Only `noctis.shard` (field `shard` alone) and the 5.5 lane metrics are emitted. Event handler errors and panics, and overflow drops, are hard-wired `error!` logs that a bot author cannot redirect. No test checks metric names or label cardinality.

**Approach:** Add a public `ErrorSink` trait with `ErrorOrigin`, `ErrorReport` and `ReportKind`, a default `LogSink`, and the builder method `AppBuilder::error_sink(origin, sink)`. The worker and the lane overflow path route through the App's sinks. The three runtime spans get the AD-29 fields. Add `noctis_handler_duration_seconds`, `noctis_handler_errors_total` and `noctis_shard_status`. Add a public metric catalog (`noctis::observe::METRICS`) that every emitted metric must match, and pin it with static and recorded-label tests.

## Boundaries & Constraints

**Always:**
- **Error Sinks (`crates/noctis-core/src/sink.rs`, re-exported at the core and facade roots):**
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum ErrorOrigin { Command, Component, Autocomplete, Event, Render }`. `Component` covers component interactions and modal submits. It has `pub const ALL: [ErrorOrigin; 5]` and `as_str()` (`"command"`, `"component"`, `"autocomplete"`, `"event"`, `"render"`), and `Display` prints `as_str()`.
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy)] pub enum ReportKind<'a>` has these variants:
    - `Error(&'a HandlerError)`: a handler or hook returned `Err`.
    - `Panic(&'a str)`: a panic. The payload is `panic_message`'s text.
    - `Dropped`: a non-droppable event was dropped because the overflow was full.
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy)] pub struct ErrorReport<'a>` has these pub fields: `origin`, `kind`, `app: &'a str`, `shard: Option<u32>`, `guild_id: Option<Id<GuildMarker>>`, `module: Option<&'a str>`, `handler: Option<&'a str>` (the handler, command or Route path), `event_type: Option<&'a str>` (the Discord name, e.g. `"MESSAGE_DELETE"`). Only core constructs it.
  - `pub trait ErrorSink: Send + Sync + 'static { fn report(&self, report: ErrorReport<'_>); }`. It is synchronous and dyn-compatible. Its rustdoc says that it runs on the worker or shard task, must not block, and should spawn or forward to a channel for I/O.
  - Blanket `impl<S: ErrorSink + ?Sized> ErrorSink for Arc<S>`, so one sink can serve several origins.
  - `#[derive(Debug, Clone, Copy, Default)] pub struct LogSink;` logs every report at `error` and never logs payloads:
    - Target: `noctis::<origin>` (for example `noctis::event`).
    - Fields: `app`, `shard`, `guild_id`, `module`, `handler`, `event_type` (each only when `Some`), plus `error` (the `Display` source chain joined with `": "`, at most 16 links) or `panic`.
    - Message for `Error`: `"<origin> handler failed"`. For `Panic`: `"<origin> handler panicked"`. For `Dropped`: `"event overflow is full; event dropped"`.
- **Routing (crate-internal `ErrorSinks`, one `Arc<dyn ErrorSink>` per origin, held in `AppShared`):**
  - `report(ErrorReport)` calls the origin's sink inside `std::panic::catch_unwind(AssertUnwindSafe(..))`.
  - If the sink panics, core logs `error!(target: "noctis::error_sink", origin, panic = <message>, "error sink panicked")`. That panic is never re-reported to a sink.
- **Builder:**
  - `AppBuilder::error_sink(&mut self, origin: ErrorOrigin, sink: impl ErrorSink) -> &mut Self`. The last call per origin wins. Unset origins use `LogSink`.
  - `build()` moves the sinks into the App and resets the builder to defaults. Plugins may call it in `install`.
  - The builder's `Debug` lists the origins that have a custom sink.
- **Worker (`worker.rs` `run_handler`):**
  - Every handler run, including the constructing call and the future, runs inside `tracing::info_span!("noctis.event", app, shard, guild_id, module, handler, event_type)`. `guild_id` is a `u64`, or `Empty` when the event has no guild.
  - It records `noctis_handler_duration_seconds{app, kind="event"}` (seconds, `f64`, for Ok, Err and panic).
  - On `Err` or panic, it increments `noctis_handler_errors_total{app, kind="event"}` and reports `ErrorOrigin::Event` with the matching `ReportKind` and every context field set. The direct `error!` logs of 5.6 are removed; `LogSink` reproduces them.
- **Lane overflow (`noctis-gateway` `lanes.rs`):**
  - Add `pub trait DropObserver: Send + Sync + 'static { fn overflow_dropped(&self, shard: u32, event: &Event); }` and `LaneConfig.drop_observer: Option<Arc<dyn DropObserver>>` (default `None`). `LaneConfig`'s `Debug` becomes manual and shows only whether an observer is set.
  - On `OverflowFull`, the dropped `Inbound` is carried out of the lock. Without an observer, the drop is still counted and the 5.5 `error` log is unchanged. With an observer, the lane calls `overflow_dropped` after releasing the lock and logs nothing itself.
  - Droppable evictions never reach the observer.
  - Core's observer reports `Event`/`Dropped` with `shard`, `guild_id` (`event.guild_id()`) and `event_type`. The observer call runs under core's `catch_unwind` from `ErrorSinks`.
- **Spans:**
  - `noctis.shard` gets `app` next to `shard`, through a new `GatewayOptions.app: Arc<str>` field.
  - Core's drain wraps each interaction's handling in `info_span!("noctis.interaction", app, shard, guild_id, module = Empty, path = Empty)`. Epic 6 records `module` and `path`.
- **Shard status:**
  - Core sets `noctis_shard_status{app, shard}` to `0.0` for every shard `0..total` right after `Gateway::start`.
  - `Ready` and `Resumed` set it to `1.0`; `Disconnected` and `Ended` set it to `0.0`. This applies during startup and in `supervise`.
  - A shard number `>= total` is ignored.
- **App label:** one crate-internal `app: Arc<str>` (`"default"` until Story 7.4) in `AppShared` feeds `LaneConfig.app`, `GatewayOptions.app`, spans, metrics and `ErrorReport.app`.
- **Catalog (`crates/noctis-core/src/observe.rs`, `pub mod observe`, facade `noctis::observe`):**
  - `#[non_exhaustive] pub struct MetricInfo { pub name, pub metric_type: MetricType, pub labels: &'static [&'static str], pub description }` and `#[non_exhaustive] pub enum MetricType { Counter, Gauge, Histogram }`.
  - `pub const METRICS: &[MetricInfo]` holds exactly the 6 emitted metrics: `noctis_lane_depth[app, lane]` (gauge), `noctis_event_overflow_depth[app]` (gauge), `noctis_events_dropped_total[app, event_type]` (counter), `noctis_handler_duration_seconds[app, kind]` (histogram), `noctis_handler_errors_total[app, kind]` (counter) and `noctis_shard_status[app, shard]` (gauge).
  - `pub fn describe_metrics()` calls `metrics::describe_*!` for each row. The unit is `Seconds` for `_seconds` and `Count` for `_total`; other rows have no unit.
  - `App::run` calls `describe_metrics()` before starting the lanes.
  - `metrics` stays out of the public signatures.
- Never log payloads, tokens or interaction tokens. Do not use `unwrap`/`expect` outside tests. Every cargo command runs with `--release`.

**Never:**
- Do not install a `metrics` recorder or a tracing subscriber; the bot author does that.
- These metrics and spans belong to other stories:
  - `noctis_http_requests_total`: Story 5.9's edge (see Design Notes).
  - `noctis_ack_latency_seconds` and `noctis_autodefer_total`: 6.6.
  - `noctis_ref_lookups_total`: 7.5.
  - The render metrics and `noctis.render`: Epic 10.
  - `noctis.publish`: 6.9.
- Do not add these here:
  - Command, Component, Autocomplete or Render call sites: their handlers do not exist yet. Their stories call `ErrorSinks::report`.
  - Hook errors (13.5, which routes them to `ErrorOrigin::Event`).
  - Shutdown cancellation reports (5.8).
  - A `HandlerError` message constructor (6.8).
- No async sink trait, closure `ErrorSink` impl or env variable.
- No change to droppable-eviction logs.

## I/O & Edge-Case Matrix

Abbreviations:
- `S(o)` is a test sink recording every `ErrorReport` (fields copied out).
- `M` is a `MessageDelete` handler in Module `audit`, path `bot::h`, guild `GA`, shard 3.
- "rec" is a `metrics_util::debugging::DebuggingRecorder` (local in core unit tests, global in the testkit binary).

| # | Scenario | Input / State | Expected |
|---|---|---|---|
| 1 | Defaults | no `error_sink` call | every origin routes to `LogSink` |
| 2 | One override | `error_sink(Event, S1)` | Event → S1; the 4 others → `LogSink` |
| 3 | Last wins | `error_sink(Event, S1).error_sink(Event, S2)` | only S2 receives |
| 4 | Shared | one `Arc<S>` on all 5 origins | each report arrives once with its own `origin` |
| 5 | Routing table | a report for each of `ALL` with distinct sinks | each sink gets exactly its origin |
| 6 | Reset | build twice | second App back to `LogSink` everywhere |
| 7 | Plugin | `install` calls `error_sink(Event, S1)` | S1 receives event errors |
| 8 | `Err` | `M` returns `Err(io::Error::other("disk full"))`, `error_sink(Event, S)` | one report: `Event`, `Error(e)` with `e.to_string()=="disk full"`, `app "default"`, `shard 3`, `guild_id GA`, `module "audit"`, `handler "bot::h"`, `event_type "MESSAGE_DELETE"`; siblings run; no `LogSink` line |
| 9 | Default log | 8 without a custom sink | one `ERROR` line with target `noctis::event`, `"event handler failed"`, `module="audit"`, `handler="bot::h"`, `event_type="MESSAGE_DELETE"`, `error=disk full` (the 5.6 test keeps passing) |
| 10 | Source chain | error `outer` → source `inner` → source `root` | `error=outer: inner: root` |
| 11 | Chain cap | 20-deep chain | exactly 16 messages joined |
| 12 | Unicode / empty | messages `"💥 ünï"` and `""` | logged verbatim; empty → `error=` |
| 13 | Panic forms | `panic!("boom")`, `panic!("{}", s)`, `panic_any(5)`, a panic in the constructing call | `Panic("boom")`, `Panic(s)`, `Panic("<non-string panic>")`, `Panic("boom")`; LogSink `"event handler panicked"`; next event handled |
| 14 | Guildless | `UserUpdate` handler errs | `guild_id None`; span has no `guild_id` value; LogSink omits `guild_id` |
| 15 | Ok | handler returns `Ok` | no report; duration histogram has 1 sample; `noctis_handler_errors_total` not incremented |
| 16 | Error metrics | 1 Err + 1 panic + 1 Ok | `errors_total{app="default",kind="event"} == 2`; duration samples == 3, all ≥ 0 |
| 17 | Sink panics | sink panics on every report; 2 failing events | 2 `"error sink panicked"` lines (`target noctis::error_sink`, `origin=event`, `panic=…`); the drain and later handlers keep running; the sink is called again for event 2 |
| 18 | LogSink origins | `LogSink.report` for each origin × `Error`/`Panic` | target `noctis::<as_str>`, message `"<as_str> handler failed"` / `"… panicked"` |
| 19 | LogSink Dropped | `Dropped` with shard 2, `GUILD_CREATE`, no guild | `"event overflow is full; event dropped"`, `event_type="GUILD_CREATE"`, `shard=2`, no `guild_id` |
| 20 | Overflow + observer | lanes(capacity 1, overflow 0, observer O); push E1, E2 (non-droppable, shard 2, guild GB) | E2 → `O.overflow_dropped(2, &E2)`, E1 queued; counter `+1`; no lane `error` line |
| 21 | Overflow, no observer | same, `drop_observer: None` | 5.5 behaviour: `error` line `"event overflow is full; event dropped"` from `noctis::lanes` |
| 22 | Overflow cap > 0 | capacity 1 holding E0, overflow 2; push non-droppables E1..E4 | E1, E2 go to overflow; E3 and E4 each call O once (2 calls); counter `+2` |
| 23 | Eviction | a droppable evicts an older droppable | O not called; counter `+1` |
| 24 | Observer panics | observer panics | caught in core's observer; shard task keeps pushing (5.5 lane tests still pass); one `"error sink panicked"` line |
| 25 | Drop → sink | App with `event_lane(1,0)`, `error_sink(Event, S)`, a handler blocked on a gate, a flood of non-droppable events | S gets ≥1 `Dropped` with `shard`, `event_type`, `guild_id` = the event's |
| 26 | Lock released | observer clones a `LaneSender` it holds (clone locks the mutex) and drops the clone | no deadlock (test finishes ≤ 15 s) |
| 27 | Event span | Matrix 8 under a fmt subscriber with `FmtSpan::NEW` | one `noctis.event` span with `app="default" shard=3 guild_id=<GA> module="audit" handler="bot::h" event_type="MESSAGE_DELETE"`; the failure line is inside it |
| 28 | Interaction span | an interaction (guild GA, then a DM) reaches the drain | `noctis.interaction{app="default" shard=… guild_id=<GA>}`; the DM has no `guild_id`; `module` and `path` are declared (metadata field names) and empty |
| 29 | Shard span | E2E App, 2 shards | `noctis.shard{app="default" shard=0}` and `{… shard=1}` |
| 30 | Shard status | 2 shards: start → READY both → shard 1 disconnect → resume | gauges `0,0` → `1,1` → `1,0` → `1,1`; unknown shard 7 sets nothing |
| 31 | Ended | a shard ends after startup | its gauge `0` |
| 32 | Catalog static | `METRICS` | 6 unique names, all start with `noctis_`, first label `app`, labels ⊆ {`app`,`lane`,`event_type`,`kind`,`shard`}, no label name contains `id`, `user`, `guild`, `channel`, `message`, `name`, `content` or `error`; `_total` ⇒ Counter, `_seconds` ⇒ Histogram |
| 33 | Recorded set (core) | rec over `lanes()` pushes and drops, a worker run with Ok/Err/panic, the shard-status updater | every snapshot key is in `METRICS` with exactly its label names; values: `app=="default"`, `lane∈{interaction,event}`, `kind=="event"`, `event_type` matches `^[A-Z_]{1,64}$`, `shard` parses as `u32 < total`; no value is ≥ 15 digits or equals a fixture snowflake (`GA`, user/channel/message ids) |
| 34 | Recorded set (E2E) | global rec, FakeGateway App with an erroring handler, a panicking one and an overflow flood | same checks as 33 over the whole snapshot; all 6 catalog names present |
| 35 | Describe | `describe_metrics()` under rec | each row has its description; unit `Seconds`/`Count`/none as specified |

</intent-contract>

## Code Map

- `crates/noctis-core/src/worker.rs`:
  - `run_handler` L215-258 is the span, metrics and sink call site. Keep `catch_unwind` on both the call and the future, and keep `panic_message` (L261).
  - `drain`/`interaction_unhandled` L124-155 is where the interaction span goes.
  - Tests in `worker/tests.rs` (`Harness` L53, `Capture`, `handler_error_is_logged` L446) must keep passing.
- `crates/noctis-core/src/handler.rs` L94-99: `AppShared` (add `app: Arc<str>` and `sinks: ErrorSinks`). `HandlerError::as_error` feeds the source chain.
- `crates/noctis-core/src/app.rs`:
  - `lane_config` L263 (set `app`, `drop_observer`).
  - `run` L310-353 (`describe_metrics`, `GatewayOptions.app`, shard-status init).
  - `await_startup` L382 and `supervise` L430 (status gauge).
  - `AppBuilder` fields, `new`, `build`, and the `Debug` impls L448 and L~930.
- `crates/noctis-core/src/{sink.rs, observe.rs}` (new); `lib.rs` (`mod sink; pub mod observe;` and the root re-exports `ErrorSink, ErrorOrigin, ErrorReport, ReportKind, LogSink`).
- `crates/noctis-core/Cargo.toml`: `metrics.workspace = true`; dev `metrics-util = { workspace = true, features = ["debugging"] }`.
- `crates/noctis-gateway/src/lanes.rs`:
  - `LaneConfig` L50-83.
  - `Pushed::OverflowFull` L245/L306 (carry the `Inbound`).
  - `push` L315-352, `count_drop` L211, `type_label` L222.
  - Tests in `lanes/tests.rs`; the local-recorder pattern is at L251.
- `crates/noctis-gateway/src/gateway.rs` L36-66 (`GatewayOptions`: add `app`, plus the Debug and doctests; 10 struct literals exist across the workspace). `shard.rs` L114-122 (`run` span), `lib.rs` (export `DropObserver`).
- `crates/noctis-testkit/tests/lane_metrics.rs`: the global-recorder and capture pattern for a new binary `tests/observability.rs`. `tests/events.rs` has `Fakes`-style setup and the session wait trick (memlog: dispatch `TYPING_START` until one live session).
- `crates/noctis/src/lib.rs`: root re-exports L192, `pub mod observe` (re-export `noctis_core::observe::*`), a `# Observability and Error Sinks` crate-docs section, and the prelude (add `ErrorSink`, `ErrorOrigin`).
- Skill: `references/modules-middleware-ratelimits.md` §6 L224-241 (rewrite to the real API, drop `Hook`/`Overflow` origins), L141 (remove `[planned, Story 5.7]`). `references/app-and-sharding.md` L95 (`.error_sink` row moves to the live table), §11 L457-473 (spans + fields, the 6 live metrics with labels and semantics, shard-status values, catalog). `troubleshooting.md` L117. `SKILL.md` L21 API hash.
- Template: `examples/project-template/src/lib.rs` `configure` plus new `src/observability.rs` (an `AlertSink` that logs with `tracing` and routes `Event` and `Render`), and `AGENTS.md` (one bullet about Error Sinks and the catalog).

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-gateway/src/{lanes.rs, lanes/tests.rs, gateway.rs, shard.rs, lib.rs}`:
   - Add `DropObserver`, `LaneConfig.drop_observer`, the observer path and `GatewayOptions.app` with the shard span field.
   - Update every `GatewayOptions` literal.
   - Tests: Matrix 20-24 and 26.
2. `crates/noctis-core/src/sink.rs` (+ `sink/tests.rs`): the types, `LogSink`, `ErrorSinks`, and the `Arc` blanket impl. Tests: Matrix 1-5, 10-12, 17 (unit level), 18 and 19.
3. `crates/noctis-core/src/observe.rs` (+ tests): `METRICS`, `describe_metrics`, and the label-policy helpers in the test module. Tests: Matrix 32, 33 and 35. Matrix 33 drives the `lanes`, the worker `Harness` and the status updater under `metrics::with_local_recorder` (current-thread tokio).
4. `crates/noctis-core/src/{app.rs, handler.rs, worker.rs, lib.rs}`, `Cargo.toml`:
   - Builder method, wiring, spans, metrics and shard status (a small `pub(crate) fn set_shard_status(app, shard, total, up)` is unit-testable).
   - Tests: Matrix 6-9, 13-17, 27, 28, 30 and 31 (core level), keeping every 5.4-5.6 test green.
5. `crates/noctis/src/lib.rs`: the facade re-exports and docs. Add `crates/noctis/tests/error_sinks.rs` with a custom sink and an `Arc` shared sink through the facade path (Matrix 4 and 7).
6. `crates/noctis-testkit/tests/observability.rs` (its own binary: a global recorder and a global capturing subscriber with `FmtSpan::NEW`):
   - Matrix 25, 29, 30 (FakeGateway `close` with a resumable code on shard 1, then the shard's RESUME/RESUMED) and 34.
   - Use multi-thread tokio, and bound every wait at 15 s.
7. Skill, template, `AGENTS.md`, snapshot:
   - Update per the Code Map and re-bless `xtask/public-api.txt`.
   - Update `SKILL.md`'s API hash in the same commit.
   - Mention every new public name in the skill (skill-check).
8. `.memlog.md` (architecture): the planning decisions are logged; append any implementation decision.

**Acceptance Criteria:**
- Given a running App on the fakes, when shards, events and interactions are processed, then the spans `noctis.shard`, `noctis.event` and `noctis.interaction` carry `app`, `shard`, `guild_id`, `module` and the handler/path fields (Matrix 27-29), and every recorded metric has the `noctis_` prefix, an `app` label and a catalog entry (Matrix 34).
- Given the registered metric set, when the label test runs, then no label name or value can carry a user, guild, channel or message id or free text (Matrix 32-34).
- Given `error_sink(origin, sink)`, when an event handler errors or panics or an overflow drop happens, then that origin's sink receives the report, and without a custom sink `LogSink` logs it at `error` (Matrix 1-25).
- Given the new API, when the gates run, then the skill documents spans, metrics and Error Sinks, and `skill-check`, `api-snapshot` and every other gate pass.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 29 findings — high 0, medium 4, low 12, false 13, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) `noctis_shard_status` stays 1 when `run` stops abnormally — verified: `App::run` returns via `await_startup(..).await?` after other shards set 1, and nothing resets the gauges on error or when the future is dropped; patch: `Drop` for `ShardStatus` sets `0..total` to 0, with a unit test.
  - `[low]` `[patch]` (blind) Two Apps in one process overwrite each other's `app="default"` series — real: multi-App is a live, documented feature (skill §10) and the label is constant until 7.4; patch: one sentence in `app-and-sharding.md` next to the `"default"` note saying several Apps share series until `.app_id(..)`.
  - `[low]` `[reject]` (blind) The `LogSink` drop line lost `overflow_capacity` — true, but the capacity is the author's own `.event_lane(..)` value (default documented), and carrying it needs a new public `ErrorReport` field that the spec's field list excludes.
  - `[low]` `[defer]` (blind) Every overflow drop runs sink code on the shard task with no rate limit — the per-drop error line predates 5.7 (5.5); throttling is a cross-cutting design change; added to `deferred`.
  - `[low]` `[reject]` (blind) Handler metrics re-resolve handles on every run — a recorder key lookup per handler run is negligible next to the handler; caching handles adds per-App state for no named harm.
  - `[low]` `[patch]` (blind) E2E tests release the serial lock while the aborted App may still record — verified: each test calls `running.run.abort()` and drops the guard without awaiting, so a late shard-status set can land after the next test's `from` and break the exact `[0.0, 1.0]` sequences; patch: await the aborted `JoinHandle` before the guard drops.
  - `[false]` `[reject]` (blind) Unit tests wait with fixed 20 ms sleeps — both tests run on a current-thread runtime (required by the local recorder), where every ready handler task is polled before a 20 ms timer fires, so slow CI does not reorder them.
  - `[low]` `[reject]` (blind) Bot authors cannot construct an `ErrorReport` to unit-test a sink — true, but a sink can be driven through the testkit fakes with a failing handler, and a constructor is new public surface the spec deliberately excludes ("only core constructs it").
  - `[low]` `[reject]` (blind) Prelude lacks `ErrorReport`/`ReportKind` — costs one extra `use` line; the spec fixes the prelude contents, so the fix would edit this build's spec.
  - `[false]` `[reject]` (blind) `interaction_unhandled` opens an interaction span for non-interaction inbound — `InboundKind` has only `Interaction` and `Event`; drain's `_` arm and `recv_interaction` only ever pass interactions, so no event is mislabeled (the `Event` arm is unreachable, not wrong).
  - `[false]` `[reject]` (blind) `ReportKind::Dropped` not tied to `ErrorOrigin::Event` — the only producer (`EventDropObserver`) always uses `Event`; the claim is about a hypothetical future caller.
  - `[low]` `[reject]` (blind) `error_sink` double-wraps `Arc` sinks — one extra pointer hop per report is negligible; changing the signature changes public API the spec settled.
  - `[false]` `[reject]` (blind) `ErrorOrigin::index` duplicates declaration order — `index` is a private exhaustive `match` (a new variant fails to compile until added) and `origin_names` checks `ALL[o.index()] == o`; no silent divergence possible.
  - `[false]` `[reject]` (blind) Public-API snapshot lists the `LogSink` `ErrorSink` impl twice — `LogSink` is re-exported once (`noctis-core/src/lib.rs:91`, facade root); duplicated impl lines are a known `cargo-public-api` output pattern seen for many other types in the same file.
  - `[medium]` `[patch]` (edge) `await_startup` Err after some Ready leaves those gauges at 1 — same defect as the first blind row; same patch (`ShardStatus` `Drop`).
  - `[medium]` `[patch]` (edge) `run` aborted while shards are up leaves gauges at 1 — same defect; the `Drop` impl runs when the aborted future is dropped.
  - `[false]` `[reject]` (edge) A third-party `DropObserver` panic kills the shard task — the facade exposes no way to install a `DropObserver`; core's `EventDropObserver` reports through `ErrorSinks` under `catch_unwind` (Matrix 24).
  - `[low]` `[patch]` (edge) Two Apps share `app="default"` series — same defect as the blind multi-App row; same doc patch.
  - `[low]` `[reject]` (edge) Matrix 30's single-shard `1,0` transition is never exercised end to end — the per-shard logic is pinned by the core `shard_status_gauges` unit test (spec Task 4); an E2E variant needs a per-connection close in `FakeGateway`, new testkit surface for a cosmetic coverage gain.
  - `[medium]` `[patch]` (vgap) Logs emitted inside a handler body are never checked for the `noctis.event` span — pre-verified regression gap; patch: a worker test whose handler logs before and after `yield_now().await` and asserts both lines carry the span fields.
  - `[low]` `[patch]` (vgap) `App::run` calling `describe_metrics()` is never observed — pre-verified; patch: `HistoryRecorder` records `describe_*` calls and the E2E test asserts every `METRICS` row was described with its unit.
  - `[false]` `[reject]` (intent 1) Span fields not uniform; interaction `module`/`path` declared empty — the spec deliberately settled this (no interaction handlers before Epic 6, which records them); epics place those handlers later.
  - `[false]` `[reject]` (intent 2) Four of five origins have no producer — settled: their handlers do not exist yet; their stories call `ErrorSinks::report`; routing is unit-tested.
  - `[false]` `[reject]` (intent 3) `noctis.event`/`noctis.interaction` spans verified only at core level — settled by spec Task 4 (Matrix 27-28 at core level), which runs the real drain and worker; the in-handler span gap is covered by the vgap row.
  - `[false]` `[reject]` (intent 4) Label test checks a hand-maintained catalog — settled (R3); the E2E test also checks that every recorded metric is in the catalog.
  - `[false]` `[reject]` (intent 5) `app` label is a constant — settled: `"default"` until Story 7.4's `AppId`; the multi-App collision is handled by the doc patch above.
  - `[low]` `[reject]` (intent 6) Shard-status one-down state pinned only at unit level — same as the edge Matrix 30 row; rejected for the same reason.
  - `[false]` `[reject]` (intent 7) Duration/error metrics exist for `kind="event"` only — settled: only event handlers exist in 5.7 (Design Notes).
  - `[false]` `[reject]` (intent 8) `skill-check` passing not observable from the diff — gates are run in Verification, not encoded in the diff; no defect at any cited location.

## Design Notes

- **Five origins, not seven:** the earlier skill sketch had extra `Hook` and `Overflow` origins. The story AC and FR-22 name five, so overflow drops use `Event` (they are gateway events) and 13.5 routes hook errors to `Event`. `ReportKind` tells them apart. `#[non_exhaustive]` leaves room for more.
- **Synchronous sink:** an overflow drop happens inside `InboundSink::push` on the shard task, which must never await (AD-10). A sync `report` keeps one trait for every path. Authors forward to a channel for async I/O. `catch_unwind` around each call keeps a buggy sink from killing a worker or shard task.
- **`kind` label = handler kind:** `noctis_handler_duration_seconds` and `noctis_handler_errors_total` both use `kind ∈ {command, component, autocomplete, event, render}` (`ErrorOrigin::as_str`). Only `event` is emitted in 5.7. Errors and panics are not split by label; the sink sees `ReportKind`.
- **`noctis_shard_status` is 1/0 (up/down):** this is the simplest alert rule (`== 0`). Richer states belong to 13.5's Lifecycle Hooks.
- **`noctis_http_requests_total{app, status_class}`** moves to Story 5.9: only its edge sees every Discord response (twilight-http has no response hook). The 5.9 planner adds it to the catalog and the label whitelist.
- **Catalog is public:** operators can list what to dashboard, and the label tests have one source of truth. Later stories add rows and whitelist labels (with bounded value sets) together.
- **5.6 deferral closed:** `LogSink` logs the full `source()` chain, which fixes the 5.6 "`error = %error` drops the chain" item.
- Sketch:
  ```rust
  struct Alert;
  impl ErrorSink for Alert {
      fn report(&self, r: ErrorReport<'_>) {
          if let ReportKind::Panic(msg) = r.kind { tracing::error!(origin = %r.origin, msg, "page on-call"); }
      }
  }
  builder.error_sink(ErrorOrigin::Event, Alert);
  ```

## Verification

**Commands** (repo root; local gates only; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- expected: exit 0. Also run `-p noctis-testkit --test observability` 3 times, with no flakes.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo +1.91 check --release --workspace --all-targets` and `cargo deny check` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`, `-- deps-check`, `-- rules-catalog` and `-- skill-check` -- expected: each exits 0 (needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0).
- `test ! -d target/debug` -- expected: true.

## Auto Run Result

- **Summary:** Story 5.7 delivers the observability contract: a sync `ErrorSink` trait with `ErrorReport`/`ReportKind`, per-origin routing over five `ErrorOrigin`s (`LogSink` default logs the full `source()` chain, every sink call under `catch_unwind`), `noctis.event`/`noctis.interaction` tracing spans, a public metrics catalog (`METRICS`, `describe_metrics()` called by `App::run`) with whitelisted bounded labels, handler duration/error metrics (`kind="event"`), `noctis_shard_status` 1/0 gauges, and overflow drops reported as `ReportKind::Dropped` on origin `Event`.
- **Files changed:**
  - `crates/noctis-core/src/sink.rs` (+tests): `ErrorSink`, `ErrorReport`, `ErrorOrigin`, `ErrorSinks` routing, `LogSink`, `EventDropObserver`.
  - `crates/noctis-core/src/observe.rs` (+tests): metrics catalog, label whitelist, `describe_metrics`, `ShardStatus` gauges (reset to 0 on `Drop`).
  - `crates/noctis-core/src/worker.rs` (+tests), `handler.rs`, `app.rs` (+tests), `lib.rs`, `test_support.rs`, `Cargo.toml`: spans, handler metrics, sink wiring, shard-status wiring.
  - `crates/noctis-gateway/src/{lanes.rs,lanes/tests.rs,gateway.rs,shard.rs,lib.rs}`: `DropObserver` hook on the event lane overflow path.
  - `crates/noctis/src/lib.rs`, `crates/noctis/tests/error_sinks.rs`: facade re-exports and `.error_sink(..)` builder API.
  - `crates/noctis-testkit/tests/observability.rs` (+ small edits to `lanes.rs`, `sharding.rs`, `shard_log_redaction.rs`): E2E metrics/spans/sinks tests with `HistoryRecorder`.
  - `examples/project-template/` (`src/observability.rs`, `src/lib.rs`, `AGENTS.md`, skill `SKILL.md` + `app-and-sharding.md`, `modules-middleware-ratelimits.md`, `troubleshooting.md`): usage skill and template updated; API hash refreshed.
  - `xtask/public-api.txt`: re-blessed snapshot; `Cargo.lock`; architecture `.memlog.md` decisions.
- **Review findings:** 29 total (high 0, medium 4, low 12, false 13).
  - Patches (8 entries, 4 distinct fixes; patched by verdict: high 0, medium 4, low 4): `ShardStatus` `Drop` resets gauges to 0 on abnormal `run` exit/abort/startup error (3 medium rows); in-handler span test across `yield_now().await` (medium); multi-App `app="default"` series-sharing doc note (2 low); E2E tests await aborted `JoinHandle` before releasing the serial lock (low); `HistoryRecorder` asserts every `METRICS` row is described with its unit (low).
  - Deferred (1): per-drop overflow sink call has no rate limit (predates 5.7; cross-cutting throttling design).
  - Rejected (20): `LogSink` drop line lacks `overflow_capacity` (needs new public field excluded by spec); handler metric handles re-resolved per run (negligible); fixed 20 ms sleeps (current-thread runtime ordering, false); no public `ErrorReport` constructor (testable via testkit fakes; spec excludes); prelude lacks `ErrorReport`/`ReportKind` (spec fixes prelude); interaction span for non-interaction inbound (unreachable, false); `Dropped` not tied to `Event` (hypothetical, false); `Arc` double-wrap (negligible, public API settled); `ErrorOrigin::index` duplication (exhaustive match + test, false); duplicated `LogSink` impl in snapshot (cargo-public-api output pattern, false); third-party `DropObserver` panic (not installable via facade, false); Matrix 30 single-shard transition not E2E (pinned by unit test; needs new testkit surface) and its intent-6 duplicate; intent 1-5, 7, 8 rows (settled by spec: no interaction handlers before Epic 6, origins without producers, core-level span tests, hand-maintained catalog by R3, constant `app` label until 7.4, event-only metrics, gates run in Verification).
- **Follow-up review recommended: true** (first pass, 4 medium patched). Named risk: the new `ShardStatus` `Drop` reset and its interaction with the `run` abort/startup-error paths are unreviewed patch code; a follow-up pass should confirm the gauges cannot be left at 1 or reset while a sibling App/shard is still live.
- **Verification:** all local gates green via the session gates script (`cargo fmt --all --check`, clippy `--release --all-features -D warnings`, `cargo test --release --workspace` incl. `observability` 3x without flakes, `cargo hack test --each-feature`, `cargo +1.91 check`, `cargo deny check`, xtask `api-snapshot`, `deps-check`, `rules-catalog`, `skill-check`; no `target/debug`). CI paused by the maintainer.
- **Residual risks:** overflow sink calls unthrottled (deferred); several Apps in one process share `app="default"` series until Story 7.4 `AppId`; `noctis_http_requests_total` arrives with Story 5.9; four of five origins have no producer until their handler stories land.
