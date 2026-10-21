# Epic 7 Context: Routed components, Refs and modals

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let a bot author handle every button, select and modal through exactly one stateless handler per Route, working across all messages, shards and restarts. The epic adds O(1) dispatch, a defensive component pipeline with in-place updates, stale/expired routing, App-namespaced ports, Ref minting/resolution for large state, Middleware with typed Extensions, Route Guards and end-to-end modal submissions. No listener, closure or timer is ever kept per message.

## Stories

- Story 7.1: Route handlers and O(1) component dispatch
- Story 7.2: Component pipeline, untrusted input and in-place updates
- Story 7.3: Expired Handler and stale Payload routing
- Story 7.4: App-namespaced ports
- Story 7.5: Ref minting and resolution
- Story 7.6: Middleware and typed Extensions
- Story 7.7: Route Guards
- Story 7.8: Modal Routes and typed submissions end to end

## Requirements & Constraints

- One handler per Route id; duplicates across Modules and Routes referenced without a mounted handler fail `App::build` with a `BuildError` (CID-04). `.build()` on a component requires `R: Handled` (compile-time half).
- Untrusted input: malformed id, unknown Route, bad integrity tag or undecodable Payload yields a typed error plus logged warning and a generic ephemeral notice, never a panic (CID-06).
- No per-message/per-component closure, task or timer; memory must stay flat with N messages. Route lookup + fixed-width inline decode allocate nothing (dhat test).
- `NOCTIS_PAYLOAD_KEY` (32 bytes, plus optional previous key for rotation) is never logged or passed to a port/adapter.
- Observability: `noctis.interaction` spans across stages; `noctis_handler_duration_seconds{kind}`, `noctis_handler_errors_total{kind}`, `noctis_ref_lookups_total{result}` (hit/miss/stale/forged).
- Every public API change updates the Usage Skill, project template, `xtask/public-api.txt` and passes `skill-check`, in the same commit.
- All wire behaviour is asserted against the fake Discord HTTP server (method, path, query, headers, body).

## Technical Decisions

- **Authoring:** one struct per interaction via kind derive (`#[derive(Button)] #[button("id", .., defaults(..))]`, select kinds, `#[derive(Modal)]`); handler is `impl R { #[handler] async fn handle(self, ctx) }` with no `#[handler]` args; free-fn handlers are a compile error. `#[on_stale]` / `#[on_expired]` are `impl` methods without `self`.
- **Runtime arguments** (don't change encoding): `response = update|ephemeral|public` (default `update`) and `auto_defer = true|false` (7.2) ship on the component attributes as `__Component::RESPONSE` and the derive's `__Kind::Defer` (`Manual` when `auto_defer = false`), because `response` is a core concept and codec sits below core (`#[modal]` gets them in 7.8). The `RouteRuntime` const holds only `ttl`, `one_shot` (by_ref/spill only, 7.5), `owner`, `permissions`, `guard` (7.7).
- **Fixed pipeline (10 stages):** decode -> Ref resolve -> boot/guild/module checks -> locale -> rate limit -> auto-defer arm -> before-Middleware -> Guards -> handler -> after-Middleware + Error Sink. Stages 1-5 run no author code (except `LocaleResolver`) and only Ref/module-state/rate-limit I/O. Notices from stages 3/5/8 after an auto-defer become ephemeral follow-ups and the deferred original is deleted.
- **Responder typestate** `Responder<K, Pending|Acked, AutoDefer|Manual>` following discord.js names: `reply`/`reply_as` (4), `defer_reply` (5), `update` (7), `defer_update` (6), `show_modal` (9, only Command/Component with `Manual`; otherwise `[INT-05] show_modal needs auto_defer = false on this handler`); Acked: `edit_reply`, `fetch_reply`, `delete_reply`, `follow_up`. `Manual` alias required exactly when `auto_defer = false`. Auto-defer deadline App-wide, 2000 ms; runtime atomic state machine turns late Pending calls into `edit_reply`; `TokenExpired` after 15 min; `UpdateUnavailable` for slash-opened modal submits.
- **V2 sends/edits** carry `IS_COMPONENTS_V2` (never `EPHEMERAL` on update/edit), `allowed_mentions` none by default, attachments keep-list; legacy->V2 sends `content: null, embeds: []`. Interaction callbacks/webhooks go through the Discord edge without bot `Authorization`.
- **custom_id codec:** stale = shape/schema mismatch -> stale handler else Expired Handler; unknown route or bad tag = forged -> unknown-Route path. Payload evolution: append `Option` fields freely, otherwise bump `schema`.
- **Refs (two-phase):** render code emits `EncodedId::RefSlot`; core's Responder stores via `RefStore::put_if_absent` (retry on collision), calls `codec::finish`; identical slots share one Ref; one-shot resolves with `take`; route mismatch = forged; `RefOps` deletions run after successful send. No crate below core holds a `RefStore`. Refs are 11 base62 chars from CSPRNG.
- **Ports:** `Namespaced<P>` prefixes `<app>:<port>:` (escape `:`); adapters store keys verbatim. `Shared::new(store)` passed to normal setter gives `_shared:<port>:`. `AppId` `[a-z0-9-]{1,32}`, default application id in base62. `ctx.kv()` typed CacheStore handle: postcard + schema byte (mismatch = miss), counters 8-byte big-endian.
- **Extensions:** per-interaction type map; child replaces parent with debug trace; Middleware descriptors declare provided types; unprovided `Ext<T>` fails build, `Option<Ext<T>>` opts out; never written to ports.
- **Registry:** immutable, built once from descriptors; O(1) maps keyed by `&'static str` looked up with borrowed keys.
- **Route map:** components and modals share one App-wide map keyed by `<S as Route>::ID`; duplicates are `BuildViolation::DuplicateRoute`, and opt-in `Module::uses::<R: Handled>()` makes a used-but-unhandled Route a `BuildViolation::UnmountedRoute` (both CID-04).
- **Staged rollout:** 7.1 dispatches but sends nothing; stale/Ref/forged/undecodable/kind-mismatch ids are a crate-private `Rejection` logged once at warn (`noctis::dispatch`, `reason`), each arm replaced by 7.2/7.3/7.5. Handler failures go to the Component Error Sink (`kind="component"`).
- **Proofs:** dhat alloc test via hidden `__testing::route_table(..).probe(..)` (10 and 10 000 Routes) and memory flatness via `__testing::dispatch`; testkit `assert_memory_flat` is not added in this epic.
- **Codec:** format-digit flag bit 2 is reserved (`CodecError::ReservedFlag`, malformed, never stale).

## Cross-Story Dependencies

- Builds on Story 6.10 (Responder names, `auto_defer`, `response`, `Manual`) and Story 6.11 (kind derives, `defaults(..)`, `.build()`, `#[handler]`, `Module::add`), Story 2.5/2.6 (codec length proof, dhat test), Story 4.6 (modal decoder incl. Radio/Checkbox values), Story 5.2 (fake HTTP server), 5.6 (Module nesting), 5.9 (Discord edge), 6.6 (bot-token fallback after expiry).
- 7.1 precedes 7.2-7.8; 7.2 defines `auto_defer`/`response` reused by 7.8; 7.3's Expired Handler is the target for 7.5's missing Refs; 7.4's namespacing wraps the RefStore used in 7.5; 7.6 Middleware ordering frames 7.7 guards. Story 8.1's `ctx.update`/`ctx.respond` and 9.1 multipart attachments, 13.4 `rate_limit` extend this epic.
