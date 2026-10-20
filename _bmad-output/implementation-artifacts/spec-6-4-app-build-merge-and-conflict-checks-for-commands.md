---
title: 'Story 6.4: App-build merge and conflict checks for Commands'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: 'c7901aa0b63da004c5d62e3bd68fc21bbc87bbc8'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/commands.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `#[command]`, `#[user_command]` and `#[message_command]` descriptors sit in `Module` (`commands`, `menus`, both `expect(dead_code, "Story 6.4 …")`) but `App::build` never folds them. No cross-Module rule runs: a base split over two Modules is never merged, and conflicting metadata, duplicate paths, CMD-11/12/13/05 after merging and REG-01..03 counts all pass silently (FR-9, FR-56, FR-57, FR-75; AD-4, AD-7, AD-8). A Module also cannot yet declare a base or group description or the base metadata on its own.

**Approach:**
- `App::build` lowers every folded command and menu into `noctis_rules::command::{CommandDecl, ContextMenuDecl}` and runs the existing `validate_merge`. Each `Violation` becomes one `BuildViolation::Command(Violation)` in the single `BuildError`.
- On success it stores a merged, immutable command Registry (per scope: bases, resolved descriptions and metadata, O(1) path and menu maps) for 6.5/6.9.
- New `Module::base(name, BaseMeta)` and `Module::group(path, description)` declare node descriptions and base metadata (FR-57).
- A node nobody describes takes the description of a leaf from the lexicographically first Module id and logs a warning.
- `noctis-rules` adds Module-naming notes to the violations that lack them, so every App-build violation names its Modules.

## Boundaries & Constraints

**Always:**
- One rule engine (AD-8): core calls `validate_merge` with `Limits::default()` and never re-implements a tree, metadata or count rule.
- All violations at once, in this order: existing setter/plugin violations, then `MissingIntent` (fold order), then command violations (`validate_merge` order), then crypto/runtime. Nothing is dropped and nothing is "first wins" (FR-9).
- Lowering, in fold order (pre-order; a Module's commands in `add` order, then its `base`/`group` declarations in call order, then its children):
  - Each handler path is one `CommandDecl::handler(module_id, path)`. It carries `.description(LocalizedText::new(handler.description()))`, `.with_options(options.len(), measure_options(options, &limits))` and `.metadata(metadata.to_rules())`.
  - The handler gets one decl per scope from `CommandMetadata::scopes()`: `Global`, or one `Guild(id)` per guild in order.
  - Each menu is one `ContextMenuDecl::new(module_id, kind, menu_name).metadata(..).scope(..)` per scope.
  - `base`/`group` declarations become `CommandDecl::meta(module_id, path)` in every scope where some handler path has `path` as a word prefix. A declaration whose node exists in no scope becomes one `Global` decl, so it is reported as the REG-04 orphan.
- The description fallback (FR-57/AD-7) applies to a node (word prefix of a valid 1–3-word handler path) that has, in its scope, no handler at that exact path and no meta decl with a description. For such a node:
  1. Pick the leaf handler decls below the node whose Module id is lexicographically smallest (byte `Ord`).
  2. Of those, use the lexicographically smallest path; break a tie by fold order. Its description is the node's description.
  3. Append a synthetic `CommandDecl::meta(<that module>, node).description(..)` after all real decls, so CMD-05 counts it.
  4. Log one `tracing::warn!(target: "noctis::registry", …)` per (scope, node), but only when the command checks pass. The text is ``no description declared for `config` (scope: global): using the description of `config set` from Module `a`; declare one with `Module::base` or `Module::group` ``. Guild scopes print `scope: guild 42`.
- After a successful check, the resolved base metadata of each field is the explicit base declaration's value when set, otherwise the first participating handler's value (they agree after validation). It is never a default.
- Every violation that `validate_tree`/`validate_merge` report on decls with non-empty sources names its Modules. New notes, each added only when at least one involved source is non-empty and always placed before `validate_merge`'s scope note:
  - CMD-13 children and CMD-05: ``contributed by `a`, `b` `` (distinct sources of the node's subtree decls, first-appearance order; the CMD-05 note comes after `longest variant per field`).
  - CMD-13 options, CMD-10 and CMD-04: ``declared by `a` ``. For CMD-04 this is the decl that supplied the description, or the first handler when the description is missing.
  - REG-01/REG-02: ``registered by `a`, `b` `` (distinct sources of the counted decls).
  - An empty source prints `(unnamed)` (the existing `source_name`).
  - The macro passes `""` sources, so compile-time output and every existing `.stderr` stay byte-identical.
- No `unwrap`/`expect`/`panic` on descriptor data. `BuildViolation` stays `Clone + PartialEq + Eq`.
- Each public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill) and re-blesses `xtask/public-api.txt` with the Skill's `API hash`, in the same commit. Test names cite their rule id in snake form (`reg_03_…`).

**Never:**
- No dispatch, Responder or pipeline (6.5). No Publish JSON, diff or META-04 (6.9). No CLI flags.
- No new rule ids and no change to any rule's semantics beyond the added notes. Pattern spacing and `expand` are not re-run: descriptor paths are already expanded and `validate_tree` re-checks every word.
- No `AppBuilder::limits` setter. No Module id validation or uniqueness (Story 13.1). No Boot Flags or per-App filtering (Epic 13). No localized descriptions (Epic 11).
- No `BaseMeta.guilds`: scope comes from handlers. No compile-time macro for `Module::base`/`group` (App-build tier, see Design Notes).
- No debug builds.

## I/O & Edge-Case Matrix

`build(..)` means `App::builder().runtime(&Runtime::for_test()).module(..)….build()` inside `#[tokio::test]`. "Fails X" means `Err(BuildError)` whose `violations()` contain `BuildViolation::Command(v)` with `v.rule == X`, and only command violations unless the row says otherwise. Modules are `a`, `b`, …; `d` is a valid description. Handlers are `#[command]` descriptors unless the row says "hand-built" (`CommandHandler::__new_plain*` with `Box::leak` paths, in core tests).

### Merge and Registry (`__testing::command_tree(&App)`)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 1 | AC merge | `a`: `config set`; `b`: `config get prefix` | Ok. Tree has one `/config` with children `set` (handler) and `get` → `prefix` (handler). Two warnings: `config` takes `config set`'s description (Module `a` < `b`); `config get` takes `config get prefix`'s (Module `b`, its only contributor) |
| 2 | Same Module, pattern | `a`: `config (set\|get)` | Ok, one base, two leaves, one warning for `config` (from `config get`, the smaller path) |
| 3 | Explicit base | `a`: `config set`; `b`: `.base("config", BaseMeta::new().description("Configure"))` | Ok, `config` description `Configure`, no warning |
| 4 | Explicit group | `a`: `config set prefix`; `a`: `.group("config set", "Set values")` | description of `config set` is `Set values`; one warning for `config` only |
| 5 | Same explicit value twice | `a` and `b` both `.base("config", BaseMeta::new().description("C"))` | Ok, no conflict |
| 6 | No commands | Module with only events | Ok, empty tree (`""`), behaviour of 5.x unchanged |
| 7 | Plain 1-word commands | `a`: `ping`; `b`: `pong` | Ok, two bases with their own descriptions, no warning |
| 8 | Lexicographic tie-break | `zeta`: `cfg a`; `alpha`: `cfg z`; modules mounted zeta first | `cfg` description from `cfg z` (Module `alpha`), not registration order |
| 9 | Fallback in a group | `b`: `x g s2`; `a`: `x g s1`, `x h` | `x` takes `x g s1` (`a`, smallest path among `a`'s leaves `x g s1` < `x h`); `x g` takes `x g s1` |
| 10 | Unicode words | `a`: `ayarlar dil` (and `設定 言語`) | merged and rendered unchanged |
| 11 | Guild scope | `a`: `config set` with `guilds = [42]`; `b`: `config get` global | two trees: `guild 42` with `/config set`, `global` with `/config get`; no merge across scopes; warnings per scope |
| 12 | Two guilds | `guilds = [1, 2]` | the command appears in `guild 1` and `guild 2` |
| 13 | Base meta resolution | `a`: `config set` `permissions = MANAGE_GUILD`; `b`: `.base("config", BaseMeta::new().default_member_permissions(Permissions::MANAGE_GUILD).nsfw(false))`; `c`: `config get` (unset) | Ok (explicit base: unset inherits). Base metadata `permissions=32 nsfw=false` |
| 14 | Agreeing handlers | `a` and `b` both `contexts = [Guild, BotDm]` / `[BotDm, Guild]` | Ok (sets); rendered as the first handler's order `[Guild, BotDm]` |
| 15 | Menus | `a`: `#[user_command("Show profile")]`; `b`: `#[message_command("Show profile")]`; `c`: `#[command("show profile")]` | Ok: different types never clash (REG-03) and chat-input is separate |
| 16 | Menu in guild | menu `guilds = [7]` | listed under `guild 7` |

`command_tree` format: scopes in first appearance (commands, then menus), each a header line `global` or `guild <id>`. Under it:
- each base in first appearance: `  /<word> "<description>"`, then the set metadata fields in field order (` permissions=<bits>`, ` contexts=[Guild, BotDm]`, ` integration_types=[GuildInstall]`, ` nsfw=<bool>`), then ` -> <handler name> (<module>)` when the base has a handler;
- children in first appearance, indented two more spaces per depth: `<word> "<description>"` plus the optional ` -> …`;
- then menus in input order: `  user "<name>" -> <handler> (<module>)` / `  message …`, with the same metadata suffix before ` -> `.

Lines are joined by `\n` with no trailing newline.

### Conflicts (each a unit test asserting the variant, rule id, path and the Module notes)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 17 | META-01 | `a`: `config set` `permissions = MANAGE_GUILD`; `b`: `config get` `permissions = ADMINISTRATOR` | fails META-01 path `config`; notes `` `config set` sets `32` (from `a`) ``, `` `config get` sets `8` (from `b`) ``, then `scope: global` |
| 18 | META-01 set vs unset, no base decl | as 17 with `b` unset | fails META-01 (`leaves it unset (from `b`)`) |
| 19 | META-02 contexts / integration_types | differing sets across `a`, `b` | fails META-02 (one per field) |
| 20 | CMD-07 | `nsfw = true` vs `nsfw = false` | fails CMD-07 |
| 21 | Explicit base vs handler | `a`: `.base("config", BaseMeta::new().nsfw(true))`; `b`: `config set` `nsfw = false` | fails CMD-07 |
| 22 | Two explicit bases differ | `a` `.base(.., description "A")`, `b` `.base(.., description "B")` | fails REG-04 path `config`, notes `` `a` declares `A` ``, `` `b` declares `B` `` |
| 23 | Group descriptions differ | `.group("config set", "X")` in `a`, `"Y"` in `b` | fails REG-04 path `config set` |
| 24 | BaseMeta metadata below base | `.base("config set", BaseMeta::new().nsfw(true))` | fails META-02 (misplaced; `help: declare it on `config``) |
| 25 | Orphan | `.base("nothing", BaseMeta::new().description("d"))`, no handler | fails REG-04 `metadata only for a registered command path`, `declared by `a``, `scope: global` |
| 26 | CMD-12 | `a`: `config` (with options); `b`: `config set` | fails CMD-12 path `config`, notes naming `` `config` … (from `a`) `` and `` `config set` … (from `b`) `` |
| 27 | CMD-11 | `a`: `config set`; `b`: `config set prefix` | fails CMD-11 path `config set` with both Modules |
| 28 | REG-04 duplicate path | `a`: `config set`; `b`: `config (set\|get)` | fails REG-04 `one handler per command path` path `config set`, `registered by `a``, `registered by `b`` |
| 29 | Same descriptor twice in one Module | `a.add(ping()).add(ping())` | fails REG-04 with `registered by `a`` twice |
| 30 | REG-03 | `a` and `b` each `#[user_command("Show profile")]` | fails REG-03 path `Show profile`, both `registered by` notes |
| 31 | REG-03 case/bytes | `"Show profile"` vs `"Show Profile"` | Ok (byte equality) |
| 32 | REG-03 other scope | same menu, one `guilds = [1]` | Ok |
| 33 | META-03 via BaseMeta | handler `guilds = [42]` under `config`; `.base("config", BaseMeta::new().contexts(&[Guild]))` | fails META-03 path `config.contexts`, `declared by `b``, `scope: guild 42` |
| 34 | All at once | rows 17, 26, 28, 30 in one App plus `http_timeout(ZERO)` | one `BuildError`: `InvalidHttpTimeout` first, then the command violations in `validate_merge` order; nothing missing |
| 35 | CMD-04 Module description | `.base("config", BaseMeta::new().description(""))`; also 101 × `a`; also 100 × `😀` (UTF-16 200) | fails CMD-04 (`actual 0` / `101` / `200`) with `declared by `b`` |
| 36 | CMD-04 boundary pass | 100 × `a`; 1 char; 50 × `😀` (UTF-16 100) | Ok |
| 37 | CMD-10 via group | `.group("a b c d", "d")` | fails CMD-10 (`declared by`) |

### Boundaries (`tests/boundary/reg.rs`, `cmd.rs`; core unit tests for the failing side)

| # | Scenario | Input | Expected |
|---|----------|-------|----------|
| 38 | CMD-13 children | base `x` with 25 subcommands split over `a` (13) and `b` (12) | Ok; 26 fails CMD-13 path `x`, `limit at most 25, actual 26`, `contributed by `a`, `b`` |
| 39 | CMD-13 group children | group `x g` with 25 / 26 subcommands over two Modules | Ok / fails path `x g` |
| 40 | CMD-05 | a merged tree measuring exactly 8000 over two Modules (each half < 8000; explicit base/group descriptions so the fallback adds nothing) | Ok; the same tree + 1 character fails CMD-05 `actual 8001`, notes `longest variant per field`, `contributed by `a`, `b``, `scope: global` |
| 41 | CMD-05 fallback counted | tree at 8000 minus a fallback-described base | the synthetic description is counted (fails when it pushes past 8000) |
| 42 | REG-01 chat input | 100 global bases (`c0`..`c99`, hand-built, over two Modules) | Ok; 101 fails REG-01 path `chat-input commands` `actual 101`, `registered by `a`, `b``; a merged base counts once (100 bases + extra leaves under them stays Ok) |
| 43 | REG-01 menus | 15 user + 15 message global | Ok; 16 user fails REG-01 `user commands`; 16 message fails `message commands` |
| 44 | REG-02 | 101 bases / 16 user commands in `guild 1` | fails REG-02 with `scope: guild 1`; 100 global + 100 in guild 1 is Ok (scopes count separately) |

### Rules notes (unit, `noctis-rules`)

| # | Scenario | Expected |
|---|----------|----------|
| 45 | Empty sources | `validate_tree`/`validate_merge` with every source `""`: output identical to today (existing tests unchanged) |
| 46 | Named sources | the CMD-13/CMD-05/CMD-04/CMD-10/REG-01/REG-02 notes per Boundaries; order: rule notes, Module note, scope note |
| 47 | Mixed `""` and `a` | `contributed by (unnamed), `a`` |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/command/tree.rs`:
  - `check_node` (CMD-13 L63-80, CMD-05 L86-99): add the `contributed by` / `declared by` notes.
  - `check_description_len` (L103): CMD-04, `declared by` for the description's decl.
  - `check_depth` (L296): CMD-10.
  - `subtree_decls` (L221) gives the subtree's decls.
  - Tests L445+; update the exact asserts that use named sources.
- `crates/noctis-rules/src/command/merge.rs` -- `validate_merge`: the counts loop (L88-114) gets `registered by` before `.with_note(note)`; REG-03 and META-03 already name sources.
- `crates/noctis-rules/src/command/mod.rs` -- `CommandDecl`/`ContextMenuDecl`/`Scope`/`source_name`/`from_suffix`. Update the module docs (violation order and notes).
- `crates/noctis-rules/src/command/option.rs` -- `measure_options(&[OptionDecl], &Limits) -> usize` (CMD-05 share).
- `crates/noctis-core/src/registry.rs` -- `Registry::fold(roots, requested) -> (Self, Vec<BuildViolation>)`: pre-order stack walk with ancestors. Extend it to fold commands and menus, or add a sibling `registry/commands.rs`. Tests are in `registry/tests.rs`.
- `crates/noctis-core/src/module.rs` -- `commands()`/`menus()`: remove the `expect(dead_code)`. Add `base`/`group`, their storage and `Debug`.
- `crates/noctis-core/src/metadata.rs` -- `to_rules`/`scopes`: remove the `expect(dead_code)`; drop "\[planned\] Story 6.4" from the docs. Add `BaseMeta` here.
- `crates/noctis-core/src/command.rs` (`CommandHandler::{name, description, paths, options, metadata}`) and `menu.rs` (`ContextMenuHandler::{name, menu_name, kind, metadata}`): the descriptor data to lower.
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (`#[non_exhaustive]`, derives Clone/PartialEq/Eq): add the variant.
- `crates/noctis-core/src/app.rs` L1457 `build()` -- `Registry::fold` call at L1515; `registry: Arc::new(registry)`.
- `crates/noctis-core/src/lib.rs` -- re-exports (L139-157) and `__testing` (L160): add `command_tree`.
- `crates/noctis-core/src/test_support.rs` -- `Capture` (log capture) for the warning tests.
- `crates/noctis/src/lib.rs`:
  - root `pub use noctis_core::{…}` (L303);
  - `command` macro docs L501 ("\[planned\] Story 6.4");
  - Module docs.
- `crates/noctis/tests/events.rs` L126-145 -- build pattern with `Runtime::for_test()`. `tests/boundary.rs` declares modules with `#[path]`.
- `xtask/rules-pending.toml` -- has **no** `story = "6.4"` entry: App-build rows already have `unit` evidence from `noctis-rules` tests (`reg_0*`, `cmd_1*`, …). Nothing to delete; `rules-catalog` must still pass.
- `docs/discord-rules.md` -- rows CMD-04 (L81), CMD-05 (L82), CMD-06 (L83), CMD-11..13 (L91-93), META-01/02 (L135-136), REG-01..04 (L145-148).
- `examples/project-template/`:
  - `src/modules/setup.rs` L19-26 (`module()`: base `setup` has only `setup start`);
  - `.claude/skills/noctis/references/commands.md` §1 L106-116 (`BaseMeta` [planned], "lands in Story 6.4"), §2 rule-function table L226, §6 L585-625;
  - `modules-middleware-ratelimits.md` (Module API);
  - `SKILL.md` L21 (API hash);
  - `AGENTS.md`.

## Tasks & Acceptance

**Execution:**

**`crates/noctis-rules/src/command/{tree.rs, merge.rs, mod.rs}`**
- Add the Module notes (Boundaries; Matrix 45-47) and keep every `""`-source output unchanged.
- Update the tests that assert named-source CMD-13/CMD-05/CMD-04/CMD-10/REG-01/REG-02 violations.
- Add `cmd_13_*`, `cmd_05_*`, `cmd_04_*`, `cmd_10_*`, `reg_01_*`, `reg_02_*` note tests.
- Update the module docs.

**`crates/noctis-core/src/metadata.rs`**
- Add `#[non_exhaustive] #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct BaseMeta` with these pub fields:
  - `description: Option<&'static str>`;
  - `default_member_permissions: Option<Permissions>`;
  - `contexts: Option<&'static [InteractionContext]>`;
  - `integration_types: Option<&'static [IntegrationType]>`;
  - `nsfw: Option<bool>`.
- Give it `pub const fn new()` and by-value const setters with the field names (`description(&'static str)`). Add a doctest.
- `pub(crate) fn to_rules(&self) -> CommandMeta<'static>` (no guilds).
- Remove the dead-code expects and the "\[planned\]" wording.

**`crates/noctis-core/src/module.rs`**
- `pub fn base(self, name: &'static str, meta: BaseMeta) -> Self` and `pub fn group(self, path: &'static str, description: &'static str) -> Self`. Both are `#[must_use]`, keep call order and appear in `Debug`.
- Docs: checked at `build()`; values belong to every scope the node exists in; `group` is meant for 2-word group paths, and any other registered path just gets a description.
- Remove the dead-code expects.

**`crates/noctis-core/src/error.rs`**
- Add `#[error("{0}")] Command(noctis_rules::Violation)`.
- Doc: a chat-input or context-menu rule broken after merging every Module (FR-9, FR-56, FR-57); the notes name the Modules and the scope.

**`crates/noctis-core/src/registry.rs` (+ `registry/commands.rs` new)**
- Lower, validate, apply the fallback with warnings and build the merged command Registry. `pub(crate)`, with `cfg_attr(not(test), expect(dead_code, reason = "Story 6.5 dispatches …"))` on accessors unused until then.
- Per scope:
  - `bases` with `word`, `path`, resolved `description: &'static str`, resolved `CommandMetadata` (guilds empty), the optional handler and `children`;
  - `HashMap<&'static str, CommandEntry { module, ancestors, handler }>` keyed by full path;
  - `HashMap<(MenuKind, &'static str), MenuEntry>`.
- The Registry is built only when there are no command violations; otherwise it is empty.
- Unit tests: Matrix 1-16 at Registry level, plus the warning text with `Capture`.

**`crates/noctis-core/src/app.rs`**
- `build()` appends the command violations after `MissingIntent`, before crypto/runtime.
- Update the rustdoc (the checks it runs).
- `app/tests.rs`: Matrix 17-37, 42-44 (failing sides) and 34 (ordering), asserting `BuildViolation::Command(v)`, `v.rule`, `v.path` and the notes.

**`crates/noctis-core/src/lib.rs`**
- `pub use metadata::{BaseMeta, CommandMetadata};`.
- `__testing::command_tree(app: &App) -> String` in the format above.
- A crate-doc line.

**`crates/noctis/src/lib.rs`**
- Root re-export of `BaseMeta`.
- Fix the "\[planned\] Story 6.4" docs.
- Add a doctest of `Module::base`/`group` composing two Modules.

**Facade tests**
- `crates/noctis/tests/merge.rs` (new): Matrix 1, 3, 11, 13, 15, 17, 26, 28, 30 end to end through `#[command]`/menu macros, `App::build` and `noctis::__private::core::__testing::command_tree`.
- `crates/noctis/tests/boundary/reg.rs` (new; declared in `boundary.rs`): `reg_01_100_global_commands`, `reg_01_15_user_15_message`, `reg_02_guild_counts_separately`, `cmd_13_25_merged_children`, `cmd_05_8000_merged`.
- No trybuild fixtures (App-build tier).

**Repo docs and tracking**
- `docs/discord-rules.md`, in each row's Enforcement/How column:
  - REG-01..04 and CMD-05/06/11/12/13: `App::build` lowers every Module's descriptors and runs `validate_merge`, failing with `BuildViolation::Command`, Module notes and the scope.
  - CMD-04: `Module::base`/`group` descriptions are measured at App-build.
  - META-01/02 and CMD-07: `BaseMeta` explicit declarations.
  - CMD-06: across Modules it is REG-04.
- `xtask/rules-pending.toml`: no change (verify).
- `examples/project-template/`:
  - `setup.rs`: `.base("setup", BaseMeta::new().description("Set up the bot for this server"))`;
  - `commands.md`: §1 `BaseMeta`/`Module::base`/`Module::group` landed, with the exact fallback rule and warning text; §2 the new notes; §6 `BuildViolation::Command`, the `command_tree`-free description of merging, scopes and REG-01/02 per scope;
  - `modules-middleware-ratelimits.md`: the Module methods;
  - `AGENTS.md`: one bullet;
  - `SKILL.md`: the API hash.
- `xtask/public-api.txt` -- re-bless.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- add implementation decisions (planning decisions are already logged).

**Acceptance Criteria:**
- Given Module `a` with `#[command("config set", ..)]` and Module `b` with `#[command("config get prefix", ..)]`, when `App::build` runs, then it succeeds and `__testing::command_tree` shows one `/config` base holding `set` and `get prefix`, with one deterministic fallback warning per undescribed node naming the source Module.
- Given conflicting declarations across Modules (Matrix 17-37, 38-44 failing sides), when `App::build` runs, then it returns one `BuildError` listing every violation as `BuildViolation::Command` with the rule id, the Module ids in notes, the values and the scope, and nothing is silently dropped or overridden.
- Given `Module::base`/`Module::group` declarations, when they agree they are used independently of any leaf (FR-57); when two Modules differ the build fails REG-04 naming both; when absent the lexicographically-first-Module rule applies and logs the warning.
- Given each check, when `cargo test --release --workspace` runs, then a unit test asserts the exact variant and rule id. Given `cargo run --release -p xtask -- rules-catalog`, it passes with `rules-pending.toml` unchanged (no 6.4 entries existed). `api-snapshot` and `skill-check` pass with the Usage Skill documenting `BaseMeta`, `Module::base`/`group` and `BuildViolation::Command`.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 34 findings — high 0, medium 5, low 20, false 9, maybe-false 0
- findings:
  - `[medium]` `[patch]` Usage Skill `modules-middleware-ratelimits.md` §1 snippet would fail `App::build` and does not compile — confirmed: the template's `profile` handler describes itself "Show a member's profile card", so `.base("profile", ..description("Show a profile card"))` is REG-04 (`check_descriptions`), `.group("profile badges", ..)` has no handler below it (REG-04 orphan, or CMD-12 next to the invokable `profile`), and `BaseMeta` is not in `noctis::prelude`; patch: make the base/group lines consistent and import `BaseMeta`.
  - `[low]` `[reject]` A base present in several scopes cannot declare `contexts`/`integration_types` via `BaseMeta` (META-03 in the guild copy) — real but fails loudly and has a workaround (set them on the global handlers); a scoped declaration needs new public surface the spec excludes ("No `BaseMeta.guilds`").
  - `[low]` `[reject]` Orphan `base`/`group` declaration always reported as `scope: global` — the declaration has no scope of its own; the spec (Approach, Matrix 25) prescribes the single `Global` decl; cosmetic.
  - `[low]` `[reject]` `Module::base`/`group` paths are not spacing-checked; `group("config  set", ..)` reports REG-04 orphan instead of CMD-02 — confirmed (orphan meta decls skip `validate_name`), but the build still fails loudly naming the exact path; a new check is more than a direct correction for a rare typo.
  - `[low]` `[reject]` `Module::base` with a multi-word name and description only silently acts like `group` — the description lands on the named node, which is a valid result; rejecting it needs a new check; no harm shown.
  - `[medium]` `[patch]` Fallback-warning assertions in `registry/commands/tests.rs` can flake (no `rebuild_interest_cache`) — confirmed: `fold` uses `set_default` without `tracing::callsite::rebuild_interest_cache()`, while App tests in the same binary hit the same `warn!` callsite with no subscriber; the same diff fixes this race in `noctis-ui/tests/validate.rs`; patch: rebuild the interest cache after `set_default`.
  - `[low]` `[reject]` `Commands::scope` is a linear search over scopes — scopes are one per guild id plus global, a handful in practice; the spec asks O(1) path/menu maps per scope, which exist; restructuring is not a direct correction.
  - `[low]` `[reject]` Quadratic passes in `lower`/`add_fallbacks`/`build_scope` — startup-only, on tens to hundreds of declarations; no measurable harm shown; an index rewrite adds complexity.
  - `[low]` `[reject]` Uncapped `registered by` note on REG-01/02 — the note format is the spec's; long only for very large Apps; truncation adds branches.
  - `[low]` `[reject]` Fallback-borrowed CMD-04 on a base gives no "fallback from" provenance; `else continue` effectively unreachable — `#[command]` validates CMD-04 at compile time, so only doc-hidden hand-built handlers reach it; the unreachable branch is harmless.
  - `[low]` `[patch]` `docs/discord-rules.md` repeats the merge sentence in nine rows and the CMD-06 row now says REG-04 twice — the per-row sentence is what the spec asked (rejected part); the CMD-06 row's leftover "Two Modules contributing ... (see REG-04)." duplicates the new sentence; patch: delete the redundant sentence.
  - `[low]` `[reject]` Missing end-to-end tests (fallback warning via `App::build`, nested child Module declarations, global+guild declarations, `group` on 1/3-word paths) — the multi-scope part is the verification-gap finding patched below; nested folding reuses the existing pre-order walk; the rest is not required by the spec and the Registry tests cover the warning.
  - `[low]` `[reject]` `App::registry()` and render code now compiled in release builds for the `__testing` seam — `__testing` is a pre-existing unconditional `#[doc(hidden)]` module and the spec puts `command_tree` there; negligible binary cost.
  - `[low]` `[reject]` `BuildViolation::Command` exposes `noctis_rules::Violation` directly — the spec mandates `Command(noctis_rules::Violation)`; `noctis_rules` is re-exported by the facade (`noctis::rules`), so no new coupling is introduced by wrapping or not.
  - `[low]` `[reject]` An invalid leaf description feeds the fallback, giving an extra CMD-04 "declared by" on an undeclared base — only doc-hidden hand-built handlers can carry an invalid description; the build fails either way.
  - `[low]` `[reject]` Multi-word `Module::base` with description only acts as a group declaration — same as the blind-layer finding above; benign result, a new check is not a direct correction.
  - `[low]` `[reject]` Duplicate guild ids in hand-built `CommandMetadata.guilds` give a REG-04 duplicate — the macro rejects `duplicate guild id` (`noctis-macros/src/meta.rs`); hand-built only, and the failure is loud.
  - `[medium]` `[patch]` Parallel tests cache `Interest::never` for the fallback `warn!` callsite — same root cause as the flaky-assertion entry above; fixed by the same patch.
  - `[low]` `[reject]` Fallback warnings are logged even when the App build then fails on non-command violations — matches the spec ("only when the command checks pass"); moving the logging needs a new return path; noise only.
  - `[low]` `[reject]` CMD-01..03 name violations from `validate_name` carry no Module note — confirmed in `check_node`, but only hand-built handlers reach it (the macro checks names at compile time; an invalid `base`/`group` word becomes a REG-04 orphan with `declared by`); adding a note path is not a direct correction.
  - `[medium]` `[patch]` (gap) Handler options lowered into App-build CMD-05/CMD-13 are never exercised by an App-build test — pre-verified; patch: add an App-build test splitting option-heavy hand-built handlers (`__new_plain_with_options`) under one base over two Modules that fails CMD-05 with `contributed by`, and an at-limit pass.
  - `[low]` `[patch]` (gap) `is_word_prefix` word boundary untested; a `starts_with` regression passes every test — pre-verified; patch: add a Registry test with `cfg a` / `cfgx b` (different descriptions and metadata) asserting `/cfg` takes description and metadata only from `cfg a`.
  - `[low]` `[patch]` (gap) Base/group declarations applying in every scope tested only for a single-scope node — pre-verified (lowering itself verified correct); patch: add a Registry test with a base in global and guild scope plus one `Module::base(description, permissions)` asserting both trees carry it and no warning.
  - `[medium]` `[patch]` (gap) Fallback-warning assertions race with parallel App tests — same root cause and patch as the flaky-assertion entry.
  - `[low]` `[patch]` (gap) Documented order `MissingIntent` before `Command` not pinned — filed `defer`, but the order contract is new in this story, so it is this story's; patch: one assertion with a privileged-intent handler in `reg_04_every_violation_at_once` (or a sibling test).
  - `[false]` `[reject]` (intent) Fallback limited to descriptions, not every undeclared metadata field — deliberately and correctly settled by the spec's Design Notes (FR-9 forbids first wins; FR-57 is about descriptions).
  - `[false]` `[reject]` (intent) Localized base/group descriptions missing — the spec's Never list defers localized descriptions to Epic 11.
  - `[false]` `[reject]` (intent) Fallback and warning happen at App build rather than Publish — AC3 names no stage; Publish does not exist until 6.9; resolving at build is earlier and deterministic.
  - `[false]` `[reject]` (intent) Single-source rules name one Module, not "both" — the spec's Module-notes decision settles this (aggregate rules list every contributing Module).
  - `[false]` `[reject]` (intent) One `BuildViolation::Command` variant instead of one per class — spec Approach settles it; tests assert variant plus rule id.
  - `[false]` `[reject]` (intent) Registry observed only through `__testing::command_tree` — spec settles it; dispatch consumers arrive in 6.5.
  - `[false]` `[reject]` (intent) No App-build CMD-06 check across Modules — spec Design Note "CMD-06" settles it (REG-04 duplicate path).
  - `[false]` `[reject]` (intent) Tie-break uses `Module::id()` rather than an ancestor-built path — `Module::new` takes the full path-like id (`"a/b/c"`), so `id()` is that id; no ancestor composition exists to diverge from.
  - `[false]` `[reject]` (intent) `rules-pending.toml` clause vacuous — the spec's Code Map records that no 6.4 entries existed; AC4 is satisfied with the file unchanged.

## Design Notes

**Which "undeclared metadata" (decision).** AD-7's "undeclared metadata takes the value from the lexicographically first Module id" implements FR-57, which is about base and group **descriptions**. For permissions, contexts, integration types and nsfw, FR-9 forbids "first wins". The Epic 1 rule stands (without an explicit base declaration, unset is a value and must agree), so the fallback applies only to descriptions. Discord requires a description on bases and groups, so the fallback is a leaf description from the smallest Module id.

**App-build tier for Module descriptions (decision).** `Module::base`/`group` take `&'static str` at run time, so CMD-04 on them is App-build, which the catalog row already lists. A compile-time macro would duplicate `#[command]`'s description argument for little gain. Epic 11 moves descriptions to locale files anyway.

**No re-expansion (decision; supersedes memlog line "6.4 App-build runs validate_pattern_spacing").** Descriptors carry expanded paths from the macro. `validate_tree` splits on single spaces and runs CMD-01..03 on every word, so a hand-built malformed path is still caught.

**Scopes (decision).** A handler's scope set is `CommandMetadata::scopes()`. Base and group declarations follow the scopes of their node. Different scopes never merge or conflict (REG-03: global and guild may share names).

**Module notes in rules (decision).** The AC asks every violation to name the Modules. CMD-13, CMD-05, CMD-04, CMD-10 and the REG-01/02 counts lacked source notes. Adding them in `noctis-rules`, gated on non-empty sources, keeps the single engine and leaves compile-time output untouched.

**CMD-06.** Option names live inside one handler (`derive(Options)`). Across Modules, the same subcommand twice is REG-04 duplicate path (catalog CMD-06 row). No new check.

## Verification

**Commands:**
- `cargo fmt --all` -- expected: no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo test --release --workspace` -- expected: all pass, incl. `noctis-rules` note tests, `noctis-core` registry/app tests, `noctis --test merge`, `--test boundary`, `--test ui` with existing `.stderr` unchanged
- `cargo hack test --release --workspace --each-feature` -- expected: pass
- `cargo run --release -p xtask -- deps-check` -- expected: ok (no new crate edges)
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok
- `cargo run --release -p xtask -- api-snapshot --bless`, then `api-snapshot` -- expected: ok; copy the hash into SKILL.md (needs `nightly-2026-10-15` and `cargo-public-api` 0.52.0; see AGENTS.md pitfalls)
- `cargo run --release -p xtask -- skill-check` -- expected: ok
- `cargo deny check` -- expected: ok
- `cargo +1.91 check --release --workspace` -- expected: ok (MSRV)

## Auto Run Result

- **Summary:** `App::build` lowers every Module's `#[command]` handlers plus `Module::base`/`Module::group` declarations into a per-scope command Registry, runs the merge/conflict rules (REG-01..04, CMD-04/05/06/10/12/13) across Modules with "contributed by"/"declared by" Module notes, and fails with `BuildViolation::Command(noctis_rules::Violation)` (ordered after `MissingIntent`). Undeclared base/group descriptions fall back to the leaf description of the lexicographically smallest Module id, with a warning; other metadata must agree (FR-9).
- **Files changed:**
  - `crates/noctis-core/src/registry/commands.rs` (+tests): per-scope command tree, lowering, description fallback, merge checks.
  - `crates/noctis-core/src/{app.rs,error.rs,module.rs,metadata.rs,registry.rs,lib.rs,test_support.rs}`: build wiring, `BuildViolation::Command`, `Module::base`/`group`, `BaseMeta`, `__testing::command_tree`.
  - `crates/noctis-core/src/app/tests/commands.rs`: App-build tests.
  - `crates/noctis-rules/src/command/{merge,mod,tree}.rs`: Module source notes on aggregate rules.
  - `crates/noctis/src/lib.rs`, `crates/noctis/tests/{merge.rs,boundary/reg.rs}`: facade re-exports and integration tests.
  - `crates/noctis-ui/tests/validate.rs`: tracing interest-cache fix.
  - `docs/discord-rules.md`, `examples/project-template/**`, `xtask/public-api.txt`: rules doc, template, Usage Skill and API hash.
- **Review findings:** 34 (high 0, medium 5, low 20, false 9).
  - Patched (8 entries; medium 5, low 3 at entry verdict): Usage Skill base/group snippet fixed and `BaseMeta` imported; fallback-warning test flake fixed with `rebuild_interest_cache` (3 entries, one root cause); CMD-05 App-build test across two Modules plus at-limit pass; `is_word_prefix` boundary test; multi-scope base declaration test; `MissingIntent`-before-`Command` order assertion; redundant CMD-06 sentence removed from `docs/discord-rules.md`.
  - Deferred: none.
  - Rejected (17 low, 9 false), with reasons per the triage log: scoped `BaseMeta` contexts (needs excluded surface; loud failure), orphan decl reported as global (spec-prescribed), unchecked spacing in `Module::base`/`group` paths (loud REG-04), multi-word `base` acting as group (twice; benign), linear scope search and quadratic startup passes (small n), uncapped notes (spec format), CMD-04 provenance on hand-built handlers, missing extra e2e tests (covered elsewhere), `__testing` in release (pre-existing), direct `Violation` exposure (spec-mandated), invalid leaf description fallback, duplicate hand-built guild ids, warnings before failed builds (spec), name violations without Module note (hand-built only); 9 intent findings settled by spec Design Notes (description-only fallback, Epic 11 localization, build-time fallback, Module notes, single variant, `__testing` observation, CMD-06, `Module::id` tie-break, unchanged `rules-pending.toml`).
- **Follow-up review recommended: true** (first pass, 5 medium patched). Named risk: the tracing interest-cache fix for the fallback-warning tests is a concurrency fix verified only by passing runs, and the new App-build CMD-05/multi-scope tests plus the rewritten Usage Skill snippet have not had an independent review.
- **Verification:** all local gates green via `gates.sh` (fmt, clippy -D warnings, workspace tests, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by the maintainer.
- **Residual risks:** a base present in several scopes cannot declare contexts/integration types through `BaseMeta`; spacing typos in `Module::base`/`group` paths surface as REG-04 orphans rather than CMD-02; quadratic lowering on very large Apps.
