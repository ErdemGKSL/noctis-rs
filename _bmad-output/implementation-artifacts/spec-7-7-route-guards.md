---
title: 'Story 7.7: Route Guards'
type: 'feature'
created: '2026-10-21'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** AD-23 stage 8 (Route Guards) does not exist. The kind derives reject `owner`, `permissions` and `guard` with ``[CID-03] `owner` is a runtime argument … does not support yet``. A bot author therefore cannot declare "invoking user only" or "requires MANAGE_GUILD" on a Route, and has to re-check this by hand in every handler (FR-19; AD-6, AD-23; Usage Skill `[planned]`).

**Approach:**
- The seven kind attributes (`#[button]`, the five select attributes, `#[modal]`) accept three runtime arguments:
  - `owner = field`: a Payload field of type `Id<UserMarker>` that must equal the clicking user;
  - `permissions = A | B`: the member's resolved channel permissions must contain these;
  - `guard = path::to::fn`: a custom async check.
- The derive emits a core `__Guards<Ctx>` impl. The component and modal pipelines wrap the handler future, so the guards run after the before-Middleware (stage 7) and before the handler (stage 9).
- A denial sends the localized `noctis.guard.*` ephemeral notice and never runs the handler. After an auto-defer, the notice is an ephemeral follow-up and a type-5 deferred original is deleted.

## Boundaries & Constraints

**Always:**
- **Grammar** (`crates/noctis-macros/src/route.rs`):
  - `RUNTIME_ARGS` shrinks to `["rate_limit"]`, which is still rejected with the existing message.
  - `owner`, `permissions` and `guard` are each accepted once, in any position, on all seven kind attributes. Each is recorded in a new `RouteArgs.guards: GuardArgs { owner: Option<Ident>, permissions: Option<u64>, guard: Option<syn::Path> }`. None of them changes the encoding or the width proof.
  - The error texts (CID-03, spanned on the offending token, each with a `help:`):
    - `owner` without `= <ident>`, or with a non-ident value: ``"`owner` takes a field name such as `owner = user`"``.
    - `guard` without a value, or with a non-path value (literal, expression, generic arguments): ``"`guard` takes a function path such as `guard = auth::is_staff`"``.
    - A duplicate: the existing "given twice" error.
- **`permissions` value:** reuse `meta.rs::permissions(span, tokens)` (make it `pub(crate)`) as is.
  - Unknown names and wrong case keep the META-01 messages.
  - Bad syntax keeps `PERMISSIONS_SYNTAX`.
  - A bare `permissions` without a value gives `PERMISSIONS_SYNTAX`.
  - Repeated names OR together.
- **`owner` field check (in the derive, after the fields are known):**
  - The named field must exist: ``[CID-03] `owner = {x}` names no field of `{Struct}`; its fields are: a, b``, or `…; it has no fields` for a unit struct.
  - On `#[derive(Modal)]` it must be a Payload field (no input attribute): ``[CID-03] `owner` must name a Payload field; `{x}` is a modal input``.
  - The field type is checked by rustc through `__owner_id::<T: __OwnerField>(&self.x)`. Only `Id<UserMarker>` implements `__OwnerField`, with `#[diagnostic::on_unimplemented(message = "`owner` needs a field of type `Id<UserMarker>`, found `{Self}`")]`.
- **Core API** (`crates/noctis-core/src/guard.rs`, new; unit tests in `guard/tests.rs`):
  - `pub enum Access { Allow, Deny(MessageKey) }`: `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`, `#[must_use]`. It has `pub const fn deny() -> Self` (that is, `Deny(keys::GUARD_FAILED)`) and `pub const fn is_allowed(self) -> bool`.
  - Re-exports: `noctis_core::Access` and `noctis::Access`; the prelude adds `Access`.
  - A custom guard is any path callable as `path(ctx: &ComponentCtx, this: &Self)` (`&ModalSubmitCtx` for a modal) that returns a `Future<Output = HandlerResult<Access>> + Send`. The usual form is `async fn is_staff(ctx: &ComponentCtx, this: &Ban) -> HandlerResult<Access>`.
- **Hidden items** (`#[doc(hidden)] pub`, exported like the other `__` items in `noctis_core` and reached as `::noctis::__private::core::…`):
  - `trait __Guards<C>` with:
    - `const GUARDED: bool = false;`
    - `fn __guard<'a>(&'a self, ctx: &'a C) -> impl Future<Output = HandlerResult<Option<__Denial>>> + Send + 'a`, whose default body is `async { Ok(None) }`.
  - `__Denial { guard: &'static str, key: MessageKey }`, where `guard` is `"owner"`, `"permissions"` or the custom path as written (`stringify!`).
  - `fn __check_owner(user: Id<UserMarker>, owner: Id<UserMarker>) -> Option<__Denial>` (key `GUARD_OWNER`).
  - `fn __check_permissions(have: Option<Permissions>, need: Permissions) -> Option<__Denial>` (key `GUARD_PERMISSIONS`).
    - It allows when `have` contains `need`, or when `have` contains `ADMINISTRATOR`.
    - It denies when `have` is `None` (a DM or no `member`).
  - `fn __check_custom(path: &'static str, r: HandlerResult<Access>) -> HandlerResult<Option<__Denial>>`.
  - `trait __OwnerField` and `fn __owner_id`.
- **Derive emission** (`component.rs`, `modal.rs`, and `kind::or_fallback` placeholders):
  - Every component derive emits `impl __Guards<ComponentCtx> for S`, and `#[derive(Modal)]` emits `impl __Guards<ModalSubmitCtx> for S`. With no guard argument the impl is empty.
  - With guards, `GUARDED = true` and `__guard` checks them in this order: owner, then permissions, then custom. The first denial wins, and the custom guard runs only when the built-in guards passed.
  - The custom call is `quote_spanned!` on the path, so a wrong signature errors at the path.
- **Context:**
  - `CtxInner` gains `member_permissions: Option<Permissions>` (default `None`).
  - The component pipeline fills it from `interaction.member.and_then(|m| m.permissions)`.
  - `ctx_accessors!` adds `pub fn member_permissions(&self) -> Option<Permissions>` ("the member's permissions in the channel, including overwrites, as Discord resolved them; `None` in DMs") to `ComponentCtx`, `ModalSubmitCtx` and `ExpiredCtx`.
- **Pipeline** (`component.rs` constructors, `pipeline.rs`, `pipeline/component.rs`):
  - `ComponentHandler::{__new, __new_with_responder}` and `ModalHandler::__new_with_responder` gain the bound `S: __Guards<ComponentCtx>` (or `ModalSubmitCtx`).
  - Only the pipeline starters (`start`, `start_record`) wrap the handler future when `S::GUARDED`. The wrapper runs `value.__guard(&ctx).await?`, and on `Some(d)` returns `Err(HandlerError::user_condition(GuardDenied { route, guard, key }, key))` (`GuardDenied` is crate-private). Otherwise it awaits the handler.
  - `call` (the detached `__testing::call_*` seams) runs no guards. An unguarded Route adds no allocation.
- **`run_started`:**
  - It maps a returned error whose inner error is `GuardDenied` to a new `Outcome::Denied { key, guard }`.
  - For that outcome:
    - no `noctis_handler_duration_seconds` record, no `noctis_handler_errors_total` and no Error Sink report;
    - one `debug!(target: "noctis::guard", route, handler, guard, key, "denied")`; no user id or Payload is logged;
    - `send_notice(key)`;
    - drop the timer;
    - after-Middleware with the new `RunOutcome::Denied`.
- **One-shot restore:**
  - When a `one_shot` Route's Ref was taken at stage 2 and the guards deny, the exact record bytes are put back under the same Ref before the notice.
  - The put is `RefStore::put_if_absent(ref, record, ttl)`, where `ttl` is the Route's `ttl`, else `AppBuilder::ref_ttl`.
  - A `false` result or an error is one `warn!(target: "noctis::refs", "could not restore the one-shot Ref after a Guard denial")`, without the Ref value.
  - Pass the restore data (`Ref`, `Bytes`, `Duration`) to `run_started` through `Report` or a new parameter.
- **Not guarded:** the Expired Handler chain (hooks and the App `expired_handler`: there is no decoded `self`), stage 1–5 rejections, Commands, context menus, autocomplete, events and the detached seams.
- **Docs, in the same commit:**
  - `pipeline.rs` module docs, stage 8: "Route Guards: none for Commands; components and modals run their Route's guards".
  - `pipeline/component.rs` module docs: step 6 names the guards.
  - `noctis-codec/src/derive.rs` `RouteRuntime` doc: drop "Later releases add the Guard arguments"; Guards live in core's `__Guards`.
  - `docs/discord-rules.md`: the CID-03 row (L176) and the paragraph at L415-426 (only `rate_limit` is still rejected).
  - Spine: an `Amended (2026-10-21)` line on AD-6, AD-18 and AD-23 with the Design Notes decisions.
  - The skill and the template (Tasks 8 and 9), then re-bless `xtask/public-api.txt` and update the `SKILL.md` API hash.

**Never:**
- Guards on Commands, menus, events or the Expired chain.
- `rate_limit` (Story 13.4).
- New metrics or new `noctis.*` keys (the three `noctis.guard.*` keys already exist in `noctis-i18n`).
- An `Option<Id<UserMarker>>` owner, or more than one `guard =`.
- Changing the `custom_id` or the record format, or the `RouteRuntime` fields.
- Logging Payload values, user ids or Refs on a denial.
- A new external crate, or a debug build.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error handling |
|---|----------|---------------|----------|----------------|
| 1 | parse owner | `#[button("a", owner = user, defaults(..))]`, field `user: Id<UserMarker>` | `guards.owner = Some(user)`; width line unchanged | — |
| 2 | parse permissions | `permissions = MANAGE_GUILD \| Permissions::BAN_MEMBERS` | bits `32 \| 4` | — |
| 3 | permissions dup | `permissions = KICK_MEMBERS \| KICK_MEMBERS` | bits `2` | — |
| 4 | permissions all | all 51 names of `rules::command::PERMISSIONS` joined | accepted, union | — |
| 5 | permissions bad | `permissions`, `permissions = 8`, `= "X"`, `= A + B`, `= A \|\| B`, `= A \|` | error | `PERMISSIONS_SYNTAX` |
| 6 | permissions name | `MANAGE_SERVER` / `manage_guild` | error | META-01 text, help `see …PERMISSIONS` / ``use `MANAGE_GUILD` `` |
| 7 | owner bad | `owner`, `owner = "user"`, `owner = a.b`, `owner = 1` | error | ``owner` takes a field name…` |
| 8 | owner unknown | `owner = usr` on `{ user, page }` / unit struct | error | `names no field of `S`; its fields are: user, page` / `it has no fields` |
| 9 | owner type | field `Option<Id<UserMarker>>`, `u64`, `Id<GuildMarker>` | compile error | on_unimplemented message |
| 10 | owner modal input | `owner = bio` where `bio` has `#[text_input]` | error | `must name a Payload field…modal input` |
| 11 | guard bad | `guard`, `guard = "f"`, `guard = f::<T>`, `guard = 1` | error | ``guard` takes a function path…` |
| 12 | duplicates | `owner = a, owner = b`; `guard = f, guard = g` | error | "given twice" |
| 13 | rate_limit | `rate_limit(..)` | error | existing "does not support yet" text |
| 14 | guard signature | custom fn takes `&ModalSubmitCtx` on a Button / returns `Result<(), MessageKey>` / non-`Send` future | compile error at the path | rustc |
| 15 | unguarded | Route with no guard args | `GUARDED == false`; expansion otherwise unchanged; dhat tests green | — |
| 16 | owner pass | owner 42, click by 42 | handler runs | — |
| 17 | owner deny | owner 42, click by 43 | handler not run; one type-4 `EPHEMERAL` callback, `noctis.guard.owner` text; `debug` `guard="owner"` | no report, no counter |
| 18 | perms pass | need `MANAGE_GUILD`, member `"permissions": "32"` | runs | — |
| 19 | perms superset | need `32`, have `"48"` | runs | — |
| 20 | perms admin | need `MANAGE_GUILD \| BAN_MEMBERS`, have `"8"` | runs | — |
| 21 | perms missing | need `32`, have `"0"` / have `"4"` | deny `noctis.guard.permissions` | — |
| 22 | perms DM | no `member`, `user` only | deny `noctis.guard.permissions` | — |
| 23 | order | owner and permissions both fail | `noctis.guard.owner`; the custom guard is not called (call counter 0) | — |
| 24 | custom allow | `Ok(Access::Allow)` | handler runs | — |
| 25 | custom deny default | `Ok(Access::deny())` | `noctis.guard.failed` notice, `guard="<path>"` | — |
| 26 | custom deny key | `Ok(Access::Deny(k))`, tr user | notice is `notice::text(strings, tr, k)` (unknown key → generic text + the existing warn) | — |
| 27 | custom user err | `Err(HandlerError::user(k))` | like a handler user error: notice `k`, `debug`, after `UserError` | — |
| 28 | custom err | `Err(HandlerError::new(io))` | generic notice, Component sink `Error` report (handler name), counter +1, after `Failed` | — |
| 29 | custom panic | guard panics | generic notice, `Panic` report, after `Panicked` | caught |
| 30 | deny after defer, type 5 | `response = ephemeral`, guard sleeps past `ack_deadline(50ms)` then denies | type-5 defer, `DELETE …/messages/@original`, `POST /webhooks/7/tok` with `flags` `EPHEMERAL` and the notice | — |
| 31 | deny after defer, type 6 | `response = update` on a source message, same | type-6 defer, ephemeral follow-up; nothing deleted | — |
| 32 | middleware stop first | before-Middleware `Stop(Some(k))` | guards not called; `k` notice | — |
| 33 | after sees denial | after-Middleware on the Module | `outcome() == RunOutcome::Denied`; Extensions present | — |
| 34 | modal owner | `#[modal("m", owner = user, ..)]`, submit by another user | deny `noctis.guard.owner` | — |
| 35 | one-shot restore | `by_ref, one_shot, owner = user`: non-owner clicks, then owner clicks | first: deny + record restored; second: handler runs (`take` hit) | — |
| 36 | restore fails | a RefStore `put_if_absent` that errors / returns `false` | denial still sent; one `warn` on `noctis::refs`, no Ref in the line | — |
| 37 | not guarded | stale id / missing Ref on a guarded Route; detached `__testing::call_component` | Expired chain runs with no guard; the seam runs the handler | — |
| 38 | ctx accessor | click with `"permissions": "32"`; DM click | `member_permissions() == Some(MANAGE_GUILD)`; `None` | — |
| 39 | Access | `Access::deny()`, `is_allowed` | `Deny(GUARD_FAILED)`; `Allow.is_allowed()`, `!deny().is_allowed()` | — |

</intent-contract>

## Code Map

- `crates/noctis-macros/src/route.rs`:
  - `RouteArgs` L38, `Seen` L195, `parse_list` L207, `parse_arg` L325 (allowed-name list L373-384);
  - `RUNTIME_ARGS` L69, `unsupported_runtime_arg` L452;
  - tests L700+ (the L778 `owner` rejection case becomes `rate_limit`).
- `crates/noctis-macros/src/meta.rs`: L225 `permissions()` and L24 `PERMISSIONS_SYNTAX` (make them `pub(crate)`); `path_last` L201.
- `crates/noctis-macros/src/component.rs`: L86 `derive_or_fallback`, L106 `derive` (it emits `__Component`; add the `__Guards` impl next to it). The fields come from `common::kind_fields` L69.
- `crates/noctis-macros/src/modal.rs`: L929 fallback, L960 `expand` (`payload` = the Payload fields; input fields are `defs`).
- `crates/noctis-macros/src/kind.rs`: `or_fallback`/`scaffolding` (the placeholder impls must include an empty `__Guards`).
- `crates/noctis-core/src/kind.rs`: L449 `__Component`, L460 `__Modal`, L513-608 the component and modal `__Bind` impls (they call the constructors; add the bounds).
- `crates/noctis-core/src/component.rs`:
  - L66 `ctx_accessors!` (add `member_permissions`);
  - L532 `ComponentHandler` and L581 `__new_with_responder`: wrap `starter` inside `start`/`start_record` only;
  - L835 `ModalHandler::__new_with_responder`.
- `crates/noctis-core/src/command.rs`: L84 `CtxInner` and `detached_parts`.
- `crates/noctis-core/src/pipeline.rs`:
  - docs L1-40;
  - `Outcome` L333 and `run_outcome`;
  - `before_and_handler` L460 (the stage-8 comment at L482);
  - `run_started` L540-620 (histogram, notice match, `report_kind` match);
  - `notice_key`, `send_notice`.
- `crates/noctis-core/src/pipeline/component.rs`:
  - docs L1-56;
  - `Interaction` destructure L526 (add `member`);
  - `CtxInner` L640;
  - `lookup` L355 and the stage-4 Ref branch L690-715 (keep `r` and `record` for the restore).
- `crates/noctis-core/src/middleware.rs`: L170 `RunOutcome` (add `Denied`, with a doc line).
- `crates/noctis-core/src/responder.rs`: L1858 `Shared::notice`, which already does the after-defer delete plus follow-up; reuse it, no change expected.
- `crates/noctis-core/src/test_support.rs`: L218 `test_route!` (emit empty `__Guards<ComponentCtx>` and `<ModalSubmitCtx>` impls); test fixtures in core that implement Routes by hand need the same.
- `crates/noctis-core/src/lib.rs`: re-exports near L206-280 (`Permissions` at L275). `crates/noctis/src/lib.rs`: root re-exports ~L576, prelude ~L1819 and its doc list.
- `crates/noctis-i18n/src/lib.rs` L46-51 and L100-102: the existing keys (read-only).
- `crates/noctis-ports/src/store.rs` L36 `put_if_absent`; `app.rs` L300 `ref_ttl`.
- Tests:
  - wire tests follow `crates/noctis-testkit/tests/components.rs` (`click()` L731 JSON with `member.permissions`, `Running`, `wait_sent`) and `refs.rs`;
  - facade tests follow `crates/noctis/tests/components.rs` and `middleware.rs`;
  - UI fixtures go in `crates/noctis/tests/ui/cid/` (`CID-03_runtime_argument.rs` L4 switches to `rate_limit(user, 5, per = "1m")`) and `crates/noctis/tests/types/`.
- Skill (`examples/project-template/.claude/skills/noctis/`):
  - `references/routes-payloads-refs.md` L140-172 (the argument table, `[planned]` markers, the custom-guard signature);
  - `SKILL.md` L23, L159 and L374 (API hash, `[planned]` markers);
  - `references/modules-middleware-ratelimits.md` (stage 8, `RunOutcome::Denied`);
  - `references/i18n.md` L141-143;
  - `references/troubleshooting.md`.
- Template: `src/modules/setup/mod.rs` L60/L78/L92, `src/modules/profile/mod.rs` L60/L94, `src/modules/dashboard/mod.rs` (the `permissions = MANAGE_GUILD, // [planned]…` lines), and `AGENTS.md` L54. The template is not built by the gates; keep its code consistent with the skill.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/src/guard.rs` (+ `lib.rs` wiring): `Access`, `__Guards`, `__Denial`, the `__check_*` helpers, `__OwnerField`/`__owner_id` and the crate-private `GuardDenied`. Unit tests in `guard/tests.rs` for rows 19-22 (as pure checks) and 39.
2. `crates/noctis-core/src/{command.rs, component.rs, kind.rs, test_support.rs}`: the `member_permissions` field and accessor, the constructor bounds, the guarded starters, and the test-route impls.
3. `crates/noctis-core/src/{pipeline.rs, pipeline/component.rs, middleware.rs}`:
   - `Outcome::Denied`, `RunOutcome::Denied`, the one-shot restore and the docs;
   - unit tests in `pipeline/tests/guards.rs` (hand-written guarded test Routes) for rows 23, 27-29, 32, 33, 36 and 37.
4. `crates/noctis-macros/src/{route.rs, meta.rs, component.rs, modal.rs, kind.rs}`: the grammar, the owner check and the `__Guards` emission. Unit tests for rows 1-3, 5, 7, 8, 10-13 and 15 (exact messages).
5. `crates/noctis/tests/ui/cid/`:
   - `CID-03_runtime_argument` (now `rate_limit`);
   - `CID-03_owner_unknown_field`, `CID-03_owner_modal_input`, `CID-03_guard_not_a_path`;
   - `crates/noctis/tests/ui/meta/META-01_guard_unknown_permission`;
   - `crates/noctis/tests/types/owner_field_type.rs` and `guard_wrong_ctx.rs` (rows 9 and 14).

   Bless them with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`.
6. `crates/noctis/src/lib.rs`: the `Access` re-export, the prelude and its docs. `crates/noctis/tests/guards.rs`: real derives (Button, a select, Modal) with every guard kind, built through `App::builder()`, covering rows 4, 16, 24 and 38.
7. `crates/noctis-testkit/tests/guards.rs` (`FakeGateway` + `FakeDiscordHttp`; assert method, path and body):
   - `owner_guard_denies_with_ephemeral_notice` (row 17);
   - `permissions_guard_allows_admin_and_denies_dm` (rows 18, 20-22);
   - `custom_guard_deny_key` (rows 25 and 26);
   - `deny_after_type5_defer_deletes_original` (row 30);
   - `deny_after_type6_defer_follows_up` (row 31);
   - `modal_owner_guard` (row 34);
   - `one_shot_ref_survives_denial` (row 35).
8. Skill:
   - `routes-payloads-refs.md`: drop the `[planned]` markers on `owner`, `permissions` and `guard`, and document:
     - the exact semantics (field type, ADMINISTRATOR, DM deny, order, the real custom signature with `Access`, one-shot restore, not on the Expired chain or detached seams, after-defer behaviour);
     - every new error text;
     - a worked `guard = auth::is_staff` example.
   - `SKILL.md`: L23 and L159 markers, the API hash.
   - `modules-middleware-ratelimits.md`: stage 8 and `RunOutcome::Denied`.
   - `troubleshooting.md`: entries for the owner type error and the `noctis.guard.permissions` denial in DMs.
   - Mention `member_permissions()` where the ctx accessors are listed.
   - `skill-check` must find every new public item.
9. Template:
   - remove the `// [planned]…` comments from the `owner = …` and `permissions = …` lines;
   - `AGENTS.md` L54: guards are real (`owner = field`, `permissions = …`, `guard = path` returning `HandlerResult<Access>`); `rate_limit` is still planned (Story 13.4).
10. `docs/discord-rules.md` and the spine amendments. Run `cargo run --release -p xtask -- api-snapshot --bless`, then update the `SKILL.md` hash. Then run every gate in Verification.

**Acceptance Criteria:**
- Given `#[button("dash.mod", permissions = MANAGE_GUILD, defaults(..))]`, `owner = user` or `guard = path` on a Route, when a member without the permission, a different user, or a user the guard denies clicks through `FakeGateway`, then exactly one ephemeral type-4 callback carries the localized `noctis.guard.permissions` / `noctis.guard.owner` / guard-chosen text, and the handler never runs.
- Given an auto-defer that already happened before the guard decided, when the notice is sent, then it is an ephemeral follow-up, and a type-5 deferred original is deleted (a type-6 one is left alone).
- Given a `one_shot` by-Ref Route with an `owner` guard, when a non-owner clicks and then the owner clicks, then the owner's click reaches the handler.
- Given the Usage Skill, when `skill-check` runs, then it passes, guards are documented with no `[planned]` marker on `owner`/`permissions`/`guard`, the template's guarded Routes carry no planned comments, and the API hash matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Guards live in core, not in `RouteRuntime` (decision).** The checks need `ComponentCtx`, `ModalSubmitCtx` and `Permissions`, which codec cannot name. The derive emits `__Guards<Ctx>` (RPITIT, stable since 1.75; MSRV is 1.91), and `RouteRuntime` keeps only the Ref policy. Spine AD-6 is amended accordingly.
- **Guards wrap the handler future (decision).** The future is created at stage 4 and first polled after stage 7, so a wrapper gives stage 8 its place with no new pipeline state. A sentinel `GuardDenied` user-condition error carries the denial to `run_started`, which turns it into `Outcome::Denied` (no histogram, no sink). A guard's run time counts in the handler duration only when it allows the run.
- **One-shot Refs are restored on a denial (decision, closing AD-18's caveat).** Otherwise any user could burn another user's confirmation. `take` stays atomic; the restore has a small window in which the owner sees "expired".
- **Permissions semantics (decision).** The source is `member.permissions` (the resolved channel permissions Discord sends). `ADMINISTRATOR` satisfies every requirement, and a missing member (DM) is denied.
- **Accepted on all seven kind attributes, `#[modal]` included (decision)**, like `ttl`/`one_shot`. A modal's `owner` must be a Payload field, because inputs are submitted text.
- Golden sketch of the derive output (Button, `owner = user`, `permissions = MANAGE_GUILD`, `guard = auth::is_staff`):
  ```rust
  impl __Guards<ComponentCtx> for Ban {
      const GUARDED: bool = true;
      fn __guard<'a>(&'a self, ctx: &'a ComponentCtx) -> impl Future<Output = HandlerResult<Option<__Denial>>> + Send + 'a {
          async move {
              if let Some(d) = __check_owner(ctx.user_id(), __owner_id(&self.user)) { return Ok(Some(d)); }
              if let Some(d) = __check_permissions(ctx.member_permissions(), Permissions::from_bits_retain(32)) { return Ok(Some(d)); }
              __check_custom("auth::is_staff", auth::is_staff(ctx, self).await)
          }
      }
  }
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild and the dhat `alloc_dispatch`/`memory_flat` tests included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok (no new rule id; the new fixtures follow the `<area>/<RULE-ID>_<slug>` layout)
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rn "Story 7.7\|\[planned\]: runtime arguments" crates examples/project-template --include=*.rs --include=*.md` -- no hit
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
