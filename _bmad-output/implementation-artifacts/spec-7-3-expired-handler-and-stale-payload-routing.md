---
title: 'Story 7.3: Expired Handler and stale Payload routing'
type: 'feature'
created: '2026-10-21'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-7-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/routes-payloads-refs.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/responder.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Since Story 7.2 a stale `custom_id` (schema, body-kind or `N`/`T` flag mismatch against a mounted Route) and a Ref id of a `by_ref`/`spill` Route are only a `warn` with no answer, so a click on a pre-deploy message silently fails in the client. FR-18/FR-60 (AD-5, AD-23 stage 2) require them to reach a stale handler or the Expired Handler, which a bot author can override per Route or per App.

**Approach:**
- Two new method attributes in the Route struct's `impl`: `#[on_stale]` and `#[on_expired]` (no `self`), bound into the `ComponentHandler`/`ModalHandler` descriptor.
- A new App-level `AppBuilder::expired_handler(f)` and a default Expired Handler that sends the `noctis.expired` ephemeral notice.
- The component pipeline routes `Rejection::Stale` and `Rejection::RefUnresolved` (with no RefStore before Story 7.5, every Ref is missing) through the precedence chain. A hook or App handler runs like a handler: auto-defer, returned message per the Route's declared `response`, error notices, Error Sink.
- The Usage Skill documents stale/expired handling and the Payload evolution rules.

## Boundaries & Constraints

**Always:**
- **Precedence (exactly this):**
  - Stale id: Route `#[on_stale]` → Route `#[on_expired]` with `Expired::Stale` → App `expired_handler` with `Expired::Stale` → default notice.
  - Ref id (`PayloadSource::Ref` of a mounted Route): Route `#[on_expired]` with `Expired::RefMissing` → App `expired_handler` → default notice. `#[on_stale]` never runs for a Ref.
  - Every other `Rejection` keeps the 7.2 behaviour (warn + CID-06 generic notice). The checks that run before the Payload decode keep their order and win over stale: malformed, unknown Route, kind mismatch and no author.
- **Public types** (in `crates/noctis-core/src/component.rs` or a new `crates/noctis-core/src/expired.rs`; re-exported from `noctis-core`, the facade root and `noctis::prelude`):
  - `#[non_exhaustive] #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum Expired { Stale, RefMissing }` with `pub const fn as_str(self) -> &'static str` (`"stale"`, `"ref_missing"`).
  - `pub struct ExpiredCtx` (one `Arc`, cheap `Clone`) with the same accessors as `ComponentCtx`: `module`, `shard`, `guild_id`, `channel_id`, `user_id`, `user_locale`, `guild_locale`, `state::<T>()` (reuse the `ctx_accessors!` macro), plus `route(&self) -> &'static str`. Its `Debug` is like the other contexts and adds `route`.
- **Hook methods** (method attributes in `crates/noctis-macros`, re-exported as `noctis::on_stale`/`noctis::on_expired` and from the prelude):
  - Component Route shapes: `#[on_stale] async fn <any>(ctx: ComponentCtx[, r: ComponentResponder<D>]) -> HandlerResult<R>` and `#[on_expired] async fn <any>(ctx: ComponentCtx, expired: Expired[, r: ComponentResponder<D>]) -> HandlerResult<R>`, with `R: IntoResponse<Component>`.
  - Modal Route shapes: the same with `ModalSubmitCtx`, `ModalResponder<D>` and `R: IntoResponse<ModalSubmit>`.
  - Hooks inherit the Route's declared `response` and `auto_defer`. The same INT-05 assertions as `#[handler]` apply (`__assert_no_responder::<Self>()` / `__assert_responder::<Self, R>()`), so a hook of an `auto_defer = false` Route takes `ComponentResponder<Manual>` and may `show_modal`.
  - The attributes take no arguments. Each struct has at most one `#[on_stale]` and one `#[on_expired]`; a second one is a duplicate-definition compile error, as with two `#[handler]` methods.
- **Hook compile errors** (spanned; texts exact; every one is also a macro unit test):
  - receiver present → ``an `#[on_stale]` method takes no `self`: the Payload did not decode`` (likewise `` `#[on_expired]` ``);
  - not `async` → ``an `#[on_stale]` method must be `async` ``;
  - generic → ``an `#[on_stale]` method cannot be generic``;
  - arguments → ``` `#[on_stale]` takes no arguments ```;
  - wrong parameter count → ``an `#[on_stale]` method takes `(ctx[, r: <Kind>Responder<..>])` `` / ``an `#[on_expired]` method takes `(ctx, expired: Expired[, r: <Kind>Responder<..>])` ``;
  - on a command, context-menu or event struct → E0277 ``` `#[on_stale]` and `#[on_expired]` are methods of a component or modal struct ``` (label `not a component or modal struct`);
  - wrong ctx, `Expired` or responder type → E0277 ``the parameters of this `#[on_stale]`/`#[on_expired]` method do not fit `{S}` `` with a note listing the four shapes above.
- **Descriptor binding:**
  - `X::descriptor()` (emitted by `#[handler]`) returns the descriptor with its hooks attached, so `Module::add::<X>()` registers the handler and its hooks together.
  - Mechanism: inherent items emitted by the hook attributes shadow a blanket fallback trait in `kind.rs`, exactly like `__NoHandler`/`__NOCTIS_HAS_HANDLER` (see Design Notes).
  - `ComponentHandler`/`ModalHandler` get `on_stale`/`on_expired: Option<Arc<..>>` and hidden attach methods. `__new`/`__new_with_responder` keep working without hooks. `Debug` prints `on_stale: bool` and `on_expired: bool`.
- **App-level handler:**
  - Signature: `pub fn expired_handler<F, Fut, R>(&mut self, f: F) -> &mut Self where F: Fn(ExpiredCtx, Expired) -> Fut + Send + Sync + 'static, Fut: Future<Output = HandlerResult<R>> + Send + 'static, R: IntoResponse<Component> + IntoResponse<ModalSubmit>`.
  - The last call wins. It is stored erased in `AppShared`, and `AppBuilder`'s `Debug` shows `expired_handler: bool`.
  - It takes no responder and is always auto-deferred: armed like an `AutoDefer` handler shaped by the Route's declared `response`, even on an `auto_defer = false` Route.
- **Running a hook or the App handler** (same machinery as the handler; Story 7.2):
  - Builds the context from the same parts and the `Shared` with the Route's declared `response` and source message.
  - Arms the auto-defer, runs under `catch_unwind` raced against the cancel token, and sends a returned message per the declared `response`. For example, under `update` with a source message it is a type-7 update.
  - An error or panic sends the localized ephemeral notice, then goes to the Component Error Sink with `kind="component"`. A user error is `debug` only.
  - `noctis_handler_duration_seconds{kind="component"}` and `noctis_handler_errors_total{kind="component"}` cover the run.
  - The `Report`'s `handler` is the struct's path (`entry.handler.name()`).
  - Everything runs inside span `noctis.interaction` with `module` and `route` recorded.
- **Default Expired Handler:**
  - Sends one type-4 callback `{"type":4,"data":{"flags":32832,"components":[{"type":10,"content":<noctis.expired text>}],"allowed_mentions":{"parse":[]}}}` immediately: no timer and no handler. The text is in the User Locale, then the Guild Locale, else en-US (`notice::text`, key `keys::EXPIRED`).
  - It uses the key `noctis.expired` for both `Expired::Stale` and `Expired::RefMissing` (epics AC).
  - A failed send is one `warn` on `noctis::dispatch`: `could not send the expired notice`, with `reason` and `%error`. There is no sink entry and no counter.
- **Logging:**
  - Stale and Ref ids are no longer a `warn`. Each one is exactly one `debug!` on target `noctis::dispatch`, `"component interaction expired"`, with `reason` (`stale` | `ref_unresolved`), `shard`, `route` and `hook` (`on_stale` | `on_expired` | `app` | `default`).
  - `Rejection::sends_notice` and its doc are replaced by this routing. No `Rejection` variant is removed: the alloc probe labels stay.
- **Payload evolution (FR-60):** the codec behaviour is unchanged and must be proven end to end.
  - Appending an `Option` field keeps old ids decoding (as `None`).
  - Any other change without a `schema` bump is `undecodable`, which gets the generic notice.
  - A bumped `schema` makes old ids stale.
- **Docs, skill and template (same commit as the API):**
  - `docs/discord-rules.md` CID-06 row: stale ids and Refs go to the Expired Handler chain, and the unit-test names are added.
  - Spine AD-23 gets an `Amended (2026-10-21)` line naming the precedence and that the hooks are `impl` methods.
  - Usage Skill:
    - `routes-payloads-refs.md`: §1 "Untrusted ids" drops the 7.3 `[planned]`; the methods table gets the real signatures with no `[planned, Story 7.3]`; §6 Payload evolution; §8 rewritten with the precedence table and a compiling example that returns `V2Message`, with no `ctx.update`/`ctx.reply_text`.
    - `app-and-sharding.md`: `.expired_handler(f)` moves to the implemented table.
    - `troubleshooting.md`: rows L117 and L120.
    - `responder.md` L649.
    - `i18n.md`: the `noctis.stale` row reads "reserved; v1 sends `noctis.expired` for stale ids too".
  - Template:
    - `src/modules/setup/handlers.rs`: `SetupState` gets an `#[on_stale]` and an `#[on_expired]` that re-render step 1 for `ctx.user_id()`.
    - Template `AGENTS.md` gets one line on the hooks.
  - Re-bless `xtask/public-api.txt` and update the skill's `API hash`.

**Never:**
- RefStore lookup, Ref minting, TTL or one-shot (7.5); App ids (7.4); Middleware (7.6); Guards (7.7); `response`/`auto_defer` on `#[modal]` (7.8); `ctx.respond`/`ctx.update`/`ctx.reply_text` (8.1); testkit `is_expired_notice` (8.6).
- A hook receiving the decoded Payload or any part of the stale body; verifying a tag before routing a stale id (the codec checks schema and shape before the tag, by design); running the handler for a stale or Ref id.
- New external crates; a debug build; any per-message state, closure, task or timer beyond the one auto-defer task per interaction; a second callback for one interaction.
- Changing codec decode order or `StaleReason`; removing `keys::STALE`.

## I/O & Edge-Case Matrix

**Fixtures:**
- `Buy` = `#[button("shop.buy", defaults(style = Primary, label = "Buy"))] struct Buy { item: u64 }` (schema 0, no hooks; `0shop.buy:47` is item 255).
- `Fresh` = `#[button("fresh", ..)] struct Fresh { page: u16 }` with `#[on_stale]` returning `V2Message` "stale-view" and `#[on_expired]` returning "expired-view {expired:?}".
- `Late` = `#[button("late", ..)]` with only `#[on_expired]`.
- `Eph` = `Fresh`'s twin with `response = ephemeral` (`"eph"`).
- `Man` = `#[button("man", auto_defer = false, ..)]` whose `#[on_expired]` takes `ComponentResponder<Manual>`.
- `R` = `#[button("r", by_ref, ..)] struct R { x: u64 }` (no hooks).
- `RH` = `#[button("rh", by_ref, ..)]` with `#[on_stale]` and `#[on_expired]`.
- `V61` = `#[button("v61", schema = 61, ..)] struct V61 { a: u8 }`.
- `Bio` = `#[derive(Modal)] #[modal("bio", ..)]`; `BioH` = the same with `#[on_stale]` taking `ModalSubmitCtx`.
- `App` = `expired_handler(|ctx, e| async move { Ok(V2Message::new().text(format!("app {} {}", ctx.route(), e.as_str()))) })`.

**Notation:** "notice" = the CID-06 generic type-4; "expired notice" = the default Expired Handler type-4 with the `noctis.expired` text. Source message V2 = flags 32768. D = the ack deadline (2000 ms, paused clock).

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| 1 | Stale schema, no hooks, no App | `Buy`; `1shop.buy:147` | expired notice (exactly 1 request); handler not run; one debug `reason="stale" hook="default"`; no warn | — |
| 2 | Stale schema max digit | `Buy`; `1shop.buy:z47` (S=61) | as row 1 | — |
| 3 | Stale against schema 61 | `V61`; `0v61:1` (schema 0) and `1v61:11` (S=1) | expired notice each | — |
| 4 | Stale body kind | `Buy`; `0shop.buy:~0123456789A` | expired notice (Stale, not Ref) | — |
| 5 | Stale flags | `Buy`; `4shop.buy:0047` (N on a button); `8shop.buy:47abcdef` (T on unsigned) | expired notice each | — |
| 6 | Not stale: invalid schema digit | `1shop.buy:047` (S=`0`); `2shop.buy:47` (reserved flag); `Gshop.buy:47` (generation 1) | warn `malformed` + generic notice, never expired | 7.2 path |
| 7 | Not stale: unknown / kind / author | `1nope:147`; `1shop.buy:147` with component_type 3; stale id without `member`/`user` | `unknown_route` / `kind_mismatch` / `no_author` + generic notice | 7.2 path |
| 8 | Signed + stale | signed Route `sig`, ring A; id with schema 1 and a garbage tag | expired chain (stale is checked before the tag); no forged warn | — |
| 9 | on_stale wins | `Fresh`; `1fresh:11` on a V2 source | `on_stale` runs; one type-7 `{"type":7,"data":{flags:32768,components:[stale-view],allowed_mentions:{parse:[]},attachments:[]}}`; debug `hook="on_stale"` | — |
| 10 | on_expired for stale | `Late`; stale id | `on_expired` runs with `Expired::Stale`; type 7 "expired-view Stale" | — |
| 11 | App handler for stale | `Buy` + `App`; stale id | App runs with `ExpiredCtx::route() == "shop.buy"`, `Expired::Stale`; type 7 "app shop.buy stale" | — |
| 12 | Route beats App | `Late` + `App`; stale id | only `on_expired` runs | — |
| 13 | Ref, no hooks | `R`; `0r:~0123456789A` | expired notice; debug `reason="ref_unresolved" hook="default"` | — |
| 14 | Ref skips on_stale | `RH`; `0rh:~0123456789A` | `on_expired(Expired::RefMissing)` runs, never `on_stale` | — |
| 15 | Ref → App | `R` + `App`; Ref id | App runs with `Expired::RefMissing`, text "app r ref_missing" | — |
| 16 | Ref stale schema | `R`; `1r:1~0123456789A` | Stale (schema checked first) → expired notice | — |
| 17 | Ephemeral Route hook | `Eph`; stale id | type 4 with flags 32832 (V2 + EPHEMERAL) holding stale-view | — |
| 18 | Hook auto-defer | `Fresh` `on_stale` sleeps past D, returns view | `{"type":6}` at D, then `PATCH …/messages/@original` with the V2 flag | — |
| 19 | Hook answers before D | `Fresh` `on_stale` returns at D−1 ms | only the type-7 callback | — |
| 20 | App handler auto-defer | `App` sleeps past D on `Buy` (update) / on `Eph`-like Route | `{"type":6}` / `{"type":5,"data":{"flags":64}}`, then PATCH | — |
| 21 | Manual Route hook | `Man`; stale id; hook calls `r.show_modal(form)` | one type-9 callback; no auto-defer | — |
| 22 | Manual Route, App handler | `Man` without hooks + `App` sleeping past D | `{"type":6}` at D (App handler is always auto-deferred), then PATCH | — |
| 23 | Hook with responder | `#[on_expired] async fn e(ctx: ComponentCtx, x: Expired, r: ComponentResponder) -> HandlerResult<Responded>` calls `r.reply_as(m, Ephemeral)` | type 4 flags 32832 | — |
| 24 | Hook user error | hook returns `Err(HandlerError::user(key))` | localized notice with `key`; debug only; no sink, no counter | — |
| 25 | Hook error / panic | `Err(HandlerError::new(..))`; panic in hook; panic in App handler | generic notice; sink `Error`/`Panic` with origin Component; `noctis_handler_errors_total{kind="component"}` +1 | sink |
| 26 | Cancelled | shutdown token fired before / during hook | sink `Cancelled`, no notice | sink |
| 27 | Default notice locale | stale id with `locale:"tr"`; none + `guild_locale:"de"`; none; `"xx-YY"` | text of `noctis.expired` resolved tr / de / en-US / en-US (Builtin falls back to the en-US text) | — |
| 28 | Default notice send fails | fake answers 500 | one warn `could not send the expired notice`; no retry, no sink, no panic | log |
| 29 | Modal stale, hook | `BioH`; submit `1bio:147` with V2 source | `on_stale(ModalSubmitCtx)` runs; type 7 | — |
| 30 | Modal stale, slash-opened | `BioH`; submit without `message` | returned V2 → type 4 public (7.2 fallback) | — |
| 31 | Modal stale, no hooks | `Bio`; stale submit | expired notice | — |
| 32 | Evolution: append Option | Route `struct Buy { item: u64, qty: Option<u8> }` (no bump); `0shop.buy:47` | handler runs with `item = 255, qty = None` | — |
| 33 | Evolution: append non-Option | `struct Buy { item: u64, qty: u8 }` (no bump); `0shop.buy:47` | warn `undecodable` + generic notice (why the bump is needed) | 7.2 path |
| 34 | Evolution: remove field | one-field Route; `0shop.buy:47,9` | warn `undecodable` + generic notice | 7.2 path |
| 35 | Evolution: bump | `Buy` with `schema = 1`; old `0shop.buy:47` | stale → expired chain | — |
| 36 | Length bounds | stale id of exactly 100 chars (24-char route, padded body); 101 chars | expired chain / `malformed` + generic notice | — |
| 37 | Unicode | `1shöp.buy:147` | `malformed` + generic notice | — |
| 38 | Macro ok | `#[on_stale]` / `#[on_expired]` with each allowed shape on Button, each select kind, Modal | compiles; descriptor carries the hooks (`Debug` shows `on_stale: true`) | — |
| 39 | Macro errors | `self` receiver; non-async; generic; `#[on_stale(x)]`; zero params; `on_expired` without `Expired`; three params on `on_stale`; `CommandCtx` on a component | the exact texts in Boundaries | macro unit tests + trybuild `tests/types/on_stale_*.rs` |
| 40 | Wrong kind | `#[on_stale]` on a `#[derive(SlashCommand)]` / `EventHandler` struct | E0277 "are methods of a component or modal struct" | trybuild `tests/types/on_stale_on_command.rs` |
| 41 | Duplicate hook | two `#[on_stale]` in one struct | duplicate-definition error (E0592/E0201) | trybuild `tests/types/on_stale_twice.rs` |
| 42 | INT-05 hooks | `Man` hook without a responder; `auto_defer = true` Route hook taking `ComponentResponder<Manual>`; `ComponentResponder` hook calling `show_modal` | INT-05 E0277 texts (same as `#[handler]`) | trybuild `ui/int/INT-05_hook_*.rs` |
| 43 | Hook without handler | struct with `#[on_stale]` but no `#[handler]` | the CID-04 missing-handler error only | trybuild `ui/cid/CID-04_hook_without_handler.rs` |
| 44 | App handler last call wins | `expired_handler(a)` then `expired_handler(b)` | only `b` runs | — |
| 45 | Allocation proof | `alloc_dispatch` stale/Ref probe rows | unchanged labels; still 0 blocks / 0 bytes | test |
| 46 | Memory flat | `memory_flat` with 1 000 stale clicks on `Fresh` | flat | test |
| 47 | Headers | every callback/webhook request in rows 1-31 | no `authorization` header, no `with_components` | test |

</intent-contract>

## Code Map

- `crates/noctis-core/src/pipeline/component.rs`:
  - `Rejection::sends_notice` (L111) and `Rejecter::reject` (L251): replace "Stale/RefUnresolved → silent" with the expired routing.
  - `run_component_over` stage 4 (L466-476): on `Err(Stale | RefUnresolved)`, rebuild the parts (`CtxInner` is not `Clone`; build them through a small closure) and run the chain.
  - Update the module doc (L1-35) stage list and the rejection paragraph.
- `crates/noctis-core/src/pipeline.rs`: `arm` (L188), `Armed`, `Report` (L283), `Started` (L305) and `run_started` (L341) are reused unchanged for hook and App-handler runs. Update the module doc L59 (stale/Ref sentence).
- `crates/noctis-core/src/component.rs`:
  - `ComponentHandler` (L262) and `ModalHandler` (L432) get the hook fields, hidden attach methods and the `Debug` fields.
  - Reuse `boxed`, `erase` and `declared`, and `ctx_accessors!` (L55) for `ExpiredCtx`.
  - Update the doc L1-11.
- `crates/noctis-core/src/kind.rs`:
  - Add the hook binding traits and shapes next to `__Bind` (L97).
  - Add the `__Routed`-style bound with the "component or modal struct" `on_unimplemented`.
  - Add the fallback trait beside `__NoHandler` (L654).
  - Reuse `__assert_responder`/`__assert_no_responder` (L629-650).
- `crates/noctis-core/src/{app.rs (AppBuilder L1188, Debug), handler.rs (AppShared L203)}`: `expired_handler` storage.
- `crates/noctis-core/src/notice.rs` (`text`, `locale`) and `crates/noctis-i18n/src/lib.rs` (`keys::EXPIRED`, `keys::STALE` docs): read-only, except the doc of `STALE`.
- `crates/noctis-core/src/lib.rs`: re-exports `Expired`, `ExpiredCtx`. `__testing` seams stay. The probe at L402 keeps the `ref_unresolved` label.
- `crates/noctis-macros/src/`:
  - New `hook.rs` modelled on `autocomplete.rs` (`expand_or_fallback`, `check`, unit tests).
  - `lib.rs`: two `#[proc_macro_attribute]`s, `on_stale` and `on_expired`, plus the crate doc.
  - `handler.rs`: `descriptor()` attaches the hooks, and the shape unit tests are updated.
- `crates/noctis/src/lib.rs`: root re-exports next to `handler` (L991) with docs and a doctest; prelude (L1764) `on_expired`, `on_stale`, `Expired`, `ExpiredCtx`.
- Tests:
  - `crates/noctis-core/src/pipeline/tests/components.rs`: `rejects` (L243) and rows L377-383 and L548 change from silent to the expired notice. Add a new `pipeline/tests/expired.rs` (registered next to `mod components;` in `pipeline/tests.rs` L2854).
  - `crates/noctis-testkit/tests/components.rs`: derived fixtures L99-300 and helpers `assert_notice` (L497) and `click` (L531).
  - `crates/noctis/tests/alloc_dispatch.rs` L170 is unchanged; `crates/noctis/tests/memory_flat.rs`.
  - trybuild `crates/noctis/tests/types/`, `ui/int/`, `ui/cid/`.
- `docs/discord-rules.md` L179 (CID-06); spine AD-23 (L907); skill files listed in Boundaries; `examples/project-template/{src/modules/setup/handlers.rs, AGENTS.md}`; `xtask/public-api.txt`; the skill `SKILL.md` L23 API hash.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-core/src/{component.rs or expired.rs, lib.rs}`: `Expired`, `ExpiredCtx`, the hook fields and attach methods on both descriptors, and the `Debug` changes. Unit tests: attach, `Debug`, `Expired::as_str`.
2. `crates/noctis-core/src/kind.rs`: hook binding traits for `__ComponentKind`/`__ModalKind` (the four shapes each), the wrong-kind bound, the fallback trait, and per-descriptor attach impls (see Design Notes).
3. `crates/noctis-core/src/{app.rs, handler.rs}`: `AppBuilder::expired_handler` with rustdoc and a doctest, the `AppShared` field, and `Debug`.
4. `crates/noctis-core/src/pipeline/component.rs`, `pipeline.rs`: the routing chain, the default notice, logging and the module docs.
   - Update the existing component tests.
   - Add `pipeline/tests/expired.rs` covering rows 1-31 and 44 through hand-built descriptors. Every request asserts method, path, body and the absent `authorization` header (row 47).
   - Test names: `stale_ids_reach_the_expired_handler`, `hook_precedence`, `refs_go_to_on_expired`, `expired_hooks_answer_like_handlers`, `default_expired_notice_locale_and_failure`, `modal_stale_hooks`.
5. `crates/noctis-macros/src/{hook.rs, lib.rs, handler.rs}`: macro unit tests for rows 38-39.
6. `crates/noctis/src/lib.rs`: re-exports, docs and prelude.
7. trybuild: `crates/noctis/tests/types/on_stale_{receiver,not_async,args,params,on_command,twice,wrong_ctx}.rs` and `on_expired_missing_expired.rs`, `crates/noctis/tests/ui/int/INT-05_hook_{manual_without_responder,manual_responder_without_flag,show_modal_needs_manual}.rs`, `crates/noctis/tests/ui/cid/CID-04_hook_without_handler.rs`. Bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui` and read every `.stderr`.
8. `crates/noctis-testkit/tests/components.rs`: end-to-end with real derives through `FakeGateway`/`FakeDiscordHttp`:
   - `stale_click_gets_the_expired_notice` (row 1);
   - `on_stale_rerenders_in_place` (row 9);
   - `app_expired_handler_runs` (row 11);
   - `ref_id_reaches_on_expired` (row 14);
   - `appended_option_field_still_decodes` (row 32);
   - `modal_stale_submit_runs_on_stale` (row 29).
9. `crates/noctis/tests/memory_flat.rs`: row 46.
10. Docs, spine AD-23 amendment, Usage Skill, template, then `cargo run --release -p xtask -- api-snapshot --bless` and the skill `API hash`.
11. Run every gate in Verification.

**Acceptance Criteria:**
- Given a mounted `Buy` with no hooks and no App handler, when a click with `custom_id` `1shop.buy:147` arrives through `FakeGateway`, then `FakeDiscordHttp` records exactly one `POST /api/v10/interactions/{id}/{token}/callback`. Its body is the type-4 ephemeral V2 notice with the en-US `noctis.expired` text, it has no `authorization` header, and the handler did not run.
- Given `Fresh` with an `#[on_stale]` method returning a `V2Message`, when its stale id is clicked on a V2 message, then exactly one type-7 callback carries that View (`flags: 32768`, `attachments: []`), and neither `#[on_expired]` nor the handler runs.
- Given `AppBuilder::expired_handler` returning a `V2Message`, and a Route without hooks, when its stale or Ref id arrives, then the App handler runs with the Route id and `Expired::Stale`/`Expired::RefMissing`, and its message is sent per the Route's declared `response`.
- Given a `by_ref` Route with `#[on_stale]` and `#[on_expired]`, when a Ref id arrives, then only `#[on_expired]` runs, with `Expired::RefMissing`.
- Given a Route that appended an `Option` field without a `schema` bump, when an old id arrives, then the handler runs with that field `None`.
- Given the trybuild suites, when `cargo test --release -p noctis --test ui` runs, then rows 39-43 fail to compile with the stated texts and every earlier fixture still passes.
- Given the Usage Skill, when `skill-check` runs, then it passes, the skill documents `#[on_stale]`, `#[on_expired]`, `expired_handler`, the precedence and the Payload evolution rules (append `Option` fields, otherwise bump `schema`), and no `[planned, Story 7.3]` marker or "Stories 7.3/7.5" stale wording remains.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Hook signatures (decision).**
  - `#[on_expired]` takes `expired: Expired` (the shape the Usage Skill already announced), so one method can tell a stale fall-through from a missing Ref. `#[on_stale]` takes only `ctx`.
  - Hooks reuse the Route's `response`/`auto_defer` and the INT-05 assertions, so a Route has one declaration and the auto-defer timer works the same for both.
  - The App handler has no responder and is always auto-deferred, because it is generic over all Routes.
- **Ref ids go to the Expired Handler now (decision).** No RefStore exists before Story 7.5, so every Ref is missing. Routing them here leaves 7.5 to add only the lookup, plus the forged and stale record outcomes, in front of the same chain.
- **Default text is `noctis.expired` for both reasons (decision)**, as the epics AC requires. `keys::STALE` stays reserved (no public-API removal).
- **Logging level (decision).** Stale clicks are expected after every deploy, so they are `debug`, not `warn`. FR-19's logged warnings cover malformed, unknown and forged input only.
- **Binding mechanism (golden sketch; any equivalent that keeps the observable behaviour is fine):**
  ```rust
  // #[on_stale] beside the method, in the same impl:
  #[doc(hidden)] pub fn __noctis_on_stale() -> <Self as __Kind>::StaleHook /* or __Hook<Kind> */ {
      #core::__assert_no_responder::<Self>();            // or __assert_responder::<Self, R>()
      <<Self as __Kind>::Kind as #core::__BindStale<#shape, Self, _>>::__bind(Self::method)
  }
  // kind.rs: pub trait __NoHooks { fn __noctis_on_stale() -> __Absent {..} fn __noctis_on_expired() -> __Absent {..} }
  // impl<T: ?Sized> __NoHooks for T {}
  // #[handler]'s descriptor(): { use #core::__NoHooks as _; #core::__AttachHooks::__attach(#bind, Self::__noctis_on_stale(), Self::__noctis_on_expired()) }
  ```
  - `__AttachHooks<St, Ex>` is implemented for `ComponentHandler`/`ModalHandler` over (`__Absent` | their hook type) and for the command, menu and event descriptors over (`__Absent`, `__Absent`) only. Write explicit impls with no blanket impl, so coherence holds.
  - `__BindStale`/`__BindExpired` carry the wrong-kind and wrong-parameters `on_unimplemented` texts.
- **Golden wire (row 9):**
  ```text
  click 1fresh:11 on V2 source → POST /interactions/{id}/{tok}/callback
  {"type":7,"data":{"flags":32768,"components":[..stale-view..],"allowed_mentions":{"parse":[]},"attachments":[]}}
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
- `grep -rn "planned, Story 7.3\|Stories 7.3/7.5\|Story 7.3 routes\|(Story 7.3)" crates examples/project-template docs --include=*.rs --include=*.md` -- prints nothing
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged
