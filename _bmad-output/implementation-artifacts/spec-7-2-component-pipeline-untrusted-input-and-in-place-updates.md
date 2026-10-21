---
title: 'Story 7.2: Component pipeline, untrusted input and in-place updates'
type: 'feature'
created: '2026-10-21'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 7.1 dispatches clicks and modal submits to their handlers, but nothing answers: rejected ids only log, signed Routes are always `forged` (no `NOCTIS_PAYLOAD_KEY`), component/modal handlers take no Responder, `response`/`auto_defer` on the component kind attributes are still "not supported yet", and no auto-defer is armed. Implements FR-19, FR-21 (components), NFR-4, NFR-6; AD-5, AD-11, AD-23, AD-29; rules CID-06, INT-01, INT-04 (plus INT-02, INT-05, INT-13, CV2-01, CV2-02, MSG-05 on the new paths).

**Approach:**
- Rejections send the CID-06 generic ephemeral notice; the App loads a payload key ring and verifies (and signs) `signed` Routes.
- `Responder` gains the kinds `Component` and `ModalSubmit` with discord.js flows (`update` type 7, `defer_update` type 6, `reply`/`defer_reply` for all kinds, `show_modal` on `ComponentResponder<Manual>`), `…_with_response` variants, and the `UpdateUnavailable` runtime case.
- `#[button]` and the five select attributes accept `response = update|ephemeral|public` (default `update`) and `auto_defer = true|false` (default `true`); the component pipeline arms the auto-defer shaped by `response` and sends a returned message per that response.
- Every row of the response table is asserted on the wire through `FakeDiscordHttp`.

## Boundaries & Constraints

**Always:**
- **Responder kinds (AD-11).** In `crates/noctis-core/src/responder.rs` add public sealed kinds `pub enum Component {}` and `pub enum ModalSubmit {}` (next to `Command`) and the aliases `pub type ComponentResponder<D = AutoDefer> = Responder<Component, Pending, D>;` and `pub type ModalResponder<D = AutoDefer> = Responder<ModalSubmit, Pending, D>;`. Re-export all four from `noctis-core` and the facade root; the two aliases also from `noctis::prelude`.
- **Typestate (compile time).**
  - `reply`, `reply_as`, `defer_reply`, `defer_reply_as` move to `impl<K: ResponderKind, D: DeferMode> Responder<K, Pending, D>` (all three kinds).
  - `update(msg)`, `defer_update()` exist only in `impl<D> Responder<Component, Pending, D>` and `impl<D> Responder<ModalSubmit, Pending, D>` (a shared private helper; `CommandResponder::update` stays E0599, existing fixture `INT-04_command_update` unchanged).
  - `show_modal` exists on `Responder<Command, Pending, D>` and `Responder<Component, Pending, D>`, both `where D: CanShowModal` (E0277 `[INT-05] show_modal needs auto_defer = false on this handler`); `ModalResponder` has none (E0599).
  - `…_with_response` variants: `reply_with_response(msg)`, `defer_reply_with_response()` on all kinds; `update_with_response(msg)`, `defer_update_with_response()` on Component/ModalSubmit. Each returns `Result<(Responder<K, Acked, D>, Option<CallbackResource>), ResponderError>`, sends `?with_response=true`, and the resource is `None` exactly when no callback was sent (the interaction was already acknowledged, so the call became an edit/follow-up or sent nothing).
  - New public `CallbackResource` (`#[non_exhaustive]`, `Debug`, `Clone`): `kind(&self) -> u8` (the resource `type`), `message(&self) -> Option<&twilight_model::channel::Message>`; parsed from `resource` of the callback response (`resource.type`, `resource.message`); a body without `resource` gives `ResponderError::Transport`.
  - `IntoResponse<Component>` and `IntoResponse<ModalSubmit>` for `V2Message`, `Components`, `LegacyMessage` (plus the existing generic `Responded`); the `on_unimplemented` message becomes ``"`{Self}` is not a response of this handler"``.
  - `ResponderError::UpdateUnavailable` (new variant): ``"there is no message to update: this modal was not opened from a component message"``.
- **Runtime state machine** (`Shared`, gate-serialized, at most one callback per interaction, INT-03):
  - `Shared` gets the interaction's `source: Option<SourceMessage { id, channel_id, format: MessageFormat, ephemerality }>` (component message; a modal submit's `message` when present) and a declared `Declared { Reply(Ephemerality) | Update }`.
  - `update(msg)` Pending → type 7 `{"type":7,"data": edit_body(source.format)}` (flags never contain `EPHEMERAL`); `original` becomes `Sent(V2|Legacy)` with the source's id, channel and ephemerality. After any ack it becomes `edit_reply(msg)`.
  - `defer_update()` Pending → `{"type":6}` with no `data`; `original` = the source message (shape `Sent(source.format)`); after an ack it sends nothing (`debug`).
  - `update`/`defer_update` (and their `_with_response`) with `source == None` → `Err(UpdateUnavailable)`, no request, state unchanged.
  - `reply`/`reply_as` after a type-6 ack (auto-defer or `defer_update`) → a follow-up with that ephemerality (a new message, never an edit of the component's message); after a type-5 ack → edit of the deferred reply (existing behaviour); `defer_reply` after any ack sends nothing.
  - The default ephemerality of `reply`/`defer_reply`: `Reply(e)` → `e`; `Update` → `Public`.
  - `notice` after a type-6 ack never deletes the component's message: it is an ephemeral follow-up.
  - `show_modal` after any ack → `ModalUnavailable`, no request (existing behaviour, now also for components).
- **Auto-defer (INT-01, stage 6).** `arm` is shared by commands and components. `auto_defer = true`: at `received_at + AppBuilder::ack_deadline` while Pending send `{"type":6}` when the declared response is `Update` and a source message exists; otherwise `{"type":5}` plus `"data":{"flags":64}` only for `Reply(Ephemeral)` (so a slash-opened modal submit with the default `update` defers type 5 public). `auto_defer = false`: no auto-defer; one `error!` on `noctis::dispatch` with `handler` and `module` if still Pending at `received_at + ACK_DEADLINE` (the existing 2.9 s constant). The timer is aborted when the run ends.
- **Returned message** (`HandlerResult<R>` with `R: IntoResponse<Component|ModalSubmit>`): while Pending, `Update` with a source → type 7; `Update` without a source (slash-opened modal) → type 4 public (same fallback as the auto-defer); `Reply(e)` → type 4 with `e`. After an ack: `edit_reply` (type-5 or type-6 original). `Responded` sends nothing.
- **Wire (AD-11, INT-13, CV2-01/02, MSG-05).**
  - Every V2 send/edit carries `flags` with `IS_COMPONENTS_V2` (32768); edits and type 7 never carry `EPHEMERAL`; every body carries `allowed_mentions` (`{"parse":[]}` unless opted in).
  - Every edit body (type 7 `data`, `PATCH …/messages/@original`, follow-up edit, bot-token channel edit) carries the attachments keep-list `"attachments": []` (messages with attachments stay `AttachmentsUnsupported` until Story 9.1). Update existing command edit assertions accordingly.
  - Updating a legacy source into V2 sends `content: null`, `embeds: []` (and the existing `poll: null`, `sticker_ids: []`); a `LegacyMessage` update/edit of a V2 message is `ResponderError::Invalid(UiError::V2ToLegacy)` with no request.
  - Callbacks and interaction-webhook requests carry no `Authorization` header; `with_components` is never sent: remove `with_components: Some(true)` from `Transport::follow_up` (set `None`).
- **INT-02.** After 15 min: `edit_reply`/`delete_reply`/`fetch_reply` on a public original with known id and channel (an `update`d/`defer_update`d component message always qualifies) use the bot-token channel routes; everything else `TokenExpired`, no request.
- **Payload keys (AD-5, AD-29, NFR-6).**
  - `AppBuilder::payload_key(PayloadKey) -> Self` and `AppBuilder::previous_payload_key(PayloadKey) -> Self` (last call wins). `App::from_env` reads `NOCTIS_PAYLOAD_KEY` and `NOCTIS_PAYLOAD_KEY_PREVIOUS` (trimmed; empty = unset) with `PayloadKey::from_hex` (64 hex digits); a bad value is `EnvError::InvalidPayloadKey { var, source: KeyError }`, whose text names the variable and never the value.
  - `App::build` violations: `BuildViolation::PreviousKeyWithoutKey` (``[CID-06] a previous payload key is set without a current one: set NOCTIS_PAYLOAD_KEY (or call `.payload_key(..)`) too``) and `BuildViolation::MissingPayloadKey { route }` per mounted `signed` Route, fold order (``[CID-06] Route `{route}` is `signed`, but no payload key is configured: set NOCTIS_PAYLOAD_KEY (64 hex digits) or call `.payload_key(..)` ``). `ComponentHandler`/`ModalHandler` record `signed: bool` from `S::SHAPE`.
  - `AppShared` holds `Option<Arc<PayloadKeys>>`; it is never logged (its `Debug` is already redacted), never in a span field, never passed to a port or adapter.
  - Dispatch verifies with `Decoded::verified_payload::<S>(&keys)` (current or previous key passes; a bad/missing tag stays `forged`). The allocation probe keeps using unsigned decode.
  - **Send-side resolution.** Before encoding any outgoing V2/legacy message or modal, the Responder resolves every unresolved `CustomId` via a new hidden `noctis_ui::CustomId::resolve_for_send(&mut self, nonce: SelectNonce, signer: Option<&dyn Sign>) -> Result<Option<PendingRef>, EncodeError>` that passes the nonce only to select Routes and the signer only to signed Routes. One nonce per send, drawn with `getrandom` uniformly in `0..=SelectNonce::MAX`. A Ref slot (`Some(slot)`) or any `EncodeError` → `ResponderError::Encode { reason }` naming the Route, no request (Refs arrive with Story 7.5).
- **CID-06 notice.** Every `Rejection` except `Stale` and `RefUnresolved` (Stories 7.3/7.5 route those; they stay warn-only) sends one type-4 callback `{"type":4,"data":{"flags":32832, "components":[TextDisplay(noctis.error.generic text in the User, then Guild Locale, else en-US)], "allowed_mentions":{"parse":[]}}}` after its `warn`, through a `Shared` with an interaction transport (no auto-defer armed). A failed send is one `warn` (`could not send the error notice`). `Rejection` stays `pub(crate)` (the typed error). Rejections still never panic and never run a handler.
- **Component derives** (`crates/noctis-macros/src/route.rs`, `component.rs`): for `#[button]` and the five select attributes, `response` and `auto_defer` leave `RUNTIME_ARGS`' "not supported yet" error:
  - `response = update | ephemeral | public` (bare ident), at most once; `auto_defer = true | false` (bool literal), at most once.
  - Errors (CID-03, span on the value or the repeated key): ``[CID-03] `response` takes `update`, `ephemeral` or `public` ``; ``[CID-03] `auto_defer` takes `true` or `false` ``; ``[CID-03] #[button] argument `response` is given twice``.
  - The derive emits `const RESPONSE: __Response` on `__Component` (`#[doc(hidden)] pub enum __Response { Update, Ephemeral, Public }` in `kind.rs`) and `type Defer = Manual` for `auto_defer = false`. `RouteRuntime` stays `EMPTY` (it carries the Ref-related runtime args of Story 7.5).
  - `#[modal]` keeps the "runtime argument … not supported yet" error for `response`/`auto_defer` (Story 7.8); a modal handler's declared response is `Update`, its mode `AutoDefer`.
- **Handler binding** (`kind.rs`, `crates/noctis-macros/src/handler.rs`):
  - Components: shapes `(self, ctx: ComponentCtx)` and `(self, ctx: ComponentCtx, r: Responder<Component, Pending, D>)`, returning `HandlerResult<R: IntoResponse<Component>>`. Modals: `(self, ctx: ModalSubmitCtx)` and `(self, ctx: ModalSubmitCtx, r: Responder<ModalSubmit, Pending, D>)`, `R: IntoResponse<ModalSubmit>`. `__CX`/`__CXR` stay unimplemented (`__ComponentParams`/`__ModalParams` notes updated: ``component handlers take `(self, ctx: ComponentCtx[, r: ComponentResponder<..>])` ``, modal likewise; no "Story 7.2" text).
  - `#[handler]` recognizes `ComponentResponder` and `ModalResponder` (last path segment) as responders, like `CommandResponder`/`Responder`.
  - INT-05 checks become kind-generic: `impl<K> __ManualTakesManual<Responder<K, Pending, Manual>> for Manual`, `impl<K, D> __ManualIsDeclared<D> for Responder<K, Pending, AutoDefer>`, `impl<K> __ManualIsDeclared<Manual> for Responder<K, Pending, Manual>`. Messages: ``[INT-05] `auto_defer = false` is declared, so the handler must take the `Manual` responder (`CommandResponder<Manual>` or `ComponentResponder<Manual>`)``; ``[INT-05] this handler takes a `Manual` responder, so its attribute must declare `auto_defer = false` ``; ``[INT-05] `auto_defer = false` handlers take a `Manual` responder``. Re-bless the existing INT-05 `.stderr` files and read the diff.
- **Descriptors** (`component.rs`): `ComponentHandler`/`ModalHandler` store the declared response, `auto_defer: bool`, `signed: bool`, and a start that builds `Responder<_, Pending, D>` over the interaction's `Shared` and returns a `HandlerFuture` resolving to `ResponseValue`. `__new` (tests) keeps working with defaults (`Update`, `AutoDefer`, handler without responder); add hidden `__new_with_responder::<S, D, F, Fut, R>` constructors used by `__Bind`. `__testing::call_component`/`call_modal` keep passing a detached responder.
- **Observability (AD-29).** The whole run (rejection notice, auto-defer task, handler, response send, error notice) is inside span `noctis.interaction` (`app`, `shard`, `guild_id`, `module`, `route`). `noctis_handler_duration_seconds{kind="component"}` covers handler + send of the returned message; `noctis_handler_errors_total{kind="component"}` counts non-user errors, panics, cancellations and failed sends of a returned message. A handler error/panic sends the localized ephemeral notice exactly like commands (`notice::key_for`), before the timer is dropped.
- **Docs and skill in the same commit:** `docs/discord-rules.md` rows CID-06 (notice, key ring, the two build violations), INT-01 (type 6 for components), INT-04, INT-05 ("Implemented for components" sentence), INT-13 (`with_components` never sent); spine AD-6 runtime-arguments sentence: `response`/`auto_defer` ship on component attributes via `__Component::RESPONSE` + `__Kind::Defer` (`RouteRuntime` for Story 7.5's args). Usage Skill: `responder.md` (component/modal flows table, equivalences, `UpdateUnavailable`, `_with_response`), `routes-payloads-refs.md` (drop the "Current limits" block for 7.2 items; `response`/`auto_defer` no longer planned), `app-and-sharding.md` (`payload_key`, env vars, **64 hex digits**, `openssl rand -hex 32`), `troubleshooting.md` (rejection row: notice now sent; the two build texts), `modules-middleware-ratelimits.md` L43. Template: `bootstrap.rs` writes 64 lowercase hex digits (drop the `base64` dependency from its `Cargo.toml`), `README.md` env table, `.env.example` comment, handlers use `ComponentResponder`/returned messages consistently; template `AGENTS.md` one line on `response`/`auto_defer`. Re-bless `xtask/public-api.txt`, update the skill's `API hash`.

**Never:**
- Stale/expired routing or the Expired Handler (7.3), App ids (7.4), Ref minting/resolution (7.5), Middleware (7.6), Guards (7.7), `response`/`auto_defer` on `#[modal]`, `ModalResponder<Manual>` fixtures and Radio/Checkbox E2E (7.8), `ctx.respond`/`ctx.update`/`ctx.reply_text` and `AlreadyResponded` (8.1), deterministic-render snapshots (8.2), multipart attachments (9.1).
- New external crates; a debug build; logging, `Display`ing or span-recording a key or token; any per-message closure, task or timer beyond the one auto-defer task per interaction (aborted at run end); a second callback for one interaction.
- Reading the source message through twilight's `Message` (V2 component trees may not parse): read `message.id`, `message.channel_id`, `message.flags` from the raw interaction JSON with a small serde struct.

## I/O & Edge-Case Matrix

Fixtures: `Buy` = `#[derive(Button)] #[button("shop.buy", defaults(style = Primary, label = "Buy"))] struct Buy { item: u64 }` (`0shop.buy:47` = item 255); `Ban` = same with `response = ephemeral`; `Pub` = `response = public`; `Open` = `auto_defer = false` taking `ComponentResponder<Manual>`; `Sig` = a `signed` button; `Bio` = `#[derive(Modal)] #[modal("bio", ..)]`. "notice" = the CID-06 type-4 ephemeral callback. Source message V2 = flags 32768, legacy = flags 0, ephemeral = flags 64|32768. D = ack deadline (2000 ms, paused clock).

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | Malformed id | `""`, 101 chars, `0shöp:1`, `0Shop.buy:47`, flag-2 set `2shop.buy:47`, no `data` | warn `malformed` + notice each; handler not run | no panic |
| 2 | Unknown / family / kind | `0nope:`; modal id in a click; Button id with component_type 3 | warn `unknown_route` / `unknown_route` / `kind_mismatch` + notice | — |
| 3 | Undecodable | `0shop.buy:` (missing field), `0shop.buy:47,9` (extra), `0shop.buy:!!` | warn `undecodable` + notice | — |
| 4 | Modal invalid | submit JSON without `custom_id`; required input missing | warn `malformed` / `submit_invalid` + notice | — |
| 5 | No author | click without `member`/`user` | warn `no_author` + notice | — |
| 6 | Stale / Ref | schema-1 id for schema-0 Route; `by_ref` id | warn `stale` / `ref_unresolved`, **no** request | 7.3/7.5 |
| 7 | Notice locale | rejection with `locale: "tr"`; none + `guild_locale: "de"`; none; `"xx-YY"` | text resolved tr / de / en-US / en-US (Builtin falls back to en-US text) | — |
| 8 | Notice send fails | fake answers 500 to the notice | one `warn` "could not send the error notice"; no retry; no panic | log |
| 9 | Signed, valid | `Sig` id tagged with current key | handler runs | — |
| 10 | Signed, rotation | tagged with previous key; ring `current+previous` | handler runs | — |
| 11 | Signed, rotation over | tagged with previous key; ring has only current | warn `forged` + notice | — |
| 12 | Signed, bad / missing tag | last tag char flipped; tag and `T` flag stripped | warn `forged` (BadTag) / warn `forged` (MissingTag, codec check 4 precedes the flag check) + notice each | — |
| 13 | No key, signed mounted | `Sig` mounted, no key | `BuildError` with `MissingPayloadKey { route: "sig…" }` `[CID-06]` | build fails |
| 14 | Previous without current | only `previous_payload_key` | `PreviousKeyWithoutKey` | build fails |
| 15 | Env key | `NOCTIS_PAYLOAD_KEY` = 64 hex lower; 64 hex upper; 63 chars; 65 chars; `0x` + 62 hex; 64 chars with `g` at index 10; `"  <64hex>  "`; `""` | ok; ok; `InvalidPayloadKey` Length 63; Length 65; InvalidChar 1; InvalidChar 10; ok (trimmed); unset (no key) | error text names the var, never the value |
| 16 | Key secrecy | full flow at TRACE with key `ab`×32 | no log line, span field or `Debug` of App/AppBuilder contains `abab` | test |
| 17 | Send-side signing | handler returns a message with `Sig{..}.build()` | sent `custom_id` verifies with the key; with no key → impossible (build fails, #13) | — |
| 18 | Select nonce | message with two selects | both ids carry the same 2-char nonce, different across two sends (probabilistic: 3 sends not all equal) | — |
| 19 | Ref slot at send | `by_ref` component in a message | `ResponderError::Encode` naming the Route, zero requests | handler error → notice |
| 20 | update, V2 source | `Buy` + `r.update(msg)` | one POST callback `{"type":7,"data":{flags:32768, components, allowed_mentions:{parse:[]}, attachments:[]}}` | — |
| 21 | update, legacy source | source flags 0 | type-7 data adds `content:null, embeds:[], poll:null, sticker_ids:[]` | — |
| 22 | update with Legacy body on V2 source | `r.update(LegacyMessage::new("x"))` | `Invalid(V2ToLegacy)`, zero requests | handler error → notice |
| 23 | update, ephemeral source | source flags 64\|32768 | type-7 flags = 32768 only; later expiry treats original as ephemeral | — |
| 24 | defer_update | `r.defer_update()` then `done()` | POST `{"type":6}`, no `data` | — |
| 25 | Equivalence update | A: `update(m)`; B: `defer_update()` + `edit_reply(m)` | B = `{"type":6}` then `PATCH /webhooks/{app}/{token}/messages/@original` whose body equals A's type-7 `data` | test |
| 26 | Equivalence reply | A: `reply_as(m, e)`; B: `defer_reply_as(e)` + `edit_reply(m)` (e ∈ Public, Ephemeral) | B type 5 (`flags:64` only if Ephemeral) + PATCH body = A's data minus `EPHEMERAL` | test |
| 27 | reply on component | `Buy` + `r.reply(m)`; `Ban` + `r.reply(m)`; `Buy` + `reply_as(m, Ephemeral)` | type 4 public / type 4 flags 32832 / type 4 flags 32832 | — |
| 28 | defer_reply on component | `Buy`; `Ban` | `{"type":5}`; `{"type":5,"data":{"flags":64}}` | — |
| 29 | Auto-defer update | `Buy` sleeps past D then `r.update(m)` | `{"type":6}` at D (`noctis_autodefer_total` +1), then PATCH @original with V2 flag | — |
| 30 | Auto-defer ephemeral / public | `Ban`, `Pub` sleep past D, return `m` | `{"type":5,"data":{"flags":64}}` / `{"type":5}`, then PATCH @original | — |
| 31 | Handler answers before D | `Buy` updates at D−1 ms | only the type-7 callback; no type 6 ever (task aborted) | — |
| 32 | reply after type-6 auto-defer | `Buy` sleeps past D, `r.reply(m)` | type 6, then `POST /webhooks/{app}/{token}` (follow-up, public) — never an edit of the component message | — |
| 33 | defer_reply / defer_update after ack | after auto-defer | no second callback (debug log) | — |
| 34 | Returned message | `Buy` returns `m`; `Ban` returns `m`; `Buy` returns `Responded` without calling anything | type 7; type 4 flags 32832; nothing sent (and auto-defer still fires at D if run outlives it) | — |
| 35 | Manual component | `Open` (auto_defer=false), `r.show_modal(form)` | one `{"type":9,"data":{custom_id,title,components}}`; no auto-defer at D | — |
| 36 | Manual, late | `Open` sleeps 3 s, never answers | exactly one `error!` on `noctis::dispatch` with `handler` and `module`; no callback | log |
| 37 | Manual, defers itself | `Open` calls `defer_update()` / `defer_reply()` | type 6 / type 5 (public, since response = update) | — |
| 38 | Manual + response | `#[button(.., response = ephemeral, auto_defer = false)]`, `r.reply(m)` | type 4 flags 32832 | — |
| 39 | show_modal after ack | unit test: a `ComponentResponder<Manual>` whose `Shared` was already acked (a `defer_update` through a second handle, the gate race) calls `show_modal` | `ModalUnavailable`, no type-9 request | typed error |
| 40 | Modal from component | `Bio` submit with `message` (V2), handler `r.update(m)` | type 7 | — |
| 41 | Modal from slash | `Bio` submit without `message`: `update`, `defer_update`, `update_with_response` | each `Err(UpdateUnavailable)`, zero requests, responder still Pending-usable (`reply` afterwards → type 4) | typed error |
| 42 | Modal auto-defer | slow `Bio` with source; without source | `{"type":6}`; `{"type":5}` (public) | — |
| 43 | Modal returned message | `Bio` returns `m` with source; without source | type 7; type 4 public | — |
| 44 | with_response | `reply_with_response`, `defer_reply_with_response`, `update_with_response`, `defer_update_with_response` | request query `with_response=true`; `CallbackResource { kind: 4/5/7/6, message }` parsed from the fake's `{"resource":{"type":..,"message":{..}}}`; body without `resource` → `Transport` | — |
| 45 | with_response after ack | auto-defer fired, then `update_with_response(m)` | PATCH @original, returns `None` resource | — |
| 46 | Headers | every callback / webhook request in rows 20-45 | no `authorization` header; no `with_components` query; channel bot-token routes (row 48) do carry `Bot …` | test |
| 47 | Follow-ups | `follow_up(m, Ephemeral)` after update | `POST /webhooks/{app}/{token}` query `wait=true` only, flags 32832 | — |
| 48 | Token expired | after 15 min: `edit_reply` after `update` on public source; on ephemeral source; `follow_up` | `PATCH /channels/{c}/messages/{source_id}` with `Bot` auth; `TokenExpired` (no request) ×2 | typed error |
| 49 | Handler error / panic | `Err(HandlerError::new(..))` before any ack; panic after type-6 auto-defer | notice type 4 ephemeral; ephemeral follow-up and the component message is **not** deleted; sink `Error`/`Panic`, `errors_total{kind="component"}` +1 | sink |
| 50 | User error | `Err(HandlerError::user(key))` | localized notice with `key`; `debug` only, no sink, no counter | — |
| 51 | Cancelled | shutdown during handler | sink `Cancelled`, no notice | sink |
| 52 | Macro args | `response = update/ephemeral/public`; `auto_defer = true/false`; omitted | compile; consts/Defer as declared; defaults Update/AutoDefer | — |
| 53 | Macro bad args | `response = edit`; `response = "update"`; `auto_defer = 1`; `auto_defer = yes`; `response` twice; `auto_defer` twice; on `#[modal]` | the CID-03 texts above; modal keeps "not supported yet" | compile error (macro unit tests + trybuild `ui/cid/CID-03_component_bad_response.rs`) |
| 54 | INT-05 components | `auto_defer = false` + `ComponentResponder`; `ComponentResponder<Manual>` without the flag; flag + no responder; `ComponentResponder` + `show_modal`; `ModalResponder` + `show_modal`; `CommandResponder::update` | E0277 INT-05 ×3; E0277 `[INT-05] show_modal needs…`; E0599; E0599 (existing) | trybuild `ui/int/INT-05_component_*.rs`, `INT-04_modal_show_modal.rs` |
| 55 | Wrong responder kind | component handler taking `CommandResponder`; modal taking `ComponentResponder` | E0277 "the parameters of this `#[handler]` method do not fit" | `tests/types/component_command_responder.rs` |
| 56 | Allocation guard | Story 7.1 `alloc_dispatch` with signed ids rejected by the probe | still 0 blocks / 0 bytes | test |
| 57 | Memory flat | Story 7.1 `memory_flat` with handlers that `update` over a detached/fake transport | still flat | test |

</intent-contract>

## Code Map

- `crates/noctis-core/src/responder.rs` -- kinds, aliases, typestate impls (L454-639 today: `Responder<Command, Pending, D>` methods become kind-generic), `Shared` (L751+: add `source`, `Declared`, `update`/`defer_update`, reply-after-type-6 → follow-up, edit keep-list, `prepare` → send-side `CustomId` resolution with keys + nonce), `notice` (L1136: type-6 case), `auto_defer` (L962: type 6 vs 5), `CallbackResource`, `UpdateUnavailable`.
- `crates/noctis-core/src/responder/transport.rs` -- `callback(.., with_response)` (L264) returns the parsed resource for `_with_response`; `follow_up` L182 drop `with_components`; `callback_message_id` L462 reused.
- `crates/noctis-core/src/responder/tests.rs` -- existing fixture harness (`Fixture::interaction`, `requests()`, `skip`); add component/modal rows 20-48; update edit-body assertions for `attachments: []`.
- `crates/noctis-core/src/pipeline.rs` -- `arm` (L173) generalize over kind/declared response/source; `execute` (L295) pattern for notice-before-timer-drop, `send_notice` (L407), `report_logged`; module doc "Components and modals" section.
- `crates/noctis-core/src/pipeline/component.rs` -- rejection → `warn` + notice (`reject` L187 gains the transport parts); stage 4 builds `Shared` (interaction transport, followup cap as commands, source from raw JSON), arms the timer, runs `target.start(parts, &decoded, shared, keys)`, sends a returned message, error notices; update module doc (L1-23).
- `crates/noctis-core/src/component.rs` -- descriptors (L202-374): response/auto_defer/signed fields, responder-taking start, `__new_with_responder`; `ComponentCtx` doc L123 ("Responder parameters arrive with Story 7.2" removed).
- `crates/noctis-core/src/kind.rs` -- `__Component::RESPONSE`, `__Response`, `__Bind` impls for `__CR<Responder<Component|ModalSubmit, Pending, D>>` (replace the `no_extra!` `__CR` arm), generic INT-05 traits (L531-583).
- `crates/noctis-core/src/registry.rs` -- signed-Route and key build violations (fold has the Route map from 7.1).
- `crates/noctis-core/src/app.rs` -- `payload_key`/`previous_payload_key`, `from_env` (L158; env consts L53-65), `AppShared` keys; `crates/noctis-core/src/error.rs` -- `EnvError::InvalidPayloadKey`, the two `BuildViolation`s.
- `crates/noctis-core/src/lib.rs` -- re-exports (L240), `__testing` seams (keep `call_component`/`call_modal`, `dispatch`, `route_table`).
- `crates/noctis-core/src/handler.rs` -- `AppShared` (keys field).
- `crates/noctis-ui/src/route.rs` L284-357 -- add `CustomId::resolve_for_send` (hidden) and shape access through `ErasedRoute`.
- `crates/noctis-codec/src/{key.rs, layout.rs L447 verified_payload, parts.rs SelectNonce}` -- reused read-only.
- `crates/noctis-macros/src/route.rs` (`RUNTIME_ARGS` L41, `parse_arg` L299), `component.rs` (emit `RESPONSE`, `scaffolding(.., manual)` L117), `handler.rs` (responder recognition L10-74), `kind.rs` -- macro side; their unit tests (component.rs L366-381 expect the old error for `response`/`auto_defer`: update).
- `crates/noctis/src/lib.rs` -- facade re-exports (L516), prelude (L1600), `Button` derive docs (L1087) gain a `response`/`auto_defer` example.
- `crates/noctis/tests/ui/{cid,int}/`, `crates/noctis/tests/types/` -- trybuild fixtures (rows 53-55); re-bless INT-05 `.stderr`.
- `crates/noctis-testkit/tests/components.rs` -- extend Story 7.1 harness (`FakeGateway.dispatch("INTERACTION_CREATE", ..)`, `FakeDiscordHttp` recorded requests with headers/query) for rows 1-5, 9-11, 20, 29, 32, 35-36, 40-43, 46, 49.
- `examples/project-template/{src/bootstrap.rs, Cargo.toml, README.md, .env.example, AGENTS.md, src/modules/**/handlers.rs}`, skill references listed in Boundaries; `xtask/public-api.txt`.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-ui/src/route.rs` -- `CustomId::resolve_for_send` + unit tests (select/signed/plain/ref-slot/missing signer).
2. `crates/noctis-core/src/{app.rs, handler.rs, error.rs, registry.rs}` -- key ring, env parsing, build violations; unit tests rows 13-16.
3. `crates/noctis-core/src/responder.rs`, `responder/transport.rs` -- kinds, typestate, state machine, keep-list, `with_components` removal, `_with_response`, `CallbackResource`, send-side resolution; unit tests rows 17-34, 39, 41, 44-48 in `responder/tests.rs` (plus `compile_fail` doctests for `ComponentResponder::show_modal` without `Manual` and `ModalResponder::show_modal`).
4. `crates/noctis-core/src/{kind.rs, component.rs}` -- descriptors, bindings, generic INT-05 traits.
5. `crates/noctis-core/src/{pipeline.rs, pipeline/component.rs}` -- notice on rejection, `arm`, returned-message send, error notices; unit tests rows 1-8, 35-38, 42-43, 49-51 in `pipeline/tests.rs`.
6. `crates/noctis-macros/src/{route.rs, component.rs, handler.rs, kind.rs}` -- args and responder recognition; macro unit tests row 52-53.
7. `crates/noctis/src/lib.rs` -- re-exports, prelude, derive docs.
8. trybuild: `crates/noctis/tests/ui/cid/CID-03_component_bad_response.rs`, `ui/int/INT-05_component_auto_defer_false_without_manual.rs`, `INT-05_component_manual_without_auto_defer_false.rs`, `INT-05_component_auto_defer_false_without_responder.rs`, `INT-05_component_show_modal_needs_manual.rs`, `INT-04_modal_show_modal.rs`, `tests/types/component_command_responder.rs` (+ `.stderr`, bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`, read every diff).
9. `crates/noctis-testkit/tests/components.rs` -- end-to-end wire rows listed in the Code Map; zero-request assertions for rows 6, 19, 22, 41.
10. Docs, spine AD-6 sentence, Usage Skill, template (bootstrap hex + drop `base64`), template `AGENTS.md`; then `cargo run --release -p xtask -- api-snapshot --bless` and the skill `API hash`.
11. Every gate in Verification.

**Acceptance Criteria:**
- Given a mounted `Buy` and a `FakeGateway` click with `custom_id` `0nope:`, when it is dispatched, then the handler does not run, one `warn` with `reason="unknown_route"` is logged, and `FakeDiscordHttp` records exactly one `POST /api/v10/interactions/{id}/{token}/callback` whose body is the type-4 notice with flags 32832 and no `authorization` header.
- Given `NOCTIS_PAYLOAD_KEY` = key A and `NOCTIS_PAYLOAD_KEY_PREVIOUS` = key B with a mounted `Sig`, when clicks tagged with A, with B and with a flipped tag arrive, then the first two run the handler and the third is `forged` with the notice; no log line or span field contains the key's hex.
- Given `Buy` whose handler takes `ComponentResponder` and calls `update(m)`, when clicked on a V2 source message, then exactly one type-7 callback carries `flags: 32768`, `allowed_mentions: {"parse": []}`, `attachments: []`, and no `EPHEMERAL`.
- Given `Ban` (`response = ephemeral`) sleeping past the ack deadline and then returning `m`, when clicked, then the wire shows `{"type":5,"data":{"flags":64}}` then `PATCH …/messages/@original` with the V2 flag.
- Given `Open` (`auto_defer = false`, `ComponentResponder<Manual>`), when clicked and it calls `show_modal`, then only a type-9 callback is sent; when it never answers, exactly one error naming the handler is logged at the 2.9 s deadline.
- Given a submit of a slash-opened modal, when its handler calls `update`/`defer_update`, then each returns `UpdateUnavailable` and the fake records no request for them.
- Given the trybuild suites, when `cargo test --release -p noctis --test ui` runs, then rows 53-55 fail to compile with the stated `[CID-03]`/`[INT-05]` texts and every pre-existing fixture still passes (INT-05 `.stderr` re-blessed with generic wording).
- Given the docs, skill and template, when `rules-catalog`, `skill-check` and `api-snapshot` run, then all pass, and no `[planned, Story 7.2]` marker remains in the skill.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Scope split with 7.8 (decision).** 7.2 ships `ModalResponder` (needed for the story's `UpdateUnavailable` wire row and the modal auto-defer fallback); `response`/`auto_defer` on `#[modal]`, `ModalResponder<Manual>` fixtures and Radio/Checkbox E2E stay in 7.8. Modal handlers here are `Update` + `AutoDefer`.
- **Reply after a type-6 ack is a follow-up (decision).** `@original` is then the component's message; turning `reply` into an edit would overwrite the panel the user clicked. A follow-up keeps "reply = new message".
- **Default ephemerality of `reply` under `response = update` is `Public` (decision)**, matching discord.js `reply` without flags.
- **Returned message of a slash-opened modal with `update` is a type-4 public reply (decision)**, the same fallback the auto-defer uses; only the explicit `update`/`defer_update` (and Story 8.1's `ctx.update`) return `UpdateUnavailable`.
- **V2 → legacy stays a runtime `UiError::V2ToLegacy` (decision):** the source message's format is runtime data, so no type can carry it; CV2-01's row already documents the runtime error.
- **Send-side signing and select nonces (decision).** Without resolving unresolved `CustomId`s at send, no select kind and no `signed` Route could ever be sent; 7.2 resolves with the key ring and one CSPRNG nonce per send. Story 8.2 keeps its determinism/freshness tests on top.
- **Key format is 64 hex digits (decision).** The shipped codec (`PayloadKey::from_hex`, Story 2.4) defines `NOCTIS_PAYLOAD_KEY`; the template/skill's base64 text was wrong and is fixed; no base64 crate in core.
- **Runtime args carried by `__Component::RESPONSE` + `__Kind::Defer` (decision)**, not codec's `RouteRuntime`: `response` is a core concept and codec sits below core; `RouteRuntime` keeps Story 7.5's `ttl`/`one_shot`.
- **`_with_response` returns `Option<CallbackResource>`:** `None` names the "already acknowledged, no callback" case without a new error; only the four discord.js methods get variants (no `reply_as_with_response`).
- Golden wire (row 25):
  ```text
  A: POST /interactions/{id}/{tok}/callback  {"type":7,"data":D}
  B: POST /interactions/{id}/{tok}/callback  {"type":6}
     PATCH /webhooks/{app}/{tok}/messages/@original  D
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild, `alloc_dispatch`, `memory_flat`, testkit components included)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok (API hash matches)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `grep -rn "planned, Story 7.2\|planned: Story 7.2\|arrive with Story 7.2\|arrive with Stories 7.2" crates examples/project-template/.claude docs --include=*.rs --include=*.md` -- prints nothing
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
