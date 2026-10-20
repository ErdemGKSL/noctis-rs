---
title: 'Story 6.10: Simplification: remove the widget envelope and view-state support from shipped crates'
type: 'refactor'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: 'd5bdb24eb357efeae0cd966eae742f92e7e89cd6'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
warnings: ['oversized', 'multiple-goals']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Epics 1-6 shipped the withdrawn widget layer (the View envelope `E`, `view` Routes, `Flags::E`) in `noctis-codec`, `noctis-ui`, `noctis-macros` and the facade, and shipped the Command Responder under pre-discord.js names (`defer`, `modal`, `edit_original`, `followup`, `delete_original`) with `opens_modal`/`OpensModal`/`NoModal` and a bare `ephemeral` flag. Maintainer decisions §1-§2 (binding) withdraw the former and fix the latter. Implements correct course 2026-10-20 (FR-26..28 withdrawn, FR-25 revised), FR-21; AD-5, AD-6, AD-11, AD-19, AD-20 (withdrawn), AD-27; rules CID-01, CID-03, INT-01, INT-03, INT-04, INT-05, INT-06, INT-08, INT-10.

**Approach:** Two removal/rename passes, no new features beyond `reply_as`/`fetch_reply`:
1. Codec: delete every `E` item; layout becomes `F route ":" [S] [N] body [T]`; flag bit 2 is reserved and decodes as a new `CodecError::ReservedFlag`.
2. Responder: `Responder<K, S = Pending, D = AutoDefer>` with sealed `D = AutoDefer | Manual`, discord.js method names, and `auto_defer = true|false` plus `response = public|ephemeral` on `#[command]`, `#[user_command]`, `#[message_command]`.

The free-function authoring surface stays; Story 6.11 converts it.

## Boundaries & Constraints

**Always:**
- **Codec removals:** `ViewEnvelope`, `EnvelopeError`, `VIEW_ENV_LEN`, `Flags::E`, `RouteShape::view()`, `RouteShape::has_view()`, the `view` field of `RouteShape`, `Parts::with_envelope` and its `envelope` field, `Decoded::envelope()`, `EncodedId`/Ref-slot `envelope()` accessors, `RouteWidth::envelope()`, `Section::Envelope`, `EncodeError::{MissingEnvelope, UnexpectedEnvelope}`.
- **Constants:** `MAX_REF_ID_LEN` becomes 41 (1 + 24 + 1 + 1 + 2 + 12); its const assert changes to `== 41`. `MAX_LEN = FMT_LEN + route.len() + SEP_LEN + S + N + max(inline, by_ref)`. The width Display drops the `E` term: `shop.buy: F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)`.
- **Reserved flag:** `Flags::from_bits` masks to `0b1101`.
  - `parse_format` checks, in order: the generation (`F >= 16`, `CodecError::Generation`), then bit 2 → `CodecError::ReservedFlag`. Its Display is `format digit sets the reserved flag 2`.
  - This applies to both `decode` and `RefRecord` parsing, before the route is scanned.
  - Never a panic, never `DecodeError::Stale`.
  - `Flags` Debug letters are `S`, `N`, `T` (`Flags(SNT)` for all three).
- **Route derive and UI:**
  - `#[route(..)]` accepts only `by_ref`, `overflow = spill`, `schema = n`, `select` and `signed`. `view` and `opens_modal` are unknown arguments: ``[CID-03] unknown #[route] argument `view` `` with `= help: the codec arguments are `by_ref`, `overflow = spill`, `schema = n`, `select` and `signed``.
  - `RUNTIME_ARGS` = `ttl`, `one_shot`, `owner`, `permissions`, `guard`.
  - `CustomId::new` encodes at once when `!has_select() && !is_signed()`.
- **Responder type:** `Responder<K: ResponderKind, S: ResponderState = Pending, D: DeferMode = AutoDefer>`.
  - `pub enum Command {}` with no parameter; `IntoResponse<Command>` replaces `IntoResponse<Command<M>>`.
  - Sealed `pub trait DeferMode` with hidden `const AUTO_DEFER: bool`; `pub enum AutoDefer {}` (true) and `pub enum Manual {}` (false), both deriving `Debug, Clone, Copy, PartialEq, Eq, Hash`.
  - Sealed `pub trait CanShowModal: DeferMode`, implemented only for `Manual`, with `#[diagnostic::on_unimplemented(message = "[INT-05] show_modal needs auto_defer = false on this handler", label = "this handler auto-defers")]`.
  - `pub type CommandResponder<D = AutoDefer> = Responder<Command, Pending, D>`. `Acked` keeps `D`.
- **Methods:** no aliases for the old names.
  - **Pending:**
    - `reply(msg)`: type 4 with the declared response.
    - `reply_as(msg, Ephemerality)`: type 4 with that ephemerality.
    - `defer_reply()`: type 5 with the declared response.
    - `defer_reply_as(Ephemerality)`: type 5 with that ephemerality.
    - `show_modal(modal)`: type 9, `where D: CanShowModal`.
  - **Acked:**
    - `edit_reply(msg)`;
    - `fetch_reply() -> Result<twilight_model::channel::Message, ResponderError>`;
    - `delete_reply()`;
    - `follow_up(msg, Visibility) -> FollowupHandle`;
    - `done()`.
  - **Behaviour is unchanged apart from these renames.** Today's reply, defer, modal, edit, follow-up and delete code runs as before. A `reply`/`reply_as` after the auto-defer edits the original and keeps the auto-defer's ephemerality (INT-10). A `defer_reply*` after the auto-defer sends nothing.
- **`fetch_reply` transport rules** (`ResponderError::NoReply` is a new variant, Display `there is no reply message to fetch`):

  | Situation | Request | Auth |
  |---|---|---|
  | Interaction, token valid | `GET /webhooks/{app}/{token}/messages/@original` (`Route::GetInteractionOriginal`) | no bot token |
  | Token expired, public original with known id and channel | `GET /channels/{c}/messages/{id}` (`Route::GetMessage`) | bot token |
  | Token expired, otherwise | none; `TokenExpired` | — |
  | Dispatcher message transport, reply id known | `GET /channels/{c}/messages/{id}` | bot token |
  | Dispatcher message transport, no reply yet | none; `NoReply` | — |
  | Detached transport | none; `NoReply` | — |

  - Any request first passes `InteractionTarget::token()` (`InvalidToken`).
  - An error status is `Rejected` (status and code). A body that is not a twilight `Message` is `Transport`, with a `debug!` on `noctis::responder`.
  - A fetched public message's id and channel are stored in `Original` when it had none.
- **Handler arguments** (on `#[command]`, `#[user_command]`, `#[message_command]`):
  - `auto_defer = true | false`, a bool literal. Default `true`.
  - `response = public | ephemeral`, an ident. Default `public`.
  - The descriptors expose `auto_defer() -> bool`, replacing `opens_modal()`. `ephemerality()` stays: the declared response, fed by the hidden `__with_ephemerality`.
  - `auto_defer = false`:
    - arms no auto-defer, only the missed-deadline logger: `error!` on `noctis::dispatch` with fields `handler` and `module`, message ``an `auto_defer = false` handler did not answer within the 3 s ack deadline``;
    - leaves `defer_reply`/`defer_reply_as` callable;
    - keeps applying the declared response to `reply` and `defer_reply`.
  - The pipeline's `Resolved::opens_modal()` becomes `auto_defer()` and covers menus too.
- **INT-05 at expansion:**
  - The handler must take `CommandResponder<Manual>` exactly when `auto_defer = false` is written.
  - The macros emit `__assert_manual::<T>()` (hidden trait `__DeclaresManual`, implemented only for `Responder<Command, Pending, Manual>`) or `__assert_auto_defer::<T>()` (hidden trait `__DeclaresAutoDefer`, implemented only for the `AutoDefer` one) on the responder type.
  - The constructors' defer-mode turbofish is `_` with a responder and `AutoDefer` without one.
- **`#[autocomplete(auto_defer..)]` / `#[autocomplete(response..)]`** fail with `[INT-06] autocomplete cannot be deferred: `#[autocomplete]` takes no `auto_defer` or `response``. Any other argument keeps ``#[autocomplete]` takes no arguments`.
- **Docs and skill:**
  - Every rustdoc, comment, test name and doc example in `crates/` uses the new names. No mention of a View envelope, widgets or view state remains (the image `Template::view`/`ErasedView`/`view!` are unrelated and stay).
  - `docs/discord-rules.md`, the Usage Skill, the template and the `xtask` UD-3 lint are updated in the same change.
  - `xtask/public-api.txt` is re-blessed and the skill's `API hash` line updated.

**Never:**
- No deprecated aliases, no `#[route(view)]` acceptance, no envelope in any test generator.
- No change to the bot-token-skip/`with_components` behaviour of interaction requests (Story 7.2), to component/modal responders (Epic 7) or to the free-function surface (Story 6.11).
- No `ctx.respond`/`ctx.update` (Epic 8).
- No new dependency. No debug builds.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Reserved flag, plain | `decode("2a:1")`, `decode("3a:11")`, `decode("6a:001")`, `decode("Fall:z00x000001")` | `Err(CodecError::ReservedFlag)` each; `decode_route::<R>` → `DecodeError::Malformed(ReservedFlag)` | typed, no panic |
| 2 | Reserved flag vs earlier checks | `""`, 101 chars, `"2é:1"`, `"!a:1"`, `"Ga:1"` (generation 1, bit 2 of 16 is 0), `"Ia:1"` (F=18: gen 1, flags 2) | `Empty`, `TooLong{101}`, `NotAscii{1}`, `InvalidChar{0}`, `Generation{1}`, `Generation{1}` (generation wins) | typed |
| 3 | Reserved flag, malformed rest | `"2"`, `"2:1"`, `"2a"` | `ReservedFlag` (format checked before the route) | typed |
| 4 | Other flags still decode | `"0a:1"`, `"1a:11"`, `"4a:001"`, `"8a:000000"`, `"Dall:z00x000001"` (S+N+T = 13) | ok; `flags()` = `-`, `S`, `N`, `T`, `SNT`; route, schema, nonce, body, tag as encoded | — |
| 5 | Truncation sections | `"1a:"`, `"4a:0"`, `"8a:12345"` | `Truncated{Schema}`, `Truncated{Nonce}`, `Truncated{Tag}`; `Section` has no `Envelope` | typed |
| 6 | RefRecord flags | record raw `2r:x`, `Fr:z`, `4r:00`, `8r:000000` | `ReservedFlag`, `ReservedFlag`, `UnexpectedSection{Nonce}`, `UnexpectedSection{Tag}` | typed |
| 7 | Encode shapes | INLINE, `select`, `signed`, `select+signed`, `schema=61`, BY_REF, SPILL with `Parts` matching | ids with `F` ∈ {0,1,4,5,8,9,C,D}; `F & 2 == 0` always (proptest over all shapes and payloads) | — |
| 8 | Encode missing/unexpected parts | select Route without nonce; nonce on non-select | `MissingNonce`, `UnexpectedNonce` (envelope variants gone) | typed |
| 9 | Width, inline | `shop.buy` {u64,u8} | `F 1 + route 8 + sep 1 + S 0 + N 0 + body 14 + T 0 = 24 (limit 100)` | — |
| 10 | Width, by-ref worst case | 24-char route, `by_ref, select, schema = 61` | total 41 = `MAX_REF_ID_LEN`; encoded Ref id `.len() == 41` | — |
| 11 | Width, inline boundary | inline payload making total 100 / 101 | `assert_fits` passes / CID-01 const panic `worst case 101` | compile error |
| 12 | `#[route]` args | `view`; `view = true`; `opens_modal`; `colour`; `ttl = "1m"`; `select, signed, schema = 61, overflow = spill` | CID-03 unknown `view` (help lists 5 args); same for `view = true`; CID-03 unknown `opens_modal`; CID-03 unknown `colour`; runtime-arg CID-03; ok | compile error |
| 13 | CustomId resolves | INLINE / `schema = 3` / BY_REF / SPILL fitting / `select` / `signed` Route built as button | resolved at build: yes / yes / no (slot) / yes / no / no | — |
| 14 | `response` on `#[command]` | `response = public`; `response = ephemeral`; omitted | `ephemerality()` Public / Ephemeral / Public | — |
| 15 | `response` invalid | `response = update`; `response = "ephemeral"`; `response`; `response = loud`; `response = public, response = ephemeral` | `[INT-04] a Command cannot `update` its message: write `response = public` or `response = ephemeral``; then ``response` takes `public` or `ephemeral`` ×3; ``duplicate `#[command]` argument `response``` | compile error |
| 16 | Bare/legacy flags | `ephemeral`; `opens_modal` on `#[command]`, `#[user_command]`, `#[message_command]` | ``unknown `#[command]` argument `ephemeral`: write `response = ephemeral``; ``unknown … `opens_modal`: write `auto_defer = false` and take `CommandResponder<Manual>``` (attribute name per macro) | compile error |
| 17 | `auto_defer` values | `= true`; `= false` + Manual; omitted; `= 1`; `= "false"`; bare `auto_defer`; given twice | ok, `auto_defer()` true; ok false; ok true; ``auto_defer` takes `true` or `false`` ×3; duplicate error | compile error |
| 18 | INT-05 pairing | `auto_defer = false` + `CommandResponder`; `auto_defer = false` + no responder; `CommandResponder<Manual>` without flag; `auto_defer = true` + Manual; Manual + flag; also each on `#[user_command]` | E0277 `[INT-05] `auto_defer = false` is declared, so the handler must take `CommandResponder<Manual>``; expansion error `[INT-05] `auto_defer = false` handlers take `r: CommandResponder<Manual>``; E0277 `[INT-05] this handler takes `CommandResponder<Manual>`, so its attribute must declare `auto_defer = false`` ×2; ok | compile error |
| 19 | `show_modal` mode | `show_modal` on `CommandResponder` (AutoDefer) / on `Manual` | E0277 `[INT-05] show_modal needs auto_defer = false on this handler` / compiles | compile error |
| 20 | Typestate | second `reply` on Acked; `update` on CommandResponder; `defer_reply_as(CallbackFlags::EPHEMERAL)`; `edit_reply(m, Ephemerality::Ephemeral)` | E0599 (INT-03); E0599 (INT-04); E0308 (INT-08); E0061 (INT-10) | compile error |
| 21 | Auto-defer wire | `response = public` / `ephemeral`, handler idle past `ack_deadline` | callback `{"type":5}` / `{"type":5,"data":{"flags":64}}`; then returned message is `PATCH …/@original` | — |
| 22 | Declared response wire, Pending | `reply(m)` with public / ephemeral; `reply_as(m, Ephemeral)` on public; `reply_as(m, Public)` on ephemeral; `defer_reply()` both; `defer_reply_as(Ephemeral)` on public | type 4 flags without/with 64; with 64; without; type 5 without/with 64; with 64 | — |
| 23 | `auto_defer = false` wire | public / ephemeral, handler `reply(m)`; handler `defer_reply()` then `edit_reply`; handler idle 3 s | no auto callback ever; type 4 per declared; type 5 per declared then PATCH; one `error!` naming handler and module, no callback | log only |
| 24 | After auto-defer | `reply_as(m, Ephemeral)` on a public command after auto-defer; `defer_reply()` after auto-defer | `PATCH …/@original` (stays public); no request | — |
| 25 | `show_modal` runtime | Manual over interaction / over `Dispatcher` / after `defer_reply` race | type 9; `ModalUnavailable`, no request; (typestate prevents the race; runtime state check still returns `ModalUnavailable`) | typed |
| 26 | `fetch_reply` | valid token after `reply`; after `defer_reply`; 404 response; body `{}`; token 15 min old public with id+channel known; expired ephemeral; expired public id unknown; Dispatcher after reply; Dispatcher after typing defer; detached; bad token | `GET …/webhooks/{app}/{tok}/messages/@original` no auth → `Message`; same; `Rejected{404,..}`; `Transport`; `GET /channels/c/messages/id` with bot token; `TokenExpired` no HTTP; `TokenExpired` no HTTP; `GET /channels/c/messages/id`; `NoReply`; `NoReply`; `InvalidToken` | typed |
| 27 | Renamed calls wire | `edit_reply`, `delete_reply`, `follow_up(m, Public)` after reply | `PATCH`/`DELETE …/@original`, `POST /webhooks/{app}/{tok}?wait=true…` exactly as before the rename | — |
| 28 | Autocomplete args | `#[autocomplete(auto_defer = false)]`; `#[autocomplete(response = ephemeral)]`; `#[autocomplete(ephemeral)]`; `#[autocomplete(foo)]` | `[INT-06] …` ×2; ``#[autocomplete]` takes no arguments`` ×2 | compile error |
| 29 | Unicode/odd input | `decode("2é")`; `#[route("a", vïew)]` | `NotAscii{1}`; CID-03 unknown argument `vïew` | typed / compile error |

</intent-contract>

## Code Map

- `crates/noctis-codec/src/layout.rs`:
  - L1-32 module doc and constants (drop `VIEW_ENV_LEN`);
  - `Parts` L105-165;
  - `Checked`/`check_parts` L183-240;
  - `encode` flags L282-312;
  - `Decoded` L360-400 (`envelope` field and accessor);
  - `verified_payload` flag check L505-525 (expected/found over `N|T` only; rustdoc step 5 says `N`/`T`);
  - `parse_format` L568-577 (add the reserved check);
  - `decode` L697-708 (delete the `E` block) and its rustdoc error list;
  - tests L796-937 (`constants_match_ad_5` drops 5; `cid_06_decode_matrix` replaces the `2a:` rows with Matrix 1-5; the `all` case becomes `Dall:z00x000001`).
- `crates/noctis-codec/src/parts.rs`: `Flags` L18-90 (delete `E`, mask, Debug letters); delete `ViewEnvelope` L91-200 and its tests; module doc L1-2.
- `crates/noctis-codec/src/error.rs`:
  - `Section::Envelope` L89-101;
  - `StaleReason::Flags` docs L223-229 (`N`/`T`);
  - `EncodeError` L277-282;
  - delete `EnvelopeError` L357+;
  - add `CodecError::ReservedFlag` after `Generation`.
- `crates/noctis-codec/src/route.rs` L88-175: `RouteShape` (`view` field, `view()`, `has_view()`, docs, test L459-462).
- `crates/noctis-codec/src/derive.rs`: `RouteWidth` L49-180 (`envelope` field, accessor, Display term, sum); tests L405-490 (`View` route, `Long` total 46 → 41).
- `crates/noctis-codec/src/encoded.rs`: L15-35 (`MAX_REF_ID_LEN` 41), `envelope` slot field and accessor L60-92, `finish` flags L194-225, proptest L241-290 (drop the envelope generator).
- `crates/noctis-codec/src/record.rs` L110-140 (drop the `E` row; the reserved bit is caught by `parse_format`), tests L260-285.
- `crates/noctis-codec/src/lib.rs`: layout table L30-60 (flags `1/4/8`, bit 2 reserved), re-exports L175-190, the `MAX_REF_ID_LEN` (41) doc at L118.
- `crates/noctis-codec/tests/{common/mod.rs L72-91, alloc_decode.rs, layout.rs, refs.rs, tags.rs}`: drop the `View` routes and the envelope parts; add the reserved-flag rows to the alloc guard's rejected set.
- `crates/noctis-macros/src/route.rs`: `Args.view` L32, `RUNTIME_ARGS` L40-48, `ARGS_HELP` L50, arg parse L255-290, `SHAPE` emission L383-402, tests L449, L528-540, L631-640.
- `crates/noctis-ui/src/route.rs` L215-245 (`CustomId::new`), `src/lib.rs` L37-41; `tests/components.rs` L16, L64, L500-545.
- `crates/noctis-core/src/responder.rs`:
  - module doc and compile_fail doctests L1-70;
  - sealed module L88-98 (replace `Modal` with `Defer`);
  - markers L100-176 (replace with `DeferMode`/`AutoDefer`/`Manual`/`CanShowModal`/`__DeclaresManual`/`__DeclaresAutoDefer`/`__assert_manual`/`__assert_auto_defer`);
  - `Ephemerality` doc L178-205;
  - `ResponderError` (add `NoReply`);
  - `IntoResponse` impls L388-410;
  - `Responder` struct/alias L412-440;
  - Pending impls L455-525;
  - Acked impls L527-575;
  - `Shared::reply`/`defer` L831-880 (take an `Ephemerality` argument);
  - `modal` L927;
  - `edit_original`/`followup`/`delete_original` (rename internals to `edit_reply`/`follow_up`/`delete_reply`; add `fetch_reply`).
- `crates/noctis-core/src/responder/transport.rs`: rename `Transport::{edit_original, delete_original, followup}` (L104, L127, L150) to `edit_reply`, `delete_reply`, `follow_up`; add `Transport::fetch_reply(message_id)` beside them, plus `channel_fetch(app, c, id)` beside `channel_edit` L310. Reuse `send`, `http_error`. Every private helper named after an old method is renamed too, so the AC grep finds nothing.
- `crates/noctis-core/src/responder/tests.rs`: rename calls; add Matrix 22, 24, 26 tests (the `discord()`/`discord_with_message()` answer fixtures L31, L627).
- `crates/noctis-core/src/command.rs`:
  - `boxed` L292;
  - `CommandHandler` fields and `from_call` L310-350 (`opens_modal` → `auto_defer: D::AUTO_DEFER`);
  - the four `__new*` constructors L360-470 (`M: ModalMarker` → `D: DeferMode`);
  - `opens_modal()` → `auto_defer()` L526-530;
  - docs L211, L520, L541;
  - test L787.
- `crates/noctis-core/src/menu.rs` L100-225: `__new_user`/`__new_message` become generic over `D: DeferMode` with `CommandResponder<D>`; add an `auto_defer` field and accessor; docs L189-200.
- `crates/noctis-core/src/pipeline.rs`: module doc L14; `Resolved::opens_modal` L132-138 → `auto_defer()` (menus included); `arm` L153-195 (the log text).
- `crates/noctis-core/src/pipeline/tests.rs` L1504-1525: rename to `int_05_manual_never_auto_defers`. Add a menu `auto_defer = false` case and the Matrix 21/23 wire tests near `declared_ephemeral` L271 and `int_01_auto_defer_at_deadline` L1036.
- `crates/noctis-core/src/lib.rs` L215-230 re-exports: drop `ModalMarker`, `NoModal`, `OpensModal`, `__DeclaresNoModal`, `__DeclaresOpensModal`, `__assert_*`; add the new items.
- `crates/noctis-core/src/autocomplete.rs` L224 doctest comment (no change needed beyond names).
- `crates/noctis-macros/src/command.rs`:
  - `EXPECTED`/`OPENS_MODAL_RESPONDER` L27-33;
  - `Args` L35-42;
  - `parse_ephemeral`/`ephemerality_call` L87-108 (generalize to `parse_response`, `parse_auto_defer`, shared with `menu.rs`);
  - `parse_args` L141-210;
  - `modal_check` L629-655 → `defer_check`;
  - `constructor` turbofish;
  - tests L985-1006, L1231-1335.
- `crates/noctis-macros/src/menu.rs`: `PLANNED` L66 (drop `opens_modal`), `EXPECTED` L68, `Args` L74, parse L101-145, expand L210-285 (emit the assertion and the turbofish), tests L437-461.
- `crates/noctis-macros/src/autocomplete.rs` L60-66 (INT-06 message) and test L168-172.
- `crates/noctis/src/lib.rs`:
  - route example and doc L79, L88, L1151, L1834;
  - Responder docs L320-345;
  - root re-exports L464;
  - `#[command]` docs L765-855 (`response`, `auto_defer`, `show_modal` example asserting `!feedback().auto_defer()`);
  - menu docs L918-936;
  - prelude L1347-1362 (`OpensModal` → `Manual`).
- Facade tests:
  - `crates/noctis/tests/{commands.rs L300-430, menus.rs, derive.rs L30-46 L142-152, derive_hygiene.rs L39, boundary/cid.rs L6-70}`;
  - `crates/noctis-testkit/tests/commands.rs` L55-90, L326, L409, L733.
- trybuild, `crates/noctis/tests/ui/`:
  - `int/INT-03_second_reply`, `INT-04_command_update` (re-bless);
  - `INT-04_command_modal` (call `r.show_modal(())`, keep `// [INT-04]`);
  - rename `INT-05_modal_responder_without_opens_modal` → `INT-05_manual_without_auto_defer_false`, `INT-05_opens_modal_without_modal_responder` → `INT-05_auto_defer_false_without_manual`, `INT-05_opens_modal_without_responder` → `INT-05_auto_defer_false_without_responder`;
  - new `INT-05_show_modal_needs_manual`, `INT-04_command_response_update`, `INT-06_autocomplete_auto_defer`, `cid/CID-03_view_withdrawn`;
  - `INT-08_defer_flags` (→ `defer_reply_as`), `INT-10_edit_ephemerality` (→ `edit_reply`);
  - `cid/CID-03_unknown_argument.stderr` and any other CID-03 `.stderr` (re-bless).
- `xtask/src/skill_check/doctrine.rs`: module doc L15-21, `RESPONDER_CALLS` L64 → `["reply", "update", "defer_reply", "defer_update", "edit_reply"]`, `OpensModal` acceptance L408, tests L515-605.
- `docs/discord-rules.md`:
  - CID-01 L174 (formula without `E`, 41, drop the withdrawal note);
  - CID-03 L176 (no change needed beyond `view`);
  - INT-01 L156, INT-03 L158, INT-04 L159, INT-05 L160, INT-06 L161, INT-08 L163, INT-10 L165 (drop "shipped as / Story 6.10" clauses; new names);
  - §1 L313-362 (layout without `E`, flags `1/4/8` with "2 reserved: decodes as `CodecError`", table without `VIEW_ENV_LEN`, delete the withdrawal paragraph, formula, 41);
  - §2 L421-430 (width line, args list, drop the `opens_modal` sentence).
- Template:
  - `examples/project-template/src/modules/profile/handlers.rs` L1-5 (keep only the Story 6.11 note);
  - `.claude/skills/noctis/`:
    - `SKILL.md` (§8 L453-471: drop the three 6.10 rows and their sentence; `API hash` L23);
    - `references/responder.md` L84, L117, L187-192, L213-239, L264, L277, L298-310, L378, L401-402 (also add `reply_as`, `fetch_reply` rows);
    - `references/commands.md` L16-18, L95-110, L765-786;
    - `references/routes-payloads-refs.md` L95-98, L314-315, L330, L371, L403;
    - `references/limits-cheatsheet.md` L234 (41);
    - `references/modals.md` L156-157, L482-483;
    - `references/troubleshooting.md` L26, L103-104 (add the `ReservedFlag` and INT-06 rows);
    - `references/components-v2.md` (only if it names old items).

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-codec/src/{parts.rs,error.rs,route.rs,layout.rs,encoded.rs,record.rs,derive.rs,lib.rs}` and `crates/noctis-codec/tests/*`:
   - remove every `E` item;
   - add `CodecError::ReservedFlag` and the `parse_format` check;
   - set `MAX_REF_ID_LEN` = 41;
   - update the docs and tests.
   - Unit-test Matrix 1-10 (the new rows go in `cid_06_decode_matrix` and `record.rs` tests). The width test covers 9-10. The proptest asserts `F & 2 == 0` (7).
2. `crates/noctis-macros/src/route.rs`, `crates/noctis-ui/src/{route.rs,lib.rs}`, `crates/noctis-ui/tests/components.rs`: drop `view`; `opens_modal` becomes unknown (Matrix 12, 13, 29); macro unit tests.
3. `crates/noctis-core/src/responder.rs` (+ `transport.rs`, `tests.rs`), `command.rs`, `menu.rs`, `pipeline.rs` (+ `tests.rs`), `lib.rs`:
   - the new type parameters, markers, renamed methods, `reply_as`, `fetch_reply`, `NoReply`, `auto_defer()`;
   - unit tests for Matrix 21-27, including `int_05_manual_never_auto_defers` and `fetch_reply_*` tests.
4. `crates/noctis-macros/src/{command.rs,menu.rs,autocomplete.rs}`:
   - `response`/`auto_defer` parsing shared by both macros;
   - the INT-04/05/06 messages, assertions and turbofish;
   - macro unit tests for Matrix 14-18, 28.
5. `crates/noctis/src/lib.rs` and facade/testkit tests: new names, re-exports, prelude, docs; boundary passes for `auto_defer = false` commands and menus.
6. `crates/noctis/tests/ui/**`: add, rename and re-bless the fixtures listed in the Code Map (Matrix 12, 15-20, 28), using `TRYBUILD=overwrite cargo test --release -p noctis --test ui`. Then inspect every changed `.stderr`.
7. `xtask/src/skill_check/doctrine.rs`: UD-3 call list and `Manual`-only acceptance, with tests.
8. `docs/discord-rules.md`: the edits listed in the Code Map.
9. Template and Usage Skill: the edits listed in the Code Map. Then run `cargo run --release -p xtask -- api-snapshot --bless` and set `SKILL.md`'s `API hash` to the printed hash.
10. The final grep (AC 6), then every gate in Verification.

**Acceptance Criteria:**
- Given the codec, when `cargo test --release -p noctis-codec` runs, then the reserved-flag, width and 41-character tests pass, the decode proptests (whose generators produce no envelopes) pass, and `tests/alloc_decode.rs` (Story 2.6 zero-allocation guard, now with reserved-flag rejects) passes.
- Given a handler `#[command("feedback", description = "d", auto_defer = false)] async fn feedback(_: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> { Ok(r.show_modal(form).await?.done()) }` in an App with `FakeDiscordHttp`, when the interaction arrives, then exactly one type-9 callback is sent, no type-5 callback is ever sent, and `feedback().auto_defer()` is `false`.
- Given `#[user_command("Report", response = ephemeral, auto_defer = false)]` with `CommandResponder<Manual>`, when it runs end to end, then `ContextMenuHandler::auto_defer()` is `false`, `ephemerality()` is `Ephemeral`, and `r.defer_reply()` sends `{"type":5,"data":{"flags":64}}`.
- Given the trybuild suite, when it runs, then every fixture in `tests/ui/int/` and `tests/ui/cid/` matches its blessed `.stderr`, and the new `.stderr` files contain `[INT-04]`, `[INT-05]`, `[INT-06]` and `[CID-03]` as listed.
- Given `xtask rules-catalog` and `xtask skill-check`, when they run, then they pass. `skill-check` flags `.edit_reply(` on a non-`ctx` receiver and a `CommandResponder<OpensModal>` parameter, and accepts `CommandResponder<Manual>`.
- Given the repo, when `grep -rnE 'ViewEnvelope|VIEW_ENV_LEN|has_view|view envelope|#\[route\([^)]*\bview\b|opens_modal|OpensModal|NoModal|edit_original|delete_original|followup\(' crates docs README.md examples xtask` runs, then it prints nothing, except `xtask/src/skill_check/doctrine.rs` test strings that assert `OpensModal` is rejected.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Reserved flag as its own `CodecError` variant** (not `InvalidChar{0}` or `Generation`), so tests and logs name the cause. It is checked in `parse_format`, right after the generation check, so the result is a phase-1 error (`DecodeError::Malformed`) and never Stale.
- **Accessors kept.** `CommandHandler::ephemerality()` / `ContextMenuHandler::ephemerality()` / `CommandCtx::ephemerality()` keep their names and type. Commands only have public or ephemeral, so `Ephemerality` already is the declared `response`. Components get a richer response type in Epic 7.
- **`show_modal` uses a method-level bound** (`where D: CanShowModal`) instead of an inherent impl on `Manual` alone. That way rustc reports E0277 with the `[INT-05]` text instead of a bare E0599. Golden fixture:
  ```rust
  #[command("feedback", description = "d")]
  async fn feedback(_: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
      Ok(r.show_modal(form()).await?.done()) // error[E0277]: [INT-05] show_modal needs auto_defer = false on this handler
  }
  ```
- **`fetch_reply` returns `twilight_model::channel::Message`.** That is discord.js parity, and the facade already re-exports `twilight_model` as `noctis::model`. Test fixtures must answer with a complete message JSON (id, channel_id, author, content, timestamp, edited_timestamp, tts, mention_everyone, mentions, mention_roles, attachments, embeds, pinned, type). Story 7.2 may refine the matrix.
- **`INT-04_command_modal` is kept** (by-construction marker) with `show_modal`. Its `.stderr` now shows the INT-05 bound, which is expected: a Command responder without `Manual` cannot open a modal.

## Verification

**Commands:**
- `cargo fmt --all` -- no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (trybuild included under the featureless run)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt`
- `cargo run --release -p xtask -- skill-check` -- ok (API hash matches)
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- The grep in AC 6 -- empty

**Manual checks:**
- No CI job changes: `.github/workflows-disabled/ci.yml` already runs every gate above.
