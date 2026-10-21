---
title: 'Story 7.6: Middleware and typed Extensions'
type: 'feature'
created: '2026-10-21'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** AD-23 stages 7 (before-Middleware) and 10 (after-Middleware) do not exist: a Module cannot run shared code around its handlers, stop processing, or hand typed data (`Ext<T>`) to handlers, after-Middleware and Error Sinks. The Usage Skill sketches this API as `[planned, Story 7.6]` (FR-20; AD-23, AD-9, AD-4 App-build; Conventions BuildError).

**Approach:**
- Modules register function Middleware as values: `Module::before(Middleware::before(f).provides::<T>())` and `Module::after(Middleware::after(f))`.
- `App::build` folds a per-handler chain (root Module first, then registration order). It fails with `BuildViolation::MissingExtension` when a handler's `Ext<T>` has no provider on its path.
- Each pipeline runs the chain around the handler: Commands, context menus, `Dispatcher` prefix Commands, components, modal submits and events.
- `#[handler]` accepts `Ext<T>` and `Option<Ext<T>>` parameters and extracts them from the run's frozen Extensions.

## Boundaries & Constraints

**Always:**
- **New module** `crates/noctis-core/src/middleware.rs` (unit tests in `middleware/tests.rs`). Re-exports:
  - root `noctis_core` and `noctis`: `Middleware`, `BeforeMiddleware`, `AfterMiddleware`, `BeforeCtx`, `AfterCtx`, `Flow`, `Invocation`, `RunOutcome`, `Ext`, `Extensions`, `ExtensionMissing`;
  - the prelude adds `Middleware`, `BeforeCtx`, `AfterCtx`, `Flow` and `Ext`.
- **Constructors.** `pub enum Middleware {}` is uninhabited and only hosts the constructors:
  - `Middleware::before<F, Fut>(f: F) -> BeforeMiddleware`, where `F: Fn(BeforeCtx) -> Fut + Send + Sync + 'static` and `Fut: Future<Output = HandlerResult<Flow>> + Send + 'static`;
  - `Middleware::after<F, Fut>(f: F) -> AfterMiddleware`, where `F: Fn(AfterCtx) -> Fut + Send + Sync + 'static` and `Fut: Future<Output = ()> + Send + 'static`.
- **The Middleware values.** Both are `Clone + Debug`; `Debug` shows the name, the scope and `provides`.
  - The name is `std::any::type_name::<F>()`, exposed as `name() -> &'static str`.
  - `#[must_use]` builders:
    - `BeforeMiddleware::provides::<T: Send + Sync + 'static>(self) -> Self` (a repeated `T` is kept once);
    - on both types, `events_only(self)` and `skip_events(self)` (the last call wins; the default runs for every handler kind).
  - "Events" means `#[derive(EventHandler)]` handlers. Everything else (slash Commands, context menus, `Dispatcher` Commands, components, modal submits) is "non-event".
- **`Module::before(self, m: BeforeMiddleware) -> Self` and `Module::after(self, m: AfterMiddleware) -> Self`** (`#[must_use]`). Both record in call order, and the Module's `Debug` lists the Middleware names.
- **`Flow`:** `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Flow { Continue, Stop(Option<MessageKey>) }`.
- **`BeforeCtx`** is an owned, cheap `Clone` handle (one `Arc`, with a `std::sync::Mutex` map inside; no guard is held across an `.await`). Accessors:
  - `invocation() -> Invocation`, `module()` (the handler's Module id), `handler()` (the struct path), `shard()`, `guild_id()`, `channel_id() -> Option<..>` and `user_id() -> Option<Id<UserMarker>>` (`None` for events);
  - `state::<T>() -> Option<State<T>>`, `kv() -> Kv` and `http()`;
  - `insert<T: Send + Sync + 'static>(&self, value: T)`, `get::<T>() -> Option<Ext<T>>` and `contains::<T>() -> bool`.
- **`AfterCtx`** is the same owned `Clone` handle, read-only. It has the read accessors above, plus `outcome() -> RunOutcome`, `extensions() -> &Extensions` and `get::<T>() -> Option<Ext<T>>`.
- **`Invocation`** (`#[non_exhaustive]`, `Copy`, `Debug`, `Eq`) has the variants:
  - `Command { path }`: slash Commands and `Dispatcher` Commands;
  - `ContextMenu { name }`;
  - `Component { route }`;
  - `Modal { route }`;
  - `Event { event_type }`.

  All fields are `&'static str`.
- **`RunOutcome`** (`#[non_exhaustive]`, `Copy`, `Debug`, `Eq`) has the variants:
  - `Completed`: the handler returned `Ok` and its response was sent;
  - `Stopped`: a before-Middleware returned `Stop`;
  - `UserError`: a `HandlerError::user` or a failed validator, from the handler or a before-Middleware;
  - `Failed`: any other error, a failed send or `ExtensionMissing`;
  - `Panicked`.

  `is_completed()` is `true` only for `Completed`.
- **`Ext<T>`:** a `pub struct` over `Arc<T>`. It implements `Deref<Target = T>`, `AsRef<T>` and `Clone`, `Debug` when `T: Debug`, and `into_arc(self) -> Arc<T>`.
- **`Extensions`** is not publicly constructible. It is a small `Vec` keyed by `TypeId` that keeps the type name and the inserting Middleware's name. Its API is `get::<T>() -> Option<&T>`, `contains::<T>()`, `len()`, `is_empty()` and `type_names() -> impl Iterator<Item = &'static str>`.
  - `Debug` prints the type names only, never a value.
  - An empty map allocates nothing.
  - When an insert replaces an existing type, it emits one `debug!(target: "noctis::middleware", extension = <type name>, previous = <old mw>, by = <new mw>, "extension replaced")`; the last insert wins.
  - An insert after this run's before-Middleware ended (a leaked `BeforeCtx` clone) is dropped, with one `debug` on the same target.
- **`ExtensionMissing`** (`thiserror`, `#[non_exhaustive]`, accessors `extension()` and `handler()`): `handler `{handler}` takes `Ext<{extension}>`, but no Middleware inserted it in this run`.
- **Fold** (`registry.rs`, `registry/commands.rs`):
  - Every folded Module computes once a chain: its parent's `before` list followed by its own (filtered per handler kind at run time, or pre-split), and the same for `after`. Each `Entry`, `RouteEntry`, `CommandEntry` and `MenuEntry` stores that chain (`Arc` slices, shared by the Module's entries).
  - Update the `ancestors` `expect(dead_code)` reasons so they no longer cite Story 7.6.
- **Build check** (in `Registry::fold`, after the existing checks):
  - Walk the folded Modules in fold order. Within a Module, take event handlers, commands, menus, components and modals, each in `add` order.
  - A required type `T` is satisfied when a before-Middleware on the handler's chain declares it and its scope covers the handler's kind.
  - Each unsatisfied `T` gives one violation. A handler gives at most one violation per `T`, in parameter order.
  - The new violation, with a doctest:
    ```rust
    #[error("handler `{handler}` (Module `{module}`) takes `Ext<{extension}>`, but no before-Middleware on its Module path provides it: add `.before(Middleware::before(f).provides::<{extension}>())` to Module `{module}` or an ancestor, or take `Option<Ext<{extension}>>`")]
    MissingExtension { module: &'static str, handler: &'static str, extension: &'static str }
    ```
    `extension` is `type_name::<T>()`.
- **Descriptors.** `CommandHandler`, `ContextMenuHandler`, `EventHandler`, `ComponentHandler` and `ModalHandler` each gain a `requires: &'static [__ExtType]` field (default `&[]`).
  - `__ExtType` holds `fn() -> TypeId` and `fn() -> &'static str`, built by `pub const fn of::<T: Send + Sync + 'static>()`.
  - The hidden trait `__RequireExtensions` (`fn __require_extensions(self, &'static [__ExtType]) -> Self`) sets the field.
  - Both items live in `kind.rs` and are exported like the other `__` items.
- **Extraction** (`kind.rs`):
  - The hidden trait `__Extensions` (`fn __extensions(&self) -> &Extensions`) is implemented for `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx` and `EventCtx`. It returns a `static` empty map while no chain has frozen one.
  - `pub fn __ext<T, C: __Extensions>(ctx: &C, handler: &'static str) -> HandlerResult<Ext<T>>` returns `Err(HandlerError::new(ExtensionMissing{..}))` when the type is missing.
  - `pub fn __ext_opt<T, C: __Extensions>(ctx: &C) -> Option<Ext<T>>`.
- **Where Extensions live.** The handler sees the frozen `Arc<Extensions>` when its future is first polled, so the slot is set after the before chain and before stage 9.
  - The contexts are created before stage 7 (component stage 4, the command `start` closure, `EventCtx::new`), so they hold a shared slot that the pipeline fills. Suggested: `Option<Arc<OnceLock<Arc<Extensions>>>>`, allocated only when the chain is non-empty.
  - With an empty chain, a run adds no heap allocation (the existing `alloc_dispatch.rs` and `memory_flat.rs` dhat tests stay green).
- **`#[handler]` macro** (`crates/noctis-macros/src/handler.rs`):
  - The typed parameters after `ctx` may include any number of `Ext<T>` / `Option<Ext<T>>`, at any position. Detection is syntactic: the type's last path segment is `Ext` with exactly one type argument, or `Option<that>`.
  - The other parameters are classified into the existing shapes `__C`, `__CX`, `__CR` and `__CXR`.
  - With at least one Ext parameter, the bind argument becomes a `move` closure. It takes the remaining parameters with their written types and returns `async move { … }`. That block:
    - extracts each Ext with `__ext`/`__ext_opt`, writing `match … { Ok(e) => e, Err(e) => return Err(e) }` rather than `?`, so inference holds;
    - then calls `Self::<method>(this, <all args in written order>).await`.
  - The descriptor is wrapped in `__require_extensions(const { &[__ExtType::of::<T>(), …] })`, listing only the required (non-`Option`) types.
  - Without Ext parameters, the expansion is byte-identical to today.
  - Errors:
    - an `Ext` with other generic arguments: ``"`Ext` takes one type: `Ext<T>`"``;
    - an Ext as the first parameter after `self`: ``"the first parameter after `self` is the context (`ctx: …Ctx`); put `Ext<T>` after it"``;
    - `PARAMS` now reads `(self, ctx[, path | target | event][, Ext<T> | Option<Ext<T>> …][, r: <Kind>Responder<..>])`.
- **Hooks and autocomplete take no Ext** (`hook.rs`, `autocomplete.rs`): an `Ext`/`Option<Ext>` parameter is a `syn::Error`, either ``"`#[on_stale]`/`#[on_expired]` hooks run no Middleware, so they take no `Ext<T>`"`` or ``"autocomplete runs no Middleware, so it takes no `Ext<T>`"``.
- **Run semantics** (shared helpers in `middleware.rs`; called from `pipeline.rs::run_started`/`execute`, `pipeline/component.rs`, `Dispatcher::run_command` and `worker.rs::run_handler`):
  1. Middleware run only when a handler future exists, after the auto-defer was armed (interactions).
     - No Middleware runs for: a start panic, a filtered event (`call` → `None`), autocomplete, the Expired chain (hooks and the App `expired_handler`), stage 1–5 rejections, the detached `__testing::call_*` seams, or a run already cancelled.
  2. Before chain, in order, sequentially. Each call and its future run under `catch_unwind`, inside a `noctis.middleware{middleware, phase = "before"}` span, raced against the cancel token like the handler.
     - `Continue` → the next Middleware.
     - `Stop(k)` → skip the rest and the handler, with outcome `Stopped` and one `debug` (`noctis::middleware`, `middleware`, `handler`, "stopped"). Then:
       - interactions with `Some(k)`: `send_notice(k)` in the run's notice locale;
       - interactions with `None`: send nothing; if the state is `DEFERRED` with a type-5 deferred original, delete it (a failure is a `warn`);
       - events: send nothing (the `k` is ignored).
     - `Err(e)` or a panic → skip the rest and the handler, and treat it like a handler error or panic (notice, user-condition `debug` or report). The report carries `middleware: Some(name)`.
  3. Freeze the map, fill the slot, then run stage 9 as today (stage 8, Guards, slots in here with Story 7.7).
     - `noctis_handler_duration_seconds` starts at stage 9 and is recorded only when the handler ran.
  4. The notice, then the auto-defer timer drop, as today.
  5. After chain:
     - It runs in reverse chain order: the own Module's last-registered first, the root's first-registered last.
     - It runs for every outcome except a cancellation, with `RunOutcome` and the frozen map (whatever was inserted when a before-Middleware stopped or failed).
     - Each runs under `catch_unwind` in a `noctis.middleware{…, phase = "after"}` span.
     - A panic is a separate `ReportKind::Panic` report to the same origin's sink with `middleware: Some(name)`, plus one `noctis_handler_errors_total` increment. The remaining after-Middleware still run.
  6. Error Sink routing, as today.
- **`ErrorReport`** (`sink.rs`) gains:
  - `pub extensions: Option<&'a Extensions>`: `Some` (possibly empty) for every Command, menu, `Dispatcher`, component, modal or event handler run (stopped or failed ones included), `None` for overflow drops, autocomplete and Expired-chain runs;
  - `pub middleware: Option<&'a str>`: the failing Middleware.

  Update `sink/tests.rs`, `LogSink` (it logs `middleware` when set and never extension values) and every constructor.
- Extensions are never serialized or written to any port. `T` needs only `Send + Sync + 'static`.
- **Docs in the same commit:**
  - `observe.rs` module docs: the span list (`noctis.middleware{app, middleware, phase}`).
  - `pipeline.rs` docs: the stage 7/10 lines (L18, L23, L637).
  - `pipeline/component.rs` module docs.
  - Spine AD-23: an `Amended (2026-10-21)` line with the Design Notes decisions.
  - Skill and template (Tasks 9 and 10), then re-bless `xtask/public-api.txt` and update the `SKILL.md` API hash.

**Never:**
- Locale override (`set_locale`; Story 11.3), Guards (7.7), `State<T>` handler parameters, Middleware on autocomplete or the Expired chain, new metrics, or Middleware registered by type or derive.
- Holding a `Mutex` guard across `.await`, logging Extension values, or writing Extensions to a port.
- Changing the `custom_id` or record formats, adding a new external crate, or making a debug build.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error handling |
|---|----------|---------------|----------|----------------|
| 1 | macro, no Ext | `(self, ctx: CommandCtx)` | expansion unchanged (existing `shapes` test passes as is) | — |
| 2 | macro, one Ext | `(self, ctx: ComponentCtx, s: Ext<A>)` | shape `__C`, closure wrapper, `requires = [A]` | — |
| 3 | macro, optional | `(self, ctx, s: Option<Ext<A>>)` | shape `__C`, no `__require_extensions` | — |
| 4 | macro, mixed | `(self, ctx: CommandCtx, p: P, a: Ext<A>, b: Option<Ext<B>>, r: CommandResponder)` | `__CXR<P, CommandResponder>`, call order preserved, requires `[A]` | — |
| 5 | macro, Ext before path | `(self, ctx, a: Ext<A>, p: P)` | `__CX<P>` | — |
| 6 | macro, qualified | `a: noctis::Ext<A>`, `b: core::option::Option<noctis::Ext<B>>` | detected | — |
| 7 | macro, bad generics | `Ext`, `Ext<A, B>`, `Ext<'a, A>` | error | ``"`Ext` takes one type: `Ext<T>`"`` |
| 8 | macro, Ext as ctx | `(self, a: Ext<A>)` | error | the "first parameter … is the context" message |
| 9 | macro, too many | `(self, ctx, x, y, r, z)` (non-Ext) | error | the updated `PARAMS` text |
| 10 | macro, hooks/autocomplete | an `Ext<A>` param on `#[on_stale]`, `#[on_expired]`, `#[autocomplete(f)]` | error | the hook/autocomplete messages |
| 11 | macro, event | `(self, ctx: EventCtx, ev: MessageCreate, a: Ext<A>)` | `__CX<MessageCreate>`, requires `[A]` | — |
| 12 | provides dedupe | `.provides::<A>().provides::<A>()` | `provides` holds `A` once | — |
| 13 | scope last wins | `.events_only().skip_events()` | skip_events | — |
| 14 | build, own provider | handler `Ext<A>` in M, M provides A | ok | — |
| 15 | build, ancestor | provider in M's grandparent | ok | — |
| 16 | build, child/sibling only | provider only in a child or sibling of M | `MissingExtension{module: M, handler, extension: "…::A"}` | listed with every other violation |
| 17 | build, scope mismatch | `events_only` provider + Command handler; `skip_events` provider + event handler | one violation each | — |
| 18 | build, opt-out | `Option<Ext<A>>`, no provider | ok | — |
| 19 | build, many | two handlers missing A and B; `Ext<A>` twice in one handler | violations in fold order; one per (handler, T) | exact message text asserted once |
| 20 | build, two mounts | one Module mounted under a provider root and under a bare root | one violation (bare path only) | — |
| 21 | order | root `before(r1,r2)` `after(a1,a2)`, child `before(c1)` `after(ca1)`, handler in child | r1 r2 c1 handler ca1 a2 a1 | — |
| 22 | scope | child Middleware, handler in root | child Middleware not run | — |
| 23 | stop | r2 → `Stop(None)` | c1 and the handler skipped; ca1 a2 a1 run with `Stopped` | `debug` "stopped" |
| 24 | stop notice, pending | slash command, `Stop(Some(k))` before 2 s | one type-4 `EPHEMERAL` callback with `k`'s text (tr user: the tr text, non-ASCII intact) | — |
| 25 | stop notice, deferred | Middleware sleeps past `ack_deadline`, then `Stop(Some(k))` | type-5 defer, `DELETE @original`, ephemeral follow-up; component `response = update`: type 6 + follow-up, nothing deleted | — |
| 26 | silent stop | `Stop(None)` pending / after a type-5 defer | no request / only `DELETE …/@original` | delete failure = `warn` |
| 27 | before error | `Err(HandlerError::new(io))` | handler skipped, generic notice, sink report `Error` with `middleware: Some(name)`, counter +1, after chain with `Failed` | — |
| 28 | before user error | `Err(HandlerError::user(k))` | notice `k`, `debug`, no report, after with `UserError` | — |
| 29 | before panic | panics in the call or the future | generic notice, `Panic` report with `middleware`, after with `Panicked` | caught |
| 30 | after panic | a1 panics | ca1, a2 still ran; a1's `Panic` report (origin's sink, `middleware: a1`); main outcome reported unchanged | caught |
| 31 | cancel | shutdown token fires during before | no handler, no after; `Cancelled` report | — |
| 32 | events | event Module with r1/c1/a1, handler `Ext<A>` | same order; `Stop(Some(k))` sends nothing; an error goes to the Event sink with `middleware` | — |
| 33 | event filtered | handler's derived filter rejects the event | no Middleware call | — |
| 34 | not run | autocomplete, stale/expired chain, unknown Route rejection, detached `__testing::call_component` | counting Middleware sees 0 calls; detached `Ext<A>` → `ExtensionMissing` | — |
| 35 | Dispatcher | prefix `run_command` with a Module Middleware `Stop(Some(k))` | Middleware ran with `Invocation::Command`; public reply notice to the message | — |
| 36 | ext value | r1 inserts `A(7)` | handler `*a == A(7)`; after `get::<A>()` and `ErrorReport.extensions` have it | — |
| 37 | replace | r1 inserts `A(1)`, c1 inserts `A(2)` | handler sees `A(2)`; one `debug` "extension replaced" naming both Middleware, no value | — |
| 38 | optional absent | `Option<Ext<B>>`, nothing inserted | `None` | — |
| 39 | declared not inserted | provider declares A, returns `Continue` without insert | handler not run; `ExtensionMissing` → generic notice + sink | — |
| 40 | late insert | a clone kept in a spawned task inserts after freeze | ignored, `debug` | — |
| 41 | ZST / unit | `insert(Marker)` | `Ext<Marker>` works | — |
| 42 | never a port | run that inserts Extensions, handler writes nothing | every memory store passed to the builder has `usage().entries == 0` | — |
| 43 | Debug | `A { secret: "hunter2" }` inserted | `format!("{:?}", extensions)` names `A`, never contains `hunter2`; `LogSink` output neither | — |
| 44 | empty chain | no Middleware anywhere | `ErrorReport.extensions == Some(empty)`; dhat tests unchanged | — |
| 45 | no sink fields | overflow drop / autocomplete report | `extensions: None`, `middleware: None` | — |

</intent-contract>

## Code Map

- `crates/noctis-core/src/module.rs`: the `Module` struct (add `before`/`after` Vecs), `Debug`, `children()`.
- `crates/noctis-core/src/registry.rs`:
  - L28-90 `Entry`/`RouteEntry`, with their `ancestors` `expect(dead_code)`;
  - L130-215 `fold`: the stack carries `ancestors`, so carry the chain next to it; the check goes next to `check_uses` (L300).
- `crates/noctis-core/src/registry/commands.rs`: L23-50 `CommandEntry`/`MenuEntry` and `Folded`.
- `crates/noctis-core/src/error.rs`: `BuildViolation` (next to `UnmountedRoute` ~L316, same doc and doctest style).
- `crates/noctis-core/src/pipeline.rs`:
  - module docs L1-30;
  - `Report` L316;
  - `execute` L316-336 and `run_started` L362-463 (insert the before chain between start and poll, and the after chain between the notice/timer drop and the sink);
  - `run_interaction` L549-660 (stage 7 comment L637) and `start` L507;
  - `Dispatcher::run_command` L1187.
- `crates/noctis-core/src/pipeline/component.rs`:
  - `run_component_over` L500-730: stage 4 creates the future and `parts: Arc<CtxInner>`; the Expired branch must skip Middleware;
  - `Report` built at L640.
- `crates/noctis-core/src/worker.rs` L376-452 `run_handler` (events; `call` returns `None` when filtered).
- `crates/noctis-core/src/command.rs` L84-120 `CtxInner` (shared by Command/Component/Modal/Expired ctxs; add the slot).
- `crates/noctis-core/src/handler.rs`:
  - `EventCtx` and its `CtxInner` (`EventCtx::new`);
  - `EventHandler` (add `requires`);
  - `AppShared`.
- `crates/noctis-core/src/component.rs`: L520 `ComponentHandler`, L769 `ModalHandler`, and the `ctx_accessors!` L66.
- `crates/noctis-core/src/menu.rs`: L102 `ContextMenuHandler`.
- `crates/noctis-core/src/kind.rs`:
  - the shape markers L70-95;
  - `__Bind` impls (unchanged);
  - `__AttachHooks` L794: add `__RequireExtensions`, `__ExtType`, `__Extensions`, `__ext` and `__ext_opt` here.
- `crates/noctis-core/src/responder.rs`:
  - L1854 `Shared::notice` (reuse for `Stop(Some)`);
  - `delete_reply` L1914 and `Shape::Deferred` (reuse for the silent stop).
- `crates/noctis-core/src/sink.rs`: `ErrorReport` L128 and `LogSink`; `sink/tests.rs` L41 and L315.
- `crates/noctis-core/src/lib.rs` L206-260 re-exports; `observe.rs` L10-22 span docs.
- `crates/noctis-macros/src/handler.rs` (`shape` L57, `expand` L130, tests); `hook.rs`; `autocomplete.rs`; `command.rs::is_responder` (the syntactic-classifier pattern).
- `crates/noctis/src/lib.rs`: root re-exports ~L563, the prelude L1819-1873 (its doc list too).
- Wire-test pattern: `crates/noctis-testkit/tests/components.rs` and `events.rs` (`FakeGateway` + `FakeDiscordHttp`, capture subscriber); facade test pattern: `crates/noctis/tests/components.rs`, `error_sinks.rs`.
- Skill:
  - `references/modules-middleware-ratelimits.md` §1 L24 and §5 L236-278 (the planned sketch to replace), §6 (`ErrorReport` fields);
  - `app-and-sharding.md` L625-632 (extractor paragraph);
  - `adapters.md` L342 (Middleware `kv()` [planned]);
  - `i18n.md` L160 (`set_locale` → `[planned, Story 11.3]`);
  - `SKILL.md` L23 (API hash).
- Template:
  - `src/modules/dashboard/mod.rs` and `handlers.rs` L40-44 (the guild check to move into Middleware);
  - `locales/en-US.yaml` L42 `common.guild_only`;
  - `AGENTS.md` (the handler-shape bullets L54-57).

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/src/middleware.rs` (+ `lib.rs` wiring): the types, `Extensions`, `Ext`, the contexts and the run helpers, with unit tests for rows 12, 13, 36-38, 40, 41 and 43.
2. `crates/noctis-core/src/{module.rs, registry.rs, registry/commands.rs, error.rs}`: registration, chains, `MissingExtension`. Unit tests in `registry/tests.rs` for rows 14-20.
3. `crates/noctis-core/src/{kind.rs, command.rs, component.rs, menu.rs, handler.rs}`: `requires`, `__ExtType`, `__RequireExtensions`, `__Extensions`, `__ext`, `__ext_opt`, the ctx slots.
4. `crates/noctis-core/src/{pipeline.rs, pipeline/component.rs, worker.rs, sink.rs, responder.rs}`: stages 7/10 per Run semantics, `ErrorReport` fields, silent-stop helper. Unit tests in `pipeline/tests/middleware.rs` (detached/fake transports, `Capture` log) for rows 21-31, 33, 34, 39, 44 and 45.
5. `crates/noctis-macros/src/{handler.rs, hook.rs, autocomplete.rs}`: classification, wrapper and errors, with unit tests for rows 1-11.
6. `crates/noctis/src/lib.rs`: the re-exports, the prelude and its docs.
7. `crates/noctis/tests/middleware.rs`: real derives, each handler kind with Ext parameters in several positions (rows 2-6 and 11 compile and run). Build violations through `App::builder().build()` (rows 16 and 19 exact text). Row 42 with shared memory stores.
8. `crates/noctis-testkit/tests/middleware.rs` (real wire, `FakeGateway` + `FakeDiscordHttp`), asserting method, path and body:
   - `stop_notice_is_ephemeral_reply` (row 24);
   - `stop_after_defer_deletes_original_then_follows_up` (row 25, with `ack_deadline(Duration::from_millis(50))`);
   - `silent_stop_after_defer_deletes_original` (row 26);
   - `event_middleware_order_and_extensions` (row 32);
   - `dispatcher_runs_middleware` (row 35);
   - `extension_reaches_handler_and_after` (rows 21 and 36, over a component click).
9. Skill: rewrite §5 of `modules-middleware-ratelimits.md` with the real API. It covers:
   - a `load_settings` example, `provides`, `Flow::Stop`, `events_only`/`skip_events`, the ordering diagram and the after-on-every-outcome rule;
   - that nothing runs for autocomplete or the Expired chain;
   - `ExtensionMissing` and `MissingExtension` (exact text), and Extensions never reaching a port.

   Then drop the `[planned, Story 7.6]` markers, update the §6 `ErrorReport` fields, `app-and-sharding.md` (Ext parameters real, `State<T>` parameters still `[planned]`), `adapters.md` L342 and `i18n.md` L160, and `troubleshooting.md` (a `MissingExtension` entry). Every new public item must be mentioned (skill-check).
10. Template:
    - `src/modules/dashboard/mod.rs`: `pub struct DashGuild(pub Id<GuildMarker>)`, `async fn guild_only(ctx: BeforeCtx) -> HandlerResult<Flow>`, which inserts `DashGuild` or returns `Flow::Stop(Some(keys::COMMON_GUILD_ONLY))`, and `.before(Middleware::before(guild_only).skip_events().provides::<DashGuild>())`;
    - `handlers.rs`: `Dashboard::handle` takes `guild: Ext<DashGuild>` instead of its `guild_id()` check (autocomplete keeps `ctx.guild_id()?`);
    - `AGENTS.md`: one Middleware bullet, and the handler-shape bullets now allow `Ext<T>`/`Option<Ext<T>>` after `ctx`.
11. Spine AD-23 amendment. Run `cargo run --release -p xtask -- api-snapshot --bless`, then the `SKILL.md` hash. Then every gate in Verification.

**Acceptance Criteria:**
- Given before- and after-Middleware on a parent and a child Module, when a slash command, a component click and a gateway event run through `FakeGateway`, then the recorded order is parent-before, child-before, handler, child-after, parent-after, and a `Stop` prevents the handler from running.
- Given a before-Middleware that inserts `T` and a handler taking `Ext<T>`, when the interaction runs, then the handler receives the value, a second insert of `T` by the child wins with one `debug` trace, and the after-Middleware and the Error Sink's `ErrorReport.extensions` see `T`, while no port store holds any entry.
- Given a handler taking `Ext<T>` with no providing before-Middleware on its Module path, when `App::build` runs, then it returns `BuildViolation::MissingExtension` naming the handler, its Module and `T`, together with every other violation; with `Option<Ext<T>>` it builds.
- Given the Usage Skill, when `skill-check` runs, then it passes, Middleware/`Ext<T>`/ordering are documented with no `[planned, Story 7.6]` left, and the API hash matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Function Middleware, by value (decision).** Middleware are not interactions (§7 covers interactions only). A value can carry configuration (`move` closures), and the shape matches the skill's planned sketch. An owned `BeforeCtx` with interior mutability avoids the higher-ranked `async fn(&mut Ctx)` bound, which stable Rust cannot type with a `Send` future.
- **Independent before/after lists (decision).** After-Middleware run for every outcome except a shutdown cancellation, so audit and metrics Middleware see stops and failures. Pairing them like tower would hide stops from parents.
- **Not for autocomplete, the Expired chain or rejections (decision).** AD-23's amendment runs hooks through stages 6, 9 and 10 only. Autocomplete has a separate 2.5 s path without a Responder.
- **`events_only` / `skip_events` (decision).** Without them, an auth Middleware with I/O would run on every `MESSAGE_CREATE`. The scope also counts in the build check.
- **`Stop(None)` (decision).** It sends nothing. A Pending interaction then fails on Discord's side; the skill says to use `Stop(Some(key))` for denials. A type-5 "thinking…" original is deleted so it never lingers.
- **Locale override deferred to Story 11.3** (its ACs own FR-20's locale override); the skill keeps it `[planned, Story 11.3]`.
- Golden sketch of the macro wrapper (one Ext):
  ```rust
  <<Self as __Kind>::Kind as __Bind<__C, Self, _>>::__bind(NAME, move |this: Self, ctx: ComponentCtx| async move {
      let s = match __ext::<GuildSettings, _>(&ctx, NAME) { Ok(e) => e, Err(e) => return Err(e) };
      Self::handle(this, ctx, s).await
  })
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild, dhat `alloc_dispatch`/`memory_flat` included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok (no rule added)
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rn "Story 7.6" crates examples/project-template --include=*.rs --include=*.md` -- no hit
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
