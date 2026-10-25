---
title: 'Story 16.3: Per-event and per-handler overhead'
type: 'refactor'
created: '2026-10-25'
status: 'done'
baseline_revision: 'effbf2bb0cdc3d26b6e0dbc4529133b7f802300e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-16-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** One handled event with one concurrent no-op handler costs 7 heap blocks after Story 16.2 (`pipeline_alloc`: 7 / 12 / 5 for one concurrent / two concurrent / ordered): every run builds a labeled metric key (`histogram!` with a dynamic `app` label allocates a label `Vec` and does a recorder lookup), allocates a fresh `Arc<LocaleCell>`, collects the concurrent runs into a `Vec` and then `join_all` boxes them again, and every handler but the last gets a full `Event` clone even when its payload type rejects the event.

**Approach:** Cache the event-path metric handles (handler duration/errors per App, drop counter per event type) on first use; check each handler's payload filter before cloning; await a single concurrent handler inline and join several through one allocation; skip Middleware state when no Middleware covers events; reuse a per-App `Arc<LocaleCell>` per guild locale record when nothing can change the run's locales. Then lower `pipeline_alloc`'s `RECORDED`, re-run the bench and record it. No public API change.

## Boundaries & Constraints

**Always:**
- Observable behavior unchanged: span names, fields and nesting when a subscriber is enabled; log targets/levels/messages; error reports (fields, `Cancelled` reports, panic messages); Middleware order and `RunOutcome`s; NFR-4 panic/cancel handling; interaction priority, permits, ordering, shutdown phases (Story 16.2 contract).
- Metric names, label keys and label values unchanged (`noctis_handler_duration_seconds{app, kind="event"}`, `noctis_handler_errors_total{app, kind="event"}`, `noctis_events_dropped_total{app, event_type}`, lane gauges). A handle is registered at most once per label set per App (per lanes instance for drops) and then reused; never registered before its first use (D1).
- Every existing test passes byte-for-byte unchanged (worker, registry, pipeline incl. `pipeline/tests/middleware.rs` calls of `crate::worker::run_handler(&entry, shard, guild, event, &app, &cancel)`, lanes, partition, testkit, facade); `run_handler`'s signature stays.
- A handler whose payload filter rejects the event still passes through `run_handler`'s stages up to the call exactly as today (span, stage-3 "module disabled here" trace, a `Cancelled` report when cancel fired first); it only never receives a clone (D3).
- `unhandled` stays 0 blocks; new keyed collections carry no Discord-id key (deps-check `keyed-map`).
- No new dependency; `metrics` 0.24.6 handles (`Counter`, `Histogram`), std `OnceLock`/`RwLock`, `futures_util::future::join_all` 0.3 (already in the workspace).

**Never:**
- No public API change: `cargo run --release -p xtask -- api-snapshot` passes without `--bless`; the Usage Skill `API hash` line is untouched. `EventHandler`, `EventCtx` keep their public items; new members are crate-private or on the crate-private `events::sealed::Sealed` trait.
- Do not touch the interaction pipelines' metrics (`pipeline.rs`, `pipeline/component.rs`, autocomplete), the lanes' partition/bound logic, the drain loop's ordering, or `crates/noctis-core/benches/pipeline*` measurement code (numbers must stay comparable).
- No bench in any gate, no `[pipeline]` table in `budgets.toml`, no CI file change (no new gate).
- Do not share one `LocaleCell` between runs when a Middleware covering events or a `LocaleResolver` exists.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| accepts, plain/boxed payload | every `payloads!` type `P`, event of `P`'s variant / another variant / `GatewayHeartbeatAck` | `P::accepts(&e) == P::extract(e.clone()).is_some()` (true / false / false) | — |
| accepts, `UnavailableGuild` | `GuildDelete{unavailable: Some(true)}` / `Some(false)` / `None`; `Event::UnavailableGuild` | true / false / false; true. `GuildDelete` handler: true for all three `GuildDelete` | — |
| Share, empty | 0 handlers | no clone, no handler call, event dropped | — |
| Share, one accepting | [A] | A gets the original (0 clones) | — |
| Share, all accept | [A, A, A] | clones to 0 and 1, original to 2 (2 clones, as today) | — |
| Share, rejecting first/middle/last | [R, A] / [A, R, A] / [A, R] | [None, orig] 0 clones / [clone, None, orig] 1 / [orig, None] 0 | — |
| Share, none accept | [R, R] | both get `None`, 0 clones; both still traverse `run_handler` stages | rejected runs report `Cancelled` only if cancel fired first (as today) |
| Share, ordered + concurrent | ordered [A], concurrent [R] / ordered [R], concurrent [A, A] | orig to ordered A, concurrent job carries `None` / ordered `None`, concurrent clone + orig | — |
| Share, last acceptor's Module disabled | [A, A] with handler 1's Module off in the guild | handler 0 clone runs; handler 1 traces "module disabled here", event dropped | — |
| Concurrent count 1 | `MESSAGE_DELETE`, one handler | awaited inline in the job task: no `join_all`, no `Vec` | panic caught as today |
| Concurrent count 2..=30 | N handlers | one `join_all` over an exact-size iterator: one allocation for all N futures; all start before any ends | — |
| Concurrent count 31+ | 31 handlers | all run (futures_util uses `FuturesOrdered` above 30, documented) | — |
| Handler metrics, first run | App, recorder installed, first handled event | `register_histogram` for `{app, kind=event}` once, sample recorded | — |
| Handler metrics, later runs | 100 events, 3 errors, 2 panics | still 1 `register_histogram` and ≤ 1 `register_counter` per App; 100 samples; counter = 5 | — |
| Handler metrics, no error ever | 10 Ok runs | `noctis_handler_errors_total` never registered (absent from a `DebuggingRecorder` snapshot, as today) | — |
| Handler metrics, two Apps | Apps "a" and "b" | one handle each, labels `app=a` / `app=b` | — |
| Drop counter cache | 5 dropped `TYPING_START` + 2 dropped `PRESENCE_UPDATE` | `register_counter` 2 times (one per event type), increments 7, labels `{app, event_type}` unchanged | — |
| Drop counter, gateway-only kind | dropped event whose `name()` is `None` | label `UNKNOWN`, cached under its `EventType` | — |
| No recorder | dhat test / bench (noop recorder) | handles are noop handles, cached; no per-event allocation | — |
| Recorder installed after first event | event runs, then global recorder set | that App's cached event metrics stay on the earlier recorder (documented: install before `App::run`) | — |
| Locale reuse, plain | no resolver, no event Middleware, guild G record `de`, 2 runs | both runs' `EventCtx` locale cells `Arc::ptr_eq`; locales = built-in(None, de) | — |
| Locale reuse, record changes | `GUILD_UPDATE` G → `fr`, then a run | a different cached cell holding built-in(None, fr) | — |
| Locale reuse, guildless / no record / unavailable locale | `None` guild; guild without record; record not in available set | one shared cell per key (no-record and guildless share the `None` slot); value = built-in resolution (falls back to App default) | — |
| Locale, Middleware covers events | before-Middleware overrides locale in run 1 | a new cell per run (as today); run 2 sees the unmodified resolution | — |
| Locale, `LocaleResolver` set | 3 runs | resolver called 3 times; a new cell per run (as today) | resolver panic: built-in + one `error`, as today |
| No event Middleware | entry whose chain covers no event Middleware | no `Run`, no `RunInfo`, no `Chain` clone; extensions empty; reports unchanged | — |
| Middleware present | before + after Middleware on the Module | order, extensions, `RunOutcome`, panic reports unchanged | as today |
| Span disabled | no subscriber | `noctis.event` fields not evaluated, no `Instrumented` clone of a disabled span | — |
| Span enabled | fmt/capture subscriber | `noctis.event{app, shard, guild_id, module, handler, event_type}` exactly as today, same parents | — |
| Cancel before start | cancel fired, accepted handler | `Cancelled` report, no call, permit released | — |

</intent-contract>

## Code Map

- `crates/noctis-core/src/worker.rs` -- the target. `run_concurrent` L576 (`filter_map` → `collect::<Vec<_>>()` → `join_all`), `take_event` L599 (clone for all but last), `run_ordered_job` L539 (same rule over ordered ++ concurrent), `Job` L67 (`event: Event`), `run_handler` L626 (span L636, stage 3 L645, `stage4` L655, `Around::new` L671, `EventCtx::new` L686, `handler.call` L697, `.instrument(span.clone())` L708), `record_duration` L790 / `count_error` L800 (labeled macros per call).
- `crates/noctis-core/src/handler.rs` -- `AppShared` L203 (add `event_metrics`); `EventCtx`/`CtxInner` L295-330 (`locales: Arc<LocaleCell>`; add `#[cfg(test)] fn locale_cell()`); `EventHandler` L455 (`call: Arc<ErasedHandler>`; add an `accepts: fn(&Event) -> bool` field set in `__new` from `P::accepts`, plus `pub(crate) fn accepts`).
- `crates/noctis-core/src/events.rs` -- `sealed::Sealed` L55 (`extract`); `payloads!` macro L93 (`@extract boxed|plain|custom`); `extract_unavailable_guild` L148. Add `fn accepts(event: &Event) -> bool` with matching `@accepts` arms.
- `crates/noctis-core/src/locale.rs` -- `AppLocales` L199 (`available`, `default`, `resolver`, `guilds`), `resolve_run` L255, `guild` L291, `LocaleCell::new` L382, `index` L131 (`Locale as u8`, `Locale::ALL` has 32 entries). Add the per-record cell cache.
- `crates/noctis-core/src/pipeline.rs` L857 `stage4` -- unchanged; still used when the cache does not apply.
- `crates/noctis-core/src/middleware.rs` -- `Chain::covers` L625, `Around::new` L1010 (clones the chain; builds `Run` only when covered), `Run::new` L673. Read-only except an optional `Around` constructor without a chain clone.
- `crates/noctis-core/src/registry.rs` L36 `Entry`, L200 the only `Entry` literal (fold) -- add `event_middleware: bool` = `chain.covers(true)`.
- `crates/noctis-core/src/app.rs` L3169 -- the only `AppShared` literal; add `event_metrics: Default::default()`.
- `crates/noctis-gateway/src/lanes.rs` -- `count_drop` L540 (labeled `counter!` per drop); `Shared` struct (add the drop-counter cache); lane gauges already registered once in `lanes_with` L223 (read-only evidence).
- `crates/noctis-core/tests/pipeline_alloc.rs` -- `RECORDED` (7/12/5 → measured); module doc mentions Stories 16.2/16.3.
- `crates/noctis-core/benches/PIPELINE.md` -- append the Story 16.3 section; the "Story 16.2" (b) rows are the comparison base.
- Tests to mirror: `worker/tests.rs` `handler_metrics` L943 (DebuggingRecorder, absence of errors counter), `lanes/partition_tests.rs` L531 `Counting` recorder pattern, `pipeline/tests/middleware.rs` L707-716 (filtered run through `run_handler`).
- Docs: `crates/noctis/src/lib.rs` L542 and `observe` module doc L1539; Usage Skill `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` §11 L1095 and L1153-1154 (recorder timing).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/events.rs` -- add `fn accepts(event: &Event) -> bool` to `sealed::Sealed`; generate it in `payloads!` (`boxed`/`plain`: `matches!(event, Event::$ty(_))`; `custom UnavailableGuild`: `GuildDelete` with `unavailable: Some(true)` or `Event::UnavailableGuild(_)`); unit-test the accepts/extract equivalence rows -- D2.
- `crates/noctis-core/src/handler.rs` -- `EventHandler` stores `accepts: fn(&Event) -> bool` (`P::accepts`), `pub(crate) fn accepts(&self, &Event) -> bool`; `AppShared.event_metrics: crate::worker::EventMetrics`; `#[cfg(test)] EventCtx::locale_cell()` -- D1, D2.
- `crates/noctis-core/src/registry.rs` -- `Entry.event_middleware: bool` computed once at fold -- D6.
- `crates/noctis-core/src/locale.rs` -- `AppLocales::event_cell(&self, guild: Option<Locale>) -> Option<Arc<LocaleCell>>`: `None` when `resolver` is set; else a lazily filled `[OnceLock<Arc<LocaleCell>>; 33]` slot (index `Locale as usize`, 32 = no record) holding `LocaleCell::new(self.builtin(None, guild))`; unit tests for slots, `ptr_eq`, resolver → `None` -- D5.
- `crates/noctis-core/src/worker.rs` -- (a) `pub(crate) struct EventMetrics { duration: OnceLock<Histogram>, errors: OnceLock<Counter> }` with getters registering via the unchanged macros on first use; `record_duration`/`count_error` use it. (b) Event share per D3: a small crate-private helper (generic over `T: Clone` so tests count clones) that gives the original to the last payload-accepting handler (ordered then concurrent), clones to earlier accepting ones, `None` to rejecting ones; `Job.event` becomes `Option<Event>`. (c) `run_handler(entry, shard, guild_id, event: Event, app, cancel)` stays as a wrapper over `run_entry(.., event: Option<Event>, ..)`, which runs today's stages in today's order and, at the call, treats `None` like `handler.call` returning `None`. (d) `run_concurrent`: 0 → release permit; 1 → `run_entry(..).await` inline; ≥ 2 → `join_all(handlers.concurrent.iter().enumerate().map(..))` (exact size, no intermediate `Vec`). (e) In `run_entry`: when `!entry.event_middleware` build no `Around` chain clone/`RunInfo`; locales = `app.locales.event_cell(app.locales.guild(guild_id))` when `!entry.event_middleware`, else `stage4` (unchanged); skip `.instrument(span.clone())`/`in_scope` wrappers when `span.is_disabled()`. Keep every report/log/span exactly as today -- D1, D3, D4, D5, D6, D7.
- `crates/noctis-core/src/worker/tests.rs` -- add (do not edit existing tests): share-helper rows with a clone-counting type; concurrent count 1/2/31 all start; registration-count test with a counting `metrics::Recorder` (local recorder, current-thread runtime) for 100 runs incl. errors/panics; two Apps two handles; locale reuse rows via `locale_cell()` (plain, record change, guildless, Middleware override isolation, resolver called per run); rejected-handler rows (no clone, still `Cancelled` report under cancel, "module disabled here" trace preserved) using `UnavailableGuild` + `GuildDelete` handlers on `GUILD_DELETE` -- covers the Matrix.
- `crates/noctis-gateway/src/lanes.rs` -- `Shared` gets `drop_counters: RwLock<HashMap<EventType, Counter>>` (bounded by `EventType` variants; read lock hit, write lock on miss, never held across the `DropObserver`/logs); `count_drop` uses it -- D1.
- `crates/noctis-gateway/src/lanes/partition_tests.rs` -- add the drop-counter registration test (two event types, 7 drops, 2 registrations, labels).
- `crates/noctis-core/tests/pipeline_alloc.rs` -- run; set `RECORDED` to the measured counts (expected `unhandled` 0, `one_concurrent` 3, `two_concurrent` 6, `ordered` 3); update the `RECORDED` doc comment with the 16.3 changes. A count above 3 for `one_concurrent` gets a `(decision)` memlog line with its cause.
- `crates/noctis-core/benches/PIPELINE.md` -- append "Story 16.3: per-event and per-handler overhead" (Machine, full commit hash from `git rev-parse HEAD` of the measured tree, every scenario table), a "16.2 vs 16.3" table for (b) 1 and 4 handlers at P = 1/4/16 (events/s both, ratio, p99 both), the dhat table (before 16.3 / after, blocks and bytes) and a Targets table (≥ 1.3× per (b) row, dhat targets); update the intro sentence naming the sections.
- `crates/noctis/src/lib.rs`, `examples/project-template/.claude/skills/noctis/references/app-and-sharding.md` §11 -- state that noctis registers its metric handles once (lane gauges at run start, the event-handler and drop metrics on first use) so the recorder must be installed before `App::run`; behavior docs only (API hash unchanged; `skill-check` passes).
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` -- append to AD-25 "- **Amended (Story 16.3):**" with the measured per-case dispatch allocation budget and the cached-metric-handle rule.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions D1-D8 are already appended; add implementation decisions and any missed target with measured value and cause.

**Acceptance Criteria:**
- Given an App with a recorder, when 100 events run through one event handler (with errors and panics), then the recorder sees one `register_histogram` and at most one `register_counter` for `{app, kind=event}`, and repeated drops of one event type register `noctis_events_dropped_total` once per event type, with names and labels unchanged.
- Given a `GUILD_DELETE` with `unavailable: None` and concurrent `UnavailableGuild` + `GuildDelete` handlers, when it is dispatched, then the event is never cloned (the `GuildDelete` handler gets the original) and the `UnavailableGuild` handler's run still traverses its stages without calling the handler.
- Given the final tree, when `cargo test --release -p noctis-core --test pipeline_alloc` runs, then `unhandled` = 0, `one_concurrent` ≤ 3, and `RECORDED` equals the measured counts (lower than 7 / 12 / 5).
- Given the 16.1 bench on the same 4-CPU host, when the final tree runs it, then PIPELINE.md records scenario (b) 1 and 4 handlers at P = 1/4/16 with the ratio to Story 16.2's rows; each row's ≥ 1.3× verdict is recorded, and a miss is logged with measured value and cause (D8, not a gate).
- Given the gates, when the story is done, then `api-snapshot` passes without `--bless`, `git diff --exit-code xtask/public-api.txt` is clean, deps-check (incl. `keyed-map`) passes, and every existing test passes unchanged.

## Spec Change Log

## Review Triage Log

### 2026-10-25 — Review pass
- verdicts: 27 findings — high 0, medium 3, low 8, false 16, maybe-false 0
- findings:
  - `[low]` `[patch]` (blind) `lanes.rs` doctests (`LanePartition`, `recv_batch`, `take`) now import an unused `HashMap` — real: none of the three examples uses it (search-and-replace leak from the module import at `lanes.rs:11`); patch: restore `use std::collections::VecDeque;` in the three doc examples.
  - `[false]` `[reject]` (blind) `run_entry` with `event: None` still runs stage 4 (resolver call) and `Around::new` — not a regression: before 16.3 a rejecting handler got a clone and ran every stage up to `handler.call` returning `None`; D3 and the Boundaries require the pre-call stages "exactly as today", so the resolver call count is unchanged.
  - `[low]` `[reject]` (blind) `Share` takes two separately supplied predicates; ordered `nth` scan is O(n^2) — `Chain::nth` over slice iterators is O(1) per call (O(n) total), and every caller passes the same `entry.handler.accepts`, so no disagreement is reachable; a stored predicate or mask adds generics or an allocation for no everyday harm.
  - `[medium]` `[patch]` (blind) clone-avoidance tests drive `Share` directly, never `run_ordered_job`/`run_concurrent` with mixed ordered + concurrent rejecting handlers — same root cause as the verification-gap finding (no end-to-end test of the ordered++concurrent share); patch: worker tests below.
  - `[false]` `[reject]` (blind) cached handles lose metrics for a recorder installed after the first event — settled by D1 and the Matrix row "Recorder installed after first event" (AC allows registration on first use) and documented in the facade docs and Usage Skill.
  - `[low]` `[patch]` (blind) Usage Skill §11 dropped the "call `describe_metrics()` yourself if you install the recorder later" guidance without saying what a late recorder still gets — real: interaction metrics still use per-call macros and reach a late recorder, and the Skill is bot authors' only source; patch: say a late recorder misses the lane gauges and event/dropped-event metrics, still gets the rest, and then needs `describe_metrics()` called itself.
  - `[false]` `[reject]` (blind) `count_drop`'s `RwLock` read lock contends during floods — spec-mandated shape (Tasks, lanes.rs); the read-lock hit replaces a recorder lookup plus a label `Vec` allocation per drop, so it is cheaper than the baseline, and (d) throughput is recorded in PIPELINE.md.
  - `[false]` `[reject]` (blind) shared `LocaleCell` invariant not type-enforced — no current writer reaches a shared cell (only `Run::set_locales`, built only with event Middleware, which disables sharing); the precondition is documented on `AppLocales::event_cell` and D5; a hypothetical future setter is not a defect of this diff.
  - `[low]` `[patch]` (blind) `quiet()` wrappers around existing partition tests work around a tracing interest-cache race without a recorded root cause — real and an intent-contract deviation (existing tests edited); grouped with the test-edit finding below.
  - `[false]` `[reject]` (blind) bench ratios rest on one run and are noise — the spec requires this table (D8, recorded not gated), every verdict is "missed" either way, and the text itself states the gain lies within the run-to-run spread.
  - `[false]` `[reject]` (blind) provenance: PIPELINE.md names `57277cc`, spec baseline `effbf2b`, status in-progress — `57277cc` differs from `effbf2b` only in the spec frontmatter and `sprint-status.yaml`, so the measured code is identical; status is set by the workflow at finalize, and the fix would edit this build's spec.
  - `[false]` `[reject]` (blind) spine says gauges register "at `lanes_with`", docs say "when the run starts" — `App::run` builds its lanes through `run_lanes` → `lanes_with` at run start (`app.rs:754-756`), so both statements are true for their audiences.
  - `[false]` `[reject]` (blind) `run_ordered_job` spawns a concurrent job even when no concurrent handler accepts — deliberate (D3, memlog implementation decision): rejecting runs keep spans, stage-3 traces and `Cancelled` reports; before 16.3 the same job was spawned with a clone, so no added cost.
  - `[medium]` `[patch]` (blind) no test covers a rejecting handler in the ordered part (ordered `UnavailableGuild` then `GuildDelete` on a plain `GUILD_DELETE`) — same root cause as the verification-gap finding; patch: worker tests below.
  - `[low]` `[patch]` (edge) existing partition tests wrapped in `quiet(..)` and three existing doctests rewritten, contrary to "every existing test passes byte-for-byte unchanged" — real; the race is pre-existing (the same `noctis::lanes` drop-log callsites predate 16.3) and its victims are the `lanes::tests` log assertions; patch: restore the four test bodies, fix the race in the `observed` helper of `lanes/tests.rs`, log a decision (doctest part: see the first row).
  - `[low]` `[reject]` (edge) unnamed gateway-only kinds share label `UNKNOWN` but are cached per `EventType`, so one label set can register more than once — the App's droppable set comes from the registry's dispatch types plus `DROPPABLE_EVENTS` (`app.rs` `lane_config`), so no unnamed kind is ever dropped on the App path; re-registering one key is harmless to recorders; the Matrix row settled caching under `EventType`.
  - `[false]` `[reject]` (edge) task spawned and permit held when no concurrent handler accepts — same as the blind row above: deliberate per D3; the suggested guard would drop stage-3 traces of rejecting concurrent runs.
  - `[false]` `[reject]` (edge) stage 4 and Run state built for a rejected, uncancelled run — same as the blind row above: stages are kept exactly as today per D3.
  - `[medium]` `[patch]` (vgap, pre-verified) ordered-to-concurrent hand-off with `Job.event = None` unverified: spawning only on `Some(rest)` would pass every test — patch: add `mod overhead` worker tests: (1) ordered accepting `GuildDelete` + concurrent rejecting `UnavailableGuild` on `guild_delete(None)` with the Module disabled → one "module disabled here" line per handler; (2) the same handlers enabled under a wind-down cancel (like `ordered_then_concurrent_cancelled`) → a `cancelled` report for the rejecting concurrent handler; (3) an ordered rejecting `UnavailableGuild` before an ordered accepting `GuildDelete` → the accepting handler runs once and the rejecting one traces stage 3 when disabled.
  - `[low]` `[patch]` (vgap other) three public doc examples import an unused `HashMap` — same defect as the first row; same patch.
  - `[false]` `[reject]` (intent D1) scenario (b) 1.3x target missed — settled by D8 and the epic context ("Bench numbers do not gate CI yet"; Story 16.2 D7 precedent); the miss is logged with measured values and cause in the memlog and PIPELINE.md.
  - `[low]` `[patch]` (intent D2) existing tests and doctests edited despite "every existing test passes unchanged" — same defects as the edge test-edit row and the doctest row; same patches.
  - `[false]` `[reject]` (intent D3) dhat window is push-to-handler-start, not lane pop to handler return — the intent names "the 16.1 dhat test", whose window this is; the spec pins that test's `RECORDED`.
  - `[false]` `[reject]` (intent D4) locale `Arc` reuse excludes Apps with a `LocaleResolver` — D5 settles it correctly: a resolver may return a different result per run, so a shared cell would change behavior (Matrix row "Locale, `LocaleResolver` set").
  - `[false]` `[reject]` (intent D5) "one allocation for their futures" not met under the all-futures reading — D4 settles the run-future reading; 2 concurrent = 6 blocks matches the spec's expected counts.
  - `[false]` `[reject]` (intent D6) late recorder misses event metrics — settled by D1 (AC allows "on first use") and documented; same as the blind row.
  - `[false]` `[reject]` (intent D7) span fields still declared — D7: `info_span!` evaluates fields only for an interested subscriber (library behavior), and the diff skips `in_scope`/`instrument` for a disabled span; no field is built for a disabled span.

## Design Notes

**Decisions** (memlog, planning):
- D1: Metric handles are cached lazily on first use per label set (per App: `OnceLock` for duration and errors with `kind = "event"`; per lanes instance: one `Counter` per `EventType`), not registered at App build: eager registration would put a never-incremented `noctis_handler_errors_total` into `DebuggingRecorder` snapshots (existing tests assert its absence) and bind handles before a recorder installed between build and run. A recorder installed after an App's first event misses that App's cached handles (as lane gauges already do since `lanes_with`); docs say "install before `App::run`".
- D2: "payload type or derived filter" = what the `EventHandler` derive's payload extraction checks: the event variant, plus `UnavailableGuild`'s `unavailable: Some(true)`. It becomes the sealed `accepts(&Event)`; stage 3 (Module state) stays inside `run_handler` unchanged.
- D3: Share rule: original to the last payload-accepting handler (ordered, then concurrent), clones to earlier accepting ones, `None` to rejecting ones, which still run `run_handler`'s pre-call stages so spans, traces and `Cancelled` reports are unchanged.
- D4: One concurrent handler is awaited inline in the job task; several use one `join_all` over an exact-size iterator (one boxed slice for ≤ 30; futures_util 0.3 uses `FuturesOrdered` above 30).
- D5: Locale cells are shared per guild locale record only without a `LocaleResolver` and without event Middleware (the only writer of a `LocaleCell` is `Run::set_locales`, the before-Middleware locale override, middleware.rs L716-721, reachable only when a `Run` exists); the App default is fixed after build.
- D6: `Entry.event_middleware` (precomputed `chain.covers(true)`) gates all Middleware state; with it false no `Chain` clone, `RunInfo` or `Run` is built.
- D7: `info_span!` already evaluates fields only for an interested subscriber; additionally `instrument`/`in_scope` are skipped for a disabled span.
- D8: Bench targets are recorded, not gated (epic context; as Story 16.2 D7).

**Expected block counts:** one concurrent = spawned job task + `EventCtx` `Arc` + boxed handler future = 3 (removed: label `Vec`, `LocaleCell` `Arc`, `Vec` collect, `join_all` box); two concurrent = task + one `join_all` slice + 2 × (ctx + future) = 6; ordered = runner task + ctx + future = 3.

## Verification

**Commands** (repository root; wrap long ones in `tools/story-loop/bg.sh`):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-core --lib worker`, `--lib locale`, `--lib events`, `--lib registry`, `--lib pipeline` -- pass
- `cargo test --release -p noctis-gateway` -- pass
- `cargo test --release -p noctis-core --test pipeline_alloc` -- pass with updated `RECORDED`
- `cargo bench -p noctis-core --bench pipeline -- --test` -- exit 0; then `tools/story-loop/bg.sh cargo bench -p noctis-core --bench pipeline` -- numbers into PIPELINE.md
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- deps-check`, `rules-catalog`, `api-snapshot` (no `--bless`), `skill-check`, `tw-grammar --check`; `git diff --exit-code xtask/public-api.txt` -- ok
- `cargo test --release -p noctis-image --features render --test tw_crosscheck`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is run by the gate script only (16.3 is Epic 16's last story).

## Auto Run Result

- **Summary:** Cut per-event and per-handler pipeline overhead: sealed `accepts(&Event)` payload filter so rejecting handlers get no clone (original to last accepting handler, clones to earlier ones); lazily cached metric handles (per-App duration/errors, per-lanes drop counter per `EventType`); shared per-guild `LocaleCell` when no `LocaleResolver` and no event Middleware; Middleware state built only when `event_middleware` covers; single concurrent handler awaited inline, several via one `join_all`; disabled spans skip `instrument`/`in_scope`. Expected allocations: 3 blocks for one concurrent handler, 6 for two, 3 ordered; dhat `RECORDED` updated; bench numbers recorded in PIPELINE.md (scenario (b) 1.3x target missed, recorded not gated per D8).
- **Files changed:** `crates/noctis-core/src/worker.rs` (share rule, inline/join_all, gated Middleware/span work) and `worker/tests.rs` (clone-avoidance + `mod overhead` end-to-end tests); `events.rs`/`events/tests.rs` (sealed `accepts`); `handler.rs`, `registry.rs`, `app.rs` (accepts plumbing, cached metric handles); `locale.rs`/`locale/tests.rs` (shared event cell); `crates/noctis-gateway/src/lanes.rs` and `lanes/{tests,partition_tests}.rs` (cached drop counters, `observed` helper race fix); `crates/noctis-core/tests/pipeline_alloc.rs` (new `RECORDED`); `benches/PIPELINE.md` (16.3 numbers); `crates/noctis/src/lib.rs` (metrics docs); Usage Skill `app-and-sharding.md`; ARCHITECTURE-SPINE.md and memlog.
- **Review findings:** 27 total (high 0, medium 3, low 8, false 16). Patched: 3 medium (missing end-to-end tests of the ordered++concurrent share, ordered rejecting handler, `Job.event = None` hand-off; added `mod overhead` worker tests) and 5 low (unused `HashMap` in three lanes doctests restored to `VecDeque`; Usage Skill late-recorder guidance; `quiet()` wrappers and test edits reverted, tracing interest-cache race fixed in the `observed` helper, decision logged). Deferred: none. Rejected: 3 low (two-predicate `Share` is O(n) and caller-consistent; `UNKNOWN` label re-registration unreachable on the App path; plus grouped duplicates) and 16 false (all settled by D1/D3/D4/D5/D7/D8: late recorder misses cached handles by design, stages kept for rejecting runs, `RwLock` read cheaper than baseline, bench single-run noise acknowledged, provenance commit identical code, dhat window per 16.1 test, locale sharing excluded with resolver).
- **Follow-up review recommended:** false (first pass patched 0 high; the 3 patched medium entries share one root cause, missing end-to-end tests, now covered by the added tests).
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, pipeline_alloc, deps-check, rules-catalog, api-snapshot, skill-check, tw-grammar, tw_crosscheck, cargo deny, MSRV 1.91); CI paused by maintainer.
- **Residual risks:** a metrics recorder installed after an App's first event misses lane gauges and event/dropped-event metrics (documented; install before `App::run`); scenario (b) throughput gain within run-to-run noise and below the 1.3x target; shared `LocaleCell` invariant is documented, not type-enforced.
