---
title: 'Story 13.4: Rate limits on Commands and Routes'
type: 'feature'
created: '2026-10-24'
status: 'in-progress'
baseline_revision: 'c0b96ce9a4f6c9a4695dc48b6ea69a36870f2c60'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-46 rate limits do not exist: `rate_limit(..)` is rejected as "planned" on `#[command]`, `#[user_command]`, `#[message_command]`, every component kind attribute and `#[modal]`, AD-23 stage 5 is an empty comment in both pipelines, and the template's `/profile` limit is commented as rejected.

**Approach:** Parse `rate_limit(<scope>, <n>, per = "<dur>"[, manual])` (repeatable, one per scope) on all six interaction attributes into a `RATE_LIMITS` const on core's kind traits; run AD-23 stage 5 (store counters through the App-namespaced `RateLimitStore`) before the auto-defer in the Command pipeline, the component/modal pipeline and `Dispatcher::run_command`; answer a hit with an immediate localized ephemeral `noctis.rate_limited` notice, or the most specific override (`#[on_rate_limited]` method → `AppBuilder::rate_limited_handler` → default); give handlers `ctx.rate_limits()` to `hit`, `block`, `extend` or `reset` their declared limits.

## Boundaries & Constraints

**Always:**
- Grammar, exactly: `rate_limit(<scope>, <n>, per = "<dur>")` or `rate_limit(<scope>, <n>, per = "<dur>", manual)`; `<scope>` ∈ `user | member | channel | guild | message` (bare ident); `<n>` an integer literal in `1..=4294967295` (u32); `<dur>` the existing `crates/noctis-macros/src/duration.rs` grammar, additionally capped to `1s..=1d` (86 400 s, the memory store's TTL cap). The argument may repeat, each scope at most once; declared order is kept. Every value error cites `[INT-14]` (new rule); unknown/duplicate *other* arguments keep their current messages (CID-03 on kind attributes).
- `message` scope is a compile error (`[INT-14]`) on `#[command]` and `#[user_command]`; allowed on `#[message_command]` (target message), components (source message) and `#[modal]` (source message when opened from a component message).
- Limits are per handler struct: logical key `n/<scope>/<segment>/<NAME>` (counter) and `e/<scope>/<segment>/<NAME>` (window end), `<NAME>` = the descriptor's `name` (`module_path!()::Struct`, last so it may contain any character). Segments: user `<user>`; member `<guild>.<user>`, outside a guild `0.<user>`; channel `<channel>`; guild `<guild>`, outside a guild `c<channel>`; message `<message id>`. A scope whose segment cannot be formed (no channel; message scope without a message) is **unkeyed**: stage 5 skips that limit with one `debug` on `noctis::rate_limit`.
- Keys go only through `AppShared`'s namespaced `Arc<dyn RateLimitStore>` (`<app>:rate_limit:` prefix by `Namespaced<P>`, `_shared:rate_limit:` for `Shared::new`); never prefix in core code.
- Counter = the port's 8-byte big-endian `u64` (`incr_with_ttl` contract, writable with `put`). Window end = unix milliseconds (`SystemTime`, cluster-wide) stored with `noctis_ports::encode_value` (postcard + schema byte), TTL = time to the end; a decode miss reads as absent.
- Stage 5, per declared limit in order, stopping at the first hit: auto limit → `incr_with_ttl(n, 1, Some(window))`; result `1` also `put(e, now+window, window)`; result `> n` is a hit. Manual limit → `get(n)` (absent or malformed = 0); `>= n` is a hit. On a hit, `get(e)`: `retry_after = end - now` (saturating), absent or ≤ 0 → `window`; shown time is `<t:ceil(end_ms/1000):R>` with `end = now + retry_after`.
- Stage 5 runs after stage 4 (locales) and before stage 6 (auto-defer) and stage 7 (Middleware): no callback has been sent, so the notice is a type-4 callback with flags 64 (a public reply over the `Dispatcher`'s message transport, as other notices). In the component pipeline it runs after the struct decoded (Ref resolved) and before `arm`; a taken `one_shot` record is put back (`restore_ref`) on a hit.
- A store error in stage 5 fails open: one `warn` on `noctis::rate_limit` (handler, scope, error; no ids), the limit is not applied, the run continues.
- On a hit: one `debug` on `noctis::rate_limit` (handler, scope, limit, window_secs, retry_ms; no user/guild ids), `noctis_rate_limited_total{app, kind}` +1 (`kind` = `ErrorOrigin::as_str()`: `command` or `component`), no handler, no Middleware, no after-Middleware, no Error Sink report.
- Override precedence: the struct's `#[on_rate_limited]` method → the App's `rate_limited_handler` → the default `noctis.rate_limited` notice in the User Locale. An override runs under `catch_unwind` with deadline `received_at + ack_deadline` and the shutdown token; its returned `V2Message` is sent like a notice (always ephemeral; refactor `Shared::notice` into a `notice_message(OutgoingMessage)` it calls). An error or panic sends the default notice and is reported to the origin's Error Sink (`ReportKind::Error`/`Panic`, handler = the override's struct name); a missed deadline or cancellation sends the default notice with one `warn`, no report.
- Every public API change updates `examples/project-template/` (code, `AGENTS.md`, noctis Usage Skill), re-blesses `xtask/public-api.txt` and sets the Skill's `API hash` line, in the same commit. `docs/discord-rules.md` and `noctis-rules` gain `INT-14` together.

**Never:**
- No rate limit for autocomplete, the Expired Handler chain (`#[on_stale]`/`#[on_expired]`/App `expired_handler`), events or the detached `__testing` seams; no `rate_limit` on `#[event]`/`#[autocomplete]` (existing unknown-argument errors).
- No new port op, no change to `RateLimitStore`, no in-process `HashMap` limiter (AD-1), no epoch-aligned windows.
- No `rate_limit` on `#[handler]` (takes no arguments) and no `rate_limited = path` argument; no ids or free text in metric labels.
- No debug builds, no `cargo hack` run (gate script only).

## I/O & Edge-Case Matrix

Macro grammar (unit tests in `noctis-macros`, named `int_14_*`; UI fixtures where marked [UI]):

| # | Input (on `#[command("p", ..)]` unless noted) | Expected |
|---|---|---|
| G1 | `rate_limit(user, 3, per = "10s")` | `RATE_LIMITS = [User, 3, 10 s, auto]` |
| G2 | `rate_limit(user, 3, per = "10s"), rate_limit(guild, 100, per = "1m")` | two entries, declared order |
| G3 | `rate_limit(member, 1, per = "1d", manual)` | manual, 86 400 s (max) |
| G4 | `per = "1s"` / `n = 1` / `n = 4294967295` | accepted (boundaries) |
| G5 | `n = 0` [UI `INT-14_zero_count`] / `n = 4294967296` / `n = -1` / `n = 3.0` / `n = "3"` | `[INT-14] the rate-limit count must be an integer literal in 1..=4294967295` |
| G6 | `per = "2d"` / `"86401s"` / `"1d1s"` [UI `INT-14_window_too_long`] | `[INT-14] the rate-limit window must be 1s..=1d; found "<text>"` |
| G7 | `per = "0s"` | same out-of-range message |
| G8 | `per = ""`, `"10"`, `"10S"`, `"1m1h"`, `"1.5m"`, `"10 s"`, `"١٠s"` (Arabic-Indic digits), `per = 10` | `[INT-14] \`per\` takes a duration such as "10s", "1m" or "1h30m" (units d, h, m, s)` |
| G9 | `rate_limit(user, 3)` [UI `INT-14_missing_per`] | `[INT-14] \`rate_limit\` needs a window: \`rate_limit(user, 3, per = "10s")\`` |
| G10 | `rate_limit(users, 3, per = "1s")` / `rate_limit(User, ..)` / `rate_limit("user", ..)` [UI `INT-14_unknown_scope`] | `[INT-14] unknown rate-limit scope \`users\`: expected \`user\`, \`member\`, \`channel\`, \`guild\` or \`message\`` |
| G11 | `rate_limit(user, 3, per = "1s"), rate_limit(user, 5, per = "1m")` [UI `INT-14_duplicate_scope`] | `[INT-14] a second \`rate_limit\` for scope \`user\`: declare each scope once` |
| G12 | `rate_limit(user, 3, per = "1s", manual, manual)` / `.., auto)` / `.., per = "1s", per = "2s")` / `rate_limit(3, user, per = "1s")` / `rate_limit()` / `rate_limit = 3` / `rate_limit` | `[INT-14] \`rate_limit\` takes \`(scope, count, per = "…"[, manual])\`` (one message for any shape error) |
| G13 | `message` scope on `#[command]` [UI `INT-14_message_scope_on_slash`] and `#[user_command]` | `[INT-14] scope \`message\` needs a message: use it on #[message_command], a component or a modal` |
| G14 | `message` scope on `#[message_command]`, `#[button]`, every select kind, `#[modal]` | accepted |
| G15 | any accepted form on all 7 kind attributes + `#[modal]` (incl. after `defaults(..)`, with `owner`, `guard`, `by_ref`) | accepted; codec width unchanged |
| G16 | `#[on_rate_limited]` on an `#[derive(EventHandler)]` struct [UI `INT-14_on_rate_limited_event`] | compile error whose message contains `[INT-14] #[on_rate_limited] needs a Command, context-menu, component or modal struct` |
| G17 | `#[on_rate_limited]` with `self`, not `async`, generic, or params other than `(ctx: RateLimitedCtx, hit: RateLimitHit)` / returning other than `HandlerResult<V2Message>` | macro error (`self`/async/generic) or rustc bound error from `__RateLimitedHook::__new` |

Runtime (core unit tests `int_14_*` with `MemoryRateLimitStore`; end-to-end in testkit):

| # | Scenario | Expected |
|---|---|---|
| R1 | `rate_limit(user, 3, per = "10s")`, same user invokes 4× within 10 s | runs 1–3 run handler; 4th: one type-4 callback, flags 64, text = en-US `You're doing that too often. Try again <t:N:R>.` with N ≈ first-use + 10 s; no type-5 defer, no handler, no Middleware |
| R2 | R1, other user, 4th overall call | runs (per-user key) |
| R3 | `per = "1s"`, 2 uses at n=1, wait ≥ 1.1 s, use again | 2nd limited, 3rd runs (window expired) |
| R4 | `member` in guild G1 vs G2, same user | separate counters |
| R5 | `member` / `guild` in a DM | keyed `0.<user>` / `c<channel>`; limit applies |
| R6 | `channel` scope, interaction without channel | limit skipped, `debug`, handler runs |
| R7 | `message` scope on a button: clicks on message M1 vs M2 | separate counters |
| R8 | `message` scope on a slash-opened modal submit | unkeyed: skipped, handler runs |
| R9 | `message` scope on `#[message_command]` | keyed by `data.target_id` |
| R10 | two limits `user 3/10s`, `guild 1/10s`: first call | both incremented; 2nd call: user counter 2, guild hit → notice; evaluation stops at first hit (later limits not incremented) |
| R11 | `manual` limit n=1, handler never calls `hit` | never limited |
| R12 | `manual` n=1, handler calls `ctx.rate_limits().hit(RateScope::User)` after success | next invocation limited, retry ≈ window |
| R13 | `block(User, 30s)` with declared `user 3/10s` | next invocation limited, retry ≈ 30 s; `block` with a later live end keeps the later end |
| R14 | `extend(User, 10s)` twice from a fresh state | end = now + 20 s; limited |
| R15 | `block`/`extend` with `Duration::ZERO` | `Ok(())`, no store write |
| R16 | `block(User, 2 days)` / extend past 1 day | end clamped to now + 1 day |
| R17 | `reset(User)` after a hit | next invocation runs (fresh window from now) |
| R18 | `hit`/`block`/`extend`/`reset` on an undeclared scope | `Err(RateLimitError::Undeclared { scope })`, no write |
| R19 | dynamic call on an unkeyed scope | `Err(RateLimitError::Unkeyed { scope })` |
| R20 | dynamic call from a detached test context | `Err(RateLimitError::Detached)` |
| R21 | store returns `PortError` in stage 5 | fail open: `warn`, handler runs; dynamic call → `Err(RateLimitError::Store(_))` |
| R22 | counter key holds a non-8-byte value | auto: incr `InvalidValue` → R21 path; manual `get` → treated as 0 + `warn` |
| R23 | window-end record missing (evicted) on a hit | retry = window |
| R24 | struct `#[on_rate_limited]` and App handler both set | struct method's message sent ephemeral type 4; App handler not called |
| R25 | only App `rate_limited_handler` | its message sent |
| R26 | override returns `Err` / panics | default notice sent; one Error Sink report of the origin |
| R27 | override exceeds `ack_deadline` | default notice, one `warn`, no report |
| R28 | component with `by_ref, one_shot` + `rate_limit`, limited click | Ref record put back; next allowed click resolves it |
| R29 | `Dispatcher::run_command` on a limited Command | public reply notice to the invoking message; handler not run |
| R30 | two Apps, same `app_id`, one shared `Arc<MemoryRateLimitStore>` | limit shared (cluster); different `app_id`s → independent counters |
| R31 | User Locale `tr` with an overlay `noctis.rate_limited` | notice in Turkish with `{retry}` replaced |
| R32 | autocomplete / expired chain of a rate-limited struct | never counted or limited |

</intent-contract>

## Code Map

- `crates/noctis-macros/src/command.rs` -- L33 `PLANNED` (drop `rate_limit`), L242 arm, duplicate-arg check (~L258: allow `rate_limit` to repeat); emits `__SlashCommand` consts; unit tests L1156, L1223 assert the "planned" error (convert).
- `crates/noctis-macros/src/menu.rs` -- L55 `PLANNED`, L122, test L486; emits `__ContextMenu` consts (knows user vs message kind).
- `crates/noctis-macros/src/route.rs` -- L121-123 `RUNTIME_ARGS`, L513 `unsupported_runtime_arg` (delete: `rate_limit` was the last), `Seen` parsing used by every component kind and `#[modal]`; tests L901, L1009, L1374; `crates/noctis-macros/src/component.rs` L448 test; component consts emitted ~L99/L150, modal ~L1050/L1094 (`crates/noctis-macros/src/modal.rs`, L519 hidden-const list).
- `crates/noctis-macros/src/duration.rs` -- `parse_secs` (1 s..=365 d); add the 1 d cap in the new parser, not here.
- New `crates/noctis-macros/src/rate_limit.rs` -- shared parser + `quote` of `&[core::RateLimit::__declare(..)]`, used by command/menu/route.
- `crates/noctis-macros/src/hook.rs` -- `#[on_stale]`/`#[on_expired]` pattern (`Hook` enum, inherent `__noctis_on_*()`); add `#[on_rate_limited]` here; `crates/noctis-macros/src/lib.rs` L267 exports; `crates/noctis-macros/src/handler.rs` L242 `__AttachHooks::__attach(..)` — wrap with `__AttachRateLimited::__attach_rate_limited(.., Self::__noctis_on_rate_limited())`.
- `crates/noctis-core/src/kind.rs` -- L111 `__SlashCommand`, L246 `__ContextMenu`, L462 `__Component`, L473 `__Modal` (add `const RATE_LIMITS: &'static [RateLimit] = &[];`); L153 `slash::<S>` (append `.__with_rate_limits`), menu/component/modal `__Bind` impls likewise; L800 `__NoHooks` (add `__noctis_on_rate_limited() -> __Absent`); L842 `attach_nothing!` pattern for the new attach trait (EventHandler: `__Absent` only, `#[diagnostic::on_unimplemented]` for G16).
- `crates/noctis-core/src/command.rs` L432 `CommandHandler`, `crates/noctis-core/src/menu.rs` L102 `ContextMenuHandler`, `crates/noctis-core/src/component.rs` ~L780 `ComponentHandler`, L1056 `ModalHandler` -- add `rate_limits`, `on_rate_limited`, hidden builders, `pub(crate)` getters, Debug fields; `CtxInner` (command.rs L101) gains `rate: Option<Arc<RateRun>>` (set by pipelines, `None` in `detached_parts`).
- `crates/noctis-core/src/rate_limit.rs` -- the Discord HTTP 429 types (`RateLimitScope`, `RateLimited`); do NOT reuse names. New module `crates/noctis-core/src/rate_limits.rs` (+ `rate_limits/tests.rs`) for FR-46.
- `crates/noctis-core/src/pipeline.rs` -- L17 module doc, L927 stage-5 comment in `run_interaction` (insert before `transport`; message id from `data.target_id` for message menus), L1561 `Dispatcher::run_command` (insert before L1640 `message_shared`), `send_notice` ~L760, `restore_ref`, `Report`; `crates/noctis-core/src/pipeline/component.rs` L1-63 doc, L833-859 (insert between the `started` match and `arm`), `SourceMessage::from_interaction` (responder.rs L1215) gives the message id.
- `crates/noctis-core/src/responder.rs` L2282 `Shared::notice` -- extract `notice_message`.
- `crates/noctis-core/src/notice.rs` L13 `text` -- add `text_with(strings, locale, key, args)`; key `noctis_i18n::keys::RATE_LIMITED` (`{retry}`) already exists in every built-in locale.
- `crates/noctis-core/src/handler.rs` L203 `AppShared` -- add `rate_limits: Arc<dyn RateLimitStore>`, `rate_limited: Option<Arc<AppRateLimitedFn>>`; `crates/noctis-core/src/app.rs` L1618/L2032 `expired_handler` pattern, ~L2710 builds `AppShared` from `ports.rate_limit_store`.
- `crates/noctis-core/src/observe.rs` L84/L131 metric table + `METRICS`.
- `crates/noctis-core/src/lib.rs` L275/L326 re-exports; `crates/noctis/src/lib.rs` L602 facade list, L1117 macro re-exports, L2223 prelude.
- `crates/noctis-testkit/src/response.rs` ~L486 `Notices` + `is_guard_notice`; `crates/noctis-testkit/src/app/outcome.rs` L186 `notices()`.
- Harnesses: `crates/noctis-testkit/tests/guards.rs` (FakeGateway + FakeDiscordHttp wire asserts, log `Capture`); `crates/noctis-testkit/tests/testapp.rs`.
- `crates/noctis/tests/ui/cid/CID-03_runtime_argument.{rs,stderr}` -- delete (feature now accepted); `crates/noctis/tests/ui/int/` -- new fixtures.
- `docs/discord-rules.md` L152-168 (§7, add INT-14 after INT-13), L176 CID-03 row (remove the "rate_limit rejected" clause, mention INT-14); `crates/noctis-rules/src/rule.rs` ~L50 `INT_*` list.
- `examples/project-template/src/modules/profile/handlers.rs` L23-28 (`/profile` comment), `EditBio`/`BioForm` submit handler; Skill `references/modules-middleware-ratelimits.md` §7 (L513-537 drafts a different API: replace), `references/testing.md` L180, `references/modals.md` L256, `references/commands.md`, `references/components-v2.md`/`routes-payloads-refs.md` runtime-argument lists, `references/i18n.md` L325, `references/app-and-sharding.md` §11 metrics, `SKILL.md` L23 `API hash`; `examples/project-template/AGENTS.md` L57 (`rate_limit(..)` is [planned] sentence).
- Spine `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/ARCHITECTURE-SPINE.md` L337 (AD-6 says `RouteRuntime` carries `rate_limit`), L1076 (AD-23 stage 1-5 port I/O).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/rule.rs`, `docs/discord-rules.md` -- add `INT_14 = "INT-14"` and the row "**INT-14** Declared rate limits | `rate_limit(scope, n, per)`: scope ∈ user/member/channel/guild/message, n 1..=2^32−1, window 1 s..=1 d, one per scope, `message` only where a message exists **[N]** | — | Compile-time / Runtime | …" (macro + stage 5, fixtures, unit tests `int_14_*`); update CID-03 row -- rule id shared by code, docs, tests.
- `crates/noctis-core/src/rate_limits.rs` (new) + `rate_limits/tests.rs` -- `pub enum RateScope { User, Member, Channel, Guild, Message }` (`as_str`, Display, Copy/Eq/Hash/Debug); `pub struct RateLimit` (`Debug, Clone, Copy, PartialEq, Eq`, private fields, getters `scope()`, `max_uses() -> u32`, `window() -> Duration`, `is_manual()`, hidden `pub const fn __declare(scope, max_uses: u32, window_secs: u32, manual: bool)`); `pub struct RateLimitHit` (getters `scope()`, `limit() -> RateLimit`, `retry_after()`, `retry_at_unix_secs()`, `relative_timestamp() -> String` = `<t:N:R>`); `pub struct RateLimitedCtx(Arc<CtxInner>)` (`module`, `path`, `user_id`, `guild_id`, `channel_id`, `user_locale`, `guild_locale`, `state::<T>()`); `pub struct RateLimits` handle with `async fn hit(&self, RateScope) -> Result<u64, RateLimitError>`, `block(&self, RateScope, Duration)`, `extend(..)`, `reset(&self, RateScope)` (all `Result<(), RateLimitError>`); `#[non_exhaustive] thiserror enum RateLimitError { Undeclared { scope }, Unkeyed { scope }, Detached, Store(#[from] PortError) }`; crate-private `RateRun`, key building, `stage5(app, run) -> Stage5 { Pass | Hit(RateLimitHit) }`; hook types `__RateLimitedHook` + erased `RateLimitedFn`/`AppRateLimitedFn` -- the FR-46 engine; unit-test R1-R23, R30 and key formats.
- `crates/noctis-core/src/kind.rs`, `command.rs`, `menu.rs`, `component.rs` -- `RATE_LIMITS` consts, descriptor fields/builders, `__AttachRateLimited`, `__NoHooks` fallback, `ctx.rate_limits()` on `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx` (built from `CtxInner.rate`) -- surface.
- `crates/noctis-core/src/handler.rs`, `app.rs` -- `AppShared.rate_limits` (namespaced store), `AppShared.rate_limited`, `AppBuilder::rate_limited_handler<F, Fut>(f)` (`F: Fn(RateLimitedCtx, RateLimitHit) -> Fut + Send + Sync + 'static`, `Fut: Future<Output = HandlerResult<V2Message>> + Send + 'static`; last call wins; doctest) -- App override.
- `crates/noctis-core/src/responder.rs`, `notice.rs` -- `Shared::notice_message`, `notice::text_with` -- notice with `{retry}`.
- `crates/noctis-core/src/pipeline.rs`, `pipeline/component.rs` -- stage 5 in `run_interaction`, `run_component_over`, `Dispatcher::run_command` per Boundaries; override run (deadline, catch_unwind, report); update module docs (stage lists) -- AD-23 order.
- `crates/noctis-core/src/observe.rs` -- `noctis_rate_limited_total` counter, labels `app`, `kind`; table + `METRICS` -- observability.
- `crates/noctis-core/src/lib.rs`, `crates/noctis/src/lib.rs` -- export `RateScope`, `RateLimit`, `RateLimitHit`, `RateLimitedCtx`, `RateLimits`, `RateLimitError`; prelude adds `RateScope`, `RateLimitedCtx`, `RateLimitHit`; facade re-exports `on_rate_limited` with a doctest -- public API.
- `crates/noctis-macros/src/rate_limit.rs` (new), `command.rs`, `menu.rs`, `route.rs`, `component.rs`, `modal.rs`, `hook.rs`, `handler.rs`, `lib.rs` -- parser (G1-G15), consts emission on all six attributes, `#[on_rate_limited]` (G16-G17); convert the "planned"/"not supported yet" tests -- compile-time tier.
- `crates/noctis/tests/ui/int/INT-14_*.rs` + `.stderr` (the 7 [UI] fixtures), delete `crates/noctis/tests/ui/cid/CID-03_runtime_argument.*` -- diagnosed evidence for `rules-catalog`.
- `crates/noctis/tests/rate_limits.rs` (new) -- facade: real derives of all kinds expose `RATE_LIMITS` (G1-G3, G14, G15), descriptor Debug shows `rate_limits`/`on_rate_limited`.
- `crates/noctis-testkit/src/response.rs`, `app/outcome.rs` -- `Notices.rate_limited` + `Response::is_rate_limited()` (match the en-US/locale template around `{retry}`) -- testing kit.
- `crates/noctis-testkit/tests/rate_limits.rs` (new) -- end to end with real derives over FakeGateway/FakeDiscordHttp (copy `guards.rs` harness): R1, R2, R7, R10, R12, R13, R17, R24-R29, R31, R32 asserted on the wire; one TestApp case asserting `is_rate_limited()`.
- Spine + memlog -- append "Amended (2026-10-24, Story 13.4)" to AD-6 (`RATE_LIMITS` lives on core's kind traits, not `RouteRuntime`) and AD-23 (stage 5 may also `get`/`put` the window record in the same store; fails open) -- architecture contract stays true.
- Template: `examples/project-template/src/modules/profile/handlers.rs` -- remove the `[planned]` comment on `/profile`; on the `BioForm` modal (`src/modules/profile/mod.rs`) declare `rate_limit(user, 1, per = "1m", manual)` and call `ctx.rate_limits().hit(RateScope::User).await?` in its `#[handler]` after a successful save; Skill `modules-middleware-ratelimits.md` §7 rewritten for the shipped API (grammar, scopes and fallback keys, window semantics "N uses per window starting at the first use", manual + dynamic API, override precedence with `#[on_rate_limited]` and `rate_limited_handler` examples, fail-open, Cluster sharing, `INT-14` errors), drop `[planned]` in `testing.md`, `modals.md`, runtime-argument lists, `AGENTS.md` L57; add the metric to §11; re-bless `xtask/public-api.txt` and set `SKILL.md` `API hash` -- AGENTS.md policy.

**Acceptance Criteria:**
- Given `#[derive(SlashCommand)] #[command("profile", rate_limit(user, 3, per = "10s"))]` in a running App, when one user invokes it a fourth time within 10 s, then the fourth interaction's only callback is type 4 with flags 64 and the localized `noctis.rate_limited` text containing `<t:N:R>`, sent before any auto-defer, and the handler and Middleware never run.
- Given `rate_limit(member, 1, per = "1m")` on a `#[button]` and on a `#[modal]`, when the limit is hit, then the same notice is sent and a taken `one_shot` Ref is restored.
- Given scopes user, member, channel, guild and message, when limits are keyed, then each scope uses its own key under the App's `rate_limit` namespace, and two Apps with different ids never share counters while two with the same id over one store do.
- Given a handler that calls `ctx.rate_limits().hit/block/extend/reset(..)` for a declared scope, when the next invocation arrives, then stage 5 sees the updated limit.
- Given an `#[on_rate_limited]` method on the struct and an App `rate_limited_handler`, when a limit is hit, then only the struct's message is sent; with only the App handler, the App's; with neither, the default notice.
- Given the new public API, when the story is done, then the Usage Skill documents rate limits, the template's `/profile` limit compiles without the `[planned]` note, and `api-snapshot`, `skill-check` and `rules-catalog` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why kind-trait consts, not `RouteRuntime`:** Commands have no `RouteRuntime`, and scopes/keys are core concepts (codec sits below core), exactly like `RESPONSE`/`EPHEMERALITY`; one `RATE_LIMITS` const per kind trait with an empty default keeps hand-written test impls compiling.
- **Window semantics:** a window starts at the first counted use and lasts `per` (counter TTL set on create); this needs only `incr_with_ttl` on the happy path. The window-end record exists only to show the time remaining and for `block`/`extend`; it is best effort (R23).
- **Fallback keys:** `member` in a DM keys the user under guild `0`, `guild` in a DM keys the DM channel, so `contexts = [BotDm]` cannot bypass a limit; keys never collide with `user` because the scope is part of the key.
- **Fail open:** a store outage must not take every command down; abuse protection degrades, availability does not.
- **Example:**
  ```rust
  #[derive(SlashCommand)]
  #[command("daily", description = "Claim", rate_limit(user, 1, per = "1d", manual))]
  struct Daily;
  impl Daily {
      #[handler]
      async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
          let msg = claim(&ctx).await?;                     // only a success is charged
          ctx.rate_limits().hit(RateScope::User).await?;
          ctx.respond(msg).await
      }
      #[on_rate_limited]
      async fn slow(ctx: RateLimitedCtx, hit: RateLimitHit) -> HandlerResult<V2Message> {
          Ok(V2Message::new(Components::new().push(TextDisplay::new(format!("Next claim {}", hit.relative_timestamp())))))
      }
  }
  ```

## Verification

Run from `/home/claude/lanes/d/noctis-rs`, release only (wrap workspace-wide runs in `tools/story-loop/bg.sh`):

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo test --release -p noctis-macros` and `cargo test --release -p noctis-core` -- expected: pass, incl. `int_14_*`
- `cargo test --release -p noctis --test ui` -- expected: pass with the new `INT-14_*` fixtures (bless `.stderr` with `TRYBUILD=overwrite` once, then review)
- `cargo test --release -p noctis --test rate_limits` and `cargo test --release -p noctis-testkit --test rate_limits` -- expected: pass
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace` -- expected: all pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, hash equals `SKILL.md` `API hash`
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- expected: exit 0 (`rules-catalog` counts INT-14 covered)
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` and `cargo +1.91 check --release --workspace` -- expected: exit 0
- `cargo check --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template` -- expected: template compiles (delete `target/template` if disk is tight)
- `cargo hack` -- not run here (gate script, epic-last story only)
