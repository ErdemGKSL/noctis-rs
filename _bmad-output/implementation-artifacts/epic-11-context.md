# Epic 11 Context: Localization

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Let bot authors ship YAML Locale files that compile into typed, argument-checked string accessors, localize Commands (names word by word, descriptions, options, choices) through Discord's localization maps at Publish, and give every handler the correct User and Guild Locale with sensible fallbacks and overrides. Localization is compiled, so missing keys and placeholder mismatches fail the build instead of reaching users. It builds on `Locale`, `MessageKey` and `Translate` (Epic 1) and the default `noctis.*` strings (Epic 6).

## Stories

- Story 11.1: `i18n!` typed content strings
- Story 11.2: Module-scoped Locale catalogs
- Story 11.3: User and Guild Locale resolution
- Story 11.4: Command localization in Publish
- Story 11.5: Runtime Locale overlays through LocaleSource

## Requirements & Constraints

- Typed content strings: a key missing from the default Locale, or a call with wrong argument count/type, is a compile error; a key missing in another Locale is a build warning and falls back to the default at runtime (LOC-04). Every occurrence of a repeated placeholder is replaced.
- Locale file stems must be exactly one of Discord's 32 codes as `Locale` prints them (`en-US.yaml`, `tr.yaml`); any other stem, including `en.yaml`, is a LOC-01 compile error whose help names matching codes (`en-US.yaml`, `en-GB.yaml`).
- Module catalogs merge per language at `App::build`; a key defined by two Modules is a `BuildError` naming the key and both Modules. Disabling a Module at runtime does not remove its strings.
- Resolution: interactions go User -> Guild -> default; guild events go Guild -> default. A code with no file first falls back to its `Locale::sibling()` file (`en-GB` -> `en-US`; `pt-BR` has none). Handlers read `ctx.user_locale()` / `ctx.guild_locale()`; an App `LocaleResolver` or a before-Middleware can override.
- Command localization: Publish emits `name_localizations` / `description_localizations`; full-path keys override per-word translations; context-menu names localize as whole names; a sibling's values are emitted for a code without its own file. Localized values follow CMD-01..04 and OPT-09 (LOC-02), localized option names must be distinct (LOC-03), the CMD-05 budget counts the longest variant per field (LOC-05), and dispatch routes on default names only (LOC-06). Violations are typed errors naming Locale, key and rule id.
- Overlays (`LocaleSource`) apply to content keys only; overlaying `cmd.*` / `cmdword.*` is a typed error, as is an overlay whose placeholders differ from the compiled set (error names the key).
- Each story with new public API updates the Usage Skill (i18n reference, file naming, resolution order, the rule "User Locale for ephemeral single-user UI, Guild Locale for shared content", command keys, overlays), reconciles the project template (`strings.rs`, `locales/{en-US,tr}.yaml`), and passes `skill-check`.

## Technical Decisions

- `i18n!(default = "en-US", dir = "locales")`, both optional, paths relative to the crate root; parses YAML with `yaml-rust2` 0.13 and emits `include_bytes!` for every file read so edits rebuild. Generates `Strings::of(locale)` with nested accessors mirroring the YAML tree (one method per leaf, args in placeholder order, named or positional), a `MessageKey` const per key in `keys::*` (dots -> `_`, uppercased), and a `catalog()` descriptor.
- Reserved key families: `cmd.<path>.name|description`, `cmd.<path>.opt.<opt>.name|description|choice.<value>`, `cmdword.<word>`, `ctxcmd.<key>.name` (from `#[user_command(.., key = "...")]` / `#[message_command]`). `<path>` is Command Path words as nested YAML maps. These get no accessors and no `keys::` consts; only `#[command]` and Publish read them. A Command word equal to `name`, `description` or `opt` is a compile error when locale files are read.
- With the `i18n` feature and a default-locale file, `#[command]` reads `cmd.<path>.description` (and option descriptions) from that file using the same `dir`/`default` as `i18n!` or `#[command(.., locales = "<dir>")]`; the literal may be omitted, a differing literal fails to compile, and a missing key with no literal is CMD-04. Without the feature or file, Epic 6's literal rule applies unchanged.
- `#[command]` validates default-locale names in its own crate; merged and non-default validation runs at `App::build` on the merged catalog, which is the single source for Publish JSON.
- `Locale` (closed 32-code enum), `MessageKey`, `Translate` and `Locale::sibling()` live in `noctis-rules`; `noctis-i18n` (layer 3) holds the locale runtime, merged catalogs and default `noctis.*` strings; `i18n!` lives in `noctis-macros`. The `i18n` facade feature gates `i18n!`, public `noctis::i18n` items and locale reading in `#[command]`; core depends on `noctis-i18n` unconditionally. Framework strings use only the reserved `noctis.` namespace via `&dyn Translate`.
- Literal vs localized text is decided by syntax, never the feature: a string literal is hard-coded text, a path (`keys::X`, a `MessageKey`) is a locale key. Template key-looking literals become `keys::...` paths in Story 11.1; the "[planned, Epic 11]" compile error for paths in `components!` `defaults(..)` is lifted here.
- Locale resolution is pipeline stage 4 (only `LocaleResolver` author code runs there, no port I/O); before-Middleware (stage 7) may override the locale. Responder resolves `Text` keys in the response locale (ephemeral/modal: User then Guild; public: Guild then User; else en-US).
- Content key segments are `[a-z][a-z0-9_]*`; placeholders are named or positional (`{0}..{n}`), never mixed; non-default values must match the default's placeholder set; an orphan content key in a non-default file is an error. YAML is parsed through yaml-rust2's event parser (aliases rejected, duplicate keys reported with line/column).
- LOC-04's partial-locale warning is a `deprecated`-lint const at the call site. `noctis.*` keys in locale files override `Builtin`, checked against a hidden `noctis_rules::FRAMEWORK_KEYS` table. `AppBuilder::locales(Catalog)` takes several catalogs layered by `noctis_i18n::AppStrings`; a key in two catalogs is `BuildViolation::DuplicateLocaleKey { key, first, second }` (origins `<crate>/<dir>`); `Module::locales` reuses this engine.
- `defaults(..)` paths lower to hidden `noctis_ui::DefaultText::Key`, validated at send time; modal input-attribute texts and `Choices` labels stay literal-only (localize via per-render `InputOverrides`). `ctx.*_locale()` return `&str` until Story 11.3.
- trybuild fixtures: `tests/ui-i18n/loc/` (11.1) and `tests/ui-i18n/` (11.4), `compile_fail` only, run in the `i18n` feature run.

## Cross-Story Dependencies

- 11.1 provides the macro, keys and `catalog()` descriptor used by 11.2 (merging), 11.4 (command keys) and 11.5 (placeholder sets).
- 11.2's merged catalog is the input for 11.4's Publish JSON and `App::build` validation, and for 11.5 overlays.
- 11.3 fulfils the Middleware locale override deferred from Story 7.6; 11.4 extends Story 6.1's description rule.
