---
title: 'Story 8.1: Responder ergonomics: ctx.respond, ctx.update and ctx.reply_text'
type: 'feature'
created: '2026-10-21'
status: 'in-progress'
baseline_revision: '810880385b71d05135d80f65f7c87d0c09d34b7c'
baseline_revision: 'e1cebc2f36bf3639f0553b373ddb1d3f891abf8e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/async-and-concurrency.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Handlers can answer only by returning a message or by taking a typestate `Responder` and choosing the flow (`reply`, `update`, `defer_*`, `follow_up`). The Usage Skill, the template and the spine already present `ctx.respond(msg)`, `ctx.update(msg)` and `ctx.reply_text(text)` as the default way to answer, marked `[planned, Story 8.1]`, but no context has them.

**Approach:**
- Bind every interaction context to its run's responder state (`Arc<Shared>`).
- Add the three methods. Each picks the callback type, deferral, flags and ephemerality from the declared `response` and the runtime state machine, through the same gate the typestate and the auto-defer use.
- Add a `handler_answered` flag so that a second initial response made through either API returns `ResponderError::AlreadyResponded` without a request.
- Reconcile the template, the facade's template-copy test, the Usage Skill, `docs/discord-rules.md` and the API snapshot.

## Boundaries & Constraints

**Always:**
- Every send goes through `Shared::prepare`: Ref minting, the select nonce, signing, the Limits validator (Strict rejects, Lenient truncates), `IS_COMPONENTS_V2` on V2 bodies, `allowed_mentions` with `parse: []` (MSG-05) and `attachments: []` on edit bodies. Interaction requests carry no bot `Authorization` header and never send `with_components` (INT-13).
- Every state decision is made under `Shared::gate`. The handler, the auto-defer and the notice still send at most one callback (INT-03).
- `ctx.update` exists only on `ComponentCtx`, `ModalSubmitCtx` and `ExpiredCtx`. `CommandCtx` has no `update` (E0599, INT-04).
- The three methods return `HandlerResult<Responded>`. A `ResponderError` becomes the `HandlerError` (`HandlerError::new`), so `err.as_error().downcast_ref::<ResponderError>()` finds it.
- The typestate `Responder`, returned messages (`IntoResponse`), `Shared::finish`, `Shared::notice` and `Shared::auto_defer` keep their shipped wire behaviour. The only change is the `AlreadyResponded` refusal.
- Release-only builds. trybuild stays `compile_fail` only.

**Never:**
- No `_with_response` variants of the ctx methods.
- No new rule ids or Limits.
- No attachment support (Story 9.1).
- No `Ctx` trait. No `ctx.limits()`, `ctx.cache()`, `ctx.render()` or `ctx.modules()`; they stay `[planned]`.
- No change to the auto-defer deadline.
- No automatic escaping of user text (Story 8.2).
- `ctx.update` never sends a new message when there is no source message: no type-4 fallback.
- No `unwrap`/`expect` outside tests. No new dependencies.

## I/O & Edge-Case Matrix

Notation:
- **Answered** means that `Shared::handler_answered` is `true`.
- **Deferred5** means state `DEFERRED` with `original.shape == Shape::Deferred`: an unedited type-5 loading message.
- **Declared vis** is `Shared::default_ephemerality()`: the declared `e`, or `Public` under `update`.
- CB is `POST …/interactions/100/tok/callback`, ORIG is `…/webhooks/7/tok/messages/@original` and FU is `POST …/webhooks/7/tok?wait=true`.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | respond, pending | Command `response = public`, PENDING | One CB `{"type":4,"data":{..,"flags":32768,"allowed_mentions":{"parse":[]}}}`. State REPLIED, answered. Returns `Ok(Responded)`. | — |
| 2 | respond, ephemeral command | `response = ephemeral`, PENDING | Type 4 with `flags` 32832 (64\|32768). | — |
| 3 | respond after a Command's auto-defer | Deferred5 (public or ephemeral), not answered | One `PATCH ORIG` (V2 edit body). No callback. The ephemerality stays the defer's (INT-10, INT-12). Sets answered. | — |
| 4 | respond after the handler's own `defer_reply()` | Deferred5, answered | `PATCH ORIG` (the edit is the completion of the loading message). | — |
| 5 | respond after a type-6 defer | DEFERRED_UPDATE (auto or own) | One FU with the declared vis's flags. The clicked message is untouched. | INT-11 cap → `FollowupCap`; after 15 min → `TokenExpired` (no request) |
| 6 | respond once answered | REPLIED, or DEFERRED with the original already edited | FU with the declared vis. A second respond is another FU. | as row 5 |
| 7 | respond on a component, pending | `response = update` / `ephemeral` / `public` | Type 4 with Public / Ephemeral / Public (the declared vis). Never type 7. | — |
| 8 | respond, slash-opened submit | ModalSubmit, no source, PENDING | Type 4 with the declared vis (Public under `update`). | — |
| 9 | respond after a modal or a stop | state MODAL or STOPPED | No request. | `AlreadyResponded` |
| 10 | respond over the Dispatcher (prefix) | message transport PENDING / typing-deferred | A message reply (`POST /channels/5/messages`) / the existing `edit_prepared` post of a deferred reply. | An ephemeral declared vis → `EphemeralUnsupported`, as `reply` |
| 11 | respond, invalid message | 41 components, or text > 4000 (`discord_len`), Strict | No request. Answered is not set. | `Invalid(UiError)` |
| 12 | respond, Lenient over-long text | 4001-char text, Lenient | Sent, truncated to 4000 with `…`. | — |
| 13 | respond, Unicode boundary | 2000 × "😀" (4000 UTF-16 units), Strict | Sent. 2001 × "😀" → `Invalid`. | — |
| 14 | respond, legacy message | `LegacyMessage`, PENDING / Deferred5 | Type 4 legacy body / `PATCH ORIG` legacy body (the deferred original takes the message's format). | — |
| 15 | update, pending | Component with a V2 source, PENDING | One CB `{"type":7,"data":{..flags 32768, no 64}}`. Answered. | — |
| 16 | update, legacy source | source format Legacy | Type 7 data has `content: null`, `embeds: []` (CV2-02). | — |
| 17 | update after the type-6 auto-defer | DEFERRED_UPDATE, not answered | One `PATCH ORIG` (= the clicked message). Answered. | — |
| 18 | update after a type-5 auto-defer | `response = ephemeral\|public`, Deferred5, not answered | One `PATCH ORIG` (the deferred reply, exactly like the typestate `update`). | — |
| 19 | update, slash-opened submit | ModalSubmit, no source, PENDING | **No request.** State stays PENDING; answered is not set. | `UpdateUnavailable` |
| 20 | update, slash-opened submit after the auto-defer | no source, DEFERRED (type 5 public) | **No request.** | `UpdateUnavailable` |
| 21 | update once answered | answered (by any ctx method or typestate Pending method), any state | No request. | `AlreadyResponded` |
| 22 | update after a modal | MODAL | No request. | `AlreadyResponded` |
| 23 | update on `CommandCtx` | compile | E0599 (`compile_fail,E0599` doctest carrying `// [INT-04]`). | compile error |
| 24 | update, legacy body | `ctx.update(LegacyMessage)` on a component | Does not compile (`IntoEdit`, CV2-01, E0277). | compile error |
| 25 | reply_text, pending | `ctx.reply_text("Saved.")` | One CB type 4 with `data.components == [{"type":10,"id":1,"content":"Saved."}]`, flags 32832, `allowed_mentions {"parse":[]}`. | — |
| 26 | reply_text public | `.public()`, PENDING | Type 4, flags 32768. | — |
| 27 | reply_text, Deferred5, same visibility | ephemeral command auto-deferred, default ephemeral | `PATCH ORIG` with the text (INT-12). | — |
| 28 | reply_text, Deferred5, other visibility | public command auto-deferred, default ephemeral | `DELETE ORIG`, then an ephemeral FU. A failed delete is one `warn` on `noctis::dispatch`, and the FU is still sent. | FU errors as row 5 |
| 29 | reply_text after type 6 or once answered | DEFERRED_UPDATE / REPLIED | An FU of the chosen visibility (ephemeral by default). | as row 5 |
| 30 | reply_text after a modal or a stop | MODAL / STOPPED | No request. | `AlreadyResponded` |
| 31 | reply_text over the Dispatcher | message transport | Always public: a message reply (PENDING) or a channel post (after). | — |
| 32 | reply_text with a `MessageKey` | key in the catalog; user `tr`, guild `de` | Ephemeral: the `tr` text. `.public()`: the `de` text. Neither locale known, or an unparsable code: `en-US`. | — |
| 33 | reply_text, missing key | key in no catalog | The `noctis.error.generic` text, one `warn` on target `noctis::i18n` (`notice::text`). | — |
| 34 | reply_text, text inputs | `String`, `&'static str`, `MessageKey` | All compile through `impl Into<NoticeText>`. `u32` → E0277. | compile error |
| 35 | reply_text, empty or whitespace | `""`, `"  "` | Sent as is. No client rule exists (MSG-09 only needs ≥ 1 component). Discord's answer is surfaced. | A 4xx → `Rejected` |
| 36 | reply_text, length | 4000 chars Strict → sent; 4001 Strict → no request; 4001 Lenient → truncated | as rows 11-13 | `Invalid` |
| 37 | reply_text, mentions | `"<@1> @everyone <@&2>"` | Text unchanged, `allowed_mentions {"parse":[]}`: nobody pinged (MSG-05). | — |
| 38 | ctx then typestate | `ctx.respond`/`update`/`reply_text` took the initial slot, then any `Responder<_, Pending, _>` method (`reply`, `reply_as`, `*_with_response`, `defer_reply(_as)`, `update`, `defer_update`, `show_modal`) | No request (checked before `prepare` and again under the gate). | `AlreadyResponded` |
| 39 | typestate then ctx | `r.reply(..)` (or `defer_update()`), then `ctx.update(..)` | No request. | `AlreadyResponded` |
| 40 | typestate then respond | `r.reply(..)`, then `ctx.respond(..)` / `reply_text(..)` | An FU (not an error). | as row 5 |
| 41 | auto-defer alone | the auto-defer fired, the handler has not answered | `handler_answered` stays false: the typestate `reply`/`update` keep the shipped late behaviour (an edit or an FU). | — |
| 42 | the auto-defer races ctx | `ack_deadline(0)` and `ctx.respond` | Exactly one callback. Then rows 3/5 decide. | — |
| 43 | detached seams | `__testing::call_command/_component/_modal` with ctx methods | No request. Validation and encoding run. `ctx.update` succeeds on the placeholder source. | `Invalid` / `Encode` as in production |
| 44 | an unbound context | `CtxInner.shared == None` (hand-built in crate unit tests only) | No request. | `NoReply` |
| 45 | `response_locale` on component contexts | declared `Reply(e)` → e's order; `Update` with an ephemeral source → user, guild; public or no source → guild, user | `"en-US"` fallback | — |

</intent-contract>

## Code Map

- `crates/noctis-core/src/command.rs`:
  - L64-81: the `CommandCtx` docs. The new `compile_fail,E0599` `[INT-04]` doctest goes here.
  - L84-106: `CtxInner`. Add `shared: Option<Arc<Shared>>`.
  - L118-136: `detached_parts`. Set `shared: None`.
  - L148-163: `from_parts`/`detached`.
  - L246-251: `response_locale`, the model for the component contexts.
  - L663-676: `run_detached`. Bind the ctx to this `shared`, see `call_command` just below it.
  - L848: a unit-test construction.
- `crates/noctis-core/src/component.rs`:
  - L68-140: `ctx_accessors!`. Add `response_locale` here or next to it. L136 is `detached()`.
  - L172/188/235: `ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx`.
  - L411: `declared()`.
  - L1131-1180: `run_detached`, `detached(response)` (L1147: add the placeholder source), `call_component`, `call_modal`. They must hand the ctx the same `shared`.
  - L1383: a test construction.
- `crates/noctis-core/src/menu.rs` L125-170 (the `call` closures) and L377 (the detached menu seam).
- `crates/noctis-core/src/pipeline.rs`:
  - L776-814: `start` builds `CommandCtx` inside `move |r|`. Set `shared: Some(Arc::clone(&r))`.
  - L878-901: the parts and `Shared::with_followup_cap`.
  - L1515-1545: the Dispatcher parts and `Shared::new`. Bind the ctx in the `start` closure.
- `crates/noctis-core/src/pipeline/component.rs`:
  - L586-600: `shared_for`.
  - L676-694: `parts`, then `Arc::new(parts)`, then `state`. Reorder so that `state` exists first and `parts.shared = Some(Arc::clone(&state))`. Set `parts.ephemerality` for `response_locale` (row 45).
  - The Expired chain reuses `parts` and `state`. Verify it with a grep of `from_inner(`.
- `crates/noctis-core/src/responder.rs`:
  - L333-421: `ResponderError`. Add `AlreadyResponded`.
  - L713-824: the Pending methods. L826-903: `update_methods!`. L905-945: `show_modal!`.
  - L1220-1330: `Shared` and its builders. Add `handler_answered: AtomicBool`, initialised to false.
  - Internals: L1507 `finish` (unchanged), L1534 `reply`, L1586 `defer_reply`, L1640 `update`, L1690 `defer_update`, L1754 `show_modal`. Add the answered check and set to the last five.
  - L1914 `notice`: the delete-then-follow-up pattern for row 28. L1977 `stop_silently`.
  - Read-only helpers: `send_reply`, `send_update`, `edit_prepared`, `send_follow_up`, `delete_reply`, `default_ephemerality`, `original()`.
- `crates/noctis-core/src/notice.rs` L12-30: `locale`, `text` (reuse them for `MessageKey`). `handler.rs` L228: `AppShared::strings`.
- `crates/noctis-core/src/lib.rs`: the crate re-exports (add `NoticeText`, `ReplyText`); L286+ is `__testing`.
- `crates/noctis/src/lib.rs` L563: the facade re-export list (add `NoticeText`, `ReplyText`). L1820: the prelude (unchanged: the methods need no import).
- Test patterns:
  - `crates/noctis-core/src/responder/tests.rs`: `Fixture` (L44-180), the route constants L184-196, `discord()`.
  - `responder/tests/components.rs`: `source()` L26, `update_unavailable_without_a_source` L371, the `notice_*` tests L1064-1215 (in `tests.rs`).
  - Child modules are declared at L1513-1514.
- E2E patterns:
  - `crates/noctis-testkit/tests/commands.rs`: `Running` L192-330, `invoke`, `wait_sent`, `body`.
  - `crates/noctis-testkit/tests/components.rs`: `click`, `source_message`, `assert_interaction_request`.
  - `crates/noctis-testkit/tests/modals.rs`: `slash_opened_modal_flow`.
  - `crates/noctis-testkit/tests/publish.rs` L200-260: `fake`, `serve`, `app_with`, `put_bodies`.
- `crates/noctis/tests/modal_flow.rs`: the verbatim template copy. Its stubs for `response_locale` and the returned `V2Message` become the real API.
- `xtask/src/skill_check/doctrine.rs` L17-21, L66-76 and the test `d9_responder_calls` L524-550: the lint already accepts `ctx.respond(`/`ctx.update(`/`ctx.reply_text(`. It is read-only unless a test row is missing.
- Template, under `examples/project-template/`:
  - `src/modules/{dashboard,setup,profile}/handlers.rs`: the three `// [planned, Story 8.1]` markers, every `HandlerResult<V2Message>`/`Components` handler, and the `HandlerError::user` early returns in the setup handlers.
  - `AGENTS.md` L40.
  - The skill, under `.claude/skills/noctis/`:
    - `SKILL.md` L147 and the `API hash` line (L23);
    - `references/responder.md` L8-12 and §1b (L54-96);
    - `references/views-and-recipes.md` L52;
    - `references/modals.md` L246 and L310;
    - `references/commands.md` L73-74, L735-736, L848-865;
    - `references/troubleshooting.md`.
- `docs/discord-rules.md`: rows INT-03 (L158), INT-04 (L159), INT-12 (L167) and MSG-05 (L251).

## Tasks & Acceptance

**Execution (in order):**

1. `crates/noctis-core/src/responder.rs`:
   - Add `ResponderError::AlreadyResponded`, with the message `"this handler already sent its initial response; nothing was sent"` and a doc that names rows 21/38/39.
   - Add `Shared.handler_answered: AtomicBool` and `fn answered(&self) -> bool` / `fn mark_answered(&self)` (Release/Acquire).
   - In `Shared::{reply, defer_reply, update, defer_update, show_modal}`:
     - return `AlreadyResponded` when the flag is set, checked once before `prepare` (so no Ref is minted) and again right after taking the gate;
     - set the flag under the gate on every non-error path (including the existing late-edit, follow-up and no-op paths).
   - Do not touch `finish`, `notice`, `auto_defer` or `stop_silently`.
   - Update the module docs (the INT-03 paragraph).
2. `crates/noctis-core/src/responder/ergonomics.rs` (new; `mod ergonomics;` plus `pub use` from `responder.rs`):
   - `#[non_exhaustive] pub enum NoticeText { Text(Cow<'static, str>), Key(MessageKey) }` with `From<String>`, `From<&'static str>`, `From<MessageKey>`, and `Debug`/`Clone`. Its rustdoc names the three accepted inputs. Add no `#[diagnostic::on_unimplemented]`: the bound is a plain `Into`, and row 34's E0277 is rustc's default.
   - `#[must_use] pub struct ReplyText { .. }`, holding `Arc<CtxInner>`, `NoticeText` and `Ephemerality` (default `Ephemeral`). Give it `pub fn public(self) -> Self` and `impl IntoFuture` (`Output = HandlerResult<Responded>`, `IntoFuture = BoxFuture<'static, _>`). Its `Debug` omits the text.
   - `impl Shared`:
     - `async fn ctx_respond(self: &Arc<Self>, msg)` (rows 1-14);
     - `async fn ctx_update(self: &Arc<Self>, msg)` (rows 15-22, reusing `send_update`/`edit_prepared`);
     - `async fn ctx_reply_text(self: &Arc<Self>, text: String, vis)` (rows 25-31, 35-37).
     Each method decides under the gate, calls `mark_answered` when it takes the initial slot, and returns `ResponderError`.
   - The `pub(crate) macro_rules!` that emits the ctx methods:
     - `respond(&self, msg: impl Into<OutgoingMessage>) -> impl Future<Output = HandlerResult<Responded>> + Send + '_` (convert `msg` before the `async move`, so the future is `Send` for any input);
     - `update(&self, msg: impl IntoEdit<K>)` with the same shape, emitted only for the component family;
     - `reply_text(&self, text: impl Into<NoticeText>) -> ReplyText`.
   - Every method has rustdoc with a runnable or `no_run` example, plus an `# Errors` section.
3. `crates/noctis-core/src/{command.rs, component.rs, menu.rs, pipeline.rs, pipeline/component.rs, lib.rs}`:
   - the `CtxInner.shared` field;
   - bind the ctx at every site listed in the Code Map (`grep -rn "CtxInner {\|detached_parts\|from_inner(" crates/noctis-core/src`);
   - the placeholder source (`SourceMessage { id: 1, channel_id: 1, format: V2, ephemerality: Public }`) in the detached component and modal seams;
   - `CommandCtx` gets `respond` + `reply_text`; `ComponentCtx` and `ExpiredCtx` (`IntoEdit<Component>`) and `ModalSubmitCtx` (`IntoEdit<ModalSubmit>`) get all three plus `response_locale()` (row 45);
   - the `[INT-04]` doctest;
   - the `lib.rs` re-exports.
4. `crates/noctis-core/src/responder/tests/ergonomics.rs` (new, registered next to L1513): one `#[tokio::test]` per matrix row 1-22 and 25-44, asserting method, path, query and JSON body over `Fixture`, and that no request was made where the row says so. Name the tests with the rule tokens where they apply: `int_03_ctx_then_typestate_already_responded`, `int_03_typestate_then_ctx_update_already_responded`, `int_04_update_unavailable_without_a_source_ctx` (with and without a prior `auto_defer()`), `int_12_reply_text_after_defer_edits_original`, `msg_05_reply_text_pings_nobody`. Add the equivalence tests on the wire: `ctx.update` after `auto_defer()` ≡ `defer_update`+`edit_reply`, and `ctx.respond` after a type-5 `auto_defer()` ≡ `defer_reply`+`edit_reply`. Cover rows 32-33 with `Fixture::start_with` and a test `Translate`.
5. `crates/noctis-testkit/tests/responder_ergonomics.rs` (new; copy the `Running`/`invoke`/`click`/`source_message`/`user` helpers from `commands.rs`, `components.rs` and `modals.rs`). Each test asserts method, path, headers (no `authorization`) and body through `FakeGateway` + `FakeDiscordHttp`:
   - `option_less_command_responds`: a unit `#[derive(SlashCommand)] struct Ping;` whose handler is `ctx.respond(render_ping()).await` → exactly one type-4 callback with the V2 flag (FR-61, UD-3);
   - `ephemeral_command_and_auto_defer`: rows 2 and 3, using `ack_deadline(Duration::ZERO)`;
   - `button_update_and_type_6`: rows 15 and 17;
   - `reply_text_variants`: rows 25, 26 and 28;
   - `slash_opened_submit_update_unavailable`: rows 19 and 20. Assert that no type-7 callback and no `PATCH @original` was sent, that the Component Error Sink got `UpdateUnavailable`, and that the generic ephemeral notice is the only request;
   - `mixed_already_responded`: row 38 through a handler that takes a `ComponentResponder`. Exactly one request, and the sink sees `AlreadyResponded`.
6. `crates/noctis-testkit/tests/publish.rs`: `option_less_command_publishes_empty_options` (the same `Ping`). The PUT body has `"options": []`.
7. `crates/noctis/tests/modal_flow.rs`:
   - re-copy `BioForm::handle` from the template after Task 9 (`ctx.update(..)`);
   - drop the `response_locale` stub from the local `Ctx` trait (call the real method through a forwarding impl);
   - update the header comment.
8. `docs/discord-rules.md`:
   - INT-03: the ctx methods share the gate; `handler_answered`; `AlreadyResponded` and its unit tests.
   - INT-04: `CommandCtx` has no `update` (the doctest); `ctx.update` gets `UpdateUnavailable` without a source.
   - INT-12: `ctx.respond`/`reply_text` after a type-5 defer edit `@original`.
   - MSG-05: `reply_text`.
   - Spine `ARCHITECTURE-SPINE.md` after L517: an `Amended (2026-10-21)` bullet summarising rows 3/5/18/21/28/38 and the three decision lines already in `.memlog.md`.
9. Template (`examples/project-template/`):
   - Every interactive handler ends in `ctx.respond(..)`, `ctx.update(..)` or `ctx.reply_text(..)`, returning `HandlerResult<Responded>`:
     - the component, modal, `on_stale` and `on_expired` handlers use `ctx.update(render_x(..))`;
     - Commands and the context menu use `ctx.respond`;
     - `Badge` uses `ctx.respond(V2Message::new(..))`;
     - the setup step validation uses `return ctx.reply_text(key).await`;
     - `FinishSetup`'s guild check uses `ctx.reply_text(keys::COMMON_GUILD_ONLY)`.
   - The modal openers keep `show_modal`.
   - Remove the three `[planned, Story 8.1]` markers and fix the module doc comments that say "return the message".
   - `AGENTS.md` L40: the three methods are the default; the returned message stays valid.
10. Skill (`.claude/skills/noctis/`):
    - `responder.md`: drop the markers (L9, L54). §1b gets the full state table (rows 1-31 condensed: initial / after a type-5 defer / after a type-6 defer / answered / modal), `AlreadyResponded`, row 18's advice (keep `response = update` on handlers that call `ctx.update`), `reply_text` locale and Dispatcher behaviour, `NoticeText`/`ReplyText`, and `response_locale()` on component contexts.
    - `views-and-recipes.md` L52, `modals.md` L246/L310, `commands.md` L73-74/L735-736 (and the context accessor tables near L848-865: add `respond`, `reply_text`, `update`, `response_locale`), `SKILL.md` L147: drop the markers and state the real behaviour.
    - `troubleshooting.md`: add entries for `AlreadyResponded`, and for `UpdateUnavailable` from `ctx.update`.
11. Run `cargo run --release -p xtask -- api-snapshot --bless` (after installing nightly-2026-10-15 and cargo-public-api 0.52.0 if missing, per AGENTS.md), put the printed hash on the `API hash` line of `SKILL.md`, then run every Verification gate.

**Acceptance Criteria:**
- Given a unit `#[derive(SlashCommand)] struct Ping;` whose handler is `ctx.respond(render_ping()).await`, when it is published and invoked through the fakes, then the PUT carries `"options": []` and the interaction gets exactly one type-4 callback whose `data.flags` include 32768 and whose `allowed_mentions` is `{"parse":[]}`.
- Given a Command, Component or Modal Submit handler calling `ctx.respond(msg).await?`, when it runs while unanswered, after a type-5 auto-defer, after a type-6 auto-defer, or after it already answered, then the wire shows, in that order: type 4 with the declared ephemerality; `PATCH @original`; a follow-up with the declared visibility; another follow-up.
- Given a Component or Modal Submit handler calling `ctx.update(msg).await?`, when it runs while unanswered or after the type-6 auto-defer, then a type-7 callback or a `PATCH @original` edits the source message with `IS_COMPONENTS_V2` and no `EPHEMERAL`. When the modal was opened from a slash command, then no request is sent by the call, with or without a prior auto-defer, and the error is `UpdateUnavailable`.
- Given `CommandCtx`, when a crate's doctests compile `ctx.update(..)`, then the `compile_fail,E0599` doctest carrying `// [INT-04]` fails to compile as expected and `rules-catalog` counts it.
- Given `ctx.reply_text(text)` with a `String`, a `&'static str` or a `MessageKey`, when it is sent, then it is one Text Display that is ephemeral (public with `.public()`) and pings nobody: an initial response, or a follow-up once answered or deferred (an edit of a matching type-5 loading message).
- Given a handler that answered through the ctx methods or a typestate Responder, when it attempts a second initial response through either (`ctx.update` or any typestate Pending method), then `AlreadyResponded` is returned and no request is sent.
- Given `xtask skill-check`, when it lints the reconciled template, then it passes: `ctx.respond(`/`ctx.update(`/`ctx.reply_text(` are accepted, `.reply(`/`.update(`/`.defer_reply(`/`.defer_update(`/`.edit_reply(` on other receivers are still rejected (`d9_responder_calls`), no `Story 8.1` `[planned]` marker remains, and the `API hash` matches.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why the ctx holds `Option<Arc<Shared>>` (decision).** The run already creates one `Shared` per interaction, before or alongside the ctx. Binding it is one `Arc` clone and no allocation, so `alloc_dispatch` and `memory_flat` are unaffected, and there is no cycle (`Shared` never holds a ctx). `None` exists only for contexts hand-built in crate unit tests, which answer `NoReply` (row 44).
- **`handler_answered` vs state (decision).** The state machine cannot tell an auto-defer from the handler's own defer, and the shipped typestate behaviour after an auto-defer (a late `reply` becomes an edit or a follow-up) must survive (row 41). So "answered" is a separate flag, set only by the handler-initiated paths.
- **Why `ctx.respond` never errors once answered.** The story asks for a follow-up "once the original was answered". `AlreadyResponded` therefore applies only to the APIs that are initial responses by nature: `ctx.update` and the typestate Pending methods.
- **Type-5 handling follows INT-12 (decision).** Discord turns the first follow-up after a type-5 defer into an edit of the original. So `ctx.respond` edits explicitly (the story's "edit of the deferred original"). `ctx.reply_text` edits when the visibilities match, else deletes the loading message and follows up, which is the `Shared::notice` pattern. A plain follow-up would otherwise silently become an edit with the wrong visibility.
- **Row 18 (decision).** `ctx.update` under `response = ephemeral|public` that loses the race to the type-5 auto-defer edits the deferred reply, exactly like the typestate `update`. The skill advises keeping `response = update` on handlers that call `ctx.update`.
- **Dispatcher.** `reply_text` is public over a message transport, like `notice`, because an ephemeral message reply does not exist.
- **Template stays in tracking mode.** It still uses `[planned]` API from other stories (`ctx.cache()`, `ctx.render()`, the `Ctx` trait), and `template-build` stays `continue-on-error` until Story 14.2. The copy in `modal_flow.rs` plus `cargo metadata` is the check, as in Story 7.8.

```rust
impl OpenTab {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if !allowed(&ctx) { return ctx.reply_text(keys::NOT_FOUND).await; }
        ctx.update(render_dashboard(&load(&ctx).await?, &Strings::of(ctx.response_locale()), self.tab)).await
    }
}
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (doctests including the INT-04 `compile_fail`, trybuild, dhat `alloc_dispatch`/`memory_flat`)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `cargo metadata --format-version 1 --manifest-path examples/project-template/Cargo.toml > /dev/null` -- ok
- `grep -rn "Story 8.1" crates examples/project-template docs --include=*.rs --include=*.md` -- no hit saying "planned", "arrive" or "until"
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
