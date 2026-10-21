---
title: 'Story 7.5: Ref minting and resolution'
type: 'feature'
created: '2026-10-21'
status: 'in-progress'
baseline_revision: '4a19d24eb8a6afccd26daafa428db5106b1d4fff'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `by_ref` and spilled Routes cannot be sent: the Responder answers every `EncodedId::RefSlot` with `ResponderError::Encode` ("cannot mint yet"), and dispatch treats every Ref id as missing (`Rejection::RefUnresolved`). `ttl`/`one_shot` are rejected by the derives, there is no App default TTL, no `RefOps`, and no `noctis_ref_lookups_total` (FR-17, FR-18, NFR-5, NFR-6; AD-6, AD-18, AD-23; CID-05).

**Approach:**
- The derives accept `ttl = "…"` and `one_shot` on by-Ref Routes and record them in `RouteRuntime`.
- The Responder mints and stores every slot (`put_if_absent`, retried on collision; identical slots share one Ref) before validation, then calls `codec::finish`.
- Dispatch stage 2 resolves a Ref id with `get` (`take` for `one_shot`) through the App's namespaced `RefStore`: missing goes to the Expired chain, a route mismatch is forged, a schema mismatch is stale, each counted.
- A returned `(message, RefOps)` deletes Refs after a successful send.

## Boundaries & Constraints

**Always:**
- **Macro grammar** (`crates/noctis-macros/src/route.rs`, all seven kind attributes incl. `#[modal]`):
  - `ttl = "<dur>"`: a string literal (`"15m"`, `r"15m"`). `<dur>` = one or more `<digits><unit>` groups, units `d`, `h`, `m`, `s`, each at most once, in that order, no spaces, signs, decimals or upper case. The total is 1 s..=365 d (31 536 000 s), computed with checked arithmetic.
  - `one_shot` is a flag.
  - Both need `by_ref` or `overflow = spill`.
  - Every error is `[CID-03]`, in `syn::Error` style with a `help`, pointing at the value (or name). The errors (exact message starts):
    - not a string: ``[CID-03] `ttl` takes a duration string``, help `write ttl = "15m" (units d, h, m, s, such as "1h30m")`;
    - bad syntax: ``[CID-03] `ttl = "<lit>"` is not a duration``, with the same help;
    - out of range: ``[CID-03] `ttl` must be between 1s and 365d, found `<lit>` ``;
    - inline Route: ``[CID-03] `ttl` needs a by-Ref Route`` / ``[CID-03] `one_shot` needs a by-Ref Route``, help `add by_ref or overflow = spill: only a Ref has a TTL or is consumed on use`;
    - ``[CID-03] `one_shot` is a flag and takes no value``;
    - the existing "given twice" error.
  - The parser is a crate-private fn in the new `crates/noctis-macros/src/duration.rs`, returning seconds. Story 13.4's `per = "…"` will reuse it.
  - `owner`, `permissions`, `guard` and `rate_limit` stay "not supported yet" (`RUNTIME_ARGS` shrinks to these four). `response`/`auto_defer` on `#[modal]` stay rejected until 7.8.
- **`RouteRuntime`** (`crates/noctis-codec/src/derive.rs`, stays `#[doc(hidden)] #[non_exhaustive]`):
  - It gets a private `ref_policy: RefPolicy` field.
  - Methods: `pub const fn with_ttl_secs(self, secs: u64) -> Self`, `pub const fn one_shot(self) -> Self` and `pub const fn ref_policy(&self) -> RefPolicy`.
  - The derive emits `RouteRuntime::EMPTY[.with_ttl_secs(N)][.one_shot()]`.
  - `DeclaredRuntime::RUNTIME` gets the default `RouteRuntime::EMPTY`.
- **`noctis_ui::Handled: Route + DeclaredRuntime`**. Every hand-written `impl Handled` adds `impl DeclaredRuntime for X {}`: the ui tests' `route!` macros, `noctis-core/src/test_support.rs`, and the doctests in `ui/component/select.rs`, `ui/modal/mod.rs` and `core/module.rs`.
  - `CustomId::new::<R: Handled>` captures `R::RUNTIME.ref_policy()`.
  - `ErasedRoute::encode` adds `Parts::with_ref_policy(policy)` only when `R::SHAPE.body() != BodyKind::Inline`.
  - `PendingRef::ttl()`/`is_one_shot()` then carry the Route's values.
- **Minting** (`crates/noctis-core/src/responder.rs`): `resolve_ids` and `prepare` become `async`. For every unresolved id whose `resolve_for_send` gives `Some(slot)`:
  - Equal slots within one `prepare` call reuse one Ref (`PendingRef: Eq + Hash`).
  - For each distinct slot, up to 3 attempts: `Ref::mint()`, then `refs.put_if_absent(r.as_str(), slot.record(), Some(slot.ttl().unwrap_or(app.ref_ttl)))`. `true` → `id.finish(&slot, r)`.
  - After 3 `false` answers: `ResponderError::RefCollision`.
  - A `PortError`: `ResponderError::RefStore(PortError)`.
  - A mint failure: `Encode { reason }` naming the Route.
  - In every error case nothing is sent. Refs already stored stay, and their TTL reaps them.
  - Minting runs before `noctis_ui::validate`, on every send path (`finish`, `reply`, `update`, `edit_reply`, `follow_up`, `notice`, `show_modal`), on `Transport::Interaction` and `Transport::Message`, with the App's store.
  - `Transport::Detached` mints and finishes without storing, because nothing is sent.
  - An already-resolved id is never re-minted.
  - New `#[non_exhaustive]` variants:
    - `RefStore(#[source] PortError)`, message `"the Ref could not be stored: {0}"`;
    - `RefCollision`, message `"could not mint a unique Ref after 3 attempts"`.
- **App** (`crates/noctis-core/src/app.rs`, `error.rs`, `handler.rs`):
  - `AppBuilder::ref_ttl(&mut self, ttl: Duration) -> &mut Self`: default 24 h, the last call wins, with a doctest. Outside 1 s..=365 d it is `BuildViolation::InvalidRefTtl { actual: Duration }`, message `"ref TTL {actual:?} is out of range: use a value in 1 s..=365 d"`, listed with every other violation.
  - `App::ref_ttl(&self) -> Duration`.
  - `AppShared` gains `refs: Arc<dyn RefStore>` (the resolved namespaced `ref_store`) and `ref_ttl: Duration`.
  - There is no environment variable.
- **Resolution** (`crates/noctis-core/src/{component.rs, pipeline/component.rs}`):
  - The typed start of `ComponentHandler` and `ModalHandler` returns `Result<Begin, Rejection>` with `enum Begin { Ready(HandlerFuture), Ref(Ref) }` in place of `Err(RefUnresolved)`.
  - A second erased fn decodes `decode_record::<S>(&bytes)` (a modal then does `S::from_payload`) and starts the handler.
  - The handlers gain `one_shot: bool` from `S::RUNTIME`, which adds a `DeclaredRuntime` bound to both constructors.
  - Before stage 5, the pipeline calls `app.refs.take(r.as_str())` if `one_shot`, else `get`. The outcomes:

    | Lookup result | Then | `result` label |
    | --- | --- | --- |
    | `None` | `Rejection::RefUnresolved` (the 7.3 chain) | `miss` |
    | record of another route | `Rejection::Forged` (warn + generic notice) | `forged` |
    | record that is not a `RefRecord` | `Rejection::Forged` (warn + generic notice) | `forged` |
    | another schema | `Rejection::Stale` (the stale chain: `#[on_stale]` → `#[on_expired]` with `Expired::Stale` → App → default) | `stale` |
    | body that does not decode | `Rejection::Undecodable` | `hit` |
    | modal inputs invalid | `SubmitInvalid` | `hit` |
    | decodes | the handler runs | `hit` |
    | `PortError` | new `Rejection::RefStore` (`as_str` = `"ref_store"`: one warn + generic notice) | not counted |

  - One `metrics::counter!("noctis_ref_lookups_total", "app" => app, "result" => …)` per completed lookup.
  - The Ref never appears in a log, span field or label. Logs keep `route` only.
- **`ctx.current_ref() -> Option<Ref>`**: the Ref the clicked or submitted id carried, read from `decoded.body()`. It is added to `ComponentCtx`, `ModalSubmitCtx` and `ExpiredCtx` (via `ctx_accessors!`, `CtxInner.current_ref`). It is `None` for inline ids.
- **`RefOps`** (new `crates/noctis-core/src/refs.rs`, re-exported as `noctis_core::RefOps`, `noctis::RefOps` and in the prelude):
  - `#[derive(Debug, Clone, Default, PartialEq, Eq)]` with `new()`, `#[must_use] delete(self, r: Ref) -> Self` (duplicates ignored), `delete_all(self, impl IntoIterator<Item = Ref>) -> Self`, `refs(&self) -> &[Ref]`, `len` and `is_empty`.
  - `IntoResponse<Component>` and `IntoResponse<ModalSubmit>` are implemented for `(V2Message, RefOps)`, `(Components, RefOps)` and `(Responded, RefOps)`.
  - The pipeline deletes each Ref (`refs.delete`, in order) only after `finish` succeeded, or right away for `Responded` when the handler returned `Ok`.
  - A failed send, an error or a panic deletes nothing.
  - A delete error is one `warn` on `noctis::refs` (no Ref value) and does not fail the interaction.
- **Facade:** `noctis::codec` re-exports `Ref` and `RefError`. `noctis::observe::METRICS` gets the row `noctis_ref_lookups_total` (counter, `["app","result"]`, "Ref lookups by result: hit, miss, stale or forged.") plus a table line.
- **Docs in the same commit:**
  - Skill:
    - `routes-payloads-refs.md`: drop `[planned]` on `ttl`/`one_shot`, document the grammar, `.ref_ttl` and §7 (minting, resolution table, `current_ref`, `RefOps` example, the one-shot caveat).
    - `responder.md`: L44 and L420, plus the new errors.
    - `app-and-sharding.md`: move `.ref_ttl` to the working table, add the metric row, and remove "Ref lookups [planned]".
    - `components-v2.md` L145, `modals.md` L232-233, `troubleshooting.md` (L26 list plus the new `ttl` errors) and `SKILL.md` (API hash).
  - Template:
    - `FinishSetup` becomes `by_ref, one_shot, ttl = "15m"`, with the width line `setup.done: F 1 + route 10 + sep 1 + S 0 + N 0 + ref 12 = 24 (limit 100)`.
    - Fix the `handlers.rs` L68 comment.
    - `AGENTS.md` L54: `ttl`/`one_shot` work; `owner`, `permissions`, `guard` and `rate_limit` stay planned.
  - `docs/discord-rules.md`:
    - CID-05 row: name the new tests.
    - CID-06 row: drop "no RefStore before Story 7.5" and add `ref_store`.
    - L421 paragraph.
  - Facade rustdoc L1203.
  - `pipeline.rs` L8/L579: a Command has no Ref.
  - Spine AD-6/AD-18: `Amended (2026-10-21)` lines for the decisions below.
  - Then re-bless `xtask/public-api.txt`.

**Never:**
- Guards, `owner`/`permissions` (7.7), Middleware (7.6), modal `response`/`auto_defer` (7.8) or `rate_limit` (13.4).
- `touch`/sliding TTL, Ref refresh on click, or a runtime `overflow = error`.
- Logging Ref values.
- A store handle in any crate below core.
- New external crates, or a debug build.
- Changing the `custom_id`/`RefRecord` formats or the `RefStore` trait.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | ttl ok | `"1s"`, `"90s"`, `"15m"`, `"1h30m"`, `"2d"`, `"1d2h3m4s"`, `"365d"`, `"0d1s"`, `r"15m"` | secs 1, 90, 900, 5400, 172800, 93784, 31536000, 1, 900 | — |
| 2 | ttl range | `"0s"`, `"0m"`, `"366d"`, `"365d1s"`, `"99999999999999999999d"` | out-of-range error (the last: overflow, same error) | compile CID-03 |
| 3 | ttl syntax | `""`, `"15"`, `"m"`, `"15 m"`, `" 15m"`, `"-1m"`, `"1.5h"`, `"15M"`, `"15x"`, `"1m1h"`, `"1m1m"`, `"15m "`, `"１５m"` (full-width) | "is not a duration" | compile CID-03 |
| 4 | ttl type | `ttl = 15`, `ttl`, `ttl = b"1m"`, `ttl = "1m" "2m"` | "takes a duration string" | compile CID-03 |
| 5 | inline | `#[button("a", ttl = "1m", ..)]`; `#[button("a", one_shot, ..)]` | "needs a by-Ref Route" (trybuild) | compile CID-03 |
| 6 | flags | `one_shot = true`; `one_shot, one_shot`; `ttl = "1m", ttl = "2m"` | flag/given-twice errors | compile CID-03 |
| 7 | accepted | `by_ref, ttl = "15m", one_shot`; `overflow = spill, one_shot`; `#[modal("m", by_ref, ttl = "1h", ..)]` | `RUNTIME.ref_policy()` = (900 s, true); (None, true); (3600 s, false) | — |
| 8 | still planned | `owner = x`, `permissions = X`, `guard = f`, `rate_limit(..)` | the existing "does not support yet" error | compile CID-03 |
| 9 | mint | reply with one `by_ref` button (`Stored{n:1}`), memory store, App `default` | id `0t.stored:~<11 base62>`; store holds `default:ref:<ref>` = `0t.stored:1` with TTL 24 h; one `put_if_absent` | — |
| 10 | route TTL | Route `ttl = "15m"`; `.ref_ttl(1 h)` on another Route | stored TTL 900 s; 3600 s | — |
| 11 | share | two equal `Stored{n:1}` buttons + one `Stored{n:2}` in one message | 2 distinct Refs, the equal pair identical; 2 `put_if_absent` | — |
| 12 | per send | the same message sent twice (two replies) | different Refs | — |
| 13 | collision | store answers `false` once, then `true` | 2 calls, the second Ref is used | — |
| 14 | exhausted | store always `false` | `RefCollision`, 3 calls, 0 HTTP requests, state Pending | typed |
| 15 | store down | `put_if_absent` gives `Backend` | `RefStore(Backend)`, 0 requests | typed |
| 16 | spill | spill Route value fits / does not | inline id, 0 store calls / Ref id, 1 call | — |
| 17 | select+ref | `by_ref` string select with schema 2 | `F` has S and N flags, `<nonce>~<ref>`, ≤ 41 chars | — |
| 18 | modal | `show_modal` of a `by_ref` modal | modal `custom_id` is a Ref id, stored before the type-9 callback | — |
| 19 | detached | `__testing` detached seam with a by-Ref button | Ok, id finished, no store | — |
| 20 | resolve hit | click the id from row 9 | handler gets `Stored{n:1}`; `current_ref()` is that Ref; metric `hit` = 1; record kept | — |
| 21 | one_shot | click a one_shot id twice; 2 concurrent clicks | 1st handler, 2nd → `#[on_expired]` `RefMissing` (or default notice); exactly one `hit` and one `miss` | expired chain |
| 22 | miss | unknown Ref `0rh:~0123456789A` | 7.3 behaviour unchanged; metric `miss` | expired chain |
| 23 | TTL expired | Route `ttl = "1s"`, paused clock +2 s | `miss` | expired chain |
| 24 | other App | App `a` mints, App `b` (same `Arc<MemoryRefStore>`) receives the id | `miss` (key `b:ref:<r>` absent) | expired chain |
| 25 | forged route | record `0other:1` stored under the clicked Ref | warn `reason="forged"`, generic notice, metric `forged`, no handler | CID-06 |
| 26 | garbage record | record bytes `[0xff]` or `""` | as row 25 | CID-06 |
| 27 | stale record | record `1t.stored:1` (schema 1) for a schema-0 Route | `#[on_stale]` → … chain with `Expired::Stale`; metric `stale` | expired chain |
| 28 | bad body | record `0t.stored:zzzz` | warn `undecodable` + notice; metric `hit` | CID-06 |
| 29 | lookup error | `get` gives `Backend` | warn `reason="ref_store"` + generic notice; no metric | CID-06 |
| 30 | modal submit | submit of a by-Ref modal (record ok / missing) | handler gets Payload + inputs / `#[on_expired]` | — |
| 31 | RefOps | handler returns `(msg, RefOps::new().delete(ctx.current_ref().unwrap()))` | msg sent, then `delete` → record gone | — |
| 32 | RefOps fail send | same, the fake answers 500 | no delete | — |
| 33 | RefOps error | handler returns `Err` / panics | no delete | — |
| 34 | RefOps Responded | `(r.update(m).await?.done(), ops)` | deletes after the handler returns | — |
| 35 | RefOps dup/empty | `delete(a).delete(a)`; `RefOps::new()` | `len()` 1, one delete; no store call | — |
| 36 | delete error | `delete` gives `Backend` | one warn on `noctis::refs`, interaction OK | log |
| 37 | ref_ttl build | `.ref_ttl(0 s)`, `999 ms`, `1 s`, `365 d`, `365 d + 1 s` with `.ack_deadline(3 s)` | `InvalidRefTtl` for 0, 999 ms and 365 d + 1 s (listed with `InvalidAckDeadline`); ok otherwise; `app.ref_ttl()` returns it | build |
| 38 | no Ref leak | rows 25-29 with log capture | no captured line contains the 11 Ref characters | — |

</intent-contract>

## Code Map

- `crates/noctis-macros/src/route.rs`:
  - L54-62 `RUNTIME_ARGS` (drop `ttl`, `one_shot`).
  - L183 `Seen` and L326-385 `parse_arg` (+ the `ttl`/`one_shot` arms).
  - L198-248 `parse_list`: the inline check sits next to the `by_ref`+`signed` check.
  - L37 `RouteArgs` (+ `ttl_secs: Option<u64>`, `one_shot: bool`).
  - L563-610 `route_impls`: emit `RUNTIME`.
  - Tests: the `REJECTED` table L640+ (drop the `ttl`/`one_shot` rows and add the rows 2-6 cases); `args()` L629.
- New `crates/noctis-macros/src/duration.rs`: the parser plus `#[cfg(test)]` rows 1-3.
- `crates/noctis-codec/src/derive.rs` L326-343 (`RouteRuntime`, `DeclaredRuntime`); `layout.rs` L43-102 (`RefPolicy`, `Parts::with_ref_policy` L143); `encoded.rs` (`PendingRef`, `finish`); `record.rs` L150-211 (`RefRecord::decode`, `decode_record`); `refs.rs` (`Ref::mint`, `as_str`).
- `crates/noctis-ui/src/route.rs`: L31 `Handled`; L254-280 `ErasedRoute`/`Erased`; L299 `CustomId::new`; L345-370 `resolve_for_send`; L381 `finish`.
- `crates/noctis-core/src/responder.rs`:
  - L1263 `signer()`: add a sibling `ref_target() -> Option<(&Arc<dyn RefStore>, Duration)>`.
  - L1276-1335 `resolve_ids`/`prepare`.
  - The `prepare` callers at L1371, 1401, 1517, 1615, 1637, 1715 and 1777.
  - L326 `ResponderError`.
  - L476-545 `IntoResponse`/`ResponseValue`/`Response`.
- `crates/noctis-core/src/pipeline.rs` L343-400 `run_started` (send, then `RefOps`); `command.rs` L629 and `component.rs` L900 (`Response` matches); `command.rs` L84 `CtxInner` (+ `current_ref`).
- `crates/noctis-core/src/component.rs`:
  - L240-275: `ModalStart`, `decode_payload`, `probe` (the probe stays allocation-free and inline-only).
  - L473-560: `ComponentHandler` and its start.
  - L692-780: `ModalHandler` and its start.
  - `ctx_accessors!` (L61).
- `crates/noctis-core/src/pipeline/component.rs`:
  - L83-140: `Rejection` (+ `RefStore`; update the `RefUnresolved` docs).
  - L560-600: stage 4 (insert the lookup, the metric and `current_ref`).
  - Module docs L1-45.
- `crates/noctis-core/src/{app.rs, error.rs, handler.rs, ports.rs, observe.rs, lib.rs}`:
  - The `ack_deadline` pattern (L1520, check L2030, getter L282).
  - The `AppShared` build at L2073.
  - `BuildViolation` near L223.
- Tests to convert:
  - `responder/tests/components.rs` L643 `ref_slot_is_an_encode_error`: now mints (detached / store).
  - `crates/noctis/tests/commands.rs` L391-477 `Stored`/`Unresolved`: a Ref Route now resolves.
  - `crates/noctis-testkit/tests/components.rs` L1293: keep it, and assert metric `miss`.
  - The trybuild `crates/noctis/tests/ui/cid/CID-03_runtime_argument.rs`: switch it to `owner = user` and re-bless.
- The Running/FakeGateway pattern for new wire tests: `crates/noctis-testkit/tests/components.rs`, `namespacing.rs`.
- `crates/noctis/src/lib.rs`: `codec` re-exports L1670, the root re-exports L563, the prelude L1856, the button docs L1182-1210.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-codec/src/derive.rs`: the `RouteRuntime` fields, methods and default const, with unit tests.
2. `crates/noctis-macros/src/{duration.rs, route.rs, lib.rs}`: grammar, emission and unit tests (rows 1-8).
3. `crates/noctis-ui/src/route.rs` plus every hand-written `Handled` impl: the supertrait and policy capture. Test that a `by_ref, ttl, one_shot` derive's `CustomId` slot has the policy (row 7).
4. `crates/noctis-core/src/{error.rs, app.rs, handler.rs}`: `ref_ttl`, `InvalidRefTtl`, `AppShared.{refs, ref_ttl}`. Tests for row 37.
5. `crates/noctis-core/src/responder.rs` (+ `responder/tests/refs.rs`): async minting, the new errors and the detached path. Rows 9-19, using a counting/scripted `RefStore` and the fake HTTP.
6. `crates/noctis-core/src/refs.rs`, the `responder.rs` `IntoResponse` tuples, `pipeline.rs`: `RefOps` and the post-send deletion. Rows 31-36.
7. `crates/noctis-core/src/{component.rs, pipeline/component.rs, command.rs, observe.rs}`: `Begin`, the record start, the lookup, `current_ref`, the metric, `Rejection::RefStore`. Unit tests in `pipeline/tests/refs.rs`: rows 20-30 and 38, named `cid_05_*` where they test record binding (`cid_05_record_route_mismatch_is_forged_at_dispatch`, `cid_05_refs_are_app_namespaced`), with a `DebuggingRecorder` for the metric.
8. `crates/noctis/tests/ui/cid/` trybuild fixtures (bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`, then read the `.stderr`):
   - `CID-03_ttl_on_inline_route.rs`
   - `CID-03_one_shot_on_inline_route.rs`
   - `CID-03_ttl_not_a_duration.rs` (`"15x"`)
   - `CID-03_ttl_out_of_range.rs` (`"366d"`)
   - the updated `CID-03_runtime_argument.rs`
9. `crates/noctis-testkit/tests/refs.rs` (real wire, `FakeGateway` + `FakeDiscordHttp`, shared `Arc<MemoryRefStore>` passed to `.ref_store(..)`):
   - `confirm_flow_round_trip`: a slash command replies with a `by_ref, one_shot, ttl = "15m"` button. The test clicks it (the handler runs and returns `(msg, RefOps)` deleting nothing extra) and clicks it again (default expired notice).
   - `two_apps_cannot_resolve_each_others_refs` (row 24).
   - `ref_ops_delete_after_send` (rows 31-32).
10. `crates/noctis/src/lib.rs`, the docs, skill, template and spine listed in Boundaries; then `cargo run --release -p xtask -- api-snapshot --bless` and update the `SKILL.md` API hash.
11. Memlog `(decision)` lines (already added at planning). Run every gate in Verification.

**Acceptance Criteria:**
- Given a handler that returns a message containing `by_ref` components, when it is sent through the fake Discord HTTP, then every such `custom_id` is `F route ":" [S][N] "~" <11 base62>`, each distinct slot is stored once under `<app>:ref:<ref>` with its record and TTL before the callback, and identical slots share one Ref.
- Given that id clicked through `FakeGateway`, when the Route is reusable, then the handler receives the original Payload and the record remains; when it is `one_shot`, then the second click reaches the Expired chain and the store no longer holds the key.
- Given `ttl` or `one_shot` on a Route without `by_ref`/`overflow = spill`, when it compiles, then it fails with `[CID-03] … needs a by-Ref Route`.
- Given clicks yielding hit, miss, stale and forged lookups, when a `DebuggingRecorder` is installed, then `noctis_ref_lookups_total{app, result}` counts one of each.
- Given a handler returning `(msg, RefOps)`, when the send succeeds, then core deletes the listed Refs afterwards, and when it fails, then it deletes none.
- Given the Usage Skill, when `skill-check` runs, then it passes, `ttl`/`one_shot`/`.ref_ttl`/`RefOps`/`current_ref` carry no `[planned]`, and the API hash matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **`Handled: DeclaredRuntime` (decision).** The policy must reach both the encoder (`noctis-ui`'s erased `CustomId`, which knows `R` only at `.build()`) and dispatch (get vs take). A supertrait on `Handled` is the narrowest bound every buildable Route already satisfies through the derives. The default `RUNTIME` keeps hand-written impls to one line. `Route` (AD-19) is untouched.
- **TTL grammar (decision).** `d/h/m/s` in descending order, 1 s..=365 d. This matches the skill's existing `"15m"`/`"2d"` spellings, and the upper bound catches typos. No crate is added. The App default stays 24 h, with the same range.
- **Mint before validation (AD-18).** Refs stored for a message that then fails validation or sending are orphans until their TTL. That is accepted: they are unguessable and bounded by the TTL, and AD-18 fixes the order.
- **Lookup labels (decision).** `hit` means a record bound to this Route and schema was found, even if its body later fails to decode (`undecodable`). An unparsable record counts as `forged`, because only Noctis writes records. Store errors are not lookups, so they are not counted, and they answer with the generic notice (`ref_store`).
- **`RefOps` shape (decision).** A tuple response keeps pure render code pure (`Ok((render(..), RefOps::new().delete(r)))`). `ctx.current_ref()` is the only way pure code learns a Ref, which is exactly the step-flow cancel case.
- **Warning for Story 7.7:** `one_shot` is consumed at stage 2, before the Guards of stage 8, so a click rejected by a Guard still consumes the Ref. 7.7 must decide whether to keep this (AD-23 order) or document it.
- Golden sketch (stage 2):
  ```rust
  let bytes = if entry_one_shot { app.refs.take(r.as_str()).await } else { app.refs.get(r.as_str()).await };
  match bytes { Err(_) => reject(RefStore), Ok(None) => count("miss"), Ok(Some(b)) => start_record(&b) /* labels per table */ }
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok (no store handle below core)
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rn "Story 7.5\|cannot mint yet" crates examples/project-template docs --include=*.rs --include=*.md` -- prints only intentional history references (none in skill or template)
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
