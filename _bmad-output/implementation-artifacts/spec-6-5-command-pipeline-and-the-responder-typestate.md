---
title: 'Story 6.5: Command pipeline and the Responder typestate'
type: 'feature'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: '70586997f3c568fd0d19e1fef7aa8f182d6f897c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Command interactions reach the worker and are dropped (`worker.rs::interaction_unhandled`). The 6.1-6.4 descriptors and the merged command Registry exist, but nothing resolves an interaction to its handler, and handlers cannot reply. Every handler returns `HandlerResult<()>`. There is no Responder, no declared ephemerality and no public `Dispatcher` (FR-21 callback rules, FR-61, AD-11, AD-12, AD-23, AD-24; INT-03/04/08/09/10/12, LOC-06).

**Approach:**
- Add a command pipeline to `noctis-core`. It resolves APPLICATION_COMMAND interactions from the Registry's O(1) maps by default names (LOC-06) and resolves the locale default. It has a no-op auto-defer arm point (filled by 6.6), runs the handler under `catch_unwind`, and routes errors to the Command Error Sink.
- Add the typestate `Responder<K, S = Pending>` with the aliases and states AD-11 names. It has two transports: an interaction (callback over the hedged edge path, then edits and follow-ups over twilight) and a message reply (for `Dispatcher`).
- Handlers return `HandlerResult<R: IntoResponse<Command>>`: a message, or the `Responded` from `done()`.
- Add `#[command(.., ephemeral)]` / `#[user_command(.., ephemeral)]`, and expose `noctis::Dispatcher` via `EventCtx::dispatcher()`.

## Boundaries & Constraints

**Always:**
- Pipeline order is fixed (AD-23). Stage 1 resolves. Stage 4 resolves the locale default. Stage 6 is a documented no-op arm point. Stage 9 runs the handler. Stage 10 routes errors. Stages 2, 3, 5, 7 and 8 are absent and must be marked in code comments with their landing story (7.5, 7.x/13.x, 7.6, 7.7).
- The `Dispatcher` runs stages 3 onward.
- Every V2 body goes through `noctis_ui` `MessageBody` (adds `IS_COMPONENTS_V2`, CV2-01) and through `noctis_ui::validate(&mut msg, &app_limits)` before any HTTP request.
- Initial interaction callbacks go only through `Edge::interaction_callback` (Story 5.9). Edits, follow-ups and deletes go through the App's one `twilight_http::Client` with `Request::builder(&Route::…)` raw bodies; interaction-token routes use `use_authorization_token(false)`.
- Declared ephemerality is fixed. `reply` and a returned message use the declared ephemerality. `defer` takes an explicit `Ephemerality`. `followup` takes a `Visibility`. No edit API takes or changes ephemerality (INT-10).
- An interaction never takes a worker permit. Each interaction is one task on the worker `TaskTracker`, raced against the cancel token as `run_handler` does.
- Library code has no `unwrap`/`expect` on external input. Debug output redacts tokens. No `std::sync::Mutex` guard is held across `.await`. `Responder`, `Responded` and `FollowupHandle` are `#[must_use]`.
- Test names must NOT contain `int_01`, `int_02`, `int_03`, `int_05`, `int_06`, `int_11` or `int_13`, and no fixture may carry `[INT-05]`. Those entries stay pending for 6.6/6.7/12.4, and the check fails if one gains a test.

**Never:**
- No auto-defer timer, `TokenExpired`, follow-up cap, `opens_modal`/`OpensModal`/`modal()` (6.6).
- No user-facing error notices or `noctis.*` keys (6.8).
- No autocomplete (6.7), Publish (6.9), component/modal dispatch (Epic 7), Ref finalization (7.5) or attachment upload (9.1).
- No `ComponentResponder`/`ModalResponder`/`update` (Epic 7).
- No new crate dependency edges.
- No test-only branch in core's HTTP path. The detached transport is used only by the `__testing` seams.
- No debug builds.

## I/O & Edge-Case Matrix

Abbreviations:
- **CB**: `POST /api/v10/interactions/{id}/{token}/callback`.
- **@O**: `/api/v10/webhooks/{app_id}/{token}/messages/@original`.
- **FU**: `POST /api/v10/webhooks/{app_id}/{token}?wait=true&with_components=true`.
- **V2 flags**: 32768 (public), 32832 (ephemeral, 32768|64).
- **Sink**: Command Error Sink plus `noctis_handler_errors_total{kind="command"}` +1.

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | Global slash, handler returns `V2Message` | `data.name="ping"`, no `data.guild_id` | CB `{"type":4,"data":{flags:32768,components,allowed_mentions}}` | none |
| 2 | Declared `ephemeral` | as 1 | CB data flags 32832 | none |
| 3 | 3-word path | `config` > group `set` > sub `prefix` | `ctx.command_path()=="config set prefix"`, path variant handed over | none |
| 4 | Guild command | `data.guild_id=G`, handler `guilds=[G]` | looked up in `Scope::Guild(G)` only | none |
| 5 | Global command used in a guild | `interaction.guild_id=G`, no `data.guild_id` | `Scope::Global` | none |
| 6 | Scope mismatch | `data.guild_id=G`, path only global | not resolved (no fallback) | warn `noctis::dispatch` "unknown command", no HTTP, no sink |
| 7 | Unknown path / empty name / unknown menu | `"nope"`, `""` | not resolved | as 6 |
| 8 | LOC-06 | `locale:"tr"`, `guild_locale:"de"`, default `data.name` | resolves; `user_locale()==Some("tr")`, `guild_locale()==Some("de")`, `response_locale()` = `"de"` public / `"tr"` ephemeral | none |
| 9 | No locales | neither field | `response_locale()=="en-US"`; public with only user locale → user locale | none |
| 10 | Unicode names | menu `"Profil göster"`, path word `"ayarlar"` | exact byte match resolves | none |
| 11 | USER vs MESSAGE menu with the same name | `data.type` 2 / 3 | the kind's handler, with `ResolvedUser` / `ResolvedMessage` | `MenuTargetError` → Sink |
| 12 | Other interaction types | type 3, 4, 5 | unchanged trace "no handler registered" | no HTTP |
| 13 | Malformed command data | INTEGER > i64, duplicate leaf option | handler not run | `OptionError` → Sink, no HTTP |
| 14 | No author | neither `member.user` nor `user` | dropped | warn, no HTTP |
| 15 | Handler `Err` | returns `Err(e)` | no HTTP (notice is 6.8) | Sink with `ReportKind::Error`, `handler`, `module`, `event_type="INTERACTION_CREATE"` |
| 16 | Handler panics (at call or inside the future) | `panic!("boom")` | the next interaction is still served | Sink `Panic("boom")` |
| 17 | Shutdown cancel mid-handler | cancel token fires | handler dropped | Sink `Cancelled` |
| 18 | `reply` then `done()` | `r.reply(v2)` | exactly one CB type 4; nothing after the return | none |
| 19 | `defer` | `Public` / `Ephemeral` | CB body exactly `{"type":5}` / `{"type":5,"data":{"flags":64}}` (INT-08) | none |
| 20 | INT-12 | `defer(..)`, then return `V2Message` | CB type 5, then `PATCH @O`, flags 32768, no legacy clears | none |
| 21 | Reply, then return a message | `reply(a)`, return `b` | CB type 4 (a), then `PATCH @O` (b) | none |
| 22 | Responder taken but unused | handler returns `V2Message` | as 1 | none |
| 23 | Edit formats | V2 original + V2 edit; Legacy original + V2 edit; V2 original + `LegacyMessage` edit | PATCH without clears; PATCH with `content:null, embeds:[], poll:null, sticker_ids:[]`; | last: `ResponderError::Invalid(UiError::V2ToLegacy)`, no HTTP |
| 24 | `followup` | `Public` / `Ephemeral` | FU with flags 32768 / 32832; `FollowupHandle::message_id()` = response `id` | none |
| 25 | Handle edit / delete | `h.edit(m)`, `h.delete()` | `PATCH` / `DELETE /webhooks/{app}/{token}/messages/{id}` | none |
| 26 | `delete_original` | after a reply | `DELETE @O` | none |
| 27 | Over limits | 41 components or a label of 81 chars | Strict: no HTTP; Lenient (`AppBuilder::limits`): truncated with `…`, sent | Strict: `Invalid(UiError::Limits{..})` |
| 28 | At the limits | 40 components; label of 80 chars | sent unchanged | none |
| 29 | Attachments | non-empty `AttachmentSet` | no HTTP | `AttachmentsUnsupported` (9.1) |
| 30 | Unresolved Route `custom_id` | RefSlot left in tree | no HTTP | `Encode { reason }` |
| 31 | Callback rejected | 400 `{"code":50035}` / 40060 | error / success | `Rejected{status:400, code:Some(50035)}` |
| 32 | Callback late | past `received_at`+2.9 s | no further attempt | `AckDeadlineMissed(..)` |
| 33 | Bad token | token `"a b"` | no HTTP | `InvalidToken` |
| 34 | Edit / followup rejected | 404 `{"code":10015}` | — | `Rejected{status:404, code:Some(10015)}`; transport failure → `Transport` |
| 35 | Returned message fails to send | as 27 / 31 | — | `ResponderError` wrapped in `HandlerError` → Sink |
| 36 | `[INT-03]` | `r.reply(m).await?.reply(m)` | E0599 | compile_fail |
| 37 | `[INT-04]` | `r.update(m)` / `r.modal(..)` on `CommandResponder` | E0599 | compile_fail (2 fixtures) |
| 38 | `[INT-08]` | `r.defer(CallbackFlags::EPHEMERAL)` | E0308 | compile_fail |
| 39 | `[INT-10]` | `acked.edit_original(m, Ephemerality::Ephemeral)` | E0061 | compile_fail |
| 40 | Return not a response | `HandlerResult<()>`, `HandlerResult<u8>` (command and menu) | E0277 with message "`{Self}` is not a Command response" | types fixtures |
| 41 | `ephemeral` argument | bare / absent | `ephemerality()` `Ephemeral` / `Public` | none |
| 42 | Bad `ephemeral` argument | `ephemeral = true`; given twice | "`ephemeral` takes no value"; "duplicate `#[command]` argument `ephemeral`" | compile error |
| 43 | `opens_modal` | — | still "planned and not supported yet" | compile error |
| 44 | Responder parameter forms | `(ctx[, path][, r: CommandResponder][, opts])` ok; `(ctx, opts, r)`, two responders, `(ctx, target, r)` on menus | macro error listing the forms; menu form ok | compile error |
| 45 | Dispatcher happy path | `MESSAGE_CREATE` handler: `ctx.dispatcher().run_command(CommandInvocation::new(CommandInput::new("ping"), ch, msg, user))` | `POST /channels/{ch}/messages`, flags 32768, `message_reference:{message_id, fail_if_not_exists:false}`; returns `Ok(())` | none |
| 46 | Dispatcher unknown path | `"nope"` | no HTTP | `Err(DispatchError::UnknownCommand{path})` |
| 47 | Dispatcher scope | `.guild_id(Some(G))`; path in `Guild(G)` and `Global` | guild handler wins; else Global | none |
| 48 | Message `defer(Public)`, then a message | — | `POST /channels/{ch}/typing`, then `POST` reply | none |
| 49 | Message ephemeral | declared ephemeral, `defer(Ephemeral)`, `followup(.., Ephemeral)` | no HTTP | `EphemeralUnsupported` |
| 50 | Message edit / delete | after a reply; `delete_original` before any reply | `PATCH` / `DELETE /channels/{ch}/messages/{id}`; no-op `Ok(())` | none |
| 51 | Caller options | `CommandInput::new("warn").with_option("days", OptionValue::Integer(3))`; same name twice | `days == 3`; the later value replaces the earlier | missing required → `OptionError::Missing` → Sink |
| 52 | `__testing` seams | `call_command*` / `call_context_menu` with a responder handler | `Ok(())`, detached transport, no I/O | handler errors returned as before |
| 53 | Limits config | `AppBuilder::limits(l)` / unset | `App::limits()==&l` / `Limits::default()` | none |

</intent-contract>

## Code Map

**Core (`crates/noctis-core/src/`)**
- `worker.rs`:
  - `drain` sends `InboundKind::Interaction` to `interaction_unhandled` (L180, L209).
  - `interaction_unhandled` holds the `noctis.interaction` span with empty `module`/`path`.
  - `run_handler` is the pattern for `catch_unwind` + cancel race + metrics + sink.
  - `panic_message` is reusable.
  - `worker/tests.rs` L1083 Matrix 28 asserts the `noctis.interaction` span. Keep it on a non-command interaction.
- `command.rs`:
  - Four constructors `__new`, `__new_with_options`, `__new_plain`, `__new_plain_with_options`. The erased `Fn(CommandCtx)` and minimal `CommandCtx(Arc<CtxInner{path,input}>)` change here.
  - `call_command`/`call_command_json` are the test seams.
  - Unit tests at the end must be migrated.
- `menu.rs`: `__new_user`/`__new_message` (L109/L137); `ErasedMenu` L87; `MenuInput::from_json`; `call_context_menu` L292.
- `handler.rs`: `AppShared { app, sinks, http, states, edge }` (add `limits`); `EventCtx::new` and `CtxInner` (add the dispatcher).
- `options.rs`: `CommandInput { path, options, resolved }`; only `from_json` is public. Add `new`/`with_option`.
- `edge/mod.rs`: `Edge::interaction_callback`, L154.
- `edge/callback.rs`: `CallbackRequest { interaction_id, token, body: CallbackBody::Json, with_response, received_at: tokio::time::Instant }`; `CallbackError` maps to `ResponderError`.
- `registry/commands.rs`:
  - `Commands::scope(Scope)`, `ScopeCommands::command(path)`, `ScopeCommands::menu(kind, name)`;
  - `CommandEntry`/`MenuEntry { module, ancestors, handler }`;
  - remove the `expect(dead_code, "Story 6.5 …")` attributes.
- `app.rs`:
  - `workers()` (L353);
  - `build()` L1461 creates `AppShared` at L1539;
  - add `limits` to the builder, the `App` accessor and `Debug`.
- `sink.rs`: `ErrorOrigin::Command`, `ErrorReport`, `ReportKind`.
- `observe.rs` module docs: add the `noctis.dispatch{app, guild_id, module, path}` span.
- `lib.rs`: the re-exports (L139-157) and `__testing` (L160) seams to keep.
- `edge/test_support.rs` `TestServer`: a recording upstream for core unit tests via `http_proxy(server.authority())`.
- `test_support.rs`: `Capture`, `command`, `user_menu`, `message_menu` helpers.

**UI and twilight**
- `crates/noctis-ui/src/message/mod.rs`:
  - `V2Message::{callback_body, edit_body, create_body, attachment_set}`;
  - `LegacyMessage::{callback_body, edit_body -> Result}`;
  - `MessageFormat`; `limits/mod.rs` `validate`.
- `CallbackFlags::EPHEMERAL`; `EditFlags::empty()`; `CreateFlags::empty()`.
- twilight-http 0.17.1 `Route` variants:
  - `UpdateInteractionOriginal`, `DeleteInteractionOriginal`;
  - `ExecuteWebhook{webhook_id, token, wait, with_components, thread_id}`;
  - `UpdateWebhookMessage`, `DeleteWebhookMessage`;
  - `CreateMessage`, `UpdateMessage`, `DeleteMessage`, `CreateTypingTrigger`.
- Use `Request::builder(&route).body(vec).use_authorization_token(bool).build()` and `Client::request::<T>`.

**Macros (`crates/noctis-macros/src/`)**
- `command.rs`:
  - `PLANNED` (L27) includes `ephemeral`;
  - `check_params` L405;
  - `expand` L525 builds the call;
  - unit tests L812/L966 assert that `ephemeral` is "planned" (update them).
- `menu.rs`: `PLANNED` L64, `check_params` L159, messages L56/L251/L409.

**Facade and tests**
- `crates/noctis/src/lib.rs`: the root `pub use noctis_core::{…}` (L347), the `prelude` (L987), the command macro docs (L575).
- `crates/noctis/tests/`:
  - `commands.rs`, `menus.rs`, `options.rs`, `merge.rs`, `boundary/{cmd,ctx,meta}.rs`, every `tests/ui/**` fixture with `#[command]`: the handlers return `HandlerResult<()>` and must migrate;
  - `types/command_handler_returns_value.*`, `types/context_menu_returns_value.*`: re-bless.
- `crates/noctis-testkit/tests/events.rs`: the `Running` harness pattern (FakeDiscordHttp + FakeGateway, `Runtime::for_test`). Use `FakeDiscordHttp::route(Method, path, FakeResponse)` and `.requests()`.

**Repo files**
- `xtask/rules-pending.toml`: entries with `story = "6.5"`: LOC-06 unit, INT-03 by-construction, INT-04 by-construction, INT-08 by-construction, INT-10 by-construction, INT-12 unit.
- `docs/discord-rules.md`: rows LOC-06 (L126), INT-03..12 (L158-167).
- Template:
  - `examples/project-template/src/modules/setup.rs` L107: the `ephemeral` `[planned]` comment;
  - `.claude/skills/noctis/references/responder.md`;
  - `commands.md` §1 L21-41 and §8 L690-700;
  - `app-and-sharding.md` L215/L406 (`[planned, Story 6.5]`);
  - `SKILL.md` API hash;
  - `AGENTS.md`.

## Tasks & Acceptance

**Execution (in order):**

1. **`crates/noctis-core/src/options.rs`** -- Add these on `CommandInput`, both documented and doctested:
   - `pub fn new(path: impl Into<String>) -> Self` (no options, empty resolved);
   - `#[must_use] pub fn with_option(self, name: impl Into<String>, value: OptionValue) -> Self` (replaces a same-name value).

   Rationale: callers of the Dispatcher decode their own options.

2. **`crates/noctis-core/src/responder.rs` (new, + `responder/transport.rs`, `responder/tests.rs`)** -- Public API, all `Send`, `Debug` without tokens:
   - **Markers:**
     - `pub struct Command<M = NoModal>(PhantomData<M>)`, `pub struct NoModal`;
     - `pub struct Pending`, `pub struct Acked`;
     - sealed `pub trait ResponderKind` and `pub trait ResponderState`.
   - **`pub struct Responder<K: ResponderKind, S: ResponderState = Pending>`** holds an `Arc<Shared>`:
     - `Shared` holds the transport, an `AtomicU8` state (Pending/Deferred/Replied), a `Mutex` original format (`Deferred | V2 | Legacy`), the declared `Ephemerality`, `Limits` and the HTTP handles.
     - `pub type CommandResponder<M = NoModal> = Responder<Command<M>, Pending>;`.
   - **`impl<M> Responder<Command<M>, Pending>`:**
     - `pub async fn reply(self, msg: impl Into<OutgoingMessage>) -> Result<Responder<Command<M>, Acked>, ResponderError>`: callback type 4 with the declared ephemerality.
     - `pub async fn defer(self, ephemerality: Ephemerality) -> Result<Responder<Command<M>, Acked>, ResponderError>`: type 5, only `EPHEMERAL`.
   - **`impl<K> Responder<K, Acked>`:**
     - `pub async fn edit_original(&self, msg) -> Result<(), ResponderError>`;
     - `pub async fn followup(&self, msg, visibility: Visibility) -> Result<FollowupHandle, ResponderError>`;
     - `pub async fn delete_original(&self) -> Result<(), ResponderError>`;
     - `#[must_use] pub fn done(self) -> Responded`.
   - **Values:**
     - `pub struct Responded` (opaque);
     - `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)] pub enum Ephemerality { #[default] Public, Ephemeral }` with `pub type Visibility = Ephemerality;`;
     - `pub enum OutgoingMessage { V2(V2Message), Legacy(LegacyMessage) }` (`#[non_exhaustive]`, with `From<V2Message>`, `From<LegacyMessage>` and `From<Components>`);
     - `pub struct FollowupHandle` with `message_id() -> Id<MessageMarker>`, `edit(&self, msg)` and `delete(&self)`.
   - **`#[non_exhaustive] #[derive(Debug, thiserror::Error)] pub enum ResponderError`:**
     - `Invalid(UiError)`, `Encode { reason: String }`, `AttachmentsUnsupported`, `EphemeralUnsupported`, `InvalidToken`;
     - `AckDeadlineMissed(#[from] AckDeadlineMissed)`, `Rejected { status: u16, code: Option<u32> }`, `Transport`.
   - **`IntoResponse`:** sealed `pub trait IntoResponse<K: ResponderKind>: Send + 'static`, carrying `#[diagnostic::on_unimplemented(message = "`{Self}` is not a Command response", label = "return a `V2Message`, `Components`, a `LegacyMessage`, or the `Responded` from `done()`")]`.
     - It has a `#[doc(hidden)]` conversion to a crate-visible response value.
     - It is implemented for `Responded` (any K) and for `V2Message`, `Components` and `LegacyMessage` (for `Command<M>`).
   - **Transports:**
     - Interaction (`app_id`, `interaction_id`, `token`, `received_at`; uses `Edge::interaction_callback` for type 4/5 and twilight for the rest);
     - Message (`channel_id`, `message_id`; reply via `CreateMessage` with `message_reference`, defer via `CreateTypingTrigger`, edits/deletes via channel message routes; ephemeral → `EphemeralUnsupported`);
     - `Detached` (no I/O; `__testing` only).
   - **Before HTTP:** validate → attachments check → serialize. Map `CallbackError` and twilight errors as in Matrix 31-34.
   - Doctests use `compile_fail` lines carrying `// [INT-03]`, `// [INT-04]`, `// [INT-08]` and `// [INT-10]` alongside the trybuild fixtures.

   Rationale: AD-11, INT-03/04/08/10.

3. **`crates/noctis-core/src/command.rs`** -- Handler and context changes:
   - Constructors take `F: Fn(CommandCtx, [P,] CommandResponder, [O]) -> Fut`, `Fut: Future<Output = HandlerResult<R>> + Send + 'static`, `R: IntoResponse<Command>`. Keep the four names.
   - The erased call becomes `Fn(CommandCtx, CommandResponder) -> BoxFuture<'static, HandlerResult<Response>>`.
   - Add `ephemerality: Ephemerality`, `#[doc(hidden)] __with_ephemerality` and `pub fn ephemerality()`.
   - Extend `CommandCtx` (still one `Arc`):
     - `module()`, `shard()`, `guild_id()`, `channel_id() -> Option<Id<ChannelMarker>>`, `user_id() -> Id<UserMarker>`;
     - `user_locale()`, `guild_locale() -> Option<&str>`, `response_locale() -> &str` (Matrix 8-9);
     - `ephemerality()`, `state<T>()`, `http()`.
   - Keep `command_path()`.
   - The seams keep their signatures and use the detached transport (Matrix 52).
   - Migrate the unit tests.

4. **`crates/noctis-core/src/menu.rs`** -- `__new_user`/`__new_message`:
   - `F: Fn(CommandCtx, ResolvedUser|ResolvedMessage, CommandResponder) -> Fut` with `R: IntoResponse<Command>`;
   - add ephemerality the same way;
   - migrate the tests.

5. **`crates/noctis-core/src/pipeline.rs` (new, + `pipeline/tests.rs`)** -- Pipeline and Dispatcher:
   - `pub(crate) async fn run_interaction(registry, app, inbound, cancel)` implements the stages for type-2 interactions (Matrix 1-17) inside span `noctis.interaction` with `module`/`path` recorded.
   - Commands decode with `CommandInput::from_json(raw)`; menus use `MenuInput`.
   - Scope = `data.guild_id` → `Scope::Guild`, else `Global`.
   - Returned messages: Pending → type 4; Deferred/Replied → `edit_original` (INT-12).
   - Record `noctis_handler_duration_seconds{kind="command"}`.
   - Public API:
     - `#[derive(Clone)] pub struct Dispatcher` (holds `Arc<Registry>`, `Arc<AppShared>`);
     - `pub async fn run_command(&self, invocation: CommandInvocation) -> Result<(), DispatchError>` (stages from 3 on, span `noctis.dispatch`, guild scope then global, message transport; handler failures go to the Sink, not into the result);
     - `pub struct CommandInvocation` with `new(input: CommandInput, channel_id, message_id, user_id)`, `#[must_use] guild_id(Option<Id<GuildMarker>>)`, `#[must_use] shard(u32)` and `#[must_use] locale(impl Into<String>)`;
     - `#[non_exhaustive] pub enum DispatchError { UnknownCommand { path: Box<str> } }`.
   - Tests:
     - `loc_06_dispatch_resolves_default_names` (Matrix 8);
     - `int_12_returned_message_after_defer_edits_original` (Matrix 20);
     - Matrix 1-35 and 45-51 via `TestServer` as `http_proxy`;
     - an `#[event]`-style handler test dispatching through `EventCtx::dispatcher()`.

   Rationale: AD-23, AD-24, LOC-06, INT-12.

6. **`crates/noctis-core/src/{worker.rs, handler.rs, app.rs, registry/commands.rs, observe.rs, lib.rs}`** -- Wiring:
   - `drain` spawns `run_interaction` for `InteractionType::ApplicationCommand` on the tracker; everything else keeps `interaction_unhandled`.
   - `AppShared` gains `limits: Limits`.
   - `EventCtx` gains `pub fn dispatcher(&self) -> &Dispatcher`.
   - `AppBuilder::limits(&mut self, Limits) -> &mut Self` and `App::limits() -> &Limits`.
   - Remove the dead-code expects. Document the `noctis.dispatch` span.
   - Re-export `Acked, Command, CommandInvocation, CommandResponder, DispatchError, Dispatcher, Ephemerality, FollowupHandle, IntoResponse, NoModal, OutgoingMessage, Pending, Responded, Responder, ResponderError, ResponderKind, ResponderState, Visibility`.

7. **`crates/noctis-macros/src/{command.rs, menu.rs}`** -- Macro changes:
   - Accept bare `ephemeral` (remove it from `PLANNED`, append `.__with_ephemerality(…Ephemeral)`); errors as in Matrix 42.
   - Detect a responder parameter (last segment `CommandResponder` or `Responder`) at the fixed position after ctx/path/target and before options.
   - Generate an adapter closure with the uniform shape (`|c, [p,] r, [o]| __noctis_command(c, [p,] [r,] [o])`).
   - Spanned at the return type so E0277 points there.
   - Update the parameter-form messages (Matrix 44) and the unit tests.

8. **`crates/noctis/src/lib.rs`** -- Facade:
   - root re-exports;
   - prelude adds `CommandResponder, Ephemerality, IntoResponse, Responded, Visibility`;
   - docs of `command`/`user_command` (`ephemeral`, the responder parameter, return types);
   - a doctest replying via the Responder.

9. **`crates/noctis/tests/ui/int/`** -- Add `INT-03_second_reply.rs`, `INT-04_command_update.rs`, `INT-04_command_modal.rs`, `INT-08_defer_flags.rs` and `INT-10_edit_ephemerality.rs`, each with the `// [INT-xx]` line and a blessed `.stderr`.
   - Re-bless `tests/types/*returns_value.stderr`; add `tests/types/command_handler_returns_unit.rs`.
   - Migrate every handler in `crates/noctis/tests/**` to return `HandlerResult<V2Message>`/`Components` or `Responded`.

10. **`crates/noctis-testkit/tests/commands.rs` (new)** -- End to end through FakeGateway (`INTERACTION_CREATE`) and FakeDiscordHttp routes, covering Matrix 1, 2, 18-21, 24, 45 and 49. Assert the recorded bodies and flags.

11. **`xtask/rules-pending.toml`** -- Delete the six `story = "6.5"` entries.
    - **`docs/discord-rules.md`** -- Align the How text of INT-03/04/08/10/12 and LOC-06 with the real names (`IntoResponse`, `Ephemerality`, `Visibility`, `ResponderError`).

12. **`examples/project-template/`** -- Template updates:
    - `setup.rs`: drop `ephemeral`'s `[planned]` comment.
    - `responder.md`: §1, §3 and §4 become landed. Use the exact signatures, the `ResponderError` variants, the `Dispatcher`/`CommandInvocation`/`DispatchError` section and the message-transport rules. `TokenExpired`/`FollowupCap`/`opens_modal`/`update`/Views stay `[planned]` with their stories.
    - `commands.md`: §1 handler forms and return types; §8 the landed accessors.
    - `app-and-sharding.md`: `AppBuilder::limits`, `[planned, Story 6.5]` resolved.
    - `AGENTS.md`: one bullet.
    - `SKILL.md`: the API hash.
    - **`xtask/public-api.txt`**: re-bless.
    - **memlog**: implementation `(decision)` lines.

**Acceptance Criteria:**
- **Resolution:** Given a running App with a Module holding `#[command("ping", description = "d")] async fn ping(ctx: CommandCtx) -> HandlerResult<V2Message>`, when FakeGateway dispatches a chat-input interaction for `ping`, then FakeDiscordHttp records exactly one callback with `type` 4 and `data.flags` 32768, and the `noctis.interaction` span carries `module` and `path`.
- **Responder typestate:** Given a handler taking `r: CommandResponder`, when it calls `r.defer(Ephemerality::Ephemeral).await?` and then `followup(msg, Visibility::Public)`, then the callback body is exactly `{"type":5,"data":{"flags":64}}` and the follow-up carries flags 32768. A second `reply`, `update`, `modal`, a non-`Ephemerality` defer argument and an ephemerality on `edit_original` all fail `cargo check` (trybuild fixtures carrying `[INT-03]`, `[INT-04]` ×2, `[INT-08]`, `[INT-10]`).
- **Dispatcher:** Given an `#[event]` handler on `MESSAGE_CREATE` calling `ctx.dispatcher().run_command(..)` for a registered path, when the event arrives, then the Command handler runs and its returned `V2Message` is posted as a reply to that message. An unregistered path returns `DispatchError::UnknownCommand` with no HTTP request.
- **Gates:** Given the gates, when `cargo run --release -p xtask -- rules-catalog` runs, then it passes with the six 6.5 entries removed and no 6.6/6.7 entry satisfied early. `api-snapshot` and `skill-check` pass with the Usage Skill naming every new public item.

## Spec Change Log

## Review Triage Log

## Design Notes

**Return contract (decision).** A Command handler returns `HandlerResult<R>` with `R: IntoResponse<Command<M>>`: `V2Message`, `Components`, `LegacyMessage` or `Responded`. `()` does not compile, matching responder.md §1 ("a command must answer"). Views (Epic 8) and component `()` (Epic 7) add impls later; the trait is sealed until then. `IntoResponse` is generic over the kind so component-only impls never leak to Commands.

**One shape per constructor (decision).** The macro adapts every signature to `(ctx, [path,] responder, [options])`, so core keeps four constructors instead of eight. A handler without a responder parameter simply drops it, and dropping a `Pending` responder sends nothing.

**Shared state now, timer later.** The `AtomicU8` state already decides "returned message after an ack → `edit_original`" (INT-12). Story 6.6 adds the auto-defer task as a second writer of the same atomic, and turns a late `reply` into an edit.

**Edit format (decision).** The original's format is tracked as Deferred, V2 or Legacy:
- Deferred: a V2 edit uses `MessageFormat::V2`, and a legacy edit uses the legacy body (Discord's empty loading message needs no clears);
- Legacy: a V2 edit sends the CV2-02 clears;
- V2: a legacy edit is `UiError::V2ToLegacy`.

**Message transport (decision).** Prefix replies cannot be ephemeral. Any ephemeral send there is `ResponderError::EphemeralUnsupported` rather than a silent public post, which could leak private UI. Story 15.4 may revisit this. `defer` there is a typing indicator.

**Scope (decision).** A scope comes from `data.guild_id`, which Discord sets only for guild commands. There is no global fallback for a guild-scoped invocation: a missing path means a stale registration. The Dispatcher has no `data`, so it tries the guild scope and then Global.

**Error variants instead of exposing internals (decision).** `CallbackError` and `twilight_http::Error` are mapped into `ResponderError` (`Rejected`/`Transport`), so neither becomes public API. `AckDeadlineMissed` is wrapped as Story 5.9 planned. `AppBuilder::limits` is added because the Responder must validate against App-scoped Limits (AD-24 lists `Limits` as App-scoped).

**Locale default.** `response_locale()` follows responder.md §3 and FR-43:
- ephemeral: User Locale → Guild Locale → `"en-US"`;
- public: Guild Locale → User Locale → `"en-US"`.

`LocaleResolver` arrives with Epic 11.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis-core` pipeline/responder tests, `noctis --test ui` with the new `int/` fixtures, `noctis-testkit --test commands`
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok (no new edges)
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: ok; copy the hash into SKILL.md (needs `nightly-2026-10-15` + `cargo-public-api` 0.52.0, AGENTS.md pitfalls)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok (MSRV)
