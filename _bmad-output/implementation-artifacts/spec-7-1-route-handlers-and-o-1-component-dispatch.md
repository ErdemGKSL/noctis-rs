---
title: 'Story 7.1: Route handlers and O(1) component dispatch'
type: 'feature'
created: '2026-10-21'
status: 'in-progress'
baseline_revision: '09e12a548b28dbc440d6009655a68d5258c44a47'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 6.11 ships component and modal structs (`#[derive(Button)]`, the select derives, `#[derive(Modal)]`) with a `#[handler]` method, and `Module::add::<X>()` stores their `ComponentHandler`/`ModalHandler`. But `App::build` never reads them, so no click or modal submit reaches a handler, a Route id may have two handlers, and nothing proves dispatch is O(1), allocation-free or free of per-message state. Implements FR-15, FR-58 (dynamic half), NFR-2, NFR-9, UX-DR7; AD-7, AD-9, AD-19, AD-25; rule CID-04.

**Approach:**
- `Registry::fold` gains one immutable route map: `HashMap<&'static str, RouteEntry>`. It holds components and modals together, because Route ids are unique per App.
- Folding reports two `BuildViolation`s:
  - `DuplicateRoute` when two handlers share a Route id;
  - `UnmountedRoute` when a mounted Module declares a Route with the new `Module::uses::<R>()` and no mounted Module handles it.
- The worker sends `MESSAGE_COMPONENT` and `MODAL_SUBMIT` interactions to a new pipeline. It decodes the `custom_id` once, looks the Route up with the borrowed `&str` and runs the handler on the decoded struct.
- A dhat test proves Route lookup plus fixed-width decode allocate nothing with 10 and with 10 000 Routes.
- Further tests prove that memory and alive tasks stay flat over N clicks.

## Boundaries & Constraints

**Always:**
- **Compile-time half (shipped by 6.11, kept).** These stay as they are, and their trybuild fixtures stay green:
  - the derive implements `Handled`, and the `[CID-04]` E0080 fires on a kind-derived struct without a `#[handler]` method;
  - `crates/noctis/tests/types/handler_on_free_fn.rs` (free fn → help shows `impl R { #[handler] async fn handle(self, ctx) }`) and `ui/cid/CID-04_route_without_handler.rs` keep passing.
  - The binding now takes the Route id from `<S as noctis_codec::Route>::ID.as_str()`, so a descriptor is bound by `R::ID`.
- **Binding bounds.** In `crates/noctis-core/src/kind.rs`:
  - `__Bind<__C, S, F> for __ComponentKind` adds `S: noctis_codec::Route<Payload = S> + for<'de> noctis_codec::PayloadDecode<'de>`;
  - the modal impl adds `S: noctis_ui::ModalForm`.
  - `ComponentHandler::__new`/`ModalHandler::__new` keep their signatures (an explicit `route`, for tests) and gain the same bounds. Each stores:
    - a typed start (decode, then box the handler future);
    - a plain `fn` pointer probe for components: `fn(&Decoded<'_>) -> Result<(), DecodeError>`, which calls the same generic `decode_payload::<S>` as the start.
  - The `Box<dyn Any>` `call` path stays, for `__testing::call_component`/`call_modal`.
- **Registry (AD-7, AD-25).**
  - `crates/noctis-core/src/registry.rs` adds `routes: HashMap<&'static str, Arc<RouteEntry>>`, where `RouteEntry { module, ancestors, handler: RouteHandler }` and `enum RouteHandler { Component(ComponentHandler), Modal(ModalHandler) }`.
  - It is filled in the same pre-order fold as event handlers. The first registration in fold order is kept.
  - Lookup is `routes.get(decoded.route())` with a borrowed `&str`. No String key, no `to_owned`, no scan.
- **Build violations** (new variants of `BuildViolation`, in `crates/noctis-core/src/error.rs`; the texts are required):
  - `DuplicateRoute { route, first_module, first_handler, second_module, second_handler }`: ``[CID-04] Route `{route}` has two handlers: `{first_handler}` (Module `{first_module}`) and `{second_handler}` (Module `{second_module}`): keep one handler per Route id and rename the other Route (renaming breaks its live components)``. Each extra registration after the first gives one violation.
  - `UnmountedRoute { module, route, route_type }`: ``[CID-04] Module `{module}` uses Route `{route}` (`{route_type}`), but no mounted Module handles it: add `.add::<{route_type}>()` to a mounted Module``. Here `route_type` is `std::any::type_name::<R>()`. There is one violation per (Module, Route) pair, deduplicated.
  - Order: the existing violations, then every `DuplicateRoute` in fold order, then every `UnmountedRoute` in fold order. All are reported at once.
- **`Module::uses::<R>()`** (new public API, `crates/noctis-core/src/module.rs`):
  - signature `pub fn uses<R: noctis_ui::Handled>(self) -> Self`, `#[must_use]`;
  - records `(R::ID.as_str(), type_name::<R>())` and is shown in `Debug`;
  - it declares that this Module's Views build components of a Route that another Module handles.
  - The check runs only for mounted Modules, so a Route handled in a child or sibling mounted Module passes.
  - `R` not `Handled` (e.g. `String`) is the existing `Handled` E0277 `[CID-04]` text.
- **Component pipeline**, new `crates/noctis-core/src/pipeline/component.rs`, spawned by `worker.rs` `interaction()` for `InteractionType::MessageComponent` and `ModalSubmit`:
  1. **Read the input.** For components, read `custom_id` and `component_type` from `InteractionData::MessageComponent`. For modals, read the id from `noctis_ui::ModalSubmission::from_slice(raw)`.
  2. **Look up and decode.**
     - `noctis_codec::decode` gives the `Decoded`; `routes.get(..)` gives the entry.
     - The kind must match. A component interaction needs a `Component` handler whose `ComponentKind` matches the type: 2 Button, 3 StringSelect, 5 User, 6 Role, 7 Mentionable, 8 Channel. A modal submit needs a `Modal` handler.
     - Components decode with `Decoded::payload::<S>()` (no keys). Modals decode with `S::from_submission(&submission)`.
  3. **Build the context.** Make a `ComponentCtx`/`ModalSubmitCtx` from `CtxInner` (module, shard, guild, channel, user, locales, `app`), the same way `run_interaction` does.
  4. **Run the handler** under `catch_unwind`, raced against the shutdown cancel token. Record `noctis_handler_duration_seconds{kind="component"}`.
  5. **Report a failure.**
     - `Err` that `is_user()`: `debug` on target `noctis::component`.
     - Any other `Err`, a panic or a cancellation: increment `noctis_handler_errors_total{kind="component"}` and report to the `ErrorOrigin::Component` sink through `report_logged`, with module, handler name and `event_type` `INTERACTION_CREATE`.
  - The span is `noctis.interaction` with `app`, `shard`, `guild_id`, `module`, `route`.
  - The pipeline sends no HTTP request: there are no notices, acks or responses until Story 7.2.
- **Rejections.** A rejection is a `pub(crate) enum Rejection`. It logs one `warn!` on target `noctis::dispatch` with `reason = Rejection::as_str()`, `shard` and, when known, `route`. The handler never runs, nothing is sent and nothing panics. The labels:
  - `malformed`: codec phase 1 failed, or the modal submission is unparsable;
  - `unknown_route`: no entry, or an entry of the other family;
  - `kind_mismatch`;
  - `stale` (Story 7.3 routes it);
  - `forged`: a bad, missing or unverifiable tag, so signed Routes are rejected until Story 7.2 loads `NOCTIS_PAYLOAD_KEY`;
  - `ref_unresolved` (Story 7.5);
  - `undecodable`: Malformed from `DecodeError`;
  - `submit_invalid`: a `SubmitError` other than Malformed;
  - `no_author`.
  - The CID-06 generic ephemeral notice arrives with Story 7.2.
- **NFR-2.** Dispatch stores nothing per message or component: no closure, task, timer or map entry. The one tracked task per interaction ends when the handler ends.
- **Docs and skill in the same change.**
  - `docs/discord-rules.md` CID-04 row: the App-build tier states the two texts and `Module::uses`, and drops "(Story 7.1)".
  - Spine AD-19: the dynamic-half sentence adds ``(a Module declares Routes it renders but does not handle with `Module::uses::<R>()`)``.
  - `component.rs` module docs: dispatch is live.
  - Facade `crates/noctis/src/lib.rs` docs, wherever they say dispatch is planned.
  - The Usage Skill:
    - `references/routes-payloads-refs.md` §1: dispatch is live with the limits above; one handler per Route; `uses`;
    - `references/modules-middleware-ratelimits.md`: `uses` in the Module section;
    - `references/troubleshooting.md`: the App build errors table uses the two exact texts.
  - The template's `AGENTS.md`: a one-handler-per-Route and `uses` line.
  - `xtask/public-api.txt` re-blessed, and the `API hash` in `SKILL.md` updated.
- **CI.** Add `cargo test --release -p noctis --test alloc_dispatch` to the `allocation` job in `.github/workflows-disabled/ci.yml`, and replace the "Story 7.1 adds…" comment.

**Never:**
- Things later stories own:
  - Responders, auto-defer, `response`/`auto_defer` on kind attributes, notices, `NOCTIS_PAYLOAD_KEY` (Story 7.2);
  - stale/expired handlers (7.3); Ref resolution (7.5); Middleware/Extensions (7.6); Guards (7.7); `ctx.values()`.
- A String-keyed or linear-scan route map; per-message closures, timers or tasks; new external crates (`dhat` is already a workspace dependency); debug builds.
- Scanning handler bodies for referenced Routes, or `inventory`/`linkme` registries (see Design Notes).

## I/O & Edge-Case Matrix

`Buy` = `#[derive(Button)] #[button("shop.buy", defaults(style = Primary, label = "Buy"))] struct Buy { item: u64 }`, whose `custom_id` for `item: 255` is `0shop.buy:47`. "warn X" = one `warn!` with `reason = "X"`, no handler run, zero HTTP requests.

| # | Scenario | Input / State | Expected | Handling |
|---|---|---|---|---|
| 1 | Happy click | component type 2, `custom_id` `0shop.buy:47`, `Buy` mounted | handler runs once with `self.item == 255`; ctx module/user/guild/shard/locales from the interaction | — |
| 2 | Duplicate across Modules | `Buy` added in Modules `a` and `b` | `BuildError` with one `DuplicateRoute` naming route `shop.buy`, both handler names and `a`, `b` | build fails |
| 3 | Duplicate in one Module | `.add::<Buy>().add::<Buy>()` | one `DuplicateRoute`, Module named twice | build fails |
| 4 | Same id, other family | Button `"x"` and Modal `"x"` | one `DuplicateRoute` | build fails |
| 5 | Three handlers, one id | `x` in Modules a, b, c | two violations (b vs a, c vs a) | build fails |
| 6 | Two distinct structs, distinct ids | `Buy`, `Sell` | builds; both dispatch | — |
| 7 | `uses` satisfied | `a.uses::<Buy>()`, `Buy` added in mounted `b`, or in `a`'s child | builds | — |
| 8 | `uses` unmounted | `a.uses::<Buy>()`, `Buy` added nowhere or only in an unmounted Module | one `UnmountedRoute` naming `a`, `shop.buy`, the type path | build fails |
| 9 | `uses` duplicated / in unmounted Module | `a.uses::<Buy>().uses::<Buy>()` unmounted Buy; `uses` declared only on an unmounted Module | one violation; none | — |
| 10 | `uses` non-Route | `.uses::<String>()` | E0277 ``[CID-04] Route `String` has no handler…`` | compile error (`tests/types/uses_not_a_route.rs`) |
| 11 | Empty / no Routes | App with no components | builds; any click → warn `unknown_route` | log |
| 12 | Empty `custom_id` | `""` | warn `malformed` | log |
| 13 | Length | valid id of exactly 100 chars; 101 chars | dispatched; warn `malformed` | log |
| 14 | Non-ASCII / case | `0shöp:1`; `0Shop.buy:47` | warn `malformed` ×2 | log |
| 15 | Route id bounds | registered 24-char id clicked; an unregistered id | dispatched; warn `unknown_route` | log |
| 16 | Unknown route | `0nope:` | warn `unknown_route` | log |
| 17 | Family mismatch | modal Route's id in a component interaction; Button id in a modal submit | warn `unknown_route` ×2 | log |
| 18 | Kind mismatch | `Buy`'s id with component_type 3 | warn `kind_mismatch` | log |
| 19 | Select | `#[derive(StringSelectMenu)] Pick` id (with nonce), type 3 | handler runs | — |
| 20 | Schema | id with schema 1 for a schema-0 Route | warn `stale` | log |
| 21 | Ref body | `by_ref` Route id `0r:~<11 base62>` | warn `ref_unresolved` | log |
| 22 | Signed | `signed` Route, valid or bad tag | warn `forged` | log |
| 23 | Body | `0shop.buy:` (missing field); `0shop.buy:47,9` (extra) | warn `undecodable` | log |
| 24 | No author / data | interaction without `member`/`user`; `data` absent | warn `no_author`; warn `malformed` | log |
| 25 | Handler errors | `Err(HandlerError::new(..))`; `Err(HandlerError::user(key))`; panic; cancel during run | Component sink `Error`, errors_total +1; `debug` only; sink `Panic`, worker keeps serving the next click; sink `Cancelled` | sink |
| 26 | Modal happy | submit `custom_id` `0bio:<user>` + text input | `BioForm` handler with `user` and `bio` | — |
| 27 | Modal bad | body not JSON / no `custom_id`; required input missing | warn `malformed`; warn `submit_invalid` | log |
| 28 | Alloc, 10 Routes | `Buy` + 9 handlers with other ids; 10 000 probes of valid fixed-width ids plus rejected ids | dhat blocks/bytes delta 0 | test fails otherwise |
| 29 | Alloc, 10 000 Routes | the same with 9 999 extra ids (`r0`…`r9998`, leaked before measuring) | delta 0; a `String`-field negative control allocates ≥1 | test |
| 30 | Memory flat | 100 warm-up dispatches, then 1 000, then 10 000 more of the same Route on distinct message ids | `curr_bytes` after 11 000 ≤ after 1 100 + 1 KiB | test |
| 31 | Tasks flat | N = 50 clicks through `FakeGateway` | handler count 50; `num_alive_tasks()` returns to its pre-click baseline within the wait bound | test |

</intent-contract>

## Code Map

- `crates/noctis-core/Cargo.toml` -- add `noctis-codec.workspace = true` (deps-check allows core→codec, `xtask/src/deps_check.rs` L72).
- `crates/noctis-core/src/component.rs` -- `ComponentHandler`/`ModalHandler` (L148-280): add the typed start, the component probe and the bounds. Keep `call`/`erase` for the seams. Update the module doc L6-8 and the struct docs ("App::build does not read").
  - Its unit tests (L295+) use a plain `struct Buy`: give it hand-written `Route`/`Payload`/`PayloadDecode` impls (or a modal `ModalForm`).
- `crates/noctis-core/src/kind.rs` L500-520 -- the `__Bind` impls bind by `<S as Route>::ID.as_str()` with the new bounds. `__Component::ROUTE` stays (the derive fallback uses `""`).
- `crates/noctis-core/src/registry.rs` -- `fold` (L72-160) adds the route map and the violations. Reuse the `ancestors` handling. Add `pub(crate) fn route(&self, id: &str) -> Option<&Arc<RouteEntry>>`; `len()` counts routes too.
- `crates/noctis-core/src/registry/tests.rs` -- unit tests for Matrix 2-9 (`Module::__add` with handcrafted descriptors).
- `crates/noctis-core/src/module.rs` -- `components()`/`modals()`/`uses()` accessors, `uses::<R>()`, `Debug`, a doctest.
- `crates/noctis-core/src/error.rs` L137+ -- the two variants (+ doctest of one message).
- `crates/noctis-core/src/app.rs` L1818 -- `Registry::fold` is already called here; it needs no change unless the signature changes.
- `crates/noctis-core/src/pipeline.rs` -- reuse `report_logged` (L431), `Outcome`, the metrics pattern of `execute` (L283-390), `panic_message` (worker). Declare `mod component;`. The module doc gains a "Components and modals" section.
- `crates/noctis-core/src/worker.rs` L127-155 -- route `MessageComponent`/`ModalSubmit` to the new pipeline (one tracked task each). Update the method doc.
- `crates/noctis-core/src/lib.rs` `__testing` (L260-345) -- add the hidden seams:
  - `RouteTable` / `route_table(&[Module]) -> Result<RouteTable, Vec<BuildViolation>>` and `RouteTable::probe(&self, custom_id: &str, kind: ComponentKind) -> Result<&'static str, &'static str>`, which returns the handler name or the `Rejection` label with no allocation;
  - `dispatch(app: &App, raw: &str) -> impl Future<Output = ()>`, which parses the twilight `Interaction` and runs the same function the worker spawns (shard 0, `Instant::now()`, a fresh token).
- `crates/noctis-codec/src/layout.rs` L654 `decode`, L421 `Decoded::payload` -- reused read-only. The `Decoded::route()` borrow is the lookup key.
- `crates/noctis-ui/src/modal/{form.rs L44, submit.rs L215/L257}` -- `ModalForm::from_submission`, `ModalSubmission::from_slice`/`custom_id` (read-only).
- `crates/noctis-ui/src/route.rs` L31-36 -- `Handled` and its CID-04 message (read-only; it bounds `uses`).
- `crates/noctis-codec/tests/alloc_decode.rs` -- the dhat pattern to copy (one `#[test]` per binary, splitmix ids, negative control).
- `crates/noctis-testkit/tests/commands.rs` L180-330 -- the `Running` harness (`FakeGateway.dispatch("INTERACTION_CREATE", d)`) to copy for components.
- Skill/template/docs paths are listed in Boundaries. `xtask/public-api.txt` and `SKILL.md` `API hash` come from the api-snapshot gate.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/{Cargo.toml, src/component.rs, src/kind.rs}` -- the codec dependency, the bounds, the typed start/probe, the `R::ID` binding; fix the unit tests.
2. `crates/noctis-core/src/{error.rs, module.rs, registry.rs, registry/tests.rs}` -- the route map, `uses`, the violations; unit tests Matrix 2-9, 11.
3. `crates/noctis-core/src/{pipeline.rs, pipeline/component.rs, worker.rs}` -- the dispatch and `Rejection`; unit tests in `pipeline/tests.rs` for Matrix 12-25, 27 via the `test_support` helpers (assert the `warn` reason with a captured subscriber and no recorded request).
4. `crates/noctis-core/src/lib.rs` -- the `__testing` seams.
5. `crates/noctis/Cargo.toml` -- add `dhat.workspace = true` to `[dev-dependencies]`.
6. `crates/noctis/tests/alloc_dispatch.rs` (new; one `#[test]`) -- Matrix 28-29. It uses the derived `Buy` (fixed-width `u64`), the `Pick` select, the extra handlers built with `ComponentHandler::__new::<Buy, _, _>(.., leaked id, ..)`, and `route_table` + `probe`. Measure from `dhat::HeapStats` around the probe loops only.
7. `crates/noctis/tests/memory_flat.rs` (new; one `#[test]`, `#[tokio::main(flavor = "current_thread")]`-style runtime built inside the test) -- Matrix 30 through `__testing::dispatch` on a built App.
8. `crates/noctis/tests/types/uses_not_a_route.rs` (+ blessed `.stderr`) -- Matrix 10. Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui` and read the diff.
9. `crates/noctis-testkit/tests/components.rs` (new) -- end to end with `FakeGateway`/`FakeDiscordHttp`: Matrix 1, 19, 26, 31, the 16 warn, and 2 through `AppBuilder::build`; zero recorded HTTP requests for components.
10. `.github/workflows-disabled/ci.yml` -- the allocation step.
11. Docs, spine AD-19 sentence, Usage Skill, template `AGENTS.md`, then `cargo run --release -p xtask -- api-snapshot --bless` and the `API hash` line.
12. Every gate in Verification.

**Acceptance Criteria:**
- Given `Buy` with its `#[handler]`, mounted by `Module::new("shop").add::<Buy>()` on a testkit App, when `FakeGateway` dispatches a type-3 interaction (component type 2) with `custom_id` `0shop.buy:47`, then the handler runs exactly once with `self.item == 255` and `ctx.module() == "shop"`.
- Given `Buy` added in two mounted Modules, when `AppBuilder::build` runs, then it returns a `BuildError` whose violations contain exactly one `DuplicateRoute` that names `shop.buy`, both Modules and both handler names, and whose `Display` starts with `[CID-04]`.
- Given `Module::new("dash").uses::<Buy>()` mounted while no mounted Module adds `Buy`, when `build` runs, then it fails with one `UnmountedRoute` naming `dash` and `shop.buy`. After `Buy` is added to any mounted Module, `build` succeeds.
- Given 10 or 10 000 registered Routes, when `cargo test --release -p noctis --test alloc_dispatch` runs, then the measured region (lookup by borrowed key plus fixed-width decode, valid and rejected ids) allocates 0 blocks and 0 bytes, and the negative control allocates.
- Given N same-Route clicks, when `memory_flat` and the testkit task test run, then framework memory and alive tasks do not grow with N.
- Given the docs, the skill and the template, when `xtask rules-catalog`, `skill-check` and `api-snapshot` run, then all pass. No "[planned] dispatcher" text remains in `routes-payloads-refs.md`.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why `Module::uses::<R>()` (decision).**
  - With one struct per interaction, `.build()` already proves at compile time that a handler exists (6.11). What stays dynamic is whether that struct's Module is mounted in this App.
  - Views are plain functions (maintainer decisions §1), so no descriptor can see which Routes a handler renders. Scanning bodies misses `render_x(..)` and breaks on non-Route `build()` types. Link-time registries (`inventory`) need a new crate and still cannot tell which App renders the Route.
  - An explicit, opt-in declaration is the only sound "referenced by a mounted Module's descriptors" (AD-19). Modules that render only their own Routes need nothing.
  - The skill teaches `uses` for Views that render another Module's Routes.
- **Allocation proof.**
  - The probe is a `fn` pointer instantiated per struct. It shares `decode_payload::<S>` with the real start, so the dhat test measures the same decode code.
  - The per-interaction `Arc<CtxInner>` and the one boxed handler future are the allowed budget (AD-25).
  - The `HashMap<&'static str, _>::get(&str)` with std's hasher allocates nothing.
- **Rejected ids.** Stale, Ref and forged ids are only logged here, so that 7.2 (notices, keys), 7.3 (stale/expired) and 7.5 (Refs) each replace one arm of `Rejection` without reshaping the pipeline.
- **Golden test shape:**
  ```rust
  let table = route_table(&[module]).unwrap();
  let before = dhat::HeapStats::get();
  for id in &ids { black_box(table.probe(black_box(id.as_str()), ComponentKind::Button)); }
  let after = dhat::HeapStats::get();
  assert_eq!((after.total_blocks - before.total_blocks, after.total_bytes - before.total_bytes), (0, 0));
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild, `alloc_dispatch`, `memory_flat` included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok (API hash matches)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rnE "does not read (these|it)|until component dispatch|\*\*\[planned\]\*\* dispatcher" crates/noctis-core/src crates/noctis/src examples/project-template/.claude docs` -- prints nothing
