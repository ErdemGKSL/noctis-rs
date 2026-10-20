---
title: 'Story 6.6: Automatic deferral, token lifetime and follow-up cap'
type: 'feature'
created: '2026-10-20'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 6.5 left pipeline stage 6 as a no-op. A handler that has not answered by Discord's 3 s deadline loses the interaction ("interaction failed"). The Responder ignores the 15-minute token lifetime and the 5-follow-up cap for user-installed apps. `opens_modal` / `OpensModal` / `modal()` are still planned. `Shared` uses load/store, so two initial callbacks can race (6.5 residual risk). Implements FR-21, AD-11, AD-23, AD-29; INT-01, INT-02, INT-03 (runtime part), INT-05, INT-08, INT-11.

**Approach:**
- Stage 6 arms a per-interaction auto-defer task at `received_at + ack_deadline` (default 2000 ms, `AppBuilder::ack_deadline`).
- A gate (async mutex + atomic state) serializes every initial response. Whichever of the handler and the timer gets there first sends the initial callback. A handler `reply` that finds the interaction auto-deferred becomes `edit_original`.
- `#[command(.., opens_modal)]` + `CommandResponder<OpensModal>` gives `modal(Modal)` (type 9) and disables the auto-defer. A mismatch is a compile error citing `[INT-05]`.
- After 15 min, edits and deletes of public messages fall back to bot-token channel routes. Everything else returns `ResponderError::TokenExpired` without HTTP.
- User-install-only interactions cap follow-ups at 5 (`ResponderError::FollowupCap` on the 6th).
- Record `noctis_autodefer_total` and `noctis_ack_latency_seconds`.

## Boundaries & Constraints

**Always:**
- Initial responses are type 4, 5 or 9 from the handler, or type 5 from the auto-defer. Each takes `Shared::gate` (a `tokio::sync::Mutex<()>`) and then reads `state` under it.
  - At most one initial callback is ever sent per interaction (INT-03).
  - No `std::sync::Mutex` guard is held across `.await`.
- The auto-defer sends exactly `{"type":5}` or `{"type":5,"data":{"flags":64}}`, using the handler's declared `Ephemerality` (INT-08, INT-10). It goes over `Edge::interaction_callback` (hedged path, Story 5.9).
- Every deadline and expiry is measured from `InteractionTarget::received_at` with `tokio::time::Instant`, so queueing time counts and a paused clock controls them.
- The auto-defer exists only for the interaction transport. It never runs for the `Dispatcher` (message transport) or the detached `__testing` transport. It never runs for `opens_modal` handlers.
- The auto-defer task stops whenever the handler run ends: on `Ok`, `Err`, a panic or a shutdown cancel. It is aborted through a drop guard. It never outlives `execute`.
- Validation (`prepare`) happens before the gate and before any slot or HTTP. TokenExpired and FollowupCap are checked before any HTTP request.
- Metrics carry only the `app` label (AD-29). No ids or free text go into labels or logs, and tokens are never logged.
- Library code has no `unwrap`/`expect` on external input.
- Test names: `int_01_*`, `int_02_*`, `int_03_*`, `int_05_*`, `int_11_*` must exist (rules-catalog). Do not add tests named `int_06`, `int_13`.

**Never:**
- No component or modal-submit dispatch, no `ComponentResponder`, no type-6 defer, no `#[route(.., opens_modal)]` (Epic 7). No `opens_modal` on `#[user_command]`/`#[message_command]`: it stays `planned` there.
- No user-facing notices, no deleting the deferred original on error, no `noctis.*` keys (6.8). No autocomplete (6.7), no Publish (6.9).
- No env variable for `ack_deadline`, no new crate dependency or dependency edge, no debug builds.
- No test-only branch in the HTTP path.
- No real sleeps longer than 1 s in tests. Use `ack_deadline`, a `received_at` in the past, or `tokio::time::pause`/`advance`.

## I/O & Edge-Case Matrix

Abbreviations:
- **CB**: callback.
- **@O**: `/webhooks/{app}/{token}/messages/@original`.
- **FU**: follow-up `POST /webhooks/{app}/{token}?wait=true&with_components=true`.
- **D**: `received_at + ack_deadline`.
- **T**: `received_at + 15 min`.
- **bot edit**: `PATCH /channels/{channel_id}/messages/{id}` with the bot token.
- **AD**: `noctis_autodefer_total{app}`.
- **AL**: `noctis_ack_latency_seconds{app}`.

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | Fast reply | handler `reply` before D | one CB type 4; timer never sends; AL +1 sample; AD unchanged | none |
| 2 | Slow handler, public | no response at D | CB exactly `{"type":5}` at D; AD +1; AL +1 | none |
| 3 | Slow handler, ephemeral | declared `ephemeral` | CB `{"type":5,"data":{"flags":64}}` | none |
| 4 | INT-03 late `reply` | `r.reply(v2)` after the auto-defer | no 2nd CB; `PATCH @O` with flags 32768; `reply` returns `Ok(Acked)` | none |
| 5 | Late returned message | handler returns `V2Message` at +5 s (virtual) | CB type 5 at D, then `PATCH @O` flags 32768 | none |
| 6 | Late legacy reply | `LegacyMessage` after the auto-defer | `PATCH @O` legacy body, no clears | none |
| 7 | Race at D | reply and timer both ready at D | exactly one CB (type 4 or 5); if the timer won, the reply is the PATCH | none |
| 8 | Explicit `defer` before D | `r.defer(Public)` at D−1 ms | one CB type 5; timer no-op; AD unchanged | none |
| 9 | Explicit `defer` after auto-defer | `defer(Ephemeral)` after D | no HTTP, `Ok(Acked)` (no-op, debug log) | none |
| 10 | Deadline already passed | queued; `received_at` 2.5 s ago at arm | timer fires immediately → CB type 5 | none |
| 11 | Auto-defer fails | received 3.5 s ago, or 400 | warn `noctis::dispatch` "auto-defer failed" with `handler`; state stays Pending; AD unchanged | the handler's own reply gets its typed error (`AckDeadlineMissed`) |
| 12 | Handler errors before D | `Err(e)` at +10 ms | timer aborted, no CB | Sink as 6.5 |
| 13 | Handler panics or is cancelled | before D | timer aborted, no CB | Sink `Panic`/`Cancelled` |
| 14 | `ack_deadline` bounds | `0 ms` / `2500 ms` / `2501 ms` | 0: CB type 5 right away; 2500: accepted | 2501 → `BuildViolation::InvalidAckDeadline { actual }` |
| 15 | `ack_deadline` unset | — | `App::ack_deadline() == 2000 ms` | none |
| 16 | Dispatcher (message transport) | slow handler | no typing, no timer; behavior as 6.5 | none |
| 17 | `opens_modal` + `modal(m)` | `#[command("feedback", description="d", opens_modal)]`, `r: CommandResponder<OpensModal>` | CB `{"type":9,"data":<modal JSON>}`; no timer; AL +1 | none |
| 18 | INT-05 runtime | `opens_modal` handler idle past D | no CB at D or later; at `received_at + 2900 ms` one `error!` on `noctis::dispatch` naming `handler`/`module` | none |
| 19 | Modal invalid | title of 46 chars (Strict) | no HTTP | `Invalid(UiError::Limits{..})` |
| 20 | Modal with unresolved Route id | RefSlot | no HTTP | `Encode { reason }` |
| 21 | Modal over Dispatcher | message transport | no HTTP | `ModalUnavailable` |
| 22 | INT-05 compile | `opens_modal` + `CommandResponder`; `CommandResponder<OpensModal>` without `opens_modal`; `opens_modal` with no responder parameter | compile error containing `[INT-05]` | 3 fixtures |
| 23 | `opens_modal` with a value or twice | `opens_modal = true`; twice | "`opens_modal` takes no value"; "duplicate `#[command]` argument `opens_modal`" | compile error |
| 24 | INT-04 still | `r.modal(..)` on `CommandResponder` | E0599 (existing fixture unchanged) | compile_fail |
| 25 | INT-02 boundary | edit at T−1 ms | normal `PATCH @O` | none |
| 26 | INT-02 public edit | `edit_original` at T, public original whose id is known | bot edit of the original id in `channel_id`; no webhook request | `Rejected` if Discord refuses |
| 27 | INT-02 ephemeral | ephemeral original, edit at T | no HTTP | `TokenExpired` |
| 28 | INT-02 unknown id | public, deferred and never edited before T, or 40060 ack with empty body | no HTTP | `TokenExpired` |
| 29 | INT-02 follow-up | `followup(..)` at T, either visibility | no HTTP | `TokenExpired` |
| 30 | INT-02 handle | `h.edit` / `h.delete` at T: public / ephemeral follow-up | bot edit / bot `DELETE /channels/{c}/messages/{id}` / no HTTP | ephemeral → `TokenExpired` |
| 31 | INT-02 delete original | `delete_original` at T, public, known id | bot `DELETE /channels/{c}/messages/{id}` | ephemeral or unknown → `TokenExpired` |
| 32 | INT-02 late return | handler returns a message at T (public, id known) | bot edit | as 26-28 |
| 33 | No channel | `interaction.channel` absent, at T | no HTTP | `TokenExpired` |
| 34 | INT-11 cap | `authorizing_integration_owners` = `{"1": user}` only; follow-ups 1-5 | 5 FU sent | none |
| 35 | INT-11 sixth | 6th `followup` | no HTTP | `FollowupCap` |
| 36 | INT-11 guild install | owners `{"0": guild, "1": user}` or `{"0": guild}` | 6+ follow-ups sent | none |
| 37 | INT-11 failed follow-up | Strict-invalid follow-up | no slot used (validation precedes the slot) | `Invalid` |
| 38 | Follow-up rejected | 5th FU gets HTTP 400 | slot still used (conservative) | `Rejected` |
| 39 | INT-11 empty owners | `{}` | no cap | none |
| 40 | AL value | reply at +120 ms | AL sample ≈ 0.12 (from `received_at`) | none |

</intent-contract>

## Code Map

**Core (`crates/noctis-core/src/`)**
- `responder.rs` (762 lines):
  - `sealed::Modal` (L77), `NoModal` (L92), `Command<M>`;
  - `ResponderError` (L193): add the variants here;
  - `Responder<Command<M>, Pending>::{reply, defer}` (L330-365);
  - `Shared` (L536) has `state: AtomicU8` + `original: Mutex<Original>`. `Shape`/`Original` (L510-532);
  - `Shared::{finish, reply, defer, edit_original, followup, delete_original}`;
  - `FollowupHandle { shared, message_id, format }` (add a visibility field).
- `responder/transport.rs`:
  - `InteractionTarget { app, application_id, interaction_id, token, received_at }`: add `channel_id: Option<Id<ChannelMarker>>`.
  - `callback()` sends `with_response: false`.
  - `send(app, route, body, bot)`.
  - `MessageTarget::{edit, delete}` are the bot-token channel routes to reuse for the fallback. Factor them into free fns over `(app, channel_id, id)`.
  - `message_id(body)`.
- `responder/tests.rs`: the existing Shared tests.
- `pipeline.rs`:
  - `Resolved::ephemerality()`; add `opens_modal()` there.
  - `execute(span, cancel, shared, start: impl FnOnce(CommandResponder) -> HandlerFuture, report)`: the start closure changes to take `Arc<Shared>`.
  - The stage-6 comment in `run_interaction` (≈L330) is the arm point.
  - `Dispatcher::run_command` builds a message transport.
- `pipeline/tests.rs`:
  - `Harness::start_with(module, configure)`, `inbound_raw` (L138) sets `received_at: Instant::now()`;
  - `int_12_returned_message_after_defer_edits_original` (L565) is the HTTP-assert pattern;
  - `command_metrics` (L848) is the recorder pattern.
- `command.rs`:
  - `HandlerFuture` (L286), `ErasedCommand = dyn Fn(CommandCtx, CommandResponder)` (L289);
  - the four constructors (L343-440), `ephemerality()`/`__with_ephemerality` (L497-515), `call` (L521);
  - the seams `call_command*` (L587+).
- `menu.rs`: `ErasedMenu` and `call()` take a `CommandResponder`. Adapt them to `Arc<Shared>` only as far as `execute`'s new start type needs.
- `edge/callback.rs`:
  - `ACK_DEADLINE` = 2900 ms (L31) is the INT-05 error-log instant;
  - `CallbackAck { body, already_acknowledged }`.
- `app.rs`:
  - `AppBuilder::limits` (L1164) is the setter pattern;
  - the range checks in `build()` (≈L1545-1562, e.g. `SHUTDOWN_GRACE_RANGE`) and the `App::limits()` accessor (L214);
  - `AppShared` is built in `build()`.
- `handler.rs` L99: `AppShared`; add `ack_deadline: Duration`.
- `error.rs` L128+: `BuildViolation` (`#[non_exhaustive]`, `InvalidShutdownGrace { actual: Duration }` pattern).
- `observe.rs`: the `METRICS` catalog and the table L76-88.
- `lib.rs` L178: `pub use responder::{..}` re-exports.

**Twilight:** `Interaction.authorizing_integration_owners: ApplicationIntegrationMap<AnonymizableId<GuildMarker>, Id<UserMarker>>` (fields `guild`, `user`). `Interaction.channel: Option<Channel>`. There is no callback-response model, so parse `resource.message.id` from JSON.

**UI:** `noctis_ui::Modal` (Serialize: `custom_id`, `title`, `components`) and `noctis_ui::validate(&mut Modal, &Limits)` (implements `Validate`).

**Macros (`crates/noctis-macros/src/command.rs`)**
- `PLANNED` (L27) includes `opens_modal`. `EXPECTED` (L29) lists the landed args. `parse_ephemeral` (L84) is the bare-flag pattern.
- `is_responder` (L104).
- `check_params` → `Params { responder: bool, options }` (L437-520): keep the responder `Type`.
- `adapter` (L587); `expand` (L616) builds the constructor turbofish (`__new::<#enum_ident, _, _, _>`).
- The unit tests at L921-1210 assert `EXPECTED` messages and that `opens_modal` is planned (L1083).

**Facade, tests, repo**
- `crates/noctis/src/lib.rs`: root re-exports and the prelude (adds `OpensModal`); the `command` macro docs.
- `crates/noctis/tests/ui/int/`: existing INT fixtures (pattern).
- `crates/noctis-testkit/tests/commands.rs`: the E2E pattern.
- `xtask/rules-pending.toml` L61-99: the entries with `story = "6.6"`: INT-01 unit, INT-02 unit, INT-03 unit, INT-05 diagnosed, INT-05 unit, INT-11 unit.
- `docs/discord-rules.md` L156-166.
- Template: `responder.md` §2/§4, `commands.md` (L73-85, L611), `app-and-sharding.md` (L123, L608), `modals.md` §2 (L130-150, L432), `troubleshooting.md` (L67-68, L95, L120-121), `src/modules/profile.rs`, `AGENTS.md`, `SKILL.md` (API hash).

## Tasks & Acceptance

**Execution (in order):**

1. **`crates/noctis-core/src/error.rs`, `app.rs`, `handler.rs`**
   - Add `BuildViolation::InvalidAckDeadline { actual: Duration }` with the message "ack deadline {actual:?} is out of range: use a value in 0 ms..=2500 ms".
   - Add `AppBuilder::ack_deadline(&mut self, Duration) -> &mut Self` (doctested, last call wins, default `Duration::from_millis(2000)`). `build()` checks `ACK_DEADLINE_RANGE = Duration::ZERO..=Duration::from_millis(2500)`.
   - Add `App::ack_deadline() -> Duration`, and the field in `AppShared` and in `Debug`.
   - App tests cover Matrix 14-15.

2. **`crates/noctis-core/src/responder.rs` + `responder/transport.rs`**
   - **Markers:**
     - `pub struct OpensModal` (derives as `NoModal`).
     - `pub trait ModalMarker: sealed::Modal` (sealed, public, `#[doc(hidden)] const OPENS_MODAL: bool`), implemented for both markers. `Command<M: ModalMarker>`.
   - **Modal assertions:**
     - `#[doc(hidden)] pub trait __DeclaresOpensModal` with `#[diagnostic::on_unimplemented(message = "[INT-05] `opens_modal` is declared, so the handler must take `CommandResponder<OpensModal>`", label = "take `CommandResponder<OpensModal>` here")]`, implemented only for `Responder<Command<OpensModal>, Pending>`.
     - `#[doc(hidden)] pub trait __DeclaresNoModal`, message "[INT-05] this handler takes `CommandResponder<OpensModal>`, so its `#[command]` must declare `opens_modal`", implemented only for `Responder<Command<NoModal>, Pending>`.
     - `#[doc(hidden)] pub fn __assert_opens_modal<T: __DeclaresOpensModal>() {}` and `__assert_no_modal<T: __DeclaresNoModal>() {}`.
   - **`ResponderError` (non_exhaustive), new variants with doc + Display:**
     - `TokenExpired` ("the interaction token expired (15 minutes)");
     - `FollowupCap` ("a user-installed app may send at most 5 follow-ups here");
     - `ModalUnavailable` ("a modal can only be the first response to an interaction").
   - **`impl Responder<Command<OpensModal>, Pending>`:** `pub async fn modal(self, modal: Modal) -> Result<Responder<Command<OpensModal>, Acked>, ResponderError>`.
     - Validate with `noctis_ui::validate`, encode with `Encode` on failure, then send `{"type":9,"data":modal}` under the gate.
     - Message transport → `ModalUnavailable`; a non-Pending state → `ModalUnavailable`; detached → Ok.
     - On success, `state = MODAL` (new const 3).
     - Doc: after a modal there is no original message, so return `done()`.
   - **`Shared`:**
     - add `gate: tokio::sync::Mutex<()>`;
     - `followups: AtomicU8` and `followup_cap: Option<u8>` (Some(5) when user-install-only, see Task 3);
     - `Original` gains `ephemerality: Ephemerality` and `channel_id: Option<Id<ChannelMarker>>`.
   - **Initial responses:**
     - `reply`: prepare → lock gate → if `PENDING`, callback type 4 (with `with_response: true`; store `resource.message.id` when present) and set `REPLIED`; else `edit_original` (INT-03 runtime).
     - `defer`: lock gate → if `PENDING`, type 5; else no-op Ok.
     - `finish` becomes `reply` (gate decides).
     - Each successful interaction initial callback records AL `received_at.elapsed()`.
   - **`pub(crate) async fn auto_defer(&self) -> bool`:**
     - lock gate; if `PENDING`, send the declared-ephemerality type-5 callback;
     - on Ok set `DEFERRED` and increment AD, return true;
     - on Err leave `PENDING` and return false (the caller logs).
   - **Expiry:**
     - `InteractionTarget::expired()` = `Instant::now() >= received_at + TOKEN_LIFETIME` (`const TOKEN_LIFETIME = 15 min`).
     - When expired:
       - `edit_original`/`delete_original` and `FollowupHandle::{edit, delete}` use the bot-token channel route if the message is public, its id is known and a channel id is known; otherwise `TokenExpired`, no HTTP;
       - `followup` is always `TokenExpired`.
     - Record the original id from the `PATCH @O` response `id`, and the channel from `channel_id` (else the interaction's).
   - **Cap:**
     - In `followup`, prepare/encode first.
     - Then, if `followup_cap = Some(n)`, `fetch_update` the counter to reserve a slot: `>= n` → `FollowupCap` with no HTTP.
     - Never refund a slot.
     - `FollowupHandle` stores its `Visibility`.
   - Update the module docs (the `modal` E0599 doctest stays for `NoModal`; add a doctest for `modal` on `OpensModal`).

3. **`crates/noctis-core/src/pipeline.rs`**
   - Stage 6 in `run_interaction`:
     - `InteractionTarget.channel_id = channel.as_ref().map(|c| c.id)`.
     - Cap = `Some(5)` iff `authorizing_integration_owners.guild.is_none() && authorizing_integration_owners.user.is_some()`.
     - Unless `resolved.opens_modal()`, spawn (`tokio::spawn`, wrapped in an abort-on-drop guard held by `execute`) a task: `sleep_until(received_at + app.ack_deadline)`, then `shared.auto_defer()`. On false: `warn!(target: "noctis::dispatch", handler, "auto-defer failed")`.
     - For `opens_modal`: a task with `sleep_until(received_at + ACK_DEADLINE)` that, if `shared.state() == PENDING`, logs `error!(target: "noctis::dispatch", handler, module, "an `opens_modal` handler did not answer within the 3 s ack deadline")`.
   - `execute`'s start closure takes `Arc<Shared>` (the handler builds `Responder<Command<M>>`). Returned messages go through `Shared::reply`.
   - The Dispatcher path arms nothing. Update the module docs (stage 6 is real).

4. **`crates/noctis-core/src/command.rs`, `menu.rs`**
   - Constructors gain a modal param `M: ModalMarker`: `F: Fn(CommandCtx, [P,] CommandResponder<M>, [O]) -> Fut`, `R: IntoResponse<Command<M>>`, inserted after `P`/`O` in the generics (`__new::<P, M, F, Fut, R>`, `__new_with_options::<P, O, M, ..>`, `__new_plain::<M, ..>`, `__new_plain_with_options::<O, M, ..>`).
   - Erased call: `dyn Fn(CommandCtx, Arc<Shared>) -> HandlerFuture`. Store `opens_modal: bool = M::OPENS_MODAL`, with `pub fn opens_modal(&self) -> bool` (doc).
   - Menus keep `NoModal` and adapt to the erased `Arc<Shared>` call.
   - Seams unchanged in signature. Migrate the unit tests.

5. **`crates/noctis-macros/src/command.rs`**
   - Remove `opens_modal` from `PLANNED` and add it to `EXPECTED` (after `ephemeral`). It is a bare flag: reuse `parse_ephemeral`'s shape with message "`opens_modal` takes no value".
   - `Params` keeps the responder's `Type`. The turbofish passes `::noctis::__private::core::NoModal` for `M` when the handler takes no responder, else `_`.
   - When `opens_modal` is declared:
     - no responder parameter → `syn::Error` "[INT-05] `opens_modal` handlers take `r: CommandResponder<OpensModal>` to send the modal", spanned on the signature;
     - with one → emit `::noctis::__private::core::__assert_opens_modal::<#ty>();` spanned at `#ty`, inside the generated fn.
   - When not declared and a responder is taken → `__assert_no_modal::<#ty>()`.
   - Update the unit tests (messages, the planned list, the turbofish).

6. **`crates/noctis-core/src/{observe.rs, lib.rs}`, `crates/noctis/src/lib.rs`**
   - Catalog entries:
     - `noctis_autodefer_total` (counter, `["app"]`, "interactions the auto-defer acknowledged");
     - `noctis_ack_latency_seconds` (histogram, `["app"]`, "time from gateway receipt to the initial interaction callback").
   - Add the doc table rows.
   - Re-export `OpensModal`, `ModalMarker` and the hidden assertion items; `__private` already re-exports core. The prelude gains `OpensModal`. Update the `command` macro docs (`opens_modal`, auto-defer).

7. **Tests: `crates/noctis-core/src/pipeline/tests.rs`, `responder/tests.rs`** -- Cover every Matrix row 1-40 except 22-24.
   - Names that satisfy the catalog:
     - `int_01_auto_defer_at_deadline` (2, 3, 10);
     - `int_03_late_reply_becomes_edit_original` (4, 7);
     - `int_05_opens_modal_never_auto_defers` (17, 18 with a `Capture` of the error log);
     - `int_02_token_expired_without_http` (27-31);
     - `int_11_sixth_followup_capped` (34-36).
   - Use a `received_at` set in the past and/or `ack_deadline`, and `tokio::time::pause`/`advance` once the initial callback was recorded (5 s for row 5, 15 min for rows 25-33).
   - Assert AD/AL with the recorder pattern.

8. **`crates/noctis/tests/ui/int/`**
   - Add `INT-05_opens_modal_without_modal_responder.rs`, `INT-05_modal_responder_without_opens_modal.rs` and `INT-05_opens_modal_without_responder.rs`, with blessed `.stderr` containing `[INT-05]`.
   - Add a boundary pass in `crates/noctis/tests/commands.rs`: an `opens_modal` handler compiles, and `CommandHandler::opens_modal()` is true/false.

9. **`crates/noctis-testkit/tests/commands.rs`** -- E2E through FakeGateway + FakeDiscordHttp with `ack_deadline(50 ms)`. The handler waits 300 ms and then returns a `V2Message`. Assert `{"type":5}` CB, then `PATCH @O` flags 32768, and no type-4 CB (Matrix 2, 5).

10. **`xtask/rules-pending.toml`**: delete the six `story = "6.6"` entries.
    - **`docs/discord-rules.md`**: align the How text of INT-01 (`AppBuilder::ack_deadline`, 0-2500 ms), INT-02 (`ResponderError::TokenExpired`, bot-token fallback for public edits and deletes), INT-03 (runtime gate), INT-05 (`[INT-05]` assertion; commands landed, routes Epic 7) and INT-11 (`ResponderError::FollowupCap`).

11. **`examples/project-template/`**
    - `responder.md`: §2 becomes landed (exact rules, `ack_deadline`, 0-2500 ms, the Dispatcher has no auto-defer, metrics). §4: the `CommandResponder<OpensModal>` row and `modal(m)` signature; the runtime guards become landed; the error table gains `TokenExpired`, `FollowupCap`, `ModalUnavailable`.
    - `commands.md`: `opens_modal` landed (argument table, the unknown-argument message, the planned list at L611).
    - `app-and-sharding.md`: the `.ack_deadline` row moves to landed; add the metrics rows; drop "[planned, Story 6.6]".
    - `modals.md` §2: Commands' `r.modal` landed (Story 6.6); Routes stay Story 7.8.
    - `troubleshooting.md`: rows for `[INT-05]`, `TokenExpired`, `FollowupCap`, `ModalUnavailable`.
    - `src/modules/profile.rs`: add `#[command("bio", description = "Edit your profile bio", opens_modal, contexts = [Guild])] async fn bio(ctx: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded>` opening `BioForm::modal(SaveBio { user: ctx.user_id() })`, and register it like `profile`.
    - `AGENTS.md`: one bullet.
    - `SKILL.md`: the API hash.
    - **`xtask/public-api.txt`**: re-bless.
    - **memlog**: implementation `(decision)` lines.

**Acceptance Criteria:**
- **Auto-defer:** Given a running App (FakeGateway + FakeDiscordHttp) with a slow `#[command]` handler, when the deadline passes, then exactly one type-5 callback carrying only the declared `EPHEMERAL` flag is recorded and the handler's later message arrives as `PATCH @original` with flags 32768. `noctis_autodefer_total` and `noctis_ack_latency_seconds` are recorded.
- **Late delivery:** Given a handler that answers 5 s (virtual) after `received_at`, when it returns, then its message is delivered via edit-original and no second callback is sent.
- **Modals:** Given `#[command(.., opens_modal)]` with `CommandResponder<OpensModal>`, when dispatched, then no auto-defer is armed and `modal(..)` sends callback type 9. A missed deadline logs an error naming the handler. Each of the three mismatches fails `cargo check` with `[INT-05]`.
- **Token lifetime:** Given 15 minutes since `received_at`, when the handler edits a public response, then a bot-token channel-message edit is sent. For an ephemeral one, `TokenExpired` is returned with no request recorded.
- **Follow-up cap:** Given a user-install-only interaction, when a sixth follow-up is attempted, then `ResponderError::FollowupCap` is returned and only five follow-ups were recorded.
- **Gates:** Given the gates, when they run, then `rules-catalog` passes with the six 6.6 entries removed, and `api-snapshot` and `skill-check` pass with the Usage Skill naming every new public item.

## Spec Change Log

## Review Triage Log

## Design Notes

**Gate over CAS (decision).** One `tokio::sync::Mutex<()>` around every initial response is simpler than a CAS state machine with in-flight states. It also makes a loser *wait* until the winner's callback has completed: a `PATCH @original` sent before the defer lands would 404. The atomic stays for cheap reads (`finish`, the INT-05 log). This also closes the 6.5 two-callback race.

**Auto-defer as a spawned, aborted task (decision).** A separate task still fires when a handler blocks its worker thread. An abort-on-drop guard ties its lifetime to `execute`, so there is no tracker plumbing and nothing survives a panic or a cancel.

```rust
struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop { fn drop(&mut self) { self.0.abort(); } }
```

**Bounds (decision).** `ack_deadline` is limited to 0..=2500 ms. Above 2500 ms, a hedged type-5 callback (300 ms minimum hedge, 2.9 s edge deadline) is no longer reliable. 0 means "always defer".

**Token expiry (decision).**
- Exactly 15 min from `received_at`.
- Public messages with a known id fall back to bot-token channel routes, for edits (FR-21) and, by symmetry, deletes.
- The original's id comes from the type-4 `with_response=true` callback (`resource.message.id`) or from the last `PATCH @original` response. A deferred original that was never edited has no id, so it is `TokenExpired`.
- Bot-token edits fail where the bot is not in the guild (user installs): the result is a `Rejected` from Discord.

**Cap (decision).**
- The cap applies only when `authorizing_integration_owners` has a user owner and no guild owner.
- Slots are reserved after validation and never refunded: a rejected request may still have counted at Discord.

**Scope (decision).**
- `opens_modal` lands for `#[command]` only. Menus keep it planned, because the story names `#[command]` and Discord rarely needs it there.
- Over the Dispatcher there is no auto-defer (no deadline exists), and `modal` returns `ModalUnavailable`.
- A failed auto-defer is only logged: the handler's own reply surfaces the typed error.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, including the `int_0{1,2,3,5}_*`/`int_11_*` tests, `noctis --test ui` with the three INT-05 fixtures, and `noctis-testkit --test commands`
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: ok; copy the hash into SKILL.md (needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok
