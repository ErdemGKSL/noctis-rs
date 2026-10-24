---
title: 'Story 15.2: Multiple Apps per process'
type: 'feature'
created: '2026-10-24'
status: 'in-progress'
baseline_revision: 'f9fff1c222785e2da6e5c174fa5bcf9582acf27c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-15-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-13-2-boot-time-feature-flags.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Several Apps already build on one `Runtime` with their own HTTP client, shards, Registry, Responder, `Limits`, `AppId`-labelled metrics and namespaced ports (Stories 5.x, 7.4, 10.x), but FR-47 is still incomplete. The `GuildAccessList` port exists with only `AllowAllGuilds`, and nothing consults it: AD-23 stage 3 is a placeholder comment. A Command cannot be restricted to some Apps, because `apps = [..]` is listed as `PLANNED` in the macros. The Usage Skill still marks multi-App as `[planned]`.

**Approach:**
- Enforce `GuildAccessList` at stage 3 for every interaction kind, for events and for `Dispatcher::run_command`.
- Ship a ready allow/deny Adapter, `noctis_ports::memory::GuildAccess`.
- Add an App restriction that mirrors Boot Flags: `apps = ["id", ..]` on `#[command]`, `#[user_command]` and `#[message_command]`, plus `Module::apps([..])`. `build()` prunes, before the fold, every registration or Module whose list does not hold the App's effective `AppId`, so it is never registered, dispatched, counted for intents or published.
- Move the `AppId` grammar into `noctis-rules` so the macro checks ids at compile time.
- Prove all of it with a two-App end-to-end test, and document it in the template and the Usage Skill.

## Boundaries & Constraints

**Always:**
- **Stage 3 placement.** The GuildAccessList check runs after the Command or Route is resolved (stage 1) and after the author check, but before:
  - any Ref lookup, so a denied click never `take`s a `one_shot` Ref;
  - the Expired Handler chain;
  - stage 4 locale resolution through the `LocaleResolver`;
  - rate limits, the auto-defer, Middleware, Guards and the handler.

  It is synchronous and does no port I/O.
- **When it is consulted.** Only when the interaction, event or invocation has a `guild_id`. No guild (a DM, or an event without a guild) means the App serves it.
- **A panic inside `allows`** is caught (`catch_unwind`), logged once at `error` on `noctis::dispatch` (handler or event type only), and treated as **not served** (fail closed).
- **A denied interaction** (command, context menu, component, modal submit):
  - gets one type-4 ephemeral notice with the `noctis.disabled_here` text (`keys::DISABLED_HERE`), in the built-in locale resolution (user locale, then guild locale, then App default; no `LocaleResolver`), with the same shape as the other pre-stage-4 notices;
  - plus one `debug` on `noctis::dispatch`, message `guild not served`, with fields `handler`/`route` and `kind` only (no user id, no Payload);
  - runs no handler, Middleware (before or after) or Guard, and causes no rate-limit increment, no Error Sink report and no `noctis_handler_duration_seconds` sample.
- **A denied autocomplete** gets one type-8 callback with an empty choice list; its handler does not run and nothing is reported.
- **A denied event** (`event.guild_id()` is `Some` and denied) runs none of its handlers. The event permit is released, and there is one `trace` on `noctis::dispatch`. The Discord cache is still fed (that happens before `dispatch`).
- **A denied `Dispatcher::run_command`** returns the new `Err(DispatchError::GuildNotServed { guild_id })` and sends nothing. `DispatchError` stays `#[non_exhaustive]`, and `noctis-prefix` needs no change, because its `other =>` arm already logs it.
- **App restriction semantics** are those of Boot Flags, applied in the same pre-fold selection pass, after the Boot Flag selection:
  - an unrestricted Module or registration always exists;
  - a restricted one exists only on an App whose effective `AppId` is listed;
  - a pruned Module drops its whole subtree;
  - a kept Module's `base`/`group` declaration whose node loses its last Command is dropped (the existing `retain_nodes` path).

  The effective `AppId` is the explicit `.app_id(..)`, else the id derived from the token, else `default`. It is computed once, before the fold, and reused for the App (no second `default_app_id` warning).
- **Id checking.** `apps` ids use the `AppId` grammar `[a-z0-9-]{1,32}`:
  - in the macro, a compile error;
  - in `Module::apps`, `BuildViolation::InvalidAppRestriction { module, id, reason: AppIdError }`, checked on the full trees whatever is selected, like Boot Flags.
- **`AppIdError`** keeps its variants, `#[non_exhaustive]`, derives and exact `Display` texts after the move into `noctis_rules::app_id`.
- **Same commit as the code.** The template, its `AGENTS.md`, the Usage Skill, `xtask/public-api.txt` (`--bless`) and the skill's `API hash` line change together with the code. All Rust follows `rust-best-practices`.

**Never:**
- Change Publish for the GuildAccessList: it controls serving, not registration, and guild-scoped Publish is unaffected (documented).
- Add a new metric or a rule id (`AppId` and `apps` are a framework convention, like Boot Flags).
- Add `App::from_env_prefixed` or `Runtime::run([..])`. Both are out of scope; the skill shows explicit builders and `tokio::try_join!`.
- Detect two live Apps that share an `AppId` (rejected in 7.4; the skill tells authors to set distinct ids).
- Accept `apps` on events, components or modals: the existing unknown-argument error stays.
- Wrap `GuildAccessList` in `Namespaced`.
- Make a debug build.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| GA allow hit | `GuildAccess::allow([G])`, interaction in G | handler runs normally | none |
| GA allow miss | `allow([G])`, interaction in H | `disabled_here` ephemeral notice; no handler | debug log |
| GA allow empty | `allow([])`, any guild interaction | denied; DMs still served | debug log |
| GA deny hit | `GuildAccess::deny([G])`, interaction in G | notice; no handler | debug log |
| GA deny empty | `deny([])` | serves every guild | none |
| GA duplicates | `allow([G, G])` | same as `allow([G])` | none |
| GA DM | any list, `guild_id` None (DM or user-install DM) | served; `allows` never called | none |
| GA runtime update | `Arc<GuildAccess>` shared; `insert(H)` after build | next interaction in H served (allow mode); `remove(H) -> true` then denied; `remove` of absent → `false` | none |
| GA panic | custom `allows` panics | treated as denied, notice sent | one `error` log, no report |
| Component denied, one_shot Ref | click in denied guild | notice; the Ref record remains in the store (no `take`) | debug |
| Stale id, denied guild | stale `custom_id` in denied guild | `disabled_here` notice, not the Expired chain | debug |
| Autocomplete denied | autocomplete in denied guild | type 8 with `{"choices": []}`; handler not run | none |
| Event denied | `MESSAGE_DELETE` with denied `guild_id` | no handler runs | trace |
| Event without guild | `READY`, DM `MESSAGE_CREATE` | handlers run | none |
| Dispatcher denied | `run_command` with denied `guild_id` | `Err(GuildNotServed { guild_id })`; no request | none |
| Default port | no `.guild_access(..)` | `AllowAllGuilds`: everything served | none |
| apps match | `#[command("x", apps = ["main"])]`, App `app_id("main")` | registered, dispatched, published | none |
| apps miss | same Command, App `app_id("acme")` | absent from Registry and Publish JSON; an interaction for it is "unknown command" | warn (existing) |
| apps multi | `apps = ["main", "acme"]` | present on both | none |
| apps + flags | `flags = ["beta"]`, `apps = ["main"]` | present only when both select it | none |
| apps default id | restricted Command, App without token or id (`default`) | pruned unless `"default"` is listed | none |
| apps max len | id of 32 chars | accepted | none |
| apps len 33 | 33-char id | compile error `invalid App id \`…\` in \`apps\`: App id has 33 characters: …` | compile error |
| apps empty list | `apps = []` | compile error ``apps` needs at least one App id: write `apps = ["main"]` or remove it`` | compile error |
| apps empty string | `apps = [""]` | compile error with the `AppIdError::Empty` text | compile error |
| apps uppercase / unicode | `apps = ["Main"]`, `apps = ["bot-ü"]` | compile error naming the character and its index (`InvalidChar`) | compile error |
| apps duplicate | `apps = ["a", "a"]` | compile error `\`a\` is listed twice in \`apps\`` | compile error |
| apps not a list | `apps = "main"` / `apps = [main]` | compile error ``apps` takes a list of string literals: write `apps = ["main"]``` | compile error |
| apps repeated arg | `apps = ["a"], apps = ["b"]` | existing `duplicate \`#[command]\` argument \`apps\`` | compile error |
| apps on button/event/modal | `#[button(.., apps = [..])]` | existing unknown-argument error | compile error |
| Module::apps | `Module::new("m").apps(["acme"])` on App `main` | whole subtree pruned (also components, events, Middleware) | none |
| Module::apps empty | `.apps([])` | unrestricted | none |
| Module::apps invalid | `.apps(["Bad"])` | `BuildViolation::InvalidAppRestriction { module: "m", id: "Bad", reason }` | build fails |
| Module::apps repeated | `.apps(["a"]).apps(["b"])` | appends: kept on `a` and `b` | none |
| Base pruned | every subcommand of base `cfg` restricted elsewhere | the `base("cfg", ..)` declaration is dropped | none |

</intent-contract>

## Code Map

- `crates/noctis-ports/src/access.rs` -- the `GuildAccessList` trait (sync `allows`). Add `impl<P: GuildAccessList + ?Sized> GuildAccessList for Arc<P>`, like `identify.rs:37`.
- `crates/noctis-ports/src/memory/trivial.rs`, `memory/mod.rs:50` -- `AllowAllGuilds`. Add `GuildAccess` here, or in a new `memory/access.rs` re-exported from `memory/mod.rs`.
- `crates/noctis-ports/tests/{ports.rs,memory_stores.rs}` -- the existing GuildAccessList tests (lines 553, 287, 403).
- `crates/noctis-core/src/ports.rs:110` -- `guild_access` port slot. `app.rs:2708` -- the `guild_access(impl GuildAccessList)` setter (plain, never namespaced).
- `crates/noctis-core/src/handler.rs:203` -- `AppShared`. Add `pub(crate) guild_access: Arc<dyn GuildAccessList>` and fill it where `AppShared` is built in `app.rs` (`build()`, around line 3000).
- `crates/noctis-core/src/pipeline.rs`:
  - `run_interaction`: line 1080 is the placeholder "Stage 3 (Module enablement) lands with…"; the check goes after the author check and before `stage4`;
  - `run_autocomplete` (line 1245);
  - `send_notice` (line 793) and `notice::text`, which can be reused;
  - `Dispatcher::run_command` (line 1776) and `DispatchError` (line 1652).
- `crates/noctis-core/src/pipeline/component.rs` -- after `target`/`user_id` (around line 720), before `stage4` and before `target.start` / `lookup` (the Ref `take`). Answer with the `Rejecter` and `notice_state(transport)` built-in-locale path. Add `Rejection::GuildNotServed` (`as_str` = `guild_not_served`, `expired()` = `None`), mapped to `keys::DISABLED_HERE`.
- `crates/noctis-core/src/worker.rs:192` `dispatch` -- the sync per-event fan-out. Check here, before `Job` is spawned or queued.
- `crates/noctis-core/src/module.rs`:
  - `Module` (line 186) holds `flags` and `Flagged<T>` (line 207: parallel `flags` vec);
  - `Descriptor::register(self, module, flags)` (line 22) and `__add` / `__add_flagged` (lines 295-312);
  - `selected` (line 651) is the pre-fold pruning.

  Add a parallel `apps` vec, a hidden `__add_scoped(flags, apps, descriptor)`, the public `Module::apps`, and apps-aware selection.
- `crates/noctis-core/src/registry.rs:161` `fold(.., selection: &BootFlags)`, `select` (line 358), `check_boot_flags` (line 394) -- add the effective `AppId` to the selection. Validate `Module::apps` ids on the full trees. There are about 16 `Registry::fold(` call sites, mostly tests; update them.
- `crates/noctis-core/src/app.rs`:
  - `explicit_app_id` (line 2865) is resolved before the fold;
  - `app_id = explicit.unwrap_or_else(default_app_id)` (line 2935): compute it once before the fold and reuse it;
  - `default_app_id` is at line 3140;
  - `BuildViolation` lives in `error.rs`; add `InvalidAppRestriction { module: &'static str, id: &'static str, reason: AppIdError }`, with the message ``Module `{module}` restricts to App id `{id}`: {reason}``.
- `crates/noctis-core/src/namespace.rs:53-177` -- `AppId::new` grammar and `AppIdError`. Move both to `noctis_rules::app_id` (`check(&str) -> Result<(), AppIdError>`, `MAX_LEN = 32`), with a manual `Display` and `Error` like `noctis-rules/src/boot_flag.rs`. noctis-rules has no `thiserror`. `namespace.rs` then does `pub use noctis_rules::app_id::AppIdError` and `AppId::new` calls `check`.
- `crates/noctis-rules/src/lib.rs:27,38` -- module list and docs. Add `pub mod app_id`, and add it to the facade's `noctis::rules` re-export (`crates/noctis/src/lib.rs:3367` and the doc list at line 3270).
- `crates/noctis-macros/src/flags.rs` -- the template to copy for a new `apps.rs` (`Apps` with `parse`, `is_empty`, `slice`).
- `crates/noctis-macros/src/command.rs:35,260,288`, `menu.rs:57,132,158` -- `PLANNED = ["apps"]`. Remove it and parse `apps` like `flags`.
- `crates/noctis-macros/src/kind.rs:221-240` `items(.., flags)` -- emits `__add` or `__add_flagged`. Add an `apps` param and emit `__add_scoped(flags, apps, ..)` when apps is non-empty. Other kinds pass empty `Apps`.
- `crates/noctis/src/lib.rs:2054-2115` -- the facade `ports::memory` docs and re-exports. Add `GuildAccess`.
- `crates/noctis-testkit/tests/{namespacing.rs,boot_flags.rs,lane_metrics.rs}` -- patterns for two Apps on fakes, Publish dry-run assertions and a `DebuggingRecorder`.
- `crates/noctis-prefix/src/lib.rs:224-230` -- read-only. It already handles unknown `DispatchError` variants.
- Usage Skill: `examples/project-template/.claude/skills/noctis/`:
  - `SKILL.md:23` (API hash) and `:487`;
  - `references/app-and-sharding.md` §10 (lines 888-905, `[planned]` example to rewrite) and the AppBuilder table;
  - `references/commands.md:127,853`;
  - `references/modules-middleware-ratelimits.md:69,192`;
  - `references/adapters.md:23,41,293-327`.
- `examples/project-template/src/modules/setup/handlers.rs:24` `SetupStart`. `examples/project-template/tests/flows.rs:258` is the TestApp pattern to copy.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/app_id.rs` (new), `src/lib.rs` -- move the `AppId` grammar (`check`, `MAX_LEN`) and `AppIdError` here with identical texts, plus doctests and proptests for 0, 1, 32 and 33 characters, unicode, and uppercase -- so one rules crate serves the macro and core.
- `crates/noctis-core/src/namespace.rs`, `src/lib.rs` -- re-export `AppIdError` from rules and make `AppId::new` delegate -- this keeps the public path `noctis_core::AppIdError`.
- `crates/noctis-ports/src/access.rs`, `src/memory/*`, `tests/` -- add the `Arc<P>` impl, and `GuildAccess` with `allow(impl IntoIterator<Item = Id<GuildMarker>>)`, `deny(..)`, `insert(&self, Id) -> bool` (newly added), `remove(&self, Id) -> bool`, `contains(&self, Id) -> bool`, `is_allow_list() -> bool` and `Debug`. Back it with `std::sync::RwLock<HashSet<..>>`, and recover a poisoned lock with `PoisonError::into_inner`. Add unit tests for the GA matrix rows -- the white-label Adapter.
- `crates/noctis-core/src/{handler.rs,app.rs,pipeline.rs,pipeline/component.rs,worker.rs}` -- the `AppShared.guild_access` field, and one `pub(crate) fn serves(app: &AppShared, guild: Option<Id<GuildMarker>>, what: &str) -> bool` helper (with `catch_unwind`) used by all five paths. Add `Rejection::GuildNotServed` and `DispatchError::GuildNotServed { guild_id: Id<GuildMarker> }`, with the message ``guild `{guild_id}` is not served by this App``. Replace the line-1080 placeholder with the stage-3 comment -- AD-23 stage 3.
- `crates/noctis-core/src/{module.rs,registry.rs,app.rs,error.rs}` -- add `Module::apps` (`#[must_use]`, docs with a doctest, and an "App restriction" doc section next to "Boot Flags"), `__add_scoped`, `Flagged.apps`, apps-aware selection, `InvalidAppRestriction`, and the early effective `AppId` -- the App restriction.
- `crates/noctis-macros/src/{apps.rs,command.rs,menu.rs,kind.rs,lib.rs}` -- add the `apps = [..]` parse with the exact error texts from the matrix, remove `PLANNED`, and add macro unit tests mirroring the `flags` tests (`__add_scoped (& [] , & ["main"]` for an app-only registration) -- compile-time ids.
- `crates/noctis-core/src/pipeline/tests.rs` (and `tests/components.rs`) -- unit tests for the stage-3 rows: notice, no handler, Ref not taken, stale with deny, autocomplete empty, `Dispatcher` error, panic means fail closed. Also test the event path in `worker` tests, or in testkit `events.rs`.
- `crates/noctis-core/src/registry.rs` tests -- the apps rows: match, miss, multi, with flags, default id, `Module::apps` pruning and violation, base dropped.
- `crates/noctis-testkit/tests/multi_app.rs` (new) -- the end-to-end ACs below. Use one `DebuggingRecorder` as the global recorder (a single `#[tokio::test]` per binary), or two test fns that do not install recorders.
- `crates/noctis/src/lib.rs` -- facade re-exports (`ports::memory::GuildAccess`, `rules::app_id`) and the docs table row.
- `examples/project-template/src/modules/setup/handlers.rs` -- add `apps = ["noctis-template"]` to `SetupStart`, with a comment: setup exists only on the main App; white-label Apps built from the same `configure` with another `.app_id` do not publish it.
- `examples/project-template/tests/flows.rs` -- a test `white_label_app_serves_only_its_guild`. It builds `acme` (`configure` + `.app_id("acme")` + `.guild_access(GuildAccess::allow([GUILD]))`): `/setup start` there is `TestError::UnknownCommand`, and `/profile` in `GUILD` replies. A main App with `.guild_access(GuildAccess::deny([GUILD]))` answers `/profile` in `GUILD` with the ephemeral `disabled_here` notice.
- `examples/project-template/AGENTS.md` -- add one bullet on multiple Apps: distinct `app_id`, `apps = [..]`, `.guild_access(GuildAccess::…)`, shared Adapters only through `Shared::new`.
- Usage Skill files from the Code Map:
  - remove every multi-App `[planned]`;
  - §10 example: two `App::builder()` chains with explicit `.token(..)`/`.app_id(..)` on one `Runtime`, run with `tokio::try_join!(main.run(), acme.run())`;
  - document stage-3 behavior per kind, `GuildAccess`, `Module::apps`, `apps = [..]` with its errors, `DispatchError::GuildNotServed`, `BuildViolation::InvalidAppRestriction`, and "Publish ignores GuildAccessList";
  - update the `API hash` to the hash `api-snapshot --bless` prints.
- `xtask/public-api.txt` -- re-bless.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the implementer appends `(decision)` lines for any deviation.

**Acceptance Criteria:**
- **Two Apps, own resources.** Given two Apps (`app_id` `main` and `acme`, different tokens) on one `Runtime::for_test()`, each with its own `FakeDiscordHttp`/`FakeGateway` and one shared `Arc<MemoryCacheStore>`, when both run and each receives a command:
  - each callback reaches only its own HTTP fake;
  - `!Arc::ptr_eq(main.http(), acme.http())`, and `main.runtime() == acme.runtime()`;
  - a `Resource` obtained through each App's runtime is the same `Arc`;
  - the kv keys appear under `main:cache:` and `acme:cache:`;
  - the recorder holds `noctis_handler_duration_seconds` series labelled `app="main"` and `app="acme"`.
- **Same tree, restricted Command.** Given the same `Module` tree (holding `Ping` and an `apps = ["main"]` `Admin` Command) mounted on both, when each runs `publish(PublishScope::Global).dry_run(true)`, then `main`'s JSON holds `ping` and `admin` and `acme`'s holds only `ping`, and both Apps answer `/ping`.
- **Allow and deny lists.** Given App A with `GuildAccess::allow([C])` and App B with `GuildAccess::deny([C])`, when the same command interaction from guild C arrives on each:
  - A's handler replies;
  - B's HTTP fake records exactly one request for that interaction: an ephemeral (flags 64) type-4 callback with the `This feature is disabled here.` text. There is no follow-up or edit, no handler side effect (a state counter stays at 0) and no Middleware run.
- **Usage Skill.** Given the new public API, when the story is done, the Usage Skill documents multiple Apps with no `[planned]` marker left for them, the `API hash` matches the blessed snapshot, and `cargo run --release -p xtask -- skill-check` passes.

## Spec Change Log

## Review Triage Log

## Design Notes

These decisions are also logged in the architecture memlog.
- **App restriction rides the Boot Flag pruning pass.** The story says "published only for those Apps". Pruning before the fold also keeps the Registry, intents and Publish consistent, with one mechanism. `Module::apps` mirrors `Module::flags`, so a whole subtree (components included) can be one App's. Matching is by `AppId`, the only stable per-App name, so authors restricting Commands must set `.app_id(..)` explicitly.
- **Stage 3 is checked before the Ref lookup.** That deviates from AD-23's numbering, but the check is pure: there is no port I/O, so it is observably equivalent except that a denied click never consumes a `one_shot` Ref. **Notice, not silence, for interactions:** AD-23 says stage 3 notices are localized ephemeral replies, and a silent skip shows "The application did not respond". **Silent for events and the Dispatcher:** two white-label bots that share a prefix must not both reply.
- **Fail closed on panic.** A main App must never serve a customer guild by accident. Rate limits fail open because they only throttle.
- **Story 13.3** (runtime Module enablement, also stage 3) is still backlog and may land in a parallel lane. Keep the GuildAccessList check a separate helper call, so 13.3 adds its own check next to it without a conflict in shape.

Example:
```rust
#[derive(SlashCommand)]
#[command("admin", description = "Main bot only", apps = ["main"])]
struct Admin;
builder.app_id("acme").guild_access(GuildAccess::allow([customer_guild]));
```

## Verification

**Commands** (repository root, release only):
- `cargo fmt --all` -- no diff
- `cargo test --release -p noctis-rules -p noctis-ports -p noctis-macros` -- pass
- `cargo test --release -p noctis-core` -- pass (stage-3 and selection tests)
- `cargo test --release -p noctis-testkit --test multi_app --test namespacing --test boot_flags --test publish` -- pass
- `tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot`, `skill-check`, `deps-check`, `rules-catalog` and `tw-grammar --check` -- ok
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- pass
- `cargo deny check` and `cargo +1.91 check --release --workspace` -- ok
- `cargo hack` is not run by the implementer (gate script only).
