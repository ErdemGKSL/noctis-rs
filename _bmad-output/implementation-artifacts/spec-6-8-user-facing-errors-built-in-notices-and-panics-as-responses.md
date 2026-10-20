---
title: 'Story 6.8: User-facing errors, built-in notices and panics as responses'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: '5a5afef358ddfce1e205550ab500654abf796ac8'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/i18n.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** When a Command handler returns `Err`, panics, or its options fail to decode or validate, the pipeline only reports to the Command Error Sink. The user sends nothing back, so Discord shows "The application did not respond". `noctis-i18n` is an empty crate. `HandlerError` has no user message, although the template already uses `HandlerError::user(key)`. Implements FR-22, FR-10 (validator notice), NFR-4, NFR-5; AD-13, AD-16 (`noctis.*` keys), AD-23; LOC-04.

**Approach:**
1. `noctis-i18n` gets `Builtin`, a `Translate` impl with English defaults for the reserved notice keys, plus `keys::*` consts. Core holds it as `Arc<dyn Translate>` in `AppShared`.
2. `HandlerError` gains an optional `MessageKey` and a private kind (error vs user condition).
3. After a Command or menu handler run fails, the pipeline sends one localized ephemeral notice (`Shared::notice`). It follows the AD-23 rule after an auto-defer: delete the deferred original, then send an ephemeral follow-up. Only then does it log the error and report it to the sink.

## Boundaries & Constraints

**Always:**
- **Crate edges.** `noctis-i18n` depends only on `noctis-rules`. `noctis-core` gains the unconditional `core → i18n` edge (spine graph; `deps-check` already allows both edges). The facade does not re-export anything from `noctis-i18n`, and its `i18n` feature stays empty (Epic 11).
- **Which key a notice uses:**
  - `HandlerError` with a key: that key.
  - An `OptionError` decode failure with no key: `noctis.validation.failed`.
  - Any other error, and every panic: `noctis.error.generic`.
- **Locale.** The notice is ephemeral, so the locale is User, then Guild, then `en-US`. The code is parsed with `Locale::from_str`, and an unparseable code means `en-US`.
- **Unknown key.** If `Translate` returns `None` for the key, use `noctis.error.generic`'s text and emit one `warn!` (target `noctis::i18n`, field `key`).
- **Notice body.** A V2 message with one `TextDisplay(text)`. It goes through `Shared::prepare` (the validator), so it carries `IS_COMPONENTS_V2`. Interaction notices are always `EPHEMERAL`, whatever the Command declared.
- **Order.** Send the notice before the auto-defer timer is dropped (the gate serializes the two). Then log, then report to the sink. A notice failure never panics and is never reported to a sink. It is one `warn!` (target `noctis::dispatch`, `handler`, `%error`).
- **Full logging (FR-22).**
  - Errors and panics are reported as today: `noctis_handler_errors_total` +1, then the sink.
  - When the origin (Command, or Autocomplete in `report_autocomplete`) has a custom sink, core first calls `LogSink.report(report)`, so the full error chain is logged inside the `noctis.interaction` span exactly once in both cases.
- **User conditions.** `HandlerError::user(key)` and a failed option validator (`InvalidOption`) are user conditions. They produce the notice and one `debug!` (target `noctis::command`, `handler`, `key`). They do not go to the sink and are not counted in `errors_total`.
- **Logs.** No token, user id, option value or notice text appears in logs (AD-29). No `unwrap`/`expect` on external input. No `std::sync::Mutex` guard is held across `.await`.

**Never:**
- No notice for a cancellation at shutdown (it is still reported as `Cancelled`).
- No notice for an unknown command or an authorless interaction (warn only, as today).
- No notice for autocomplete (6.7 behaviour is unchanged) or for event handlers. Event `user()` errors are reported like any other error.
- No new public setter for catalogs or translators (`AppBuilder::locales` is Epic 11). No widget keys (wizard, pager, render) in this story. No new metric. No new dependency outside the workspace.
- No debug builds. No real sleeps over 1 s in tests.

## I/O & Edge-Case Matrix

Abbreviations:
- **CB4e(t)**: one `POST …/callback` `{"type":4,"data":{"flags":32832,"components":[{"type":10,"content":t}],…}}`.
- **FUe(t)**: `POST /webhooks/{app}/{token}` with `flags` 32832 and text t.
- **DEL**: `DELETE /webhooks/{app}/{token}/messages/@original`.
- **G**: the generic text "Something went wrong. Please try again later."
- **S**: report to the Command sink.

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | Err, no key, Pending | `Err(HandlerError::new(io "boom"))` | CB4e(G), even for a public Command | S `error:boom`, errors_total +1 |
| 2 | Err with key | `HandlerError::new(e).with_message(noctis.guard.owner)` | CB4e("Only the person who started this can use it.") | S |
| 3 | User condition | `HandlerError::user(noctis.disabled_here)` | CB4e(disabled text) | no S; `debug!` with key; errors_total unchanged |
| 4 | Unknown bot key | `user(MessageKey::new("shop.missing"))` | CB4e(G) | one `noctis::i18n` warn `key=shop.missing`; no S |
| 5 | Panic at call / inside future | sync panic; panic after `yield_now` | CB4e(G) | S `panic:<msg>`; the next interaction is still served |
| 6 | Option decode error | duplicate option / i64 overflow | CB4e(validation text) | S (OptionError) |
| 7 | Validator fails | `InvalidOption` key `noctis.validation.failed` (or a bot key → G + warn) | CB4e(that text); handler not run | no S; `debug!` |
| 8 | Menu target missing | `MenuTargetError` | CB4e(G) | S |
| 9 | After auto-defer (AD-23) | handler sleeps past `ack_deadline` (paused clock), then `Err` | order: type-5 CB, DEL, FUe(G) | S |
| 10 | After explicit `defer(Public)` | handler defers, then `Err` | DEL, then FUe(G) | S |
| 11 | DEL fails | DEL answers 404 | FUe still sent | `warn!`; S for the handler error only |
| 12 | After a reply | handler `reply` OK, then `Err` | FUe(G) only, no DEL | S |
| 13 | Returned message invalid (Strict) | returns CV2-08 message | CB4e(G); the invalid message was never sent | S `error:[CV2-08]…` |
| 14 | Opens-modal answered | `modal()` sent, then `Err` | no notice request | `debug!` "no notice after a modal"; S |
| 15 | Follow-up cap | user-install interaction, 5 follow-ups used, then `Err` | no 6th request | notice `warn!` (FollowupCap); S |
| 16 | Token expired | `received_at` 16 min ago, deferred | no request | notice `warn!` (TokenExpired); S |
| 17 | Notice callback rejected | upstream 400 on the notice | no retry | `warn!`; S for the handler error only |
| 18 | Cancelled | shutdown token mid-handler | no request | S `cancelled` |
| 19 | Dispatcher, Pending | `run_command`, handler `Err` | one public reply to the invoking message with G | S |
| 20 | Dispatcher, after reply | `reply` OK, then `Err` | `POST /channels/{c}/messages` with G | S |
| 21 | Locale | user `tr`, guild `de`; user `xx-YY`; none | en-US text for all (LOC-04 fallback) | none |
| 22 | Custom sink | Command sink = Recording | Recording gets the report, and the capture has one `noctis::command` "command handler failed" error line | — |
| 23 | Default sink | no custom sink | exactly one error line (from LogSink) | — |
| 24 | `Translate` lookup | `Builtin.translate(L, k, args)` for each of the 32 locales × each key | en-US text; unknown key → `None` | — |
| 25 | Placeholders | `"Try again {retry}."` with `retry=<t:5:R>`; missing arg; extra arg; `{}`, `{a b}`, unclosed `{x`, `{{x}}`, value containing `{retry}`, `"çğ😀 {n}"` | substituted / left verbatim / ignored / verbatim ×4 / outer `{`+value+`}` / not re-substituted / unicode intact | — |
| 26 | Key set | `keys::ALL` | 9 keys, every one `is_framework()`, every one has en-US text, no duplicates | — |
| 27 | `HandlerError` API | `user(k).user_message()`; `new(e).user_message()`; `new(e).with_message(k)`; `user(a).with_message(b)` | `Some(k)` / `None` / `Some(k)` with Display = e / `Some(b)` and still a user condition | — |

</intent-contract>

## Code Map

- `crates/noctis-i18n/{Cargo.toml,src/lib.rs}`: currently only a doc comment. Add `noctis-rules.workspace = true`.
- `crates/noctis-rules/src/key.rs`: `MessageKey` (const `new`, `is_framework`) and `trait Translate`. Its doc contract: locale → `sibling()` → default; `None` only when no step has the key.
- `crates/noctis-rules/src/locale.rs`: `Locale` (32 codes, `FromStr`, `sibling`, `EnUs`).
- `crates/noctis-core/src/handler.rs` L36-93: `HandlerError(Box<dyn Error>)`. L97-115: `AppShared` (add `strings: Arc<dyn Translate>`). Build it in `app.rs`; grep `ack_deadline:` for the construction site.
- `crates/noctis-core/src/command.rs`:
  - L584 `decode`: the validator `map_err(HandlerError::new)` becomes a user condition carrying `invalid.key()`; keep the inner `InvalidOption` so the existing downcast tests (`command.rs` L934, `crates/noctis/tests/options.rs` L473) still pass.
  - L200 `response_locale` is the visibility logic to mirror.
- `crates/noctis-core/src/pipeline.rs`:
  - `execute` L245-337: the `Outcome` match and the `drop(timer)` at L313 are where the notice goes.
  - `run_interaction` L368 and `Dispatcher::run_command` L1000 build the `Report`.
  - `report_autocomplete` L813: add the custom-sink `LogSink` call.
  - `CtxInner` locales: thread `user_locale`/`guild_locale` (owned `Option<String>`) into `execute`, e.g. via a new `Report` field `locale: Locale` computed up front.
- `crates/noctis-core/src/responder.rs`:
  - `Shared` L708: state consts L671; `prepare`, `send_defer`, `followup` (cap slot, expiry check), `delete_original`, `expired_route`.
  - Add `pub(crate) async fn notice(self: &Arc<Self>, text: String) -> Result<(), ResponderError>`.
- `crates/noctis-core/src/responder/transport.rs`: `InteractionTarget::callback`, `Transport::{followup, delete_original}`, `MessageTarget::reply`.
- `crates/noctis-core/src/sink.rs`: `LogSink` and `ErrorSinks::is_custom`.
- Tests:
  - `crates/noctis-core/src/pipeline/tests.rs`: `Pipeline` harness L46-109; `errors_and_panics_reach_the_sink` L461 asserts `requests().len() == 1` (becomes 5); the menu test near L420 asserts 3; `malformed_and_authorless` L428 asserts `requests().is_empty()`; the INT-01/INT-11 tests from L976 are patterns for paused time and the cap. `test_support::Capture` captures logs.
  - `crates/noctis-testkit/tests/commands.rs`: `Running`, `invoke` L197, `wait_sent` L223, `wait_report` L235.
- Facade:
  - `crates/noctis/src/lib.rs` L15 and L304 (crate docs), L336 (Error Sinks section), L396 (re-exports, unchanged).
  - `xtask/public-api.txt` L546-555 `HandlerError`.
- Template:
  - `examples/project-template/.claude/skills/noctis/references/`:
    - `responder.md` §5 L270-293;
    - `modules-middleware-ratelimits.md` L168-171;
    - `commands.md` L431-433;
    - `i18n.md` §6 L125-141;
    - `troubleshooting.md`;
  - `SKILL.md` (API hash L21, row L278);
  - `examples/project-template/AGENTS.md`.
  - The template `src/` already calls `HandlerError::user(..)`.
- `docs/discord-rules.md` L124 (LOC-04 How cell).

## Tasks & Acceptance

**Execution (in order):**

1. **`crates/noctis-i18n`** (`Cargo.toml` + `src/lib.rs`), documented with doctests:
   - `pub mod keys` holds `pub const` `MessageKey`s plus `pub const ALL: [MessageKey; 9]`. Keys and en-US texts:
     - `ERROR_GENERIC` `noctis.error.generic`: "Something went wrong. Please try again later."
     - `VALIDATION_FAILED` `noctis.validation.failed`: "That input isn't valid. Check it and try again."
     - `EXPIRED` `noctis.expired`: "This interaction has expired. Run the command again."
     - `STALE` `noctis.stale`: "This message is out of date. Run the command again."
     - `DISABLED_HERE` `noctis.disabled_here`: "This feature is disabled here."
     - `RATE_LIMITED` `noctis.rate_limited`: "You're doing that too often. Try again {retry}."
     - `GUARD_FAILED` `noctis.guard.failed`: "You can't use this."
     - `GUARD_OWNER` `noctis.guard.owner`: "Only the person who started this can use it."
     - `GUARD_PERMISSIONS` `noctis.guard.permissions`: "You don't have permission to use this."
   - `#[derive(Debug, Clone, Copy, Default)] pub struct Builtin;` with `impl Translate`. The table is a `match` on `key.as_str()` for `EnUs`. The lookup is locale → sibling → `EnUs`, so every locale resolves to English today (LOC-04).
   - `pub fn format(template: &str, args: &[(&str, &dyn Display)]) -> String`: single pass. `{name}`, with `name` 1+ chars of `[A-Za-z0-9_]` and present in `args`, is replaced. Anything else is copied verbatim (Matrix 25).
   - Unit tests for Matrix 24-26, including `loc_04_every_locale_falls_back_to_en_us`.

2. **`crates/noctis-core/Cargo.toml`**: add `noctis-i18n.workspace = true`.
   - **`handler.rs`**:
     - `HandlerError` becomes `{ error: Box<dyn Error + Send + Sync>, key: Option<MessageKey>, user: bool }`. `new`/`From` set `None, false`.
     - Add `pub fn user(key: MessageKey) -> Self` (inner is a private `UserCondition(MessageKey)` error, Display ``user notice `<key>` ``).
     - Add `#[must_use] pub fn with_message(self, key: MessageKey) -> Self` and `pub fn user_message(&self) -> Option<MessageKey>`.
     - Add `pub(crate) fn user_condition(error, key)` and `pub(crate) fn is_user(&self)`.
     - Display/Debug/`as_error` are unchanged. Doctests for every new method (Matrix 27).
     - `AppShared` gets `strings: Arc<dyn Translate>` (= `Arc::new(noctis_i18n::Builtin)`), set where `AppShared` is built.
   - **`command.rs` `decode`**: a validator failure becomes `HandlerError::user_condition(invalid, key)`.

3. **`crates/noctis-core/src/notice.rs`** (new, `pub(crate)`; `mod notice;`):
   - `fn locale(user: Option<&str>, guild: Option<&str>) -> Locale` implements the ephemeral order with the `en-US` fallback.
   - `fn text(strings: &dyn Translate, locale, key) -> String` follows Matrix 4 (warn, then generic). As a last resort it uses the `ERROR_GENERIC` en-US text from `noctis_i18n::Builtin`.
   - `fn key_for(error: &HandlerError) -> MessageKey` follows the key rule in **Always**.
   - Unit tests for each function.

4. **`responder.rs` `Shared::notice(self: &Arc<Self>, text)`**: build `V2Message::new(Components::new().push(TextDisplay::new(text)))` and `prepare` it. Then take the gate and branch on `state()`:
   - **`PENDING`**: on the interaction transport, a type-4 callback with `EPHEMERAL` (`with_response=false`), then store `REPLIED`. On the message transport, `MessageTarget::reply`. Detached sends nothing.
   - **`DEFERRED`**:
     1. drop the gate;
     2. on the interaction transport, `delete_original` (an error is a `warn!` and the follow-up is still sent);
     3. send the follow-up through the existing `followup` with `Ephemeral` (cap and expiry apply). On the message transport, post as a plain follow-up reply instead.
   - **`REPLIED`**: drop the gate, then send the `followup` (`Ephemeral` on interactions, public on messages).
   - **`MODAL`**: send nothing and log `debug!`.
   - Unit tests in `responder/tests.rs` for each state.

5. **`pipeline.rs` `execute`**:
   - Pass the notice locale and handler name in.
   - Compute the outcome. For `Returned(Err(e))` or `Panicked`:
     - send `shared.notice(text)` (a notice error gets a `warn!`);
     - then `drop(timer)`;
     - then, if `e.is_user()`, emit the `debug!` and return;
     - else count the error, call `LogSink.report(r)` first when `sinks.is_custom(Command)`, then `sinks.report`.
   - `Cancelled`: drop the timer and report as today.
   - `report_autocomplete` gets the same custom-sink `LogSink` call.
   - Update the module docs (stage 10: "send a localized ephemeral notice, then route to the Error Sink") and the crate docs in `noctis-core/src/lib.rs` (L60-90, L336-342).

6. **Tests**:
   - `crates/noctis-core/src/pipeline/tests.rs`: Matrix 1-23, with paused time for 9 and 16 and `Capture` for 4, 22 and 23. Fix the existing request-count assertions (L461 test → 5 requests; L420 menu test; `malformed_and_authorless`). Name the AD-23 test `notice_after_auto_defer_deletes_original_then_follows_up`.
   - `crates/noctis-testkit/tests/commands.rs`, end to end through FakeGateway and FakeDiscordHttp:
     - a `#[command]` returning `Err(HandlerError::new(..))` → callback type 4 with flags 32832 and the generic text, plus a sink report;
     - a panicking `#[command]` → the same, and a following `ping` is still answered (NFR-4);
     - a slow erroring command with `ack_deadline(100ms)` → type 5, DEL, ephemeral follow-up (AC 4);
     - a validator failure via `#[derive(Options)]` → notice, no handler run, no report.
   - `crates/noctis/tests/options.rs`: assert `invalid(..)`'s error has `user_message() == Some(key)`.

7. **Facade + snapshot**:
   - `crates/noctis/src/lib.rs` docs: an "Errors shown to users" paragraph with an example using `HandlerError::user` and `with_message`.
   - Re-bless with `cargo run --release -p xtask -- api-snapshot --bless`.

8. **`docs/discord-rules.md` L124**: the LOC-04 note gains "built-in `noctis.*` strings (`noctis-i18n` `Builtin`) ship en-US; every locale falls back to it". Keep the enforcement cell `(permitted)`.

9. **`examples/project-template/`**:
   - `responder.md` §5: drop `[planned, Story 6.8]` and document the real routing (Matrix 1-3, 9, 12, 14, 18, 19), `user_message()`, the custom-sink double log and the bot-key fallback until Epic 11.
   - `modules-middleware-ratelimits.md` L171: `user`/`with_message`/`user_message` are landed; user conditions skip the sink.
   - `commands.md` L433: the validator notice is landed.
   - `i18n.md` §6:
     - add the `noctis.guard.failed` and `noctis.validation.failed` rows;
     - mark the rows not shipped yet (wizard/pager/view/render) `[planned]`;
     - "built-in defaults in many languages" becomes "English defaults; other locales fall back to them";
     - overriding through locale files is `[planned, Epic 11]`.
   - `troubleshooting.md`: rows for "users see 'Something went wrong'" (look in the Command sink/log) and for the `noctis::i18n` "missing message key" warn.
   - `AGENTS.md`: one bullet.
   - `SKILL.md`: the API hash from the re-bless.
   - Add `(decision)` lines to `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` for implementation choices.

**Acceptance Criteria:**
- **Strings:** Given `noctis-i18n`, when it is built, then `Builtin` implements `Translate` with English text for every `keys::ALL` entry (error, validation failed, expired, stale, disabled here, rate limited, guard failed/owner/permissions), and core renders notices only through `AppShared.strings: Arc<dyn Translate>`.
- **Errors:** Given a `#[command]` handler returning `Err(HandlerError)` with or without `with_message(key)`, when it runs through the FakeGateway, then exactly one ephemeral type-4 callback carries that key's text (else `noctis.error.generic`), the error chain is logged once in the `noctis.interaction` span, and the Command Error Sink receives the report afterwards.
- **Panics:** Given a handler that panics, when it is dispatched, then the user gets the generic notice, the sink gets `ReportKind::Panic`, and the next interaction on the same shard is answered.
- **After auto-defer:** Given a handler that fails after the auto-defer fired, when the notice is sent, then the deferred original is deleted and the notice is an ephemeral follow-up (AD-23).
- **Validator:** Given an option validator returning a key, when the Command is invoked, then the handler does not run and the user gets that key's ephemeral notice.
- **Docs:** Given the new public API, when the gates run, then the Usage Skill documents `HandlerError` (`new`, `user`, `with_message`, `user_message`, `as_error`), `HandlerResult` and the error-routing patterns, and `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 28 findings — high 0, medium 1, low 13, false 14, maybe-false 0
- findings:
  - `[low]` `[reject]` Blind: `send_notice` is not raced against the shutdown cancel token — the shutdown sequence waits at most 1 s after cancelling and the notice is one bounded HTTP request; rare and the fix adds a select branch.
  - `[low]` `[reject]` Blind: PENDING notice for an `opens_modal` Command failing after 3 s gets a rejected callback — same as any late callback today (one warn); rare, and the fix adds an expiry guard.
  - `[false]` `[reject]` Blind: PENDING notice stores `message_id: None`, later edits would misfire — nothing runs on this `Shared` after the notice; the outcome does not occur.
  - `[false]` `[reject]` Blind: decode failures map to `noctis.validation.failed` and also reach the sink — the spec deliberately settled this key rule (Always, Matrix 6) and decode failures are bugs worth reporting.
  - `[false]` `[reject]` Blind: custom sinks that log now double-log; Event/Component sinks do not pre-log — the spec decided `LogSink`-first for Command/Autocomplete only; FR-22's "by default" logging is met.
  - `[low]` `[patch]` Blind: `notice_respects_cap_and_expiry` warn assertions are loose (any "follow-up", second warn unchecked) — patch: assert the FollowupCap and TokenExpired error texts.
  - `[low]` `[reject]` Blind: pipeline locale selection not verified end to end (all locales render en-US) — code at `pipeline.rs` passes `notice::locale(user, guild)`, unit-tested; a stub-Translate harness is more than a direct correction and no user-visible defect exists today.
  - `[false]` `[reject]` Blind: `Report.locale = EnUs` for autocomplete is meaningless — autocomplete sends no notice (spec Never); no bad outcome.
  - `[false]` `[reject]` Blind: `user_condition` is `pub(crate)`; debug log omits the inner error — spec settled both (no new public surface; debug fields `handler`, `key`; AD-29 keeps option values out of logs).
  - `[low]` `[patch]` Blind: `with_message` doc reads as if user conditions reach the sink — patch: reword the doc sentence.
  - `[low]` `[patch]` Blind: facade "Errors shown to users" example uses bot keys that render generic text and warn until Epic 11 — patch: add a comment in the example.
  - `[false]` `[reject]` Blind: event `user()` errors reach the sink with the synthetic Display — spec Never settles "Event `user()` errors are reported like any other error", and Task 2 fixes that Display.
  - `[false]` `[reject]` Blind: spec frontmatter `status: in-progress` is stale — the workflow sets status at review and finalize.
  - `[medium]` `[patch]` Edge: DEFERRED notice deletes the original even after `edit_original` filled it (`edit_original` sets `Shape::Sent`, state stays `DEFERRED`), deleting the handler's delivered response — patch: delete only while `original().shape` is `Shape::Deferred`, plus a test.
  - `[low]` `[reject]` Edge: notice not raced against cancel — same as the first Blind row (bounded by the 1 s post-cancel wait).
  - `[low]` `[patch]` Edge: user-condition debug says "notice sent" even when the notice failed or was skipped — patch: reword the message.
  - `[low]` `[reject]` Edge: a custom sink set to `LogSink` itself logs twice — unlikely configuration and the fix needs a type check.
  - `[low]` `[patch]` Gap: autocomplete `LogSink`-before-custom-sink is untested — patch: autocomplete twin of `the_error_is_logged_once`.
  - `[low]` `[patch]` Gap: `user(a).with_message(b)` staying a user condition is unpinned — patch: add a routing test case.
  - `[false]` `[reject]` Intent D1: user conditions skip the sink despite "with or without a key" — spec Design Notes and memlog decision settle it; keyed `new(..).with_message(..)` errors still reach the sink.
  - `[false]` `[reject]` Intent D2: bot keys render generic text before Epic 11 — spec settled (no catalog setter until Epic 11, Matrix 4); reserved-key advice does not invent new `noctis.*` keys.
  - `[false]` `[reject]` Intent D3: delete-then-follow-up order — spec Design Notes settle the order as an allowed AD-23 reading.
  - `[false]` `[reject]` Intent D4: expired/disabled/rate-limit/guard notices exist as strings only — AC1 requires the strings; emission belongs to Epics 7/13.
  - `[false]` `[reject]` Intent D5: validator key vs fallback indistinguishable — the pipeline test with a bot validator key renders generic text, not validation text, so the validator's key is proven used; routing difference is spec-settled.
  - `[false]` `[reject]` Intent D6: Dispatcher notices are public — spec Design Notes decision; message transports have no ephemeral messages.
  - `[low]` `[patch]` Intent D7: no test asserts a panic is logged — patch: add a panic case asserting one `ERROR` `noctis::command` line.
  - `[false]` `[reject]` Intent D8: full logging only wired for Command/Autocomplete — FR-22 covers default logging; the spec scopes the pre-log to these origins and only Command sends notices.
  - `[low]` `[patch]` Intent D9: `i18n.md` §6 tags the implemented en-US fallback `[planned, Epic 11]` (skill-check evidence part is a verification step, not a diff defect) — patch: move the tag to the override sentence.

## Design Notes

**User conditions skip the sink (decision).**
- `HandlerError::user(key)` is a known outcome, such as "guild only", not a bug. The template already documents it as "logged at debug".
- A failed validator is user input (FR-10).
- Sending these to the sink would page on-call for normal use. Errors that carry a message (`with_message`) still go to the sink, which satisfies the AC "with or without a key".

**Delete before follow-up (decision).** AD-23 names both actions but not their order. Deleting the deferred loading message first means the follow-up is always a new message with its own `EPHEMERAL` flag. Sending the follow-up first could, if Discord fills the loading message with the first follow-up, leave a public message or delete the notice itself. The delete-first order is safe either way.

**Dispatcher notices are public (decision).** A message transport has no ephemeral messages. A prefix user would otherwise see nothing, and the generic text leaks nothing.

**Logged once (decision).** `LogSink` already logs inside the interaction span. A custom sink replaces it, so core calls `LogSink` itself first. The full error is then always in the logs, as FR-22 requires.

**Bot keys before Epic 11.** Only `Builtin` exists until Epic 11, so a bot key renders the generic text with a warn. Tests that observe "the given key" therefore use reserved keys such as `noctis.guard.owner`.

```rust
let guild = ctx.guild_id().ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))?;
db.save(x).await.map_err(|e| HandlerError::new(e).with_message(keys::SAVE_FAILED))?;
```

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, including `noctis-i18n` unit tests, `notice_after_auto_defer_deletes_original_then_follows_up` and `noctis-testkit --test commands`
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok (`core → i18n`, `i18n → rules`)
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: ok; copy the hash into SKILL.md (needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok

## Auto Run Result

- **Summary:** Handler errors, validator failures and panics now become user-facing ephemeral notices (Command origin) rendered through `noctis-i18n` built-in en-US strings with a generic fallback; `HandlerError::user(key)` conditions log at debug and skip the sink, other errors are logged once by `LogSink` before any custom sink (Command/Autocomplete); notices after auto-defer delete the deferred original then follow up; Dispatcher (message transport) notices are public; built-in strings for expired/disabled/rate-limit/guard notices are present for Epics 7/13.
- **Files changed:**
  - `crates/noctis-i18n/src/lib.rs`, `Cargo.toml` — built-in en-US notice catalog and key resolution.
  - `crates/noctis-core/src/notice.rs` — notice composition and locale selection.
  - `crates/noctis-core/src/handler.rs` — `HandlerError::user`/`with_message` routing, user-condition classification.
  - `crates/noctis-core/src/pipeline.rs`, `responder.rs` — error/panic to notice flow, LogSink-first reporting, defer-aware delete+follow-up.
  - `crates/noctis-core/src/{app,command,lib}.rs`, `Cargo.toml` — wiring and core → i18n dependency.
  - `crates/noctis-core/src/pipeline/tests.rs`, `responder/tests.rs`, `crates/noctis-testkit/tests/commands.rs`, `crates/noctis/tests/options.rs` — tests.
  - `crates/noctis/src/lib.rs` — facade docs "Errors shown to users".
  - `examples/project-template/` (AGENTS.md, Usage Skill references, API hash), `xtask/public-api.txt`, `docs/discord-rules.md`, architecture memlog — docs, snapshot, decisions.
- **Review findings:** 28 total (high 0, medium 1, low 13, false 14).
  - Patched (9; medium 1, low 8): DEFERRED notice deleted an original already filled by `edit_original` (now deletes only while shape is `Deferred`, with test); tightened cap/expiry warn assertions; `with_message` doc wording; facade example comment on bot keys before Epic 11; user-condition debug message wording; autocomplete LogSink-first test; `user(a).with_message(b)` routing test; panic-is-logged test; `i18n.md` §6 planned tag moved.
  - Deferred: none.
  - Rejected (19): notice not raced against shutdown cancel (x2, bounded by 1 s wait); late PENDING callback for modal commands (rare, one warn); custom sink equal to `LogSink` double-logs (unlikely config); end-to-end locale harness (unit-tested, no defect); 14 false/spec-settled findings (decode failures to sink, LogSink-first scope, `pub(crate)` surface, bot-key generic text, delete-then-follow-up order, public Dispatcher notices, strings-only notices for Epics 7/13, stale status, etc.).
- **Follow-up review recommended:** false — first pass patched 0 high and 1 medium (below the two-medium threshold).
- **Verification:** all local gates via `gates.sh` green (fmt, clippy -D warnings, workspace tests, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91 check). CI paused by maintainer.
- **Residual risks:** bot-defined message keys render generic text with a warn until Epic 11 adds a catalog setter; notice delivery is not raced against shutdown cancel; non-en-US locale rendering is not exercised end to end.
