---
title: 'Story 7.8: Modal Routes and typed submissions end to end'
type: 'feature'
created: '2026-10-21'
status: 'in-progress'
baseline_revision: '2f36aa76d9e8a9bff8a41a87013bcd8aa93c1035'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modals.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The modal-submit pipeline already dispatches a `#[derive(Modal)]` struct (Stories 6.11, 7.1-7.7): it decodes the inputs from the raw interaction bytes and runs `#[handler]`. Two pieces are still missing.
- `#[modal(..)]` rejects `response` and `auto_defer` (``[CID-03] `response` is a runtime argument of #[modal] that this Noctis release does not support yet``). So a submit is always `response = update` and auto-deferred, and `ModalResponder<Manual>` cannot be declared.
- No end-to-end test runs the full flow: a button opens the modal, the user submits it, and the typed handler answers, with every FR-64 input kind (Radio Group 21, Checkbox Group 22, Checkbox 23, File Upload 19, selects) going through `FakeGateway`.

The Usage Skill and the template still mark this flow `[planned, Story 7.8]` (FR-21, FR-29, FR-64; AD-11, AD-12; INT-04, INT-05, MOD-08, MOD-09).

**Approach:**
- `#[modal]` accepts `response = update | ephemeral | public` (default `update`) and `auto_defer = true | false` (default `true`), with the same grammar as the component attributes.
- The derive emits `__Modal::RESPONSE` and `__Kind::Defer = Manual` for `auto_defer = false`. `ModalHandler` carries the declared response into the pipeline, and the pipeline's existing auto-defer and finish logic apply it.
- A modal submit's `Shared` refuses `show_modal` at run time (`ModalUnavailable`, no request).
- New tests: wire tests, trybuild tests, and a facade copy of the template's bio flow. The skill, the template, the docs and the API snapshot drop the planned markers.

## Boundaries & Constraints

**Always:**
- **Grammar** (`crates/noctis-macros/src/route.rs`):
  - `unsupported_runtime_arg` loses its `modal_response` branch, so only `RUNTIME_ARGS` (`rate_limit`) is still rejected.
  - `response` and `auto_defer` on `#[modal]` use the existing `parse_response`/`parse_auto_defer`. The value errors, the "given twice" error and the spans stay as they are.
  - The `parse_auto_defer` help names the kind's responder: `ModalResponder<Manual>` on `#[modal]`, `ComponentResponder<Manual>` otherwise.
  - Delete the `RESPONSE_ARGS` const if nothing else uses it.
- **Derive** (`crates/noctis-macros/src/modal.rs`):
  - `impl __Modal` gains `const RESPONSE: __Response = __Response::{Update|Ephemeral|Public};`, and the fallback impl gets `Update`.
  - `crate::kind::scaffolding(name, Family::Modal, !attr.args.auto_defer)` replaces the hard-coded `false`.
  - The modal's `#[on_stale]`/`#[on_expired]` hooks inherit `Defer`, as component hooks already do (no new code expected; `hook_binds!` is generic).
- **Core:**
  - `__Modal` (`kind.rs`) gains `const RESPONSE: __Response;`.
  - `ModalHandler::__new_with_responder` gains a `response: __Response` parameter after `route`, mapped with the existing `component.rs::declared(..)`. The two `__Bind` impls in `kind.rs` pass `S::RESPONSE`; `ModalHandler::__new` passes `__Response::Update`; the pipeline tests pass `__Response::Update`.
  - Update the `ModalHandler` docs ("Its declared response is `update`") and the `response()` doc.
- **Auto-defer and answer:**
  - No new pipeline code. `Shared::auto_defer` and `Shared::finish` already branch on `(Declared, source)`:
    - `Update` with a source message: type 6 / type 7;
    - `Update` without one: type 5 public / type 4 public;
    - `Reply(e)`: type 5 / type 4 with `e`.
  - `auto_defer = false` arms no defer. The existing 2.9 s error names the handler.
- **Dynamic `show_modal` on a submit (INT-04/INT-05):**
  - `Shared` gains a crate-private `no_modal: bool`, `false` by default, set by a new crate-private constructor `Shared::declared_submit(..)` (same parameters as `declared`).
  - In the component pipeline, `shared_for` uses that constructor when the input is `Input::Modal`. This covers the handler state, the hooks and the notice state.
  - `Shared::show_modal` returns `ResponderError::ModalUnavailable` before resolving ids, validating or locking when `no_modal` is set, and sends no request.
- **Decoding stays as is:** `read_input` passes `raw.as_bytes()` (the gateway `RawJson`) to `ModalSubmission::from_slice` (Story 4.6). An invalid submission is still `Rejection::SubmitInvalid` (warn plus the generic CID-06 notice, never a panic; MOD-09). The e2e tests pin this.
- **Docs, in the same commit:**
  - `docs/discord-rules.md`: the INT-05 row tail ("`auto_defer` on `#[modal]` arrives with Story 7.8" becomes implemented, with the fixture names), the INT-04 row (add `INT-04_modal_manual_show_modal` and the `ModalUnavailable` runtime guard, unit test `submit_refuses_show_modal`), the CID-03 row L176 and the paragraph at L420-430 (only `rate_limit` is still rejected).
  - Spine: an `Amended (2026-10-21)` line on AD-11 and in the AD-6 runtime-argument bullet (L322-326): `#[modal]` takes `response`/`auto_defer`, carried by `__Modal::RESPONSE`/`__Kind::Defer`.
  - Skill, template, API snapshot: Tasks 7-8.

**Never:**
- A `show_modal` method on `ModalResponder`, with or without `Manual`.
- Pre-fill (Story 9.4), `.title(..)`/`.value(..)` overrides (9.3), `ctx.update`/`ctx.respond` (8.1), per-field `#[validate]`.
- Changing the codec, the `custom_id` format, the `ModalSubmission` decoder, `Rejection` labels, metrics or `noctis.*` keys.
- A new external crate, a debug build, or a required template build.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error handling |
|---|----------|---------------|----------|----------------|
| 1 | default | `#[modal("m", defaults(title = "T"))]` | `RESPONSE == Update`, `Defer = AutoDefer` | — |
| 2 | each response | `response = update` / `ephemeral` / `public` | `__Response::Update` / `Ephemeral` / `Public` | — |
| 3 | manual | `auto_defer = false` | `type Defer = …::Manual` | — |
| 4 | explicit true | `auto_defer = true, response = public` (any order, before or after `defaults`/guards/`by_ref`) | `AutoDefer`, `Public`; width line unchanged | — |
| 5 | bad response | `response = edit`, `= "update"`, bare `response`, `= Update` | error | `` [CID-03] `response` takes `update`, `ephemeral` or `public` `` |
| 6 | bad auto_defer | `auto_defer = 0`, `= "false"`, bare `auto_defer`, `= no` | error; help names `ModalResponder<Manual>` | `` [CID-03] `auto_defer` takes `true` or `false` `` |
| 7 | duplicate | `response = update, response = public`; `auto_defer = true, auto_defer = false` | error | ``[CID-03] #[modal] argument `response` is given twice`` (resp. `auto_defer`) |
| 8 | still rejected | `rate_limit(..)` on `#[modal]` | error | existing "does not support yet" text |
| 9 | INT-05 flag without Manual | `auto_defer = false`, handler takes `r: ModalResponder` | compile error | `[INT-05]` (fixture) |
| 10 | INT-05 flag without responder | `auto_defer = false`, `(self, ctx)` only | compile error | `[INT-05]` |
| 11 | INT-05 Manual without flag | default, handler takes `ModalResponder<Manual>` | compile error | `[INT-05]` |
| 12 | INT-04 no show_modal | `ModalResponder<Manual>` calls `show_modal` | E0599 | fixture |
| 13 | hook inherits | `auto_defer = false` modal with `#[on_stale] async fn s(ctx: ModalSubmitCtx, r: ModalResponder<Manual>)` | compiles; a hook taking `ModalResponder` is `[INT-05]` | — |
| 14 | runtime show_modal | `Shared::declared_submit` Pending, `show_modal(m)` | `Err(ModalUnavailable)`; zero requests; state still `PENDING` | — |
| 15 | update + source, slow | modal opened from msg 77 (`message` in the submit), handler sleeps past `ack_deadline`, returns a `V2Message` | type-6 callback, then `PATCH …/messages/@original` with `IS_COMPONENTS_V2`, without `EPHEMERAL` | — |
| 16 | update + source, fast | same, answers at once | one type-7 callback | — |
| 17 | update, slash-opened, slow | no `message` | type-5 callback, no flags; then an edit of `@original` | — |
| 18 | update, slash-opened, fast | no `message`, returns a message | type-4 public; `ModalResponder::update` gives `UpdateUnavailable` (existing test) | — |
| 19 | ephemeral, slow | `response = ephemeral`, with or without a source | type 5 `flags: 64`, then an edit | — |
| 20 | ephemeral, fast | returns a message | type 4 with `flags` including `EPHEMERAL` (32832) | — |
| 21 | public, source present | `response = public` on a component-opened modal, slow | type 5 without `EPHEMERAL` (a new message, never type 6) | — |
| 22 | manual, answers | `auto_defer = false`, `r.reply(..)` within 3 s | one type-4 callback; nothing before it | — |
| 23 | manual, silent | `auto_defer = false`, sleeps 3.2 s | no request; one error log naming the handler | — |
| 24 | manual, defers itself | `auto_defer = false`, `r.defer_reply()` then return a message | type 5, then an edit | — |
| 25 | FR-64 text/choice | submit with Text Input (4), Radio Group (21), Checkbox Group (22), Checkbox (23) values twilight drops | `self` fields: `Some("hi")`, the `Severity::High` variant, `vec![Topic::Rust, Topic::Art]`, `true` | — |
| 26 | FR-64 entities | user select plus File Upload (19) with `data.resolved.users`/`attachments` | `ResolvedUser` id 3; `ResolvedAttachment` with filename, size, url, content type | — |
| 27 | unicode | Text Input `"çğış 🦀"` (5 chars), radio value matches | decoded verbatim | — |
| 28 | empty optional | Text Input `""` on `Option<String>`, unchecked Checkbox `false`, empty Checkbox Group on an optional `Vec` | `None`, `false`, `vec![]` | — |
| 29 | invalid value | unknown radio value / missing required input / a File Upload id missing from `resolved` | no handler run; one `warn` `reason="submit_invalid"`; type-4 ephemeral generic notice | typed `SubmitError`, no panic |
| 30 | max+1 | Checkbox Group with 3 values where `max_values = 2` | as row 29 | — |
| 31 | malformed | `data.components` not an array | `reason="malformed"` warn and generic notice | — |
| 32 | full flow | click `auto_defer = false` button → type 9 with `data.custom_id` == the modal's encoded id and the declared components → submit that id from msg 77 | handler gets Payload plus values; returning a View updates msg 77 (type 7) | — |
| 33 | slash flow | `#[command(.., auto_defer = false)]` `show_modal(BioForm::blank(..))` → submit without `message` | type 9; then a public type-4 reply | — |
| 34 | guard still runs | the row-32 modal with `owner = user`, submitted by another user | `noctis.guard.owner` notice (7.7 unchanged) under every `response` | — |

</intent-contract>

## Code Map

- `crates/noctis-macros/src/route.rs`:
  - L26-53 `Response` and `RouteArgs.response`/`auto_defer`;
  - L118 `RESPONSE_ARGS`, L505-517 `unsupported_runtime_arg` (drop the modal branch);
  - L519 `parse_response`, L591 `parse_auto_defer` (the help text, L609);
  - tests from L700 (add the modal cases; the existing components cases are at L999-1010).
- `crates/noctis-macros/src/modal.rs`:
  - L925-965 `expand_or_fallback` (the fallback `__Modal`);
  - L966 `expand`: L993 `scaffolding(.., false)`, L1029 `impl __Modal`;
  - test L1156 `modal_response_args_not_supported_yet` becomes `modal_response_args` (rows 1-4 and 7).
- `crates/noctis-macros/src/component.rs` L128-155: the reference emission of `RESPONSE`/`Defer` to mirror.
- `crates/noctis-macros/src/kind.rs` L190 `scaffolding`, L215 `items`, which emits `type Defer`.
- `crates/noctis-core/src/kind.rs`:
  - L436-462 `__Response`, `__Component`, `__Modal`;
  - L566-607 the modal `__Bind` impls;
  - L747 `hook_binds!`, L907-923 `__assert_responder` (kind-generic INT-05, read-only).
- `crates/noctis-core/src/component.rs`:
  - L411 `declared()`;
  - L855-960 `ModalHandler` (`__new`, `__new_with_responder`, `response: Declared::Update` at L954);
  - L1032 the `response()` doc.
- `crates/noctis-core/src/responder.rs`:
  - L691 `ModalResponder`, L904-945 `show_modal!` (Command and Component only; unchanged);
  - L1255-1301 `Shared::new`/`declared` (add `no_modal` and `declared_submit`);
  - L1478 `finish`, L1701 `auto_defer` (read-only), L1722 `Shared::show_modal` (the new early return).
- `crates/noctis-core/src/pipeline/component.rs`:
  - L612-622 `shared_for`/`notice_state` (they choose `declared_submit` for `Input::Modal`);
  - L470-497 `read_input`.
  - Module docs: step 3 says the submit's response comes from `#[modal(response = ..)]`.
- `crates/noctis-core/src/pipeline/tests/components.rs` L714-740: `ModalHandler::__new_with_responder` call sites (add `__Response::Update`).
- `crates/noctis-core/src/responder/tests.rs`: unit tests (`update_unavailable_without_a_source` is the pattern to follow).
- `crates/noctis-ui/src/modal/submit.rs`: the decoder (read-only). Component type numbers: L158-161 and L375-384.
- Test patterns:
  - `crates/noctis-testkit/tests/components.rs`: `Running` L508, `click` L731, `source_message` L713, `bio_submit` L1106, `manual_button_opens_a_modal` L1035, `slash_opened_modal_cannot_update` L1075, `assert_interaction_request`, `body`;
  - `crates/noctis/tests/modal_derive.rs` (`TextForm`, `EntityForm`, `UploadForm` field shapes, `Colour`/`Severity`/`Topic` choices, raw JSON fixtures);
  - `crates/noctis/tests/ui/int/INT-05_component_*.rs` and `INT-04_modal_show_modal.rs`: fixtures to copy.
- Skill (`examples/project-template/.claude/skills/noctis/`):
  - `references/modals.md` L150-200, L265-275, L499;
  - `references/responder.md` L135-210, L373;
  - `SKILL.md` L315 and the `API hash` line;
  - `references/routes-payloads-refs.md` L140;
  - `references/troubleshooting.md` L26.
- Template: `src/modules/profile/mod.rs` L51-110 (`EditBio`, `BioForm`), `src/modules/profile/handlers.rs` L44-160 (`/bio`, the button handler, the submit handler), `AGENTS.md` L47-L54.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/src/{kind.rs, component.rs}`:
   - add `__Modal::RESPONSE` and the `response` parameter, with the call-site updates (`kind.rs`, `pipeline/tests/components.rs`, any other `__new_with_responder` caller found with `grep -rn "ModalHandler::__new_with_responder" crates`);
   - update the docs.
2. `crates/noctis-core/src/responder.rs`, `pipeline/component.rs`:
   - `no_modal`, `declared_submit` and the `show_modal` early return;
   - `shared_for` picks the constructor;
   - the module docs;
   - unit test `submit_refuses_show_modal` (row 14) in `responder/tests.rs`;
   - pipeline unit tests in `pipeline/tests/components.rs` for rows 15-24, using hand-built `ModalHandler`s with each `__Response`, `Manual`, and with and without a source `message`.
3. `crates/noctis-macros/src/{route.rs, modal.rs}`:
   - the grammar, the `RESPONSE` emission, the scaffolding `manual` flag and the help text;
   - unit tests for rows 1-8 (exact messages).
4. `crates/noctis/tests/ui/int/`, blessed with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`:
   - `INT-05_modal_auto_defer_false_without_manual.rs` (row 9);
   - `INT-05_modal_auto_defer_false_without_responder.rs` (row 10);
   - `INT-05_modal_manual_without_auto_defer_false.rs` (row 11);
   - `INT-05_modal_hook_manual_without_flag.rs` (row 13, negative half);
   - `INT-04_modal_manual_show_modal.rs` (row 12).
   - In `crates/noctis/tests/ui/cid/`: `CID-03_modal_response_value.rs` (row 5).
   - Delete or retarget any fixture that asserted the old "modal response not supported" text: `grep -rln "runtime argument of #\[modal\]" crates`.
5. `crates/noctis/tests/modal_flow.rs` (new), the facade test that settles the template's tracking-mode AC: a verbatim copy of the template's `EditBio`, `Pronouns`, `BioForm` and `/bio` declarations and handlers. Swap only the template-local helpers for test stubs: the storage becomes `ctx.kv()`, the View becomes a one-line `render`, and `ctx.update`/`ctx.respond`, still [planned, 8.1], become a returned `V2Message`. Build them into an `App::builder()` and assert that it builds. Also cover row 13, the positive half.
6. `crates/noctis-testkit/tests/modals.rs` (new; `FakeGateway` + `FakeDiscordHttp`; assert method, path, headers and body; copy the `Running`/`click`/`source_message`/`user` helpers from `components.rs`):
   - `button_opens_modal_and_submit_updates_message` (rows 32 and 16);
   - `fr64_inputs_decode_end_to_end` (rows 25-28): two modals, because of MOD-02's 5-component cap: one for Text/Radio/CheckboxGroup/Checkbox, one for a user select and File Upload with `resolved`;
   - `invalid_submissions_are_rejected` (rows 29-31);
   - `modal_auto_defer_follows_response` (rows 15, 17, 19, 21);
   - `manual_modal_submit` (rows 22-23);
   - `slash_opened_modal_flow` (row 33);
   - `guarded_modal_under_each_response` (row 34).
7. Skill:
   - `modals.md`: replace the `[planned, Story 7.8]` markers with the real full open/submit recipe: button `auto_defer = false` → `show_modal(Form { .. }.build())` → `#[handler] async fn handle(self, ctx: ModalSubmitCtx)` returning the View. Add the `/bio` variant, the `response`/`auto_defer` table for `#[modal]` (and which callback each gives, with or without a source message), the `ModalResponder<Manual>` rule, `ModalUnavailable`, and `SubmitError` → generic notice.
   - `responder.md` L139/147/184/206/373 and `SKILL.md` L315: drop the markers and state the real behaviour.
   - `routes-payloads-refs.md` L140 and `troubleshooting.md` L26: only `rate_limit` is still rejected; add an entry for the modal INT-05 mismatch.
   - Re-bless and update the `API hash`; `skill-check` must pass.
8. Template:
   - `BioForm` keeps the default `response` (update), with a doc line saying so, and the `/bio`-opened case now uses `response = update`'s documented fallback (a public reply);
   - remove the 7.8-related `[planned]` notes;
   - `AGENTS.md` L47/L49/L54: `#[modal(.., response = .., auto_defer = ..)]` is real; `ModalResponder<Manual>` is required exactly with `auto_defer = false`.
9. `docs/discord-rules.md`, the spine amendments, and the two Story 7.8 `(decision)` lines planning added to the architecture `.memlog.md` (keep them). Then run `cargo run --release -p xtask -- api-snapshot --bless`, update the `SKILL.md` hash, and run every gate in Verification.

**Acceptance Criteria:**
- Given an `auto_defer = false` button whose handler takes `ComponentResponder<Manual>`, when it is clicked through `FakeGateway`, then exactly one type-9 callback is sent, its `data.custom_id` is the `#[derive(Modal)]` struct's encoded Route id, and the declared Radio/Checkbox components appear in `data.components`.
- Given that modal submitted with Radio Group, Checkbox Group, Checkbox, Text Input, select and File Upload values, when it is dispatched, then the `#[handler]` receives `self` with every field populated (from the raw bytes, including what twilight drops) and `ModalSubmitCtx`, and the returned View updates the source message with one type-7 callback.
- Given `#[modal("id", response = ephemeral | public, ..)]` or the default `update`, when the handler is slower than the ack deadline, then the auto-defer is type 6 only for `update` with a source message, and otherwise type 5 with `EPHEMERAL` exactly for `ephemeral`.
- Given `#[modal("id", auto_defer = false, ..)]`, when the handler takes `ModalResponder<Manual>`, then it compiles and no auto-defer is sent; when the declaration and the responder disagree, then compilation fails citing `[INT-05]`; and `show_modal` on any `ModalResponder` is E0599.
- Given the Usage Skill and template, when `skill-check` runs, then it passes, no `Story 7.8` `[planned]` marker remains, `crates/noctis/tests/modal_flow.rs` (the template's bio flow) compiles and builds an App, and the API hash matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Scope is the remaining delta (decision).** Submit dispatch, raw-bytes decoding (`ModalSubmission::from_slice(raw.as_bytes())`), `ModalResponder`, `UpdateUnavailable`, the `show_modal` typestate for Commands and components, guards and Refs already shipped (Stories 6.10, 6.11, 7.1-7.7). 7.8 adds `response`/`auto_defer` on `#[modal]` and the end-to-end proof. Re-implementing any of it is out of scope.
- **The runtime `show_modal` guard (decision).** "A dynamic attempt is a typed error" has no typed path today, because `ModalResponder` has no method. The `no_modal` flag makes `Shared` itself refuse it, so a future erased path (Story 8.1's `ctx.respond`, the Dispatcher) cannot send a type-9 callback for a submit. It is a plain `bool` fixed at construction, so it adds no allocation.
- **`response = update` on a modal is legal** (maintainer decisions §2). It cannot know at compile time whether a slash command opened the modal. With no source message, the auto-defer is type 5 public and a returned message is a public type-4 reply, while `ModalResponder::update` returns `UpdateUnavailable`. This is existing `Shared` behaviour, now pinned by the tests.
- **Template tracking mode (decision).** A template `cargo build` needs a second dependency graph and `target/`, which the cloud disk cannot afford, and the template still uses [planned] 8.1 API. So the AC "compiles against it in tracking mode" is met by `modal_flow.rs`, which copies the template's bio declarations verbatim, plus `cargo metadata` (the ci.yml `check` step). `template-build` stays `continue-on-error` in `.github/workflows-disabled/ci.yml`, unchanged.

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild, dhat `alloc_dispatch`/`memory_flat` included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok (new fixtures follow `<area>/<RULE-ID>_<slug>`)
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `cargo metadata --format-version 1 --manifest-path examples/project-template/Cargo.toml > /dev/null` -- ok
- `grep -rn "Story 7.8" crates examples/project-template docs --include=*.rs --include=*.md` -- no hit that says "planned", "arrive" or "until"
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
