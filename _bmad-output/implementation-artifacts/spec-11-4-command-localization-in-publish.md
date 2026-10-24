---
title: 'Story 11.4: Command localization in Publish'
type: 'feature'
created: '2026-10-23'
status: 'in-progress'
baseline_revision: '3112448dddd819d1d872649dcbd1b2dac8c8060c'
review_loop_iteration: 2
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-11-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/ownership-and-lifetimes.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized', 'multiple-goals']
deferred:
  - summary: >-
      App-build LOC-03 does not compare localized names of top-level bases (per scope) or of same-kind keyed context menus against each other.
    evidence: |-
      Unverified (maybe-false). children_arrays only builds arrays under a parent node and Table::check never groups menus.
      Discord documents the distinctness constraint only for option names of one command; settle by checking whether a bulk PUT with two commands (or two USER menus) sharing a localized name in one locale is rejected.
    location: >-
      crates/noctis-core/src/registry/commands.rs children_arrays / Table::check
    severity: medium (unverified)
  - summary: >-
      tools/story-loop/gates.sh skips cargo hack except for an epic's last unfinished story, while AGENTS.md still requires cargo hack before every Rust change is done.
    evidence: |-
      gates.sh:28-42 (2026-10-23 loop change bundled into the Story 11.4 implementation commit) contradicts AGENTS.md "Running and verifying"; the decision is not logged in a memlog and the ui_i18n trybuild suite only runs under hack. Fix edits AGENTS.md (agent-context file).
    location: >-
      tools/story-loop/gates.sh:28-42, AGENTS.md
    severity: medium
  - summary: >-
      The gates.sh last-story test misfires when two lanes run an epic's last two stories, so cargo hack may never run for that epic.
    evidence: |-
      The inline Python treats any sibling story of the in-progress epic not in done/in-progress (e.g. review) as unfinished, so a lane whose sibling is in review skips hack, and the sibling has already passed its gates.
    location: >-
      tools/story-loop/gates.sh:31-41
    severity: medium
---

<intent-contract>

## Intent

**Problem:** Publish never sends localizations (`publish/json.rs`: "localizations are never sent"), although locale files already carry `cmd.*`, `cmdword.*` and `ctxcmd.*` keys and the merged catalog (11.2) holds them. `#[command]` cannot take its description from the default-locale file, `#[user_command(.., key = ..)]` is still "planned", and no LOC-02/LOC-03 check runs anywhere (FR-41, FR-12, FR-13, FR-77; AD-16; LOC-02, LOC-03, LOC-05, LOC-06).

**Approach:** At `App::build`, merge the locale catalogs before the Registry fold, resolve every registered node, option, choice and menu against the merged `AppStrings` into per-locale variants, validate them (LOC-02/03 as one typed `BuildViolation::CommandLocalization` each; LOC-05 by feeding the variants into `validate_merge`'s CMD-05 budget), and emit them in the Publish JSON. With the `i18n` feature, `#[command]` reads `cmd.<path>.description` and option descriptions from its crate's default-locale file at compile time; `i18n!` checks LOC-02/LOC-03 and the reserved-key layout of every file it reads.

## Boundaries & Constraints

**Always:**

- **Key layout (AD-16).** `<path>` = Command Path words as nested YAML maps; `<value>` = a choice value as text (`Str` as is, `Int` via `Display`).
  - Node at path `P` (base, group or subcommand): `cmd.P.name`, `cmd.P.description`.
  - Option `o` (its Discord name) of leaf `P`: `cmd.P.opt.o.name`, `cmd.P.opt.o.description`, `cmd.P.opt.o.choice.<value>`.
  - Per word: `cmdword.<word>` (command, group and subcommand words only, never options). Menus: `ctxcmd.<key>.name`.
  - A word, option name or choice value that is not a reserved-family key segment (`[A-Za-z0-9_-]+`, e.g. `çay`, `1.5`, `big red`) has no key and is never localized. That is not an error.
- **Resolution (Publish).**
  - For locale `L`, the chain is `[L]`, then `L.sibling()` (en-US↔en-GB, es-ES↔es-419; none for others).
  - A text's value is the first hit walking the chain. In each chain locale, try the full-path key first, then (names of nodes only) `cmdword.<last word>`. So `L`'s own `cmdword` beats the sibling's `cmd.P.name`.
  - Lookups use the new `AppStrings::get(locale, key)` (owner catalog's own file, no fallback).
  - Every `L` in `Locale::ALL` with a hit is emitted, including the default locale and sibling-derived codes. Values are used verbatim.
- **JSON.**
  - `name_localizations` / `description_localizations` objects keyed by `Locale` code (`"tr"`, `"en-GB"`). Command, subcommand, group and option objects get both; choice objects get `name_localizations`; context menus get `name_localizations`.
  - Insert them right after `name` / `description`. Omit a field whose map is empty, so a catalog-less App's JSON is byte-identical to today's.
  - The guild mirror (`strip_install`) keeps them. `diff.rs` already canonicalizes them; do not change it.
- **Default texts (LOC-06).** `name` is always the default word or menu literal. Dispatch is unchanged: the Registry maps hold default names only.
  - A base or group without a declared description takes `cmd.<P>.description` from its owner catalog's `default_locale()` file, with no warning. The synthetic decl's `source` is that catalog's `origin()`. This runs before the existing handler fallback, which keeps its warning.
- **Validation at `App::build`.**
  - LOC-02 runs over every emitted value with `noctis_rules::command::validate_localized`:
    - node names: `Name`, giving CMD-01/02/03;
    - descriptions: `Description`, giving CMD-04;
    - choice names: `ChoiceName`, giving OPT-09;
    - menu names: `ContextMenuName`, giving CTX-01.
  - LOC-03 runs per locale over every options array (leaf options) and every children array (subcommands/groups) with `validate_localized_option_names`.
  - Each problem is one `BuildViolation::CommandLocalization`, as defined below. A `(key, file locale, rule)` triple is reported once, naming the locale whose file holds the value, never a sibling-derived code. The violations are sorted by key bytes, then `Locale::ALL` index, then LOC-02 before LOC-03.
  - They run even when `validate_merge` fails, and are reported right after the `BuildViolation::Command`s, before `DuplicateLocaleKey`. When the merge fails (`DuplicateLocaleKey`), localization is skipped: no catalogs means no localizations.
- **LOC-05.** `lower()` passes the variants into the `CommandDecl`s: `description.localized`, `name_localized`, and `options_size` from `measure_options` over per-handler temporary `OptionDecl`/`Choice` copies carrying the localized slices. The existing CMD-05 check then counts the longest variant per field.
- **New public API:**
  - `noctis_i18n::AppStrings::get(&self, Locale, &str) -> Option<&'static str>` and `AppStrings::owner(&self, &str) -> Option<&Catalog>`.
  - `noctis_rules::command::LocalizedField` (`#[non_exhaustive]` enum: `Name`, `Description`, `ChoiceName`, `ContextMenuName`), `validate_localized(field, text, &Limits) -> Vec<Violation>` (the same checks as the defaults, `path` empty), and `validate_localized_option_names(defaults: &[&str], localized: &[Option<&str>]) -> Vec<(usize, Violation)>`.
    - In `validate_localized_option_names`, option `i`'s localized name `x` is a CMD-06 violation (path `x`) when `x` equals `defaults[j]` for some `j ≠ i`, or equals `localized[j]` for some `j < i`.
    - Equal to its own default is fine. The comparison is exact.
  - `BuildViolation::CommandLocalization { rule: RuleId /* LOC_02 | LOC_03 */, locale: Locale, key: &'static str, origin: &'static str, violation: noctis_rules::Violation }`, Display ``[{rule}] locale `{locale}`, key `{key}` (catalog `{origin}`): {violation}``. Every field is documented and the variant has a doctest.
  - `ContextMenuHandler::key() -> Option<&'static str>`, and `CommandHandler::descriptions() -> &'static [&'static str]` (parallel to `paths()`); `description()` returns the first path's description.
- **`#[command]` / `#[derive(SlashCommand)]` with the `i18n` feature.** Add a `noctis-macros` feature `i18n`; the facade's `i18n` enables it.
  - New args `locales = "<dir>"` (default `"locales"`), `default_locale = "<code>"` (default `"en-US"`; a non-code is the `i18n!` LOC-01 `ParseLocaleError` message) and hidden `__locales_from_source_file`.
  - The file is `<dir>/<code>.yaml`, relative to `CARGO_MANIFEST_DIR` (or to the source file with the hidden flag).
  - When the file exists, emit `include_bytes!` for it and parse it with the `i18n!` YAML walker. A file with errors gives one error, ``locale file `<dir>/<file>` has errors (see `i18n!`): <first error>``.
  - When the file is missing, an explicit `locales`/`default_locale` is a compile error naming the path; otherwise Story 6.1's literal rule applies unchanged.
  - Without the feature, `locales`/`default_locale` are a compile error: ``needs the `i18n` feature of `noctis` ``.
  - With a file present, the macro reads only descriptions (rules in the Matrix) and the reserved words:
    - a path word `name`, `description` or `opt` is `[CMD-01]` with an ambiguity message and a rename help;
    - per-path descriptions go into a new hidden `__SlashCommand::DESCRIPTIONS` (parallel to `PATHS`), which replaces `DESCRIPTION`.
- **Menus.** `key = "<segment>"` on `#[user_command]` / `#[message_command]` leaves `PLANNED`. It is ungated, takes a string literal, and must match `[A-Za-z0-9_-]+`; otherwise ```key` is one locale key segment (ASCII letters, digits, `_`, `-`)``.
- **`i18n!` per file.**
  - Reserved layout:
    - under `cmd`, each map is a command word (at most 3 words deep) whose keys are `name` (string), `description` (string), `opt` (map) or a child word (map);
    - `opt.<o>` maps hold only `name`, `description` (strings) and `choice` (a map of strings);
    - `cmdword.<w>` is a string;
    - `ctxcmd.<k>` holds only `name` (string).
    - Anything else is an error naming the full key.
  - LOC-02: every reserved value is checked with `validate_localized`, as ``[LOC-02] `<dir>/<file>`: `<key>`: <violation>``.
  - LOC-03: per file and `cmd.P.opt` map, the defaults are the sorted union of option segments under `cmd.P.opt` across all files of the invocation, and the localized names are this file's `cmd.P.opt.o.name`. The error is ``[LOC-03] `<dir>/<file>`: `<key>`: <violation>``.
- **Docs and gates.** Rule docs and the pending list change in the same commit:
  - update the `docs/discord-rules.md` LOC-02, LOC-03, CMD-01 (reserved words), CMD-05 and CTX-01 cells;
  - remove the four LOC-02/LOC-03 entries from `xtask/rules-pending.toml`.
- **Template, skill and snapshot** ship in the same commit (AGENTS.md).

**Never:**

- No localized names in dispatch or the Registry maps.
- No `Box::leak` or `'static` laundering for the variant slices; use an owned table that the lowered decls borrow.
- No change to `diff.rs` canonicalization, to `Catalog`'s API, or to runtime `Translate` lookups.
- No equal-value exemption for `DuplicateLocaleKey` on `cmdword.*`. 11.2's rule stands: one owner per key.
- No warning for orphan `cmd.*` / `ctxcmd.*` keys. No compile-time comparison of `#[command]`'s description with the merged catalog.
- No `LocaleSource` work (Story 11.5).
- No per-path option arrays: one `OPTIONS` per struct.
- No debug builds, no trybuild `pass` suites, and no weakening of `.cargo/config.toml`.

## I/O & Edge-Case Matrix

`C` = catalog `bot/locales` (default en-US). "→ tr x" means that `name_localizations` (or `description_localizations` for description rows) has `"tr": "x"`. Unless a row says otherwise, keys are in `C`.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Per-word name | tr `cmdword.ban: yasakla`; `/ban` | `/ban` → tr `yasakla` | none |
| 2 | Full path overrides word | row 1 + tr `cmd.ban.name: engelle` | → tr `engelle` | none |
| 3 | Same word, two nodes | tr `cmdword.ban`; `/ban` and `/mod ban` | both nodes → tr `yasakla`; base `mod` → tr only with `cmdword.mod` | none |
| 4 | Full path scoped to its node | tr `cmd.mod.ban.name: at` | `/mod ban` → `at`; `/ban` keeps the per-word value | none |
| 5 | Own per-word beats sibling's full path | en-GB `cmdword.ban: bar`; en-US `cmd.ban.name: ban-us` | en-GB `bar`, en-US `ban-us` | none |
| 6 | Sibling, no file | en-US `cmd.ban.description: Ban a member`, no en-GB file | description_localizations has en-US **and** en-GB `Ban a member` | none |
| 7 | Sibling file present | row 6 + en-GB `cmd.ban.description: Ban (GB)` | en-GB `Ban (GB)`, en-US `Ban a member` | none |
| 8 | Sibling file lacks key | en-GB file without the key | en-GB gets the en-US value (per-key chain) | none |
| 9 | es pair | es-419 has a value, no es-ES file | es-ES and es-419 both emitted | none |
| 10 | No sibling | pt-BR file missing | no `pt-BR` entry | none |
| 11 | Options | tr `cmd.ban.opt.user.{name,description}` | option object → tr name and description | none |
| 12 | Choices | tr `cmd.color.opt.c.choice.Red: Kırmızı` (value `Red`) | choice → tr `Kırmızı`; other choices have no field | none |
| 13 | Int choice | value `-1`, tr `choice.-1` | emitted | none |
| 14 | Unkeyable value/word | Float choice `1.5`, word `çay` | never localized, no error | none |
| 15 | Context menu with key | `key = "show_profile"`, tr `ctxcmd.show_profile.name: Profili göster` | menu → tr; `name` stays `Show profile` | none |
| 16 | Menu without key | entries exist, no `key` | no field | none |
| 17 | No catalogs / empty catalog | App without `.locales` | JSON byte-identical to Story 6.9 output (existing publish tests pass unchanged) | none |
| 18 | Two catalogs | `cmdword.*` in A, `cmd.*` in B | both used (owner lookup) | none |
| 19 | Key in two catalogs | `cmdword.ban` in A and B | `DuplicateLocaleKey` (11.2); no localization violations | `Err` |
| 20 | Base description from catalog | no `Module::base`; en-US `cmd.setup.description` | node description from file; no fallback warning | none |
| 21 | Declared wins | `Module::base("setup", ..description("X"))` plus the key | `X` | none |
| 22 | Neither | no declaration, no key | handler fallback + warning (unchanged) | none |
| 23 | Guild mirror | `PublishScope::Guild` | localization fields kept | none |
| 24 | Diff | remote equals local maps / remote has none | no change / `~ update … (description_localizations, name_localizations)` | none |
| 25 | LOC-02 CMD-03 | tr `cmdword.ban: Yasakla` | `CommandLocalization { rule: LOC_02, locale: Tr, key: "cmdword.ban", origin: "bot/locales", violation.rule: CMD_03 }` | `Err` |
| 26 | LOC-02 CMD-01 | tr `cmd.ban.name: "ya sakla"` / `"it's"` | CMD-01 | `Err` |
| 27 | LOC-02 CMD-02 bounds | `""` / 33 chars / 32 chars | CMD-02 / CMD-02 / ok | `Err` / `Err` / none |
| 28 | Unicode name ok | tr `kullanıcı`, hi Devanagari | ok | none |
| 29 | LOC-02 CMD-04 bounds | description `""` / 101 / 100 (UTF-16, emoji count 2) | CMD-04 / CMD-04 / ok | `Err` / `Err` / none |
| 30 | LOC-02 OPT-09 | choice name `""` / 101 / 100 | OPT-09 / OPT-09 / ok | `Err` / `Err` / none |
| 31 | LOC-02 CTX-01 | ctx name 33 / 32 / mixed case and spaces | CTX-01 / ok / ok | `Err` / none / none |
| 32 | Sibling-derived not double-reported | bad en-US value also serves en-GB | one violation, `locale: EnUs` | `Err` |
| 33 | Shared word reported once | bad `cmdword.ban` used by two nodes | one violation | `Err` |
| 34 | LOC-03 vs default | tr `cmd.ban.opt.reason.name: user`; option `user` exists | `rule: LOC_03`, key `cmd.ban.opt.reason.name`, violation CMD-06 | `Err` |
| 35 | LOC-03 same locale | tr opt `a` and opt `b` both named `x` | one violation, on `b`'s key | `Err` |
| 36 | LOC-03 own default | tr `cmd.ban.opt.user.name: user` | ok | none |
| 37 | LOC-03 subcommands | tr `cmd.mod.ban.name: kick`, sibling `mod kick` | LOC-03 on `cmd.mod.ban.name` | `Err` |
| 38 | LOC-03 other locale only | tr name equals a de name | ok (per locale) | none |
| 39 | LOC-05 | base at 7990 chars by default; tr description +20 | `BuildViolation::Command` CMD-05 (note `longest variant per field`); passes without tr | `Err` |
| 40 | Ordering | violations in several keys/locales | sorted (key, `Locale::ALL`, LOC-02 < LOC-03), after the `Command` ones | `Err` |
| 41 | Unregistered keys | `cmd.ghost.*` invalid, no `/ghost` | ignored at App-build | none |
| 42 | LOC-06 | `/ban` localized; interaction `data.name` `ban` / `yasakla` | routes / unknown command (testkit refuses: no such definition) | none / `TestError` |
| M1 | Literal, no file | `#[command("ping", description = "d")]`, no `locales/` | unchanged | none |
| M2 | File only | en-US `cmd.ping.description: Pong` | description `Pong`; the file is in `include_bytes!` | none |
| M3 | Both equal | literal `Pong` and the key | ok | none |
| M4 | Both differ | literal `X`, file `Pong` | `[CMD-04]` "description differs from `cmd.ping.description` in `<dir>/en-US.yaml`", literal and file text, help: remove the literal or make them equal | compile error |
| M5 | Neither | no literal, no key | `[CMD-04]` missing description + note ``not found as `cmd.ping.description` in `<dir>/en-US.yaml` `` | compile error |
| M6 | Pattern, per path | `config (set\|get)` with two keys | `DESCRIPTIONS` = both texts, in expansion order | none |
| M7 | Pattern, one missing | key only for `config set`, no literal | CMD-04 for `config get` only | compile error |
| M8 | Pattern, literal conflicts | literal plus one differing key | CMD-04 conflict for that path | compile error |
| M9 | Length from file | 101-char key value / 100 | CMD-04 (existing check) / ok | compile error / none |
| M10 | Reserved word | `"config opt"`, `"name"`, `"x description"` with a file present | `[CMD-01]` ambiguity | compile error |
| M11 | Reserved word, no file | `"name"`, feature on, no file | compiles (Epic 6 rule) | none |
| M12 | Option from file | no `#[option(description)]`, key present | used | none |
| M13 | Option conflicts / missing | literal ≠ key / neither | CMD-04 conflict / CMD-04 missing with key note | compile error |
| M14 | Option across expansions | keys for one option differ between paths, no literal | `[CMD-04]` "different descriptions across the Pattern's expansions" naming both keys | compile error |
| M15 | Explicit dir missing | `locales = "nope"` | error naming `nope/en-US.yaml` | compile error |
| M16 | `default_locale` | `"tr"` with `tr.yaml` / `"en"` | reads `tr.yaml` / LOC-01 message naming `en-GB`, `en-US` | none / compile error |
| M17 | Arg shape | duplicate `locales`; `locales = 5`; `locales` without the feature | `duplicate … locales`; `takes a string literal`; needs the `i18n` feature | compile error |
| M18 | Bad YAML | default file with a duplicate key | one error ``locale file … has errors (see `i18n!`)`` | compile error |
| M19 | Unicode word | `"çay"` with a file present, no literal | M5 error | compile error |
| K1 | Menu key | `key = "show_profile"` / `"a.b"` / `""` / `key = x` | ok / segment error / segment error / string-literal error | none / compile error ×3 |
| Y1 | `i18n!` layout | `cmd.ban.foo: x`; `cmd.a.b.c.d.description`; `cmdword.x: {y: z}`; `ctxcmd.k.label: x`; `cmd.ban.opt.u.min: x`; `choice: x` | each an error naming the key | compile error |
| Y2 | `i18n!` LOC-02 | tr `cmdword.ban: Yasakla`; `description: ""` | ``[LOC-02] `<dir>/tr.yaml`: `cmdword.ban`: [CMD-03] …`` | compile error |
| Y3 | `i18n!` LOC-03 | tr `cmd.ban.opt.reason.name: user` where any file has `cmd.ban.opt.user` | `[LOC-03] …` | compile error |
| Y4 | Valid template files | `examples/project-template/locales/*.yaml` | compile (`opt.name.name` is legal) | none |

</intent-contract>

## Code Map

- `crates/noctis-i18n/src/catalog.rs:152-205` `AppStrings`: add `get` and `owner`, using `owners` plus `Catalog::get`. Add unit tests next to Matrix 58-62.
- `crates/noctis-rules/src/command/`:
  - `name.rs:39,102`: `validate_name` and `validate_context_menu_name`, which `validate_localized` reuses;
  - `option.rs:517` (CMD-04 description check), `:682` (OPT-09), `:832` (`check_unique_names`, CMD-06): factor the single-text checks into helpers that `validate_localized` calls;
  - `option.rs:869` `measure_options`: reused unchanged;
  - `mod.rs:150-160` re-exports: add the new items.
- `crates/noctis-core/src/app.rs:2204-2235` `build()`:
  - move the `locale_sources`/`AppStrings::new` block before `Registry::fold`, keeping its violation push after the fold's;
  - pass `strings.as_ref()` to `Registry::fold`;
  - update the build docs' error order.
- `crates/noctis-core/src/registry.rs:146` `fold`: gains `strings: Option<&AppStrings>` and forwards it to `Commands::fold`.
- `crates/noctis-core/src/registry/commands.rs`:
  - `:62` `CommandNode`: add the variants (name, description, per option: name, description, per choice);
  - `:43` `MenuEntry` / menus: add name variants;
  - `:174` `Commands::fold`: build the variant table, then `lower`, LOC-02/03, `validate_merge`, `build`;
  - `:263` `lower`: give the decls the variants;
  - `:355` `add_fallbacks`: catalog description first;
  - `localize.rs` (variant-table build, called from `Commands::fold`): the declared-description consistency check (CMD-04, Design Notes "Declared description equals the catalog"); its violations join `validate_merge`'s `BuildViolation::Command` group; it looks up the emitted default-locale value through the same `Resolver` chain (`[default, default.sibling()]`) and covers handler-fallback descriptions;
  - `lower`: measure options with `fold`'s `limits` (the `Limits` passed to `Commands::fold`), never `Limits::default()`, as the baseline did;
  - `:425` `build_scope`.
  - Put the resolution and validation in a new `registry/commands/localize.rs`.
- `crates/noctis-core/src/publish/json.rs`: emit the maps (`node`, `option`, menus). Update the module doc ("localizations are never sent").
- `crates/noctis-core/src/error.rs:~295`: add `CommandLocalization` after `Command`. Its docs (and the LOC-02 docs cell) say a violation is reported once per key, file locale, rule and base rule (one key can break two base rules, e.g. CMD-02 and CMD-03).
- `crates/noctis-core/src/command.rs:520-620` (constructors, `description()`) and `kind.rs:111-123` `__SlashCommand`: replace `DESCRIPTION` with `DESCRIPTIONS`. Internally the handler holds one description for all paths or one per path. `crate::test_support::command(.., "Pong")` keeps working.
- `crates/noctis-core/src/menu.rs:140-240` `ContextMenuHandler`: add a `key` field, `__with_key` (hidden) and `key()`, and add it to `Debug`. A crate-private helper is named without the `__` prefix (`with_key_opt`, used by `kind.rs` for both menu kinds); `__` is reserved for hidden-public items.
- `crates/noctis-core/src/lib.rs:355-382` `AppProbe::commands()`: this is Publish's exact JSON, the integration tests' surface.
- `crates/noctis-macros/Cargo.toml`: add the `i18n = []` feature. `crates/noctis/Cargo.toml`: `i18n = ["noctis-macros/i18n"]`.
- `crates/noctis-macros/src/command.rs`:
  - `:27` `PLANNED` (drop `locales`), `:30` `EXPECTED`, `:41` `Args`, `:186-270` `parse_args`;
  - `:285` CMD-04 help, `:311-330` the decl description;
  - `:604-700` `derive`/`expand` (the `DESCRIPTION` const).
  - Put the pure resolution (`fn descriptions(paths, literal, entries: Option<&Entries>, file) -> Result<Vec<String>, Vec<syn::Error>>`) in a new `command/locale.rs` (or `i18n/command.rs`), so it can be unit-tested with in-memory entries. `LocaleArgs` has a single `impl` block.
- `crates/noctis-macros/src/options.rs:78,138,361,432-470,592-600`: optional `description`; resolution from the entries, per the Matrix.
- `crates/noctis-macros/src/menu.rs:55` `PLANNED` (drop `key`), `:57` `EXPECTED`, plus the `__with_key` emission.
- `crates/noctis-macros/src/i18n/`:
  - `load.rs` and `args.rs:13` (`__dir_from_source_file` pattern): reuse them for the single-file read;
  - `model.rs:12-13,277-375` `Walk`: add the reserved-layout, LOC-02 and LOC-03 checks after the walk, over `File::entries`; expose a `pub(crate)` single-file entry parse for `#[command]`;
  - `model.rs:988-1030`: the reserved-family tests to extend.
- `crates/noctis/tests/ui.rs:59-80` `ui_i18n`, which already globs `tests/ui-i18n/**/*.rs`. Fixture pattern: `tests/ui-i18n/loc/LOC-01_bare_language_stem.rs` (relative `../../fixtures/i18n/..` plus the hidden flag).
- `crates/noctis/tests/i18n_modules.rs`: pattern for the new `i18n_commands.rs` (`TestApp`, `i18n!` modules). `noctis::__private::core::__testing::AppProbe` is used by `tests/merge.rs:12`.
- `crates/noctis-core/src/publish/tests.rs:20-40` (`app`, `local_of` helpers) and `app/tests.rs:2930-2970` (`TestServer` dry-run pattern).
- `xtask/rules-pending.toml:16-34`: the four 11.4 entries. `xtask/src/rules_catalog/mod.rs:340-410`: fixtures must be `<area>/<ID>_<slug>.rs`, the `.stderr` must contain `[ID]`, and a unit test name must contain `loc_02` / `loc_03`.
- `docs/discord-rules.md:78,81,82,122,123,132`.
- `.github/workflows-disabled/ci.yml:65`: the `i18n,testkit` line.
- Template:
  - `examples/project-template/src/modules/{profile,setup,dashboard}/handlers.rs` (description literals, `key = "show_profile" // [planned]`);
  - `src/modules/setup/mod.rs:24` (`.base("setup", ..)`);
  - `locales/{en-US,tr}.yaml`, which already use the layout;
  - `AGENTS.md`.
- Skill `examples/project-template/.claude/skills/noctis/`:
  - `references/i18n.md:141,143-184`;
  - `references/commands.md:91,570,738,831`;
  - `references/app-and-sharding.md:365`;
  - `SKILL.md:23` (API hash).

## Tasks & Acceptance

**Execution:**
- [ ] `crates/noctis-rules/src/command/{name,option,mod}.rs`: add `LocalizedField`, `validate_localized` and `validate_localized_option_names`, with doctests and unit tests (Matrix 25-38 at rule level). They are the single validators shared by the macros and the App build.
- [ ] `crates/noctis-i18n/src/catalog.rs`: add `AppStrings::get` and `owner`, with tests. This is the merged lookup Publish needs.
- [ ] `crates/noctis-core/src/error.rs` (+ `lib.rs` docs): add `BuildViolation::CommandLocalization`, with a doctest of its Display. This is the typed error naming the Locale, key and rule.
- [ ] `crates/noctis-core/src/{command,kind,menu}.rs`: add `DESCRIPTIONS` / `descriptions()` and the menu `key`. These carry the macro output.
- [ ] `crates/noctis-core/src/registry/commands.rs` + new `registry/commands/localize.rs`, `registry.rs`, `app.rs`: the variant table, LOC-02/03, LOC-05 via the decls, the catalog description fallback, and the merge moved before the fold. This is the App-build half.
- [ ] `crates/noctis-core/src/registry/commands/localize.rs` + `registry/commands.rs`: the declared-description consistency check (Design Notes "Declared description equals the catalog"), reported as `BuildViolation::Command` CMD-04. `lower` measures options with `fold`'s `limits`. This closes review pass 1's root cause (en-US users seeing the file text while `description` is the declared one).
- [ ] `crates/noctis-core/src/publish/json.rs`: emit the maps. This is the Publish half.
- [ ] `crates/noctis-core/src/{registry/tests.rs,publish/tests.rs,app/tests.rs}`: add tests for Matrix 1-41 with `Catalog::__new` statics. Name each test after its scenario, with a `/// Story 11.4 Matrix N` doc comment. The names cover `loc_02_*`, `loc_03_*`, `cmd_05_*_localization` and `ctx_01_*`. Add one `TestServer` dry-run test asserting `TargetReport::json()` contains the maps (row 1/6). This covers the I/O matrix and the rules-catalog unit evidence. Also add, in `publish/tests/localize.rs` (or `registry/tests.rs`):
  - `cmd_04_declared_description_differs_from_catalog`: one case each for a `Module::base` description, a `Module::group` description, a file-less handler literal (non-Pattern), one path of a Pattern handler built with per-path descriptions, and a runtime option description vs `cmd.<P>.opt.<o>.description`; each gives exactly one `BuildViolation::Command` CMD-04 with the note/help below; the same node registered in the global and a guild scope gives one violation; an equal declaration (Matrix 21) and a key present only in a non-default, non-sibling locale give none; a failing catalog merge (`DuplicateLocaleKey`) gives none;
  - change `loc_dry_run_reports_localizations` so the handler literal equals its en-US `cmd.<P>.description` value;
  - `cmd_05_counts_localized_names_and_options`: with the 8000-at-the-limit fixture, CMD-05 fails once through a 32-character `tr` `cmdword.*` name only, and once through a localized option description or choice name on an options-bearing handler; a third case where an over-long, LOC-02-invalid value gives only the LOC-02 violation and no CMD-05;
  - LOC-03 on a subcommand group's children array, and on a child that exists only in a guild scope;
  - a Pattern handler's per-path `DESCRIPTIONS` reach the Registry and the Publish JSON as distinct descriptions per path;
  - `cmd_04_declared_description_differs_from_sibling_default`: en-US catalog default, en-US file without `cmd.setup.description`, en-GB file with it: (a) a differing `Module::base` description gives exactly one CMD-04 whose note names `(en-GB)`; (b) an equal one gives none; (c) no declaration (handler fallback) with a differing handler text gives exactly one CMD-04; (d) a key held only by a non-sibling locale (`tr`) gives none; plus an option case (`cmd.<P>.opt.<o>.description` only in en-GB, differing runtime option description gives one CMD-04);
  - a catalog whose `default_locale()` is `tr`: one `default_description` case (Matrix 20: `tr` `cmd.<P>.description` becomes `description`) and one `declared_descriptions` case (differing declaration gives one CMD-04 whose note names `(tr)`);
  - `cmd_04_declared_description_order`: two differing declarations (`setup`, `ban`; `ban` folded after `setup`), a `tr` `cmdword.ban: Yasakla` (one LOC-02 `CommandLocalization`), and a duplicate `ban` handler in a second Module (one `validate_merge` `Command`); assert the violation kinds in exactly this order: the `validate_merge` `Command`, CMD-04 at `ban`, CMD-04 at `setup`, then the `CommandLocalization`.
- [ ] `crates/noctis-core/src/registry/tests.rs`: in `every_parsed_type_has_a_slot`'s `check!` macro body, indent the added `None,` with 16 spaces like its neighbours (rustfmt skips macro bodies; fix by hand).
- [ ] `crates/noctis-macros/src/command/locale.rs` (or `command.rs` tests): a unit test for Matrix M16's positive case — `default_locale = "tr"` reads `tr.yaml` (in-memory entries or a fixture) and its `cmd.<P>.description` becomes the description.
- [ ] `crates/noctis-macros/{Cargo.toml,src/command.rs,src/options.rs,src/menu.rs,src/i18n/*}`, plus a new pure resolution module, and `crates/noctis/Cargo.toml`: the compile-time half (Matrix M1-M19, K1, Y1-Y4), with unit tests over in-memory entries and YAML. This is the macro half.
- [ ] `crates/noctis/tests/ui-i18n/`:
  - `loc/LOC-02_localized_word_uppercase.rs`, `loc/LOC-03_localized_option_name_collides.rs` (`i18n!` over `tests/fixtures/i18n/loc02`, `loc03`);
  - `cmd/CMD-04_description_missing_in_locale_file.rs`, `cmd/CMD-04_literal_differs_from_locale_file.rs`, `cmd/CMD-01_reserved_locale_word.rs` (path word `opt`) and `cmd/CMD-01_reserved_locale_word_name.rs` (path word `name`) (`#[command(.., locales = "../../fixtures/i18n/cmdkeys", __locales_from_source_file)]`);
  - each with a blessed `.stderr`. Every fixture YAML starts with a `#` comment naming Story 11.4.
  This is the rules-catalog diagnosed evidence.
- [ ] `crates/noctis/tests/i18n_commands.rs` (`#![cfg(all(feature = "i18n", feature = "testkit"))]`) + `tests/fixtures/i18n/{cmd,cmd_gb}/`:
  - a Module with `#[command("mod ban", locales = "tests/fixtures/i18n/cmd")]` without literals, a `Choices` option and `#[user_command("Show profile", key = "show_profile")]`, plus `.locales(catalog())`;
  - assert `AppProbe::of(&app).commands()` (rows 1-2, 6, 11-12, 15, 20);
  - an en-GB fixture variant (row 7);
  - a LOC-03-at-build `Err` (row 34);
  - `TestApp` dispatch by default name, while the localized name is refused (row 42);
  - a keyed `#[message_command(.., key = ..)]` with a `ctxcmd.<key>.name` fixture entry, asserting its `name_localizations` in `AppProbe::commands()` (both menu kinds exercised end to end).
  This is the outermost surface.
- [ ] `.github/workflows-disabled/ci.yml:65`: add `--test i18n_commands`. Keeps the gate in sync.
- [ ] `docs/discord-rules.md`, `xtask/rules-pending.toml`: update the rule cells and drop the 4 pending entries. The CMD-04 cell also states: a `Module::base`/`Module::group` description, a file-less or runtime handler description, or a runtime option description that differs from the value sent for its owner catalog's default locale (that locale's file, else its sibling's) for `cmd.<path>.description` / `cmd.<path>.opt.<o>.description` fails `App::build` (CMD-04, `declared by` note). Keeps the catalog in sync.
- [ ] Template:
  - drop the `description` literals from the template's `#[command]`/`#[option]`, and the `.base("setup", ..)` description (the catalog supplies it);
  - remove `// [planned]` from `key`;
  - `AGENTS.md`: the files are the source of command texts.
  This reconciles the template.
- [ ] Skill:
  - `i18n.md` §3-§4 and `commands.md` (`Module::base`/`Module::group` text): a declared description must equal the value sent for the default locale (its own file's, else its sibling file's) or be omitted (App-build CMD-04 otherwise); a key only in the sibling locale is compared, other non-default locales are not;
  - `i18n.md` error shape: one `CommandLocalization` per key, file locale, rule and base rule (one key can break two base rules);
  - `i18n.md` checks list: `[LOC-03]` at `cargo check` (`i18n!`) covers only `cmd.<path>.opt` maps against the option segments in the files; subcommand/group children arrays and options without keys are checked at `App::build` only. Do not list LOC-03 as "both at `cargo check` and `App::build`" without that qualification;
  - `i18n.md` §3-§4: exact key layout and resolution chain, menus, `locales`/`default_locale`, Matrix M-rules, `CommandLocalization` example, unkeyable words/values, dispatch by default names;
  - `commands.md`: drop the "[planned]" lines at 91/570/738/831;
  - `app-and-sharding.md:365`: localizations are sent;
  - run `cargo run --release -p xtask -- api-snapshot --bless`, then set `SKILL.md`'s API hash.
  This passes the AC for the skill and API gates.
- [ ] `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: the planning decisions are already appended (three `Story 11.4 planning` lines); append any implementation decision as a `(decision)` line. Traceability.

**Acceptance Criteria:**
- Given a built App whose merged catalog has `cmdword.*`, `cmd.*` and `ctxcmd.*` keys, when Publish builds JSON (`AppProbe::commands()` and a `dry_run` report), then commands, options, choices and keyed menus carry `name_localizations`/`description_localizations`, a full-path key beats the per-word one, and menus localize as whole names.
- Given only an `en-US` file, when Publish runs, then `en-US` values appear for `en-US` and `en-GB`; with an `en-GB` file, `en-GB` uses it.
- Given the `i18n` feature and a default-locale file, when `#[command]` compiles, then the description literal may be omitted, a differing literal or a missing key without a literal fails with `[CMD-04]`, and a word `name`/`description`/`opt` fails with `[CMD-01]` (trybuild under `tests/ui-i18n/`); without the feature or the file, Story 6.1's behavior is unchanged (existing `tests/ui/cmd` fixtures pass unchanged).
- Given a localized value breaking CMD-01..04, OPT-09 or CTX-01, or a colliding localized option name, when `App::build` runs, then it returns `BuildViolation::CommandLocalization` naming the rule (LOC-02/LOC-03), Locale, key and catalog; and the CMD-05 budget counts the longest variant.
- Given a localized command, when an interaction arrives, then routing uses default names only.
- Given the finished story, when the gates run, then `rules-catalog` passes with the four 11.4 pending entries removed, the API snapshot matches, the skill's API hash equals it, and `skill-check` passes.

## Spec Change Log

### 2026-10-23 — Review pass 1 loopback (bad_spec)
- Trigger: review pass 1 (blind #1, edge #1, medium): a declared description (`Module::base`/`Module::group`, a runtime or file-less handler literal) differing from the owner catalog's default-locale `cmd.<path>.description` was accepted silently; Publish sent `description = "X"` next to `description_localizations["en-US"] = <file text>` (`loc_dry_run_reports_localizations` locked in "Pong" vs "Pong!").
- Amended (outside `<intent-contract>`): Design Notes (new "Declared description equals the catalog" rule covering node, handler-per-path and option descriptions, with lookup, exact comparison, report shape, per-key dedup across scopes, placement; dedup identity refined to include the base rule), Code Map (`localize.rs` check, `lower` uses `fold`'s `limits`, `CommandLocalization` doc wording), Tasks (the check; tests `cmd_04_declared_description_differs_from_catalog`, `cmd_05_counts_localized_names_and_options`, LOC-03 on group children and guild-only children, Pattern per-path descriptions end to end; `loc_dry_run_reports_localizations` literal equals en-US; `CMD-01_reserved_locale_word_name.rs` fixture; CMD-04 docs cell; skill text), Verification (explicit trybuild and featureless `noctis-macros` runs).
- Folded-in patches: `lower` used `Limits::default()` instead of `fold`'s limits; `CommandLocalization` docs said "once per (key, locale, rule)"; missing CMD-05 localized-name/option tests; missing LOC-03 group/guild-only and Pattern per-path tests; missing `name` reserved-word fixture; trybuild/macro tests not run when `cargo hack` was skipped.
- Known-bad state avoided: mismatched `description` vs default-locale `description_localizations` with no error; options measured with default limits; doc claiming a dedup the code does not do.
- KEEP: rebuild from the code of commit `fd16a95` (Story 11.4: implementation (review pass 1)), the latest commit, changing only what the amendments require. Keep the whole localization pipeline (`Resolver` per-locale `[L, L.sibling()]` chain with full path before `cmdword`, `is_segment`/`key_path`/`choice_segment`; `Variants`/`NodeTexts`/`OptionTexts`; `Problems` with LOC-02 per value, LOC-03 via `validate_localized_option_names`, dedup and sort order; `Table` `new`/`check`/`budget`/`decls`, `measure_localized` over loop-local copies, no `Box::leak`); the `fold` ordering (catalogs merged before the Registry fold; Command, then CommandLocalization, then DuplicateLocaleKey; localization skipped when the merge fails); `add_fallbacks` taking the owner default-locale `cmd.<path>.description` with the catalog origin and no warning before the handler fallback; the Publish JSON emission (maps right after `name`/`description`, omitted when empty, byte-identical without catalogs, kept in the guild mirror); `validate_localized`/`validate_localized_option_names`/`LocalizedField`, `AppStrings::{get, owner}`, `BuildViolation::CommandLocalization` with Display and doctest, `ContextMenuHandler::key()`, `CommandHandler::descriptions()`/`__with_descriptions`; the macro half (`command/locale.rs` `load`/`descriptions`/`reserved_words`/`conflict` with M1-M19 unit tests, `locales`/`default_locale`/`__locales_from_source_file`, `include_bytes!`, menu `key`, `i18n!` reserved-layout/LOC-02/LOC-03 checks and fixtures); the tests in `publish/tests/localize.rs` (Matrix 1-41), `crates/noctis/tests/i18n_commands.rs` and fixtures, the trybuild fixtures, the template/skill/docs/rules-pending updates, and the API snapshot plus hash workflow.

### 2026-10-24 — Review pass 2 loopback (bad_spec)
- Trigger: review pass 2 (blind #1, medium): the resolution chain for the default locale is `[en-US, en-GB]` and `i18n!` lets a non-default file hold reserved keys the default file lacks, so an en-GB-only `cmd.<P>.description` (or option description) is emitted as `description_localizations["en-US"]`; the pass 1 rule looked only at the owner's own default-locale file, so a differing declared description passed App-build and an undescribed base/group took the handler fallback while en-US users saw the en-GB text (the pass 1 mismatch, reached through the sibling).
- Amended (outside `<intent-contract>`), amendment A2: Design Notes "Declared description equals the catalog" (lookup = the emitted default-locale value via the `Resolver` chain `[default, default.sibling()]`; covers the handler-fallback description; note names the file locale holding the value; non-sibling non-default locales never compared; `add_fallbacks` unchanged; skill wording replacing "a key only in a non-default locale is not compared"), Code Map (`localize.rs` lookup, `with_key_opt` naming, single `impl LocaleArgs`), Tasks (tests `cmd_04_declared_description_differs_from_sibling_default`, `tr`-default `default_description`/`declared_descriptions`, `cmd_04_declared_description_order`, Matrix M16 positive, keyed message menu end to end, `None,` indentation; skill error-shape and LOC-03 check-scope text).
- Folded-in patches: skill `i18n.md` dedup wording (add base rule) and LOC-03 compile-time scope; Matrix M16 positive test; non-en-US catalog default tests; `registry/tests.rs` indentation; merged `impl LocaleArgs`; `__with_key_opt` renamed `with_key_opt`; pinned declared-description CMD-04 order; keyed `#[message_command]` end-to-end test.
- Known-bad state avoided: a declared or handler-fallback `description` differing from the sibling-supplied default-locale localization with no error; skill text claiming a non-default-locale key is never compared; LOC-03 advertised as fully checked at `cargo check`; hidden-public `__` prefix on a crate-private helper.
- KEEP: rebuild from the code of commit `4c5a7b7` (Story 11.4: implementation (review pass 2)), the latest commit, changing only what A2 and the folded-in patches require. Keep everything in pass 1's KEEP (localization pipeline: `Resolver` chain and key helpers, `Variants`/`NodeTexts`/`OptionTexts`, `Problems` dedup per (key, file locale, rule, base rule) and sort order, `Table`, `measure_localized` with `fold`'s `limits`, no `Box::leak`; the `fold` ordering; `add_fallbacks`; the Publish JSON emission; the rules/i18n/core public API; the macro half and its fixtures; template/skill/docs/rules-pending; API snapshot + hash workflow), plus pass 2's additions: `declared_descriptions` with its report shape, dedup across scopes and key-byte sort, placed after `validate_merge`'s `Command`s; the `cmd_04_declared_description_differs_from_catalog*`, `cmd_05_counts_localized_names_and_options`, LOC-03 group/guild-only and Pattern per-path tests; the `CMD-01_reserved_locale_word_name.rs` fixture; the CMD-04 docs cell. Only `declared_descriptions`' lookup (own file -> emitted default-locale value) and its coverage of the handler-fallback case change.

## Review Triage Log

### 2026-10-23 — Review pass
- verdicts: 38 findings — high 0, medium 6, low 12, false 17, maybe-false 3
- findings:
  - `[medium]` `[bad_spec]` (blind) No App-build check that a declared description (`Module::base`/`Module::group`, a runtime or file-less handler literal) equals the owner catalog's default-locale `cmd.<path>.description` — verified: `add_fallbacks` skips described nodes and `Resolver::node` still emits the file text as the default locale's `description_localizations`, so en-US users see the file text while `description` is the declared one (`loc_dry_run_reports_localizations` locks in "Pong" vs "Pong!"). Amendment A1 (proposed in `target/story-loop/patches-11-4-p1.md`): a differing declared description is a CMD-04 `BuildViolation::Command` at App-build; Matrix 21 holds only when equal.
  - `[maybe-false]` `[defer]` (blind) LOC-03 does not check top-level bases or same-kind menus against each other's localized names — `children_arrays` only builds arrays under a parent; Discord documents the distinctness constraint only for option names of one command, so whether the PUT fails is unverified; deferred.
  - `[low]` `[reject]` (blind) An over-long catalog default description is reported both as CMD-04 (`validate_merge`) and LOC-02 — real, but `i18n!` already rejects that value at compile time (LOC-02 per file), so App-build rarely meets it; the fix needs a new skip branch.
  - `[low]` `[reject]` (blind) `#[command]` emits no rebuild dependency when the implicit default-locale file is missing — real only until the file is created once (then `include_bytes!` tracks it); the fix needs tracked paths or a build script.
  - `[low]` `[reject]` (blind) `#[command]` re-parses the default-locale YAML per invocation — compile-time cost only; the spec mandates the one file-error message; a process-wide cache adds complexity.
  - `[false]` `[reject]` (blind) Nothing ties `#[command]`'s `locales`/`default_locale` to `i18n!` — the spec settles mirrored defaults (a proc macro cannot see the `i18n!` call) and `commands.md:113` / `i18n.md:198-200` tell authors to mirror non-default settings.
  - `[false]` `[reject]` (blind) `__with_descriptions` silently ignores a non-parallel slice — the only caller is the derive, which emits `DESCRIPTIONS` parallel to `PATHS`; no reachable mismatch.
  - `[false]` `[reject]` (blind) `Problems::finish` silently drops problems — every problem comes from an `AppStrings::get` hit, so `owner` exists and `Catalog::keys()` (every key of every file) contains the key.
  - `[low]` `[reject]` (blind) Quadratic dedup and per-scope re-resolution of option texts — compile-of-App cost only for realistic sizes; the fix restructures the table.
  - `[false]` `[reject]` (blind) `validate_localized_option_names` rejects swaps — Discord's documented rule is "distinct from all other default option names of that command", which is exactly the spec's rule.
  - `[low]` `[patch]` (blind) `lower` measures options with `Limits::default()` instead of `fold`'s `limits` — the value is overwritten in `Table::decls` today; restore the `limits` parameter.
  - `[low]` `[reject]` (blind) `CommandHandler` allocates description storage twice — negligible one-time allocation; a `Cow` adds surface complexity.
  - `[false]` `[reject]` (blind) `description()` only returns the first path's description — the spec defines it so, and no non-test caller exists (grep).
  - `[false]` `[reject]` (blind) Default-locale and sibling entries are emitted — the AC ("`en-US` values are emitted for both `en-US` and `en-GB`") and the spec's Design Notes settle it.
  - `[medium]` `[defer]` (blind) `gates.sh` now skips `cargo hack` except for an epic's last story while AGENTS.md still requires it per story — real tooling/doc mismatch from the loop, not this story's code; the fix edits AGENTS.md.
  - `[low]` `[patch]` (blind) Test gaps: LOC-03 on a group's children and across scopes, Pattern per-path descriptions reaching the Registry/Publish JSON — real; add the tests.
  - `[medium]` `[bad_spec]` (edge) Declared description differing from catalog text gives mismatched `description` and en-US localization — same root cause as the first finding; amendment A1.
  - `[maybe-false]` `[defer]` (edge) Two bases localized to one name in a locale are not detected — same root cause as the LOC-03 top-level finding; unverified against Discord; deferred.
  - `[maybe-false]` `[defer]` (edge) Two same-kind keyed menus sharing a localized name are not detected — same group; deferred.
  - `[false]` `[reject]` (edge) `default_locale` default mismatch with a non-en-US `i18n!` — settled and documented (see the `locales`/`default_locale` row above).
  - `[low]` `[reject]` (edge) Stale expansion when `locales/en-US.yaml` is created later — same as the rebuild-dependency row; rejected.
  - `[low]` `[reject]` (edge) A non-UTF-8 locale path drops the `include_bytes!` — real but practically unreachable for crate paths; rejected.
  - `[false]` `[reject]` (edge) A non-segment word like `çay` takes its description from the file — `i18n::file_entries` rejects any non-`[A-Za-z0-9_-]` key segment (`model.rs:300`), so `cmd.çay.description` cannot exist in a readable file.
  - `[medium]` `[defer]` (edge) `gates.sh` hack skip misfires when two lanes run an epic's last two stories — the Python counts a sibling in `review` as unfinished; loop tooling, not this story.
  - `[low]` `[patch]` (edge) `CommandLocalization` doc says "once per (key, locale, rule)" but one key can yield two LOC-02 violations with different base rules — correct the doc to "(key, file locale, rule, base rule)".
  - `[medium]` `[patch]` (vgap) LOC-05 wiring for localized names, option texts and choice names, and the `valid_values` filter, have no App-build test — pre-verified; add `cmd_05_counts_localized_names_and_options`.
  - `[medium]` `[patch]` (vgap) The new trybuild fixtures and feature-off macro branch run in no gate this story (hack skipped) — pre-verified; run `ui` with and without `i18n` and the featureless `noctis-macros` tests once.
  - `[low]` `[patch]` (vgap) `lower` ignores `fold`'s `limits` — same root cause as the blind `Limits::default()` row.
  - `[false]` `[reject]` (intent D1) `dir`/`default` coupling read as mirrored defaults — the spec settles it (Design Notes) and the skill documents it.
  - `[false]` `[reject]` (intent D2) Full-path precedence is per chain locale — the spec settles it (Matrix 5, memlog); the AC's override holds within each locale.
  - `[low]` `[patch]` (intent D3) Reserved words `name`/`description` and the implicit-default missing-key case have no trybuild fixture — add a fixture for `name`.
  - `[false]` `[reject]` (intent D4) One option description across expansions — settled ("No per-path option arrays", Matrix M14).
  - `[false]` `[reject]` (intent D5) Template drops description literals — settled in the spec's template task; the AC allows omission.
  - `[false]` `[reject]` (intent D6) Maintainer decision §9 tension — a literal is never read as a key; the feature only adds a consistency check, as AD-16 and the epic AC prescribe.
  - `[false]` `[reject]` (intent D7) `key` on both menu kinds, unkeyed menus unlocalized — the AC lists both macros; the spec settles it (Matrix 16).
  - `[false]` `[reject]` (intent D8) Float/non-segment choices unlocalized — settled (Matrix 14); such values cannot be key segments.
  - `[false]` `[reject]` (intent D9) Compile-time `i18n!` LOC checks in addition to App-build — settled by the spec and maintainer decision §4 (compile time first).
  - `[false]` `[reject]` (intent D10) Default-locale entry in the maps — settled by the AC and Design Notes.

### 2026-10-23 — Review pass
- verdicts: 28 findings — high 0, medium 1, low 15, false 11, maybe-false 1
- findings:
  - `[medium]` `[bad_spec]` (blind) [amendment A2: Design Notes lookup now the emitted default-locale value via `[default, sibling]`, handler-fallback coverage, note names the file locale, skill wording, tests `cmd_04_declared_description_differs_from_sibling_default` and the `tr`-default cases] The default locale can get another text than `description` through its sibling — verified: `Resolver::resolve` walks `[en-US, en-GB]` for en-US, and `i18n!`'s `translation_checks` exempts reserved keys from "not in the default locale", so an en-GB-only `cmd.<P>.description` is emitted as `description_localizations["en-US"]`; `declared_descriptions`/`default_text` only read the owner's own default-locale file (as Design Notes prescribe: "no sibling"), so a differing declared or handler-fallback description passes App-build. Amendment A2 (proposed in `target/story-loop/patches-11-4-p2.md`): compare the final description with the value actually emitted for the owner's default locale.
  - `[low]` `[patch]` (blind) Skill `i18n.md` says one `CommandLocalization` "per key, file locale and rule", missing the base rule — real (code, `error.rs`, discord-rules LOC-02 say base rule); folded into the A2 loopback as a doc fix.
  - `[low]` `[patch]` (blind) Skill lists `[LOC-03]` under "both at `cargo check` and `App::build`" — real: `i18n!` checks only `cmd.<P>.opt` maps; children arrays are App-build only; folded in as a doc fix.
  - `[low]` `[reject]` (blind) Compile-time LOC-03 defaults come from file keys, so a stale `cmd.ban.opt.old.*` key makes `name: old` an error — real but the intent contract prescribes the union-of-file-segments defaults; rare, loud and fixed by deleting the stale key.
  - `[low]` `[reject]` (blind) The macro's "not found as `cmd.çay.description`" note names an unwritable key — real (Matrix M19 prescribes the M5 error); non-segment default words with a locale file are rare and the fix adds a diagnostic branch.
  - `[low]` `[reject]` (blind) `load::root` errors carry the `i18n!:` prefix when `#[command]` hits them — real but practically unreachable (`CARGO_MANIFEST_DIR` is always set by cargo; the source-file flag is hidden, test-only).
  - `[low]` `[reject]` (blind) Key-layout helpers duplicated between `noctis-macros` and `noctis-core` — no current disagreement beyond the rejected M19 note; the fix is a cross-crate refactor the Code Map did not ask for.
  - `[false]` `[reject]` (blind) Cross-Module `cmdword.*`/shared-base key collisions undocumented — skill §5 already states a key in two catalogs, `cmdword.*` included, is one `DuplicateLocaleKey`; the contract forbids an exemption.
  - `[false]` `[reject]` (blind) carried: `Problems::finish` silently drops problems — the code still reads as pass 1's row describes (every problem comes from a hit, so `owner` and `keys()` find the key); the scan/allocation cost is compile-of-App cost only.
  - `[low]` `[patch]` (blind) No test for `default_locale = "tr"` reading `tr.yaml` (Matrix M16 positive) or for a non-en-US catalog default in `default_description`/`declared_descriptions` — real; folded in: add both tests.
  - `[low]` `[reject]` (blind) `EXPECTED` lists `locales`/`default_locale` without the feature — those arguments then fail with a clear "needs the `i18n` feature" message; the fix adds a cfg branch for a cosmetic list.
  - `[false]` `[reject]` (blind) "declared by (unnamed)" vs backticked — `noctis_rules::command::merge::source_name` uses exactly this shape (`merge.rs:541` asserts `declared by (unnamed)`), so it matches the existing notes.
  - `[low]` `[patch]` (blind) Mis-indented `None,` in `registry/tests.rs` `every_parsed_type_has_a_slot` (12 vs 16 spaces) — real; folded in: re-indent.
  - `[low]` `[patch]` (blind) Two `impl LocaleArgs` blocks; `pub(crate)` `__with_key_opt` uses the hidden-public prefix — real; folded in: merge the blocks, rename to `with_key_opt`.
  - `[low]` `[reject]` (edge) carried: no rebuild dependency when the default-locale file is missing — same as pass 1's row.
  - `[false]` `[reject]` (edge) carried: `i18n!` `default = "tr"` vs `#[command]` default `en-US` — settled mirrored defaults, as pass 1's row.
  - `[false]` `[reject]` (edge) Default-locale `cmd.<P>.name`/`cmdword`/`ctxcmd` differing from the default name is unchecked — Design Notes deliberately exclude names: a default-locale name localization is a deliberate display text.
  - `[low]` `[reject]` (edge) carried: non-UTF-8 locale path drops `include_bytes!` — same as pass 1's row.
  - `[low]` `[reject]` (edge) carried: over-long catalog default description reported as CMD-04 and LOC-02 — same as pass 1's row.
  - `[low]` `[patch]` (vgap) Placement and key order of declared-description CMD-04 violations unpinned — pre-verified; folded in: add `cmd_04_declared_description_order` (merge `Command`, then declared CMD-04 in key-byte order, then `CommandLocalization`).
  - `[false]` `[reject]` (intent 1) LOC-02/LOC-05 at App-build tested only in core unit tests — the spec's Tasks place them there, and compile-time-first is maintainer decision §4.
  - `[false]` `[reject]` (intent 2) carried: maintainer decision §9 tension — as pass 1's D6 row.
  - `[false]` `[reject]` (intent 3) carried: mirrored `dir`/`default` — as pass 1's D1 row.
  - `[false]` `[reject]` (intent 4) carried: full-path precedence per chain locale — as pass 1's D2 row.
  - `[false]` `[reject]` (intent 5) App-build CMD-04 for declared descriptions beyond the AC — deliberately added by the pass 1 amendment (Design Notes, epic AC 2, FR-57).
  - `[low]` `[patch]` (intent 6) `#[message_command(.., key)]` not exercised end to end — real gap (both kinds call `__with_key_opt` in `kind.rs:357,375`); folded in: add a keyed message menu to `crates/noctis/tests/i18n_commands.rs`.
  - `[maybe-false]` `[defer]` (intent 7) carried: LOC-03 not checking top-level bases or same-kind menus — same as pass 1's deferred row; not deferred again.
  - `[false]` `[reject]` (intent 8) carried: default-locale entries emitted — as pass 1's D10 row.

## Design Notes

These decisions are logged in the architecture memlog.

- **Per-key sibling chain, own locale first.** The AC ("a sibling's values for a code without its own file") is satisfied, and an en-GB file that lacks a key still serves en-US's text, which is what the runtime `Catalog` chain does. The default locale is not part of the chain, because Discord already falls back to the default.
- **The default locale's entries are emitted too** (AC: "`en-US` values are emitted for both"). Values are verbatim, even when they equal the default.
- **Typed error shape.** `rule` is LOC-02/LOC-03 and `violation` is the base rule. LOC-03 maps to CMD-06, because a duplicate option name is CMD-06's rule. One violation per `(key, file locale, rule, base rule)` keeps sibling and shared-word reports from multiplying; the base rule is part of the identity because one value can break two base rules (e.g. an empty name breaks CMD-02, a 33-char uppercase one CMD-02 and CMD-03), and each is reported.
- **Declared description equals the catalog (CMD-04 at App-build).** Matrix 21 ("declared wins") holds when the declared text equals the file's; Publish emits the default-locale value verbatim, so a differing pair would show en-US (and en-GB via the sibling) users the file text while `description` is the declared one. The rule, per epic AC 2 and FR-57 and ordered by maintainer decision §4 (compile time first, App-build for merged Modules):
  - Covered fields: every node description at path `P` that is declared (a `Module::base` / `Module::group` description, or a handler's description for path `P`, per path for Pattern handlers; a synthetic decl from the catalog fallback (Matrix 20) is equal by construction), compared with `cmd.<P>.description`; also every undescribed base/group node whose final `description` is the handler fallback (Matrix 22 path, which `add_fallbacks` takes only when the owner's own default-locale file lacks the key): it is compared too, because the emitted default-locale value can still exist through the sibling file (e.g. en-GB) and would then differ from `description`. The check runs on the final description of every node, whatever its source (declared, catalog fallback, handler fallback); and every option description on a registered leaf `P`, compared with `cmd.<P>.opt.<o>.description`. Names, per-word `cmdword.*`, choice names and menu names are not covered: their default is the word/value/literal itself and a default-locale localization is a deliberate display text.
  - Lookup (amended in review pass 2): compare against the value Publish actually emits as `description_localizations[<owner.default_locale()>]` for that key, i.e. `AppStrings::owner(key)`, then the first hit walking `[owner.default_locale(), owner.default_locale().sibling()]` with `AppStrings::get` — exactly what the localization `Resolver` yields for that locale (reuse the `Resolver`, do not re-implement the chain). The owner's own default-locale file is not the only source: `i18n!` exempts reserved keys from "missing in the default locale", so an en-GB-only `cmd.<P>.description` (or `cmd.<P>.opt.<o>.description`) is emitted as the en-US localization and must be compared. No owner, or no emitted value for the default locale, means no check. A key held only by a locale that is neither the default nor its sibling (e.g. `tr` under an en-US default) is never compared. Comparison is exact (bytes). The same rule applies to the owner catalog whatever its `default_locale()` is (e.g. `tr`, whose sibling is none).
  - `add_fallbacks` is unchanged: it takes the owner's own default-locale file value only (LOC-06 row 20), never the sibling's; a sibling-only value therefore leads to the handler fallback, which this check then reports when it differs.
  - Report: one `BuildViolation::Command` with rule CMD-04, path `P` (option: `P` and the option name, as existing option CMD-04 violations carry it), note ``description differs from `<key>` in catalog `<origin>` (<file locale>)``, where `<file locale>` is the locale whose file holds the emitted value (the default locale, or its sibling such as `en-GB` when the value comes from the sibling file; never a sibling-derived code), plus the file's text (`the file has …`) and the `declared by` Module (as the existing CMD-04 App-build notes), help ``make them equal or remove the declaration``. Once per key across all scopes (global and guild), first occurrence in fold order.
  - Placement: computed in `localize.rs` while building the variant table, appended after `validate_merge`'s own `Command` violations, sorted by key bytes, before every `CommandLocalization`. Skipped, like all localization, when the catalog merge fails. Macro-built handlers whose default-locale file holds the key never trigger it (the macro already enforces equality, M3/M4); it catches runtime, file-less or `Module`-level declarations, the handler fallback, and any value that reaches the default locale only through the sibling file.
  - Skill wording: replace "A key only in a non-default locale is not compared" (skill `i18n.md` §4) with "A key only in a non-default locale is compared when that locale is the default locale's sibling (it is then sent as the default locale's localization); other locales are never compared."
- **`default_locale` argument.** A proc macro cannot see another `i18n!` call, so `#[command]` mirrors `i18n!`'s defaults (`locales`, `en-US`). A Module crate with other settings states them. `locales` is the AC's name; `default_locale` is the minimal addition for a non-en-US default.
- **Reserved words are CMD-01.** Every trybuild fixture needs a catalog rule id with a Compile-time tier. The restriction is a word-charset constraint, documented in the CMD-01 cell.
- **Per-path descriptions.** Pattern expansions have distinct `cmd.<path>.description` keys, so `DESCRIPTIONS` parallels `PATHS`. Options stay one array per struct, so their default description must agree across expansions. Non-default option texts are still per path at Publish.
- **Maintainer decision §9 does not apply here.** §9 decides runtime-rendered text by syntax. Command metadata is fixed at compile time and published, and docs CMD-04 / AD-16 define the literal-vs-file rule.
- **Borrowing.** Build an owned table (`Vec<&'static str>` per field) first, then lower the decls borrowing it (`CommandDecl<'t>`), then move the table into `Commands`. Option and choice copies for `measure_options` are loop-local.
- **multiple-goals.** The compile-time half (macros) and the App-build/Publish half are both required by the story's ACs. Keep them in one change.

## Verification

**Commands:**
- `cargo fmt --all`: expected no diff.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: expected clean.
- `cargo test --release --workspace --all-features`: expected all pass, including `i18n_commands`, `i18n_modules`, `i18n_app` and `i18n_locales`.
- `cargo test --release -p noctis --features i18n --test ui`: expected pass (the `ui_i18n` suite). Bless new `.stderr` files with `TRYBUILD=overwrite`, then rerun without it.
- `cargo test --release -p noctis --test ui`: expected pass (the featureless suite, unchanged).
- `cargo hack test --release --workspace --each-feature`: expected pass.
- Even when a gate run skips `cargo hack`, run explicitly: `cargo test --release -p noctis --features i18n --test ui`, `cargo test --release -p noctis --test ui` and `cargo test --release -p noctis-macros` (no features); fix any `.stderr` drift.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`: expected ok.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`: expected a match. Install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if they are missing.
- `cargo deny check`: expected ok.
- `cargo +1.91 check --release --workspace`: expected ok.
- Optional, tracking only: `cargo check --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template`. CI marks the template build `continue-on-error` until Story 14.2, so errors from API that has not landed are acceptable. An error from a template command's description, `key` or locale files is not. Delete `target/template` afterwards if disk is tight.
- `test ! -d target/debug`: expected no debug profile.
