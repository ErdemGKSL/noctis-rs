---
title: 'Story 13.3: Runtime Module enable and disable'
type: 'feature'
created: '2026-10-24'
status: 'in-progress'
baseline_revision: '1b919f03e784dfda74056690fa928e149571edc0'
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

**Problem:** A Module cannot be switched off without a deploy. `ModuleStateStore` (get/put/version) and its memory default exist since Epic 4/7, but nothing reads or writes it: the pipeline's stage 3 is a comment (`pipeline.rs` "Stage 3 (Module enablement) lands with Stories 13.2 and 13.3"), there is no `ctx.modules()`, Publish ignores per-guild state, and the template dashboard and Usage Skill call a `[planned]` API.

**Approach:** Add a per-App Module-state handle `Modules` (from `ctx.modules()` on every context and `App::modules()`): `set_enabled`/`reset` write one postcard record per Module through the namespaced `ModuleStateStore`; `is_enabled` reads an `arc-swap` snapshot (global bitset plus per-guild override bitsets over the Registry's kept Modules). `run` loads the snapshot at startup and polls `version()` every `module_state_poll` (default 5 s), so the documented propagation delay equals the poll interval. Stage 3 skips a disabled Module's Commands, menus, components, modals, autocomplete, prefix dispatch and Event Handlers; Publish refreshes the snapshot and leaves a disabled Module's guild-scoped Commands out of that guild's target.

## Boundaries & Constraints

**Always:**
- **Scope type.** `pub enum ModuleScope { Global, Guild(Id<GuildMarker>) }` (`Debug, Clone, Copy, PartialEq, Eq, Hash`), `From<Id<GuildMarker>>` (→ `Guild`) and `From<Option<Id<GuildMarker>>>` (`None` → `Global`), `Display` `global` / `guild <id>`. Every `Modules` method takes `scope: impl Into<ModuleScope>`, so `Some(guild)`, `guild` and `ModuleScope::Guild(guild)` all work (the template uses both forms).
- **API** (`Modules`, `#[derive(Clone)]`, cheap clone, `Debug` shows Module count and snapshot version):
  - `is_enabled(&self, module: &str, scope) -> bool` — sync; reads only the snapshot (no port call, no allocation, no lock beyond `ArcSwap::load`); unknown id → `false`.
  - `async set_enabled(&self, module: &str, scope, enabled: bool) -> Result<(), ModuleStateError>` — `Global` sets the Module's own global bit; `Guild(g)` sets that guild's override.
  - `async reset(&self, module: &str, scope) -> Result<(), ModuleStateError>` — `Guild(g)` removes g's override; `Global` sets the global bit back to enabled (overrides kept).
  - `async refresh(&self) -> Result<(), ModuleStateError>` — reloads the whole snapshot now (port I/O).
  - `ids(&self) -> impl Iterator<Item = &'static str> + '_` — the kept Module ids in Registry pre-order.
- **Semantics.** A Module's own state in scope `Guild(g)` is g's override if any, else its global bit (default enabled); in `Global` it is the global bit. A Module is **enabled** when it and every ancestor (13.1 tree) are enabled in that scope (AND, like Boot Flags). DMs and guildless events use `Global`.
- **Ids.** Exact, case-sensitive match against the Registry's kept Modules (after Boot Flags, plugins included). Anything else (empty, other case, unicode, a pruned or unmounted id) is `ModuleStateError::UnknownModule { id }` for writes (no port call) and `false` for `is_enabled`.
- **Storage.** Logical key = the Module id verbatim (core's `Namespaced` adds `<app>:module_state:`; ids contain no `:`). Value = `noctis_ports::encode_value(0, &Record)` with `#[derive(Serialize, Deserialize)] struct Record { global: bool, guilds: Vec<(u64, bool)> }`, `guilds` sorted by id, unique. A missing key or other schema byte reads as the default record (`global: true`, no overrides); malformed bytes read as default with one `warn` on `noctis::modules` naming the Module. No TTL.
- **Writes.** `set_enabled`/`reset`: check the id, take the per-App async write mutex (serializes this process's writers), `get` the record, apply the change; when the record is unchanged return `Ok` without `put` (no version bump); else `put`, then update the local snapshot from the written record at once (read-your-writes). One `info!` on `noctis::modules` with `module`, `scope` (Display), `enabled`.
- **Refresh order:** read `version()` first, then `get` every kept Module's key in pre-order, then swap in the snapshot tagged with that version (a write racing the refresh is picked up by the next poll).
- **Poller.** `App::run_with` (so `run`, `run_with_args` run modes, `run_until` and `TestApp`) does one `refresh()` right after `load_locale_overlays` and before `publish_at_startup`; a failure is one `warn` and the default (all enabled) snapshot stays. It then spawns a task (aborted when `run_with` returns) ticking every `module_state_poll` (`MissedTickBehavior::Delay`): `version()`; when it differs from the snapshot's version, a full refresh. Errors keep the last snapshot: the first failure of a streak is one `warn` ("could not read the Module state; keeping the last snapshot"), the recovery one `info`.
- **Before any refresh** (an App built but not running, e.g. `App::modules()` right after `build()`), the snapshot is all-enabled at version 0; `refresh()` loads it.
- **Poll setting.** `AppBuilder::module_state_poll(Duration)` (last call wins), allowed 10 ms..=1 h, default `DEFAULT_MODULE_STATE_POLL = 5 s`; else `BuildViolation::InvalidModuleStatePoll { actual }`. `App::module_state_poll() -> Duration`. The documented propagation delay across processes is one poll interval (plus the store's own latency).
- **Stage 3 (AD-23)**, cached state only, before stage 4 locales, stage 5 and stage 6 auto-defer, so no handler, Middleware, Guard, Ref lookup or defer runs:
  - chat-input Commands and context menus (`pipeline.rs` `run_interaction`, after the author check): disabled → an immediate localized ephemeral type-4 notice `keys::MODULE_DISABLED`, using the built-in locale resolution (`app.locales.builtin(discord_user, discord_guild)`, like the component pre-stage-4 rejections); one `debug!` on `noctis::dispatch` with `module`, `path`, "module disabled here".
  - components and modal submits (`pipeline/component.rs`, right after the route/author resolution, before `stage4` and the struct/Ref decode, so a `one_shot` Ref is not consumed): same notice through the existing `Rejecter`/`notice_state` path.
  - autocomplete: an empty choice list, no handler, the same `debug!`.
  - `Dispatcher::run_command` (prefix): the notice as a public message reply (a message has no ephemeral form), handler not run.
  - Event Handlers (`worker.rs` `run_handler`, before stage 4): skipped silently (`trace!` on `noctis::dispatch`), no Middleware, no Error Sink report.
- **Notice key.** Rename `noctis.disabled_here` → `noctis.module.disabled` (const `keys::DISABLED_HERE` → `keys::MODULE_DISABLED`, same text "This feature is disabled here.") in `noctis-i18n` and `noctis_rules::FRAMEWORK_KEYS`; no alias (pre-1.0 convention).
- **Publish.** Except with `clear`, Publish calls `refresh()` before any HTTP request; a failure is `PublishError::ModuleState { reason }` (nothing sent). For every guild target g (each guild scope of a `Global` Publish, and the `--publish-guild g` mirror's guild part) the chat-input nodes and menus whose handler's Module is disabled in `Guild(g)` are left out; a group or base node left without handlers is dropped. Global Commands are never filtered (also not in the mirror and not when the Module is disabled globally). REG-02 (and REG-03 for the mirror) is counted on the filtered list of every guild target.
- Every public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill), re-blesses `xtask/public-api.txt` and the Skill's `API hash` in the same commit; `docs/discord-rules.md` REG-02 row describes the per-guild filter.

**Never:**
- No change to the `ModuleStateStore` trait or its memory default (no list, delete or CAS op); no Registry swap or re-fold (only the snapshot is swapped, AD-7).
- No automatic Publish after a toggle; no Lifecycle Hook (`ModuleToggled` is Story 13.5); no rate limits (13.4).
- No `module` metric label; no port call from `is_enabled` or stages 1-5 beyond the existing ones.
- No per-guild state for guilds without an override; no toggling of Middleware, locale catalogs or Templates on their own.

## I/O & Edge-Case Matrix

Snapshot semantics (`Modules` over Modules `p`, child `p/c`, `q`; guilds 1, 2; fresh store unless stated):

| # | State / call | Expected |
|---|---|---|
| S1 | fresh store | `is_enabled(x, Global)` and `(x, Guild(1))` true for p, p/c, q |
| S2 | `set_enabled("p", Guild(1), false)` | p and p/c false in Guild(1); true in Guild(2) and Global; q true |
| S3 | `set_enabled("p", Global, false)` | p, p/c false in Global and Guild(2); q true |
| S4 | S3 + `set_enabled("p", Guild(1), true)` | p, p/c true in Guild(1); false elsewhere |
| S5 | S3 + `set_enabled("p/c", Guild(1), true)` | p/c still false in Guild(1) (ancestor AND) |
| S6 | S2 + `reset("p", Guild(1))` | back to S1 |
| S7 | S4 + `reset("p", Global)` | p true everywhere; Guild(1) override true kept (record `guilds == [(1,true)]`) |
| S8 | `reset("p", Guild(2))` with no override | `Ok`, no `put` (version unchanged) |
| S9 | `set_enabled("p", Global, true)` on a fresh store | `Ok`, no `put` (version stays 0) |
| S10 | same call twice (`Guild(1), false`) | one `put` (version 1) |
| S11 | `set_enabled("", ..)`, `("P", ..)`, `("prőfile", ..)`, `("nope", ..)` | `UnknownModule { id }` with that id, store untouched; `is_enabled` of those ids false |
| S12 | id of a Module pruned by Boot Flags | `UnknownModule`; not in `ids()` |
| S13 | `is_enabled("p", None::<Id<_>>)` / `Some(Id::new(1))` / `Id::new(1)` | same as Global / Guild(1) / Guild(1) |
| S14 | override list of 10 000 guilds on p | round-trips; `is_enabled` per guild correct; record `guilds` sorted |
| S15 | stored bytes `[0xff, ..]` (other schema) under `p` | read as default, no warn |
| S16 | stored bytes `[0x00, 0xff]` (malformed) under `p` | read as default, one `warn` on `noctis::modules` naming `p`; a later `set_enabled` overwrites it |
| S17 | key of an id no longer mounted in the store | ignored (never read) |
| S18 | detached ctx (`__testing` seam) | `is_enabled` true for any id; writes and `refresh` → `ModuleStateError::Port(PortError::Unavailable)`; `ids()` empty |
| S19 | store `put` fails | `Err(Port(..))`; local snapshot unchanged |

Propagation (two Apps, same `AppId`, one shared `Arc<MemoryModuleStateStore>`):

| # | State | Expected |
|---|---|---|
| P1 | A `set_enabled("p", Guild(1), false)` | A sees it immediately; B (poll 5 s, paused clock) sees it after advancing 5 s, not after 4.9 s |
| P2 | B's store `version()` errors for 3 ticks, then recovers | one `warn`, snapshot kept, one `info` on recovery, then refreshed |
| P3 | version unchanged | no `get` calls on the tick |
| P4 | startup refresh fails | one `warn`, all enabled, run continues; next successful tick loads |
| P5 | `module_state_poll` 9 ms / 10 ms / 1 h / 1 h + 1 ns / 0 | violation / ok / ok / violation / violation |

Dispatch (p disabled in guild 1 only):

| # | Interaction | Expected |
|---|---|---|
| D1 | `/balance` (global Command of p) in guild 1 | ephemeral type-4 "This feature is disabled here."; handler, Middleware and auto-defer never run |
| D2 | same in guild 2 / in a DM | handler runs |
| D3 | click p's button / submit p's modal in guild 1 | same notice; handler, Guards and Middleware never run; a `one_shot` Ref still exists afterwards |
| D4 | autocomplete of p's Command in guild 1 | empty choices, method not called |
| D5 | p's `MESSAGE_CREATE` Event Handler for a guild-1 message | not run; for a guild-2 message it runs |
| D6 | prefix `!balance` in guild 1 (`Dispatcher::run_command`) | public notice reply, handler not run |
| D7 | p/c's Command in guild 1 | notice (ancestor disabled) |
| D8 | locale file overriding `noctis.module.disabled` (`tr`), user locale `tr` | the overridden text |
| D9 | locale file overriding `noctis.disabled_here` | `i18n!` compile error (unknown framework key) |

Publish (`p` has global `/balance`, guild-scoped `/shop` on guild 1 and 2; `q` has guild-scoped `/admin` on guild 1; base `cfg` with `cfg a` from p and `cfg b` from q, guild 1):

| # | State / scope | Expected guild-1 JSON |
|---|---|---|
| U1 | all enabled, Global | global `[balance]`; guild 1 `[shop, admin, cfg{a,b}]`; guild 2 `[shop]` |
| U2 | p disabled in Guild(1), Global | global `[balance]`; guild 1 `[admin, cfg{b}]`; guild 2 `[shop]` |
| U3 | p disabled Global, no override | global `[balance]`; guild 1 `[admin, cfg{b}]`; guild 2 `[]` (PUT empty deletes remote) |
| U4 | p and q disabled in Guild(1) | guild 1 `[]`, base `cfg` dropped |
| U5 | U2, `--publish-guild 1` mirror | `[balance, admin, cfg{b}]` (global kept) |
| U6 | refresh fails | `PublishError::ModuleState`; no HTTP request recorded |
| U7 | `clear` | no refresh; empty target as today |
| U8 | dry run of U2 | JSON/diff as U2, nothing written |
| U9 | mirror over REG-02 only because of a disabled Module's commands | counted after filtering: no `TooManyCommands` |

Display strings (exact, tested):
- `ModuleStateError::UnknownModule`: ``unknown Module `{id}`: pass the id of a mounted Module (as written in `Module::new(..)`) that the Boot Flags select``
- `ModuleStateError::Port`: `the ModuleStateStore failed: {0}`
- `PublishError::ModuleState`: `could not read the Module state from the ModuleStateStore ({reason}): check the store and retry; nothing was sent`
- `BuildViolation::InvalidModuleStatePoll`: `module state poll interval {actual:?} is out of range: use a value in 10 ms..=1 h`
- `ModuleScope` Display: `global`, `guild 42`

</intent-contract>

## Code Map

- `Cargo.toml` L71 -- workspace already pins `arc-swap = "1.9"` (crates.io max stable 1.9.3, MIT OR Apache-2.0, unused so far); add `arc-swap.workspace = true` to `crates/noctis-core/Cargo.toml`.
- `crates/noctis-ports/src/store.rs` L103-113 `ModuleStateStore` (get/put/version), `crates/noctis-ports/src/memory/stores.rs` L151/L190 (`version` += 1 per put) -- read-only; `encode_value`/`decode_value` (used by `crates/noctis-core/src/kv.rs`) -- reuse for `Record`.
- `crates/noctis-core/src/kv.rs` -- the pattern to copy: `Kv::of(app: Option<&Arc<AppShared>>)`, detached handle, `PortError::unavailable("this context has no App")`.
- New `crates/noctis-core/src/module_state.rs` (+ `module_state/tests.rs`) -- `ModuleScope`, `Modules`, `ModuleStateError` (`#[non_exhaustive]`, `thiserror`), `Record`, the pre-order `ModuleIndex` (ids, `HashMap<&'static str, u32>`, per-Module ancestor-path mask), the `Snapshot` (version, global `Box<[u64]>` bitset, `HashMap<u64, (mask, value)>` of overridden guilds only), `ArcSwap<Snapshot>`, the write `tokio::sync::Mutex<()>`, `refresh`, and a `pub(crate) async fn poll(state, interval, cancel)` loop the unit tests drive with a paused clock. Re-export `Modules`, `ModuleScope`, `ModuleStateError` at `crates/noctis-core/src/lib.rs` root (~L263-290).
- `crates/noctis-core/src/registry.rs` L153 `fold` -- build the `ModuleIndex` from the kept trees in the same pre-order walk (`stack`, ancestors already tracked); accessor `pub(crate) fn modules(&self)`.
- `crates/noctis-core/src/handler.rs` L203 `AppShared` -- add `module_state: Arc<module_state::Shared>` (store = the namespaced `ports.module_state_store`, index, snapshot, mutex); constructed in `crates/noctis-core/src/app.rs` `build()` (~L2653, beside `kv`).
- `crates/noctis-core/src/app.rs` -- `DEFAULT_MODULE_STATE_POLL` beside `DEFAULT_ACK_DEADLINE` (L87); `AppBuilder::module_state_poll` (pattern `ack_deadline` L2022 + `InvalidAckDeadline` check in `build`); `App::modules()`, `App::module_state_poll()`; `run_with` (L931): startup `refresh` after `load_locale_overlays`, spawn the poller (abort guard like `AbortOnDrop`); update `run_until`/`build` docs (`# Errors` order).
- `crates/noctis-core/src/error.rs` L472 -- `BuildViolation::InvalidModuleStatePoll { actual: Duration }` next to `InvalidAckDeadline`, doctest on `to_string()`.
- Contexts: `crates/noctis-core/src/command.rs` L297 (`CommandCtx::kv` → add `modules()`), `component.rs` L237 macro `ctx_accessors` (ComponentCtx, ModalSubmitCtx), `autocomplete.rs` L327, `handler.rs` L355 (`EventCtx`), `middleware.rs` L810 (Before/After ctx macro): `pub fn modules(&self) -> crate::Modules { crate::Modules::of(self.0.app.as_ref()) }` with a one-line doc.
- `crates/noctis-core/src/pipeline.rs` L889 (command stage 3 comment), autocomplete ~L1040-1100 (`find_autocomplete` → empty list path), `Dispatcher::run_command` ~L1420; module docs at the top (stage list) -- stage 3.
- `crates/noctis-core/src/pipeline/component.rs` L686-698 (after `span.record`/author check, before `stage4` L711) -- stage 3 through `Rejecter`; add a `Rejection` variant or a dedicated notice helper sending `keys::MODULE_DISABLED`.
- `crates/noctis-core/src/worker.rs` L428 `run_handler` -- skip before stage 4.
- `crates/noctis-core/src/publish.rs` L404 `execute` (refresh after the token check, unless `clear`), L427 `targets(commands, scope, clear)` gains the snapshot (or a `&dyn Fn(&'static str, Id<GuildMarker>) -> bool` keep test); L471 `check_mirror` split so REG-02 counting runs on every guild target; `PublishError` (L86) new variant. `crates/noctis-core/src/publish/json.rs` L17 `local` → add a filtered variant (prune `CommandNode`s by `handler.module`, drop emptied groups/bases; filter `menus()` by `MenuEntry::module`). Tests: `publish/tests.rs` L386 `targets_split_guild_scoped_commands`, L471 `reg_02_dev_mirror_counts` patterns.
- `crates/noctis-i18n/src/lib.rs` L58-80, L114, L248 and `crates/noctis-rules/src/key.rs` L123 -- key rename; callers `crates/noctis-core/src/notice.rs` L97, `pipeline/tests.rs` L2425-2459.
- `crates/noctis/src/lib.rs` -- re-export `Modules`, `ModuleScope`, `ModuleStateError` (root list ~L607) and add `ModuleScope` to `prelude` (~L2244; template writes it unqualified).
- `crates/noctis-testkit/tests/` -- facade-level tests: patterns in `boot_flags.rs` (13.2), `cluster.rs` (two Apps), `events.rs`, `components.rs`; `TestApp::build(&mut builder)`, `.module_state_store(Arc::clone(&store))`, `.app_id("t")`.
- Template: `examples/project-template/src/modules/dashboard/handlers.rs` L70, L103-105, L126 (already `ctx.modules()`, `ModuleScope::Guild`, `Some(guild)`), `mod.rs` L100-117 `ModuleKey` (`setup`, `profile`; `dashboard` itself not toggleable), `AGENTS.md`, `README.md` L15; the template crate is compiled by no gate (13.2 deferred item; it still uses the planned `ctx.cache()`), so reconcile by reading.
- Usage Skill `examples/project-template/.claude/skills/noctis/`: `references/modules-middleware-ratelimits.md` §3 L197-209 and L317 ("Planned: `cache()`, `modules()`"), `references/adapters.md` L386-392 `[planned]` block, `references/commands.md` L853 and L907, `references/i18n.md` L324 (key row), `references/troubleshooting.md` (new error texts), `SKILL.md` L23 `API hash`, L177 example.
- `docs/discord-rules.md` L146 REG-02 row.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-i18n/src/lib.rs`, `crates/noctis-rules/src/key.rs`, `crates/noctis-core/src/notice.rs`, `crates/noctis-core/src/pipeline/tests.rs` -- rename the key to `noctis.module.disabled` / `MODULE_DISABLED`; keep `ALL` order -- the story names the key.
- `crates/noctis-core/Cargo.toml`, `crates/noctis-core/src/module_state.rs`, `module_state/tests.rs` -- types, snapshot, record codec, writes, refresh, poller; unit tests S1-S19, P1-P4 (P1/P2 with `#[tokio::test(start_paused = true)]` against a counting/failing `ModuleStateStore` wrapper) and every Display string -- the cached, swapped state (AD-7).
- `crates/noctis-core/src/registry.rs` -- `ModuleIndex` from the kept trees; test pre-order ids and ancestor masks, Boot-Flag-pruned Module absent -- one index per App.
- `crates/noctis-core/src/error.rs`, `crates/noctis-core/src/app.rs`, `app/tests.rs` -- poll setter/violation (P5), `App::modules`, `App::module_state_poll`, startup refresh and poller in `run_with`, `AppShared` wiring -- the operator surface.
- `crates/noctis-core/src/command.rs`, `component.rs`, `autocomplete.rs`, `handler.rs`, `middleware.rs` -- `modules()` on every context -- `ctx.modules()`.
- `crates/noctis-core/src/pipeline.rs`, `pipeline/component.rs`, `worker.rs` (+ their tests) -- stage 3 for Commands, menus, components, modals, autocomplete, `Dispatcher::run_command`, events; update the pipeline module docs -- AD-23 stage 3.
- `crates/noctis-core/src/publish.rs`, `publish/json.rs`, `publish/tests.rs` -- refresh, guild-target filter, REG-02 on every guild target, `PublishError::ModuleState`; tests U1-U9 -- FR-45 Publish rule.
- `crates/noctis-core/src/lib.rs`, `crates/noctis/src/lib.rs` -- re-exports and prelude -- public API.
- `crates/noctis-testkit/tests/module_toggle.rs` (new) -- facade App with Module `eco` (`/balance`, a `#[button("eco.buy", defaults(..))]`, a `MESSAGE_CREATE` `#[derive(EventHandler)]` counting runs in `State`) and Module `admin` (`/eco off|on` calling `ctx.modules().set_enabled("eco", ctx.guild_id(), ..)`); two `TestApp`s A and B sharing one `Arc<MemoryModuleStateStore>` (same `.app_id`), B with `module_state_poll(50 ms)`: A's `/eco off` in guild 1 → A's `/balance` in guild 1 answers the ephemeral notice at once; B's answers it within 2 s (poll loop), B's `/balance` in guild 2 runs; the `eco.buy` click in guild 1 answers the notice; a guild-1 `MESSAGE_CREATE` does not run the handler, a guild-2 one does; `ctx.modules().is_enabled` read in a handler of B agrees; `/eco on` restores -- outermost-surface proof of AC1, AC2, AC4.
- `docs/discord-rules.md` -- REG-02 row: per-guild filter of guild-scoped Commands of disabled Modules, counted per guild target after filtering -- rules doc stays true.
- `examples/project-template/src/modules/dashboard/handlers.rs`, `mod.rs`, `AGENTS.md`, `README.md` -- confirm calls match the landed signatures (`is_enabled(id, Some(guild))`, `set_enabled(id, ModuleScope::Guild(g), enable)`), doc the toggle: one AGENTS bullet (API, AND over the tree so disabling `profile` also disables `profile/badges`, 5 s propagation via `module_state_poll`, re-Publish to drop guild-scoped Commands, share the `ModuleStateStore` across a Cluster) -- template reconciled.
- `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md`, `adapters.md`, `commands.md`, `i18n.md`, `troubleshooting.md`, `SKILL.md` -- §3 rewritten (API, `ModuleScope` forms, semantics incl. ancestor AND, DMs = Global, storage key/record, read-your-writes, poll + delay, lost-update caveat, what stage 3 blocks per kind, one-shot Ref not consumed, Publish rule and that a toggle does not re-Publish, `ids()`, `refresh()`, detached behaviour); drop `[planned]`/"Planned: `modules()`"; key rename; troubleshooting rows for the four new error texts -- Usage Skill documents runtime enable/disable.
- `xtask/public-api.txt`, `SKILL.md` -- re-bless, set `API hash` -- AGENTS.md policy.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- planning decisions already appended; add implementation decisions if any.

**Acceptance Criteria:**
- Given two Apps sharing one `ModuleStateStore`, when one calls `ctx.modules().set_enabled(module, guild, false)`, then the writer sees it at once and the other within one `module_state_poll` (default 5 s), refreshed through `version()` into its `arc-swap` snapshot.
- Given a Module disabled in a guild, when a user invokes its global Command, clicks its component, submits its modal or triggers its Event Handler there, then the Command/component/modal gets the localized ephemeral `noctis.module.disabled` notice before any defer, and no handler, Middleware or Guard runs.
- Given per-guild Publish, when a Module is disabled for a guild, then only its guild-scoped Commands leave that guild's target JSON (REG-02 counted after filtering) and global Commands stay published.
- Given `ctx.modules().is_enabled(module, guild)` in a handler, when it is called, then it answers from the snapshot with no port call.
- Given the new public API, when the story is done, then the Usage Skill documents runtime enable/disable, the template dashboard toggles use the landed API, and `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- (decision) One record per Module (key = id) instead of one key per (Module, guild): the store has no list op, so a refresh must know every key up front; the kept Module ids are known from the Registry. Cost: N `get`s per changed version, and concurrent writers in two processes toggling the same Module within one read-modify-write window can lose one change (no CAS in the port, AD-14). Documented in the Usage Skill; acceptable for operator-speed toggles.
- (decision) `ModuleScope` + `impl Into<ModuleScope>` reconciles the two forms the Usage Skill and template already use (`Some(guild)`, `ModuleScope::Guild(g)`) and gives 13.5's `ModuleToggled` hook its scope type. `reset` exists because an override could otherwise never be removed.
- (decision) Enablement is AND over the Module tree (mirrors Boot Flag nesting and Middleware parent-first order); unknown ids read as disabled and fail writes.
- (decision) The key is renamed to the story's `noctis.module.disabled` (no alias); the text is unchanged.
- (decision) Stage 3 for components runs before the struct/Ref decode, so a disabled Module never consumes a `one_shot` Ref; notices before stage 4 use the built-in locale resolution like the existing pre-stage-4 rejections. Autocomplete answers an empty list; events skip silently.
- (decision) Fail-open: an unreadable store at startup leaves every Module enabled (warn), a failing poll keeps the last snapshot. Publish fails closed (`PublishError::ModuleState`) because it writes Discord state.
- (decision) A toggle never triggers Publish (FR-13: Publish only in the designated process or CLI); operators re-run `--publish` to drop guild-scoped Commands.

```rust
ctx.modules().set_enabled("profile", ModuleScope::Guild(guild), false).await?;
ctx.modules().set_enabled("profile", ModuleScope::Global, true).await?;
let on: bool = ctx.modules().is_enabled("profile", Some(guild)); // snapshot, no I/O
builder.module_state_poll(Duration::from_secs(5)); // default; = propagation delay
```

## Verification

Run from `/home/claude/lanes/b/noctis-rs`, release only (wrap workspace-wide runs in `tools/story-loop/bg.sh`):

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace --all-features` -- expected: all pass, incl. `module_state` unit tests, publish/pipeline/worker tests and testkit `module_toggle`
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, printed hash equals `SKILL.md`'s `API hash`
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- expected: exit 0
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` -- expected: exit 0 (arc-swap MIT OR Apache-2.0)
- `cargo +1.91 check --release --workspace` -- expected: exit 0
- `cargo hack` is not run here (only in an epic's last story, `tools/story-loop/gates.sh`)

**Manual checks:**
- `examples/project-template` is compiled by no gate (13.2 deferred item; it still calls the planned `ctx.cache()`): read `dashboard/handlers.rs` and confirm each `modules()` call matches the landed signatures.
