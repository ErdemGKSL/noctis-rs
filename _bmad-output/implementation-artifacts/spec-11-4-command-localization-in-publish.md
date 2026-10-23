---
title: 'Story 11.4: Command localization in Publish'
type: 'feature'
created: '2026-10-23'
status: 'ready-for-dev'
review_loop_iteration: 0
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
deferred: []
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
  - `:425` `build_scope`.
  - Put the resolution and validation in a new `registry/commands/localize.rs`.
- `crates/noctis-core/src/publish/json.rs`: emit the maps (`node`, `option`, menus). Update the module doc ("localizations are never sent").
- `crates/noctis-core/src/error.rs:~295`: add `CommandLocalization` after `Command`.
- `crates/noctis-core/src/command.rs:520-620` (constructors, `description()`) and `kind.rs:111-123` `__SlashCommand`: replace `DESCRIPTION` with `DESCRIPTIONS`. Internally the handler holds one description for all paths or one per path. `crate::test_support::command(.., "Pong")` keeps working.
- `crates/noctis-core/src/menu.rs:140-240` `ContextMenuHandler`: add a `key` field, `__with_key` (hidden) and `key()`, and add it to `Debug`.
- `crates/noctis-core/src/lib.rs:355-382` `AppProbe::commands()`: this is Publish's exact JSON, the integration tests' surface.
- `crates/noctis-macros/Cargo.toml`: add the `i18n = []` feature. `crates/noctis/Cargo.toml`: `i18n = ["noctis-macros/i18n"]`.
- `crates/noctis-macros/src/command.rs`:
  - `:27` `PLANNED` (drop `locales`), `:30` `EXPECTED`, `:41` `Args`, `:186-270` `parse_args`;
  - `:285` CMD-04 help, `:311-330` the decl description;
  - `:604-700` `derive`/`expand` (the `DESCRIPTION` const).
  - Put the pure resolution (`fn descriptions(paths, literal, entries: Option<&Entries>, file) -> Result<Vec<String>, Vec<syn::Error>>`) in a new `command/locale.rs` (or `i18n/command.rs`), so it can be unit-tested with in-memory entries.
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
- [ ] `crates/noctis-core/src/publish/json.rs`: emit the maps. This is the Publish half.
- [ ] `crates/noctis-core/src/{registry/tests.rs,publish/tests.rs,app/tests.rs}`: add tests for Matrix 1-41 with `Catalog::__new` statics. Name each test after its scenario, with a `/// Story 11.4 Matrix N` doc comment. The names cover `loc_02_*`, `loc_03_*`, `cmd_05_*_localization` and `ctx_01_*`. Add one `TestServer` dry-run test asserting `TargetReport::json()` contains the maps (row 1/6). This covers the I/O matrix and the rules-catalog unit evidence.
- [ ] `crates/noctis-macros/{Cargo.toml,src/command.rs,src/options.rs,src/menu.rs,src/i18n/*}`, plus a new pure resolution module, and `crates/noctis/Cargo.toml`: the compile-time half (Matrix M1-M19, K1, Y1-Y4), with unit tests over in-memory entries and YAML. This is the macro half.
- [ ] `crates/noctis/tests/ui-i18n/`:
  - `loc/LOC-02_localized_word_uppercase.rs`, `loc/LOC-03_localized_option_name_collides.rs` (`i18n!` over `tests/fixtures/i18n/loc02`, `loc03`);
  - `cmd/CMD-04_description_missing_in_locale_file.rs`, `cmd/CMD-04_literal_differs_from_locale_file.rs`, `cmd/CMD-01_reserved_locale_word.rs` (`#[command(.., locales = "../../fixtures/i18n/cmdkeys", __locales_from_source_file)]`);
  - each with a blessed `.stderr`. Every fixture YAML starts with a `#` comment naming Story 11.4.
  This is the rules-catalog diagnosed evidence.
- [ ] `crates/noctis/tests/i18n_commands.rs` (`#![cfg(all(feature = "i18n", feature = "testkit"))]`) + `tests/fixtures/i18n/{cmd,cmd_gb}/`:
  - a Module with `#[command("mod ban", locales = "tests/fixtures/i18n/cmd")]` without literals, a `Choices` option and `#[user_command("Show profile", key = "show_profile")]`, plus `.locales(catalog())`;
  - assert `AppProbe::of(&app).commands()` (rows 1-2, 6, 11-12, 15, 20);
  - an en-GB fixture variant (row 7);
  - a LOC-03-at-build `Err` (row 34);
  - `TestApp` dispatch by default name, while the localized name is refused (row 42).
  This is the outermost surface.
- [ ] `.github/workflows-disabled/ci.yml:65`: add `--test i18n_commands`. Keeps the gate in sync.
- [ ] `docs/discord-rules.md`, `xtask/rules-pending.toml`: update the rule cells and drop the 4 pending entries. Keeps the catalog in sync.
- [ ] Template:
  - drop the `description` literals from the template's `#[command]`/`#[option]`, and the `.base("setup", ..)` description (the catalog supplies it);
  - remove `// [planned]` from `key`;
  - `AGENTS.md`: the files are the source of command texts.
  This reconciles the template.
- [ ] Skill:
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

## Review Triage Log

## Design Notes

These decisions are logged in the architecture memlog.

- **Per-key sibling chain, own locale first.** The AC ("a sibling's values for a code without its own file") is satisfied, and an en-GB file that lacks a key still serves en-US's text, which is what the runtime `Catalog` chain does. The default locale is not part of the chain, because Discord already falls back to the default.
- **The default locale's entries are emitted too** (AC: "`en-US` values are emitted for both"). Values are verbatim, even when they equal the default.
- **Typed error shape.** `rule` is LOC-02/LOC-03 and `violation` is the base rule. LOC-03 maps to CMD-06, because a duplicate option name is CMD-06's rule. One violation per `(key, file locale, rule)` keeps sibling and shared-word reports from multiplying.
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
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`: expected ok.
- `cargo run --release -p xtask -- api-snapshot --bless`, then `-- api-snapshot`: expected a match. Install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if they are missing.
- `cargo deny check`: expected ok.
- `cargo +1.91 check --release --workspace`: expected ok.
- Optional, tracking only: `cargo check --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template`. CI marks the template build `continue-on-error` until Story 14.2, so errors from API that has not landed are acceptable. An error from a template command's description, `key` or locale files is not. Delete `target/template` afterwards if disk is tight.
- `test ! -d target/debug`: expected no debug profile.
