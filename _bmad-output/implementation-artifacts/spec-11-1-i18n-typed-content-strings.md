---
title: 'Story 11.1: `i18n!` typed content strings'
type: 'feature'
created: '2026-10-22'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-11-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/error-handling.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Bot authors cannot use locale files yet. `noctis::i18n!` does not exist, the `i18n` facade feature gates nothing, the App translates only the built-in `noctis.*` strings, and a path such as `label = keys::SAVE` in `defaults(..)` is a "[planned, Epic 11]" compile error. A missing key or a wrong placeholder can only be found at run time (FR-42, FR-75; AD-16; LOC-01, LOC-04).

**Approach:** `noctis-macros` gets a function-like `i18n!` macro. It reads `<dir>/*.yaml` with `yaml-rust2` 0.13 and emits `include_bytes!` for every file it reads. It generates a typed `Strings::of(locale)` accessor tree, `keys::*` `MessageKey` consts and a `catalog()` descriptor (`noctis_i18n::Catalog`). `noctis-i18n` gets `Catalog` (which implements `Translate`) and `AppStrings`, which layers catalogs over `Builtin`. `AppBuilder::locales(catalog)` installs the result as the App's strings, so every `Text::Key`, notice and `noctis.*` override resolves from the bot's files. Paths in `defaults(..)` (`label`, `placeholder`, `title`) become locale keys.

## Boundaries & Constraints

**Always:**

- **Macro surface.** The macro is `noctis::i18n!(default = "<code>", dir = "<path>")`.
  - Both arguments are optional and may come in any order, with an optional trailing comma. The defaults are `"en-US"` and `"locales"`.
  - `dir` is relative to `CARGO_MANIFEST_DIR`.
  - There is one hidden, undocumented flag, `__dir_from_source_file`, for Noctis's own trybuild fixtures only. With it, `dir` resolves against the directory of `proc_macro::Span::call_site().local_file()` (stable since 1.88; the MSRV is 1.91). Its rustdoc says it is internal.
  - The macro is item-position and emits only:
    - `pub struct Strings`;
    - `pub mod keys`;
    - `pub fn catalog() -> ::noctis::__private::i18n::Catalog`;
    - `#[doc(hidden)] pub mod __noctis_i18n` (node structs, the rows table, the catalog statics and the `include_bytes!` consts);
    - one LOC-04 warning const block per partial locale file.
  - Every generated item has a doc comment, so `missing_docs` stays quiet. A leaf accessor's doc shows the default-locale text, and a `keys::` const's doc shows its key and that text.
  - Generated paths go through `::noctis::__private::{i18n, rules}`. `__private` gains `pub use noctis_i18n as i18n;`, and the facade gains the dependency `noctis-i18n.workspace = true`.
- **Files.**
  - Locale files are the entries of `dir` whose extension is exactly `yaml`, sorted by file name. Each stem must parse as `noctis_rules::Locale` (exact, case-sensitive).
  - A file whose extension is `yml`, or `yaml` in another case (`YAML`), is a LOC-01 error.
  - Subdirectories and every other file are ignored.
  - Each file must be UTF-8 (a leading BOM is stripped) and hold zero or one YAML document. Zero documents, or a `~`/`null` root, counts as an empty map. The root must be a map.
  - The macro emits `const _: &[u8] = include_bytes!("<absolute path>");` for every file it read, including files that produced an error.
- **Key tree.**
  - A key is the dotted path of a leaf. Every map key must be a YAML string, and every leaf must be a YAML string.
  - Each of these is an error: integer, real or boolean leaves (help: quote it), null leaves, sequences, aliases, empty maps (`a: {}`), and non-string keys.
  - Every segment matches the `MessageKey` segment grammar `[A-Za-z0-9_-]+`, so a segment containing `.` (`"a.b": x`) is an error.
  - **Reserved families** are the root maps `cmd`, `cmdword`, `ctxcmd` and `noctis`. Each must be a map. Their leaves get no accessor and no `keys::` const, and no placeholder check runs on `cmd`, `cmdword` or `ctxcmd` values (Story 11.4 validates those keys).
  - `noctis.*` keys must be listed in the new `#[doc(hidden)] pub const FRAMEWORK_KEYS: &[(&str, &[&str])]` in `noctis-rules` (`src/key.rs`; name and placeholder names in `noctis_i18n::keys::ALL` order), and their placeholder set must equal the listed one. They may appear in any locale file without appearing in the default file.
  - **Content keys** are every other key. Each segment matches `[a-z][a-z0-9_]*`.
    - A segment that is not a plain identifier (`type`, `match`, `async`, …) is emitted as a raw identifier (`r#type`).
    - `self`, `super` and `crate` are errors.
    - A root-level leaf named `of` is an error, because it clashes with `Strings::of`.
- **Placeholders.**
  - The grammar is the runtime grammar of `noctis_i18n::format`: `{` followed by one or more `[A-Za-z0-9_]`, then `}`. Anything else (`{}`, `{a b}`, an unclosed `{`) is literal text.
  - A new `#[doc(hidden)] pub fn placeholder_names(template: &str) -> impl Iterator<Item = &str>` in `noctis-rules` (`src/key.rs`) yields the names in order, repeats included. A unit test proves it agrees with `format` on every case in the `placeholders` test.
  - A *named* placeholder matches `[a-z][a-z0-9_]*`. It becomes a parameter of that name, raw for keywords, and `{self}` is an error.
  - A *positional* placeholder is a canonical decimal `0|[1-9][0-9]*`. It becomes `argN`.
  - One value must not mix named and positional placeholders. Positional indexes must be exactly `0..n`, so a gap or a leading zero is an error. Any other name matching the grammar (`{Name}`, `{_x}`, `{1a}`) is an error.
  - Parameters come in order of first appearance (named) or index order (positional). Each parameter is `impl ::core::fmt::Display`, and each name appears once even when it repeats.
  - Every non-default locale value of a content key must have the same placeholder set as the default value (as a set). The error names the file, the key and the missing or extra names.
- **Locale coverage.**
  - A content key in a non-default file that the default file lacks is an error ("not in the default locale").
  - When a non-default file lacks content keys of the default file, that is LOC-04: one build warning per file, and runtime fallback.
- **The `Strings` accessor.**
  - `Strings::of(locale: Locale) -> Strings`, and `Strings: Clone + Copy + Debug`.
  - The fields of `Strings` are the root maps that hold content keys (public fields, typed by node structs). Its methods are the root leaves.
  - A node struct has public fields for child maps and methods for leaves.
  - A leaf with no placeholders returns `&'static str`. A leaf with placeholders returns `String` built by `noctis_i18n::format`, so every occurrence is replaced.
  - Rows are resolved at compile time.
    - `of` picks the file of `locale`, else its `Locale::sibling()`'s file, else the default file.
    - A key missing in the chosen file takes the sibling file's value (when that file has the key), else the default value.
- **`keys`.** It holds one `pub const NAME: MessageKey` per content key: dots become `_`, uppercased (`setup.need_channel` becomes `SETUP_NEED_CHANNEL`). Two keys that map to one name (`a.b_c` and `a_b.c`) are an error naming both. Reserved families get none.
- **`Catalog`** (new in `noctis-i18n`, `src/catalog.rs`).
  - `#[derive(Clone, Copy, Debug)]`, constructed only by the hidden `const fn __new(origin, default, files, placeholders)`.
  - `files` is `&'static [(Locale, &'static [(&'static str, &'static str)])]` in `Locale::ALL` order, with entries sorted by key bytes. It holds every key actually written in that file, reserved families included, with no fill-in.
  - Public methods:
    - `origin() -> &'static str`: `concat!(env!("CARGO_PKG_NAME"), "/", <dir as written>)`;
    - `default_locale() -> Locale`;
    - `locales()`: an iterator of the locales that have a file;
    - `get(locale, key: &str) -> Option<&'static str>`: that file only, no fallback;
    - `keys()`: every key of every file, sorted and deduplicated;
    - `placeholders(key) -> Option<&'static [&'static str]>`: content and `noctis.*` keys only.
  - `impl Translate for Catalog` tries the locale's file, then the sibling's file, then the default file, and formats with `format`.
- **`AppStrings`** (new in `noctis-i18n`).
  - `AppStrings::new(&[Catalog]) -> Result<AppStrings, Vec<DuplicateKey>>`, where `DuplicateKey { key, first, second }` holds the `&'static str` origins.
  - A key that two catalogs define in any of their files is a `DuplicateKey`.
  - Catalogs with equal `origin()` are deduplicated (idempotent registration).
  - `impl Translate`: the catalog that owns the key, then `Builtin`.
- **App.**
  - `AppBuilder::locales(&mut self, catalog: Catalog) -> &mut Self` (ungated in core) pushes the catalog.
  - `build()` makes `AppShared::strings = Arc::new(AppStrings::new(..)?)`. Each duplicate becomes `BuildViolation::DuplicateLocaleKey { key: &'static str, first: &'static str, second: &'static str }`, with the message ``locale key `{key}` is defined by two catalogs: `{first}` and `{second}` ``. These are collected with the other violations.
  - With no catalogs, the strings behave exactly as `Builtin` does today.
- **Facade, behind `#[cfg(feature = "i18n")]`.**
  - `pub use noctis_macros::i18n;`
  - `pub mod i18n { pub use noctis_i18n::{Catalog, format}; }`, with docs.
  - The `i18n = []` feature stays dependency-free.
- **Keys in `defaults(..)`.**
  - A path (after `Expr::Group` unwrapping) for `label`, `placeholder` or `title` lowers to `DefaultText::Key(<path>)`, with the path's span kept so a non-`MessageKey` path is a rustc `E0308` at the path. A string literal lowers to `DefaultText::Literal("…")` and is still checked against CV2-08, CV2-09 and MOD-01 at expansion.
  - `DefaultText` is a new `#[doc(hidden)] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum DefaultText { Literal(&'static str), Key(MessageKey) }` in `noctis-ui` `route.rs`, with `From<DefaultText> for Text`.
  - It replaces `&'static str` in `ButtonDefaults::label`, `SelectDefaults::placeholder` and `ModalDefaults::title`.
  - Resolved key text is validated after the Responder resolves it, exactly like overrides (Strict: `ResponderError::Invalid`; Lenient: truncation).
- **LOC-04 warning.** Each partial file emits:

  ```rust
  const _: () = { #[deprecated(note = "<msg>")] const W: () = (); W };
  ```

  The span is the call site. The message is ``[LOC-04] `<dir>/<file>` lacks <n> key(s) of the default locale `<default>.yaml`: `k1`, `k2`, …; they fall back at runtime``. It lists at most 10 keys, then `, and <m> more`. The author silences it with `#![allow(deprecated)]` on the module.
- **Diagnostics.**
  - Every error is a `syn::Error`; several errors are combined with `common::combine`.
  - The span is the `dir` (or `default`) literal when given, else the call site.
  - File errors read ``i18n!: `<dir>/<file>`: <problem>`` (YAML errors append `ScanError`'s `line L column C`). They name the dir as written and never an absolute path.
- **Rule ids.**
  - LOC-01 errors carry `[LOC-01]`.
  - The LOC-04 row of `docs/discord-rules.md` becomes ``Compile-time (by construction; `i18n!` generates accessors and `keys::` consts from the default locale only, so a key it lacks or a call with the wrong arguments does not compile; a key missing in another locale is a `deprecated`-lint build warning and falls back at runtime)``.
- **Template conversion.** Template key-looking literals in `defaults(..)` become `keys::…` paths (maintainer decisions §9).

**Never:**

- No `Module::locales` and no per-Module merge errors (Story 11.2).
- No change to the return types of `ctx.user_locale()`/`guild_locale()`/`response_locale()`, and no `LocaleResolver` (Story 11.3).
- No reading of `cmd.*` in `#[command]`, no Publish localization maps, no CMD/LOC-02/03/05 checks and no command-word ambiguity check (Story 11.4).
- No `LocaleSource` (Story 11.5).
- No localized `#[text_input]`/`#[radio_group]`/… attribute texts and no `Choices` labels: those stay literal-only. The MOD-04 path error drops "[planned, Epic 11]" and says to localize per render with `InputOverrides`.
- No `.yml` support, no brace escaping and no runtime file reading.
- No proc-macro panics: no `unwrap`/`expect` on input.
- No debug builds, no new `pass` trybuild suites and no weakening of `.cargo/config.toml`.

## I/O & Edge-Case Matrix

Unless a row says otherwise: `D` = `dir` as written, the default is `en-US`, and an error spans the `dir` literal or the call site.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Happy path | `i18n!()`; `locales/en-US.yaml` `common: {refresh: Refresh}`, `tr.yaml` `common: {refresh: Yenile}` | `Strings::of(Locale::Tr).common.refresh() == "Yenile"`; `keys::COMMON_REFRESH.as_str() == "common.refresh"`; `catalog().get(Tr, "common.refresh") == Some("Yenile")` | none |
| 2 | Args in any order, trailing comma | `i18n!(dir = "l", default = "tr",)` | default `tr`, dir `l` | none |
| 3 | Unknown or duplicate arg | `i18n!(path = "x")` / `dir` twice / `i18n!("x")` | ``unknown `i18n!` argument `path`: expected `default` or `dir` `` / ``duplicate `dir` `` / ```i18n!` takes `default = "…"` and `dir = "…"` `` | compile error at the token |
| 4 | Non-string arg | `dir = 3` | `` `dir` takes a string literal `` | compile error |
| 5 | Empty or absolute dir | `dir = ""` / `dir = "/abs"` | `` `dir` must be a non-empty path relative to the crate root `` | compile error |
| 6 | Default not a code | `default = "en"` | ``[LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US` `` (the `ParseLocaleError` Display) | compile error at the literal |
| 7 | Default not a code, no match | `default = "xx"` | `` [LOC-01] `xx` is not a Discord locale code `` | compile error |
| 8 | Missing dir | `dir = "nope"` | ``i18n!: `nope` is not a directory (relative to the crate root)`` | compile error |
| 9 | Default file missing (dir empty, or only `tr.yaml`) | default `en-US` | ``i18n!: the default locale `en-US` has no file `D/en-US.yaml` `` | compile error |
| 10 | Bare language stem | `en.yaml` | ``[LOC-01] locale file `D/en.yaml` is not named by a Discord locale code; help: name it `en-GB.yaml` or `en-US.yaml` `` | compile error |
| 11 | Wrong case | `EN-us.yaml` | same as row 10 with `EN-us.yaml`; help `en-GB.yaml` or `en-US.yaml` | compile error |
| 12 | One suggestion / 3+ / none | `pt.yaml` / `zh.yaml` / `xx.yaml` | help `` `pt-BR.yaml` `` / `` `zh-CN.yaml` or `zh-TW.yaml` `` (3+: `` `a`, `b` or `c` ``) / ``help: use one of Discord's 32 codes, such as `en-US.yaml`, `tr.yaml` or `pt-BR.yaml` `` | compile error |
| 13 | `.yml` or `.YAML` | `tr.yml`, `en-US.YAML` | ``[LOC-01] locale file `D/tr.yml` must use the `.yaml` extension`` | compile error |
| 14 | Hidden or underscored stem | `.tr.yaml`, `en_US.yaml` | row 10 shape (`en_US` suggests `en-GB.yaml`/`en-US.yaml`) | compile error |
| 15 | Ignored entries | `README.md`, `.gitkeep`, subdir `old/` | ignored; no `include_bytes!` | none |
| 16 | Not UTF-8 / read failure | invalid bytes | ``i18n!: `D/tr.yaml`: not UTF-8`` / ``…: cannot read: <io error>`` | compile error |
| 17 | BOM | `\u{FEFF}` prefix | parsed normally | none |
| 18 | YAML syntax error | `a: [` | ``i18n!: `D/en-US.yaml`: <ScanError info> at … line L column C`` | compile error |
| 19 | Duplicate YAML key | `a: x` twice | yaml-rust2's "duplicated key in mapping" in the row 18 shape | compile error |
| 20 | Two documents | `a: x\n---\nb: y` | `` …: a locale file holds one YAML document `` | compile error |
| 21 | Empty file / comments only / `~` | — | empty map; as the default file, `Strings` has no fields | none |
| 22 | Root not a map | `- a` / `hello` | `` …: the root must be a map of keys `` | compile error |
| 23 | Non-string leaf | `n: 3` / `b: true` / `r: 1.5` | ``…: `n` must be a string, found an integer; quote it`` (boolean / real) | compile error |
| 24 | Null, sequence or alias leaf | `a:` / `a: [x]` / `a: *x` | ``…: `a` has no value`` / `` …: `a` must be a string, found a sequence `` / `` …: `a`: YAML aliases are not supported `` | compile error |
| 25 | Empty map | `a: {}` | ``…: `a` is an empty map; remove it or add keys`` | compile error |
| 26 | Non-string key | `1: x` / `true: x` | `` …: keys must be strings; quote `1` `` | compile error |
| 27 | Dotted or bad segment | `"a.b": x` / `"a b": x` | ``…: `a.b`: a key segment is ASCII letters, digits, `_` or `-`; nest maps instead of dots`` | compile error |
| 28 | Content segment not snake case | `Setup: x` / `a-b: x` / `1st: x` / `_a: x` | ``…: `Setup`: content key segments are lowercase snake_case (`[a-z][a-z0-9_]*`)`` | compile error |
| 29 | Keyword segment | `type: x` / `profile: {match: y}` | accessors `r#type()`, `s.profile.r#match()`; consts `TYPE`, `PROFILE_MATCH` | none |
| 30 | Forbidden segment | `self: x` / `a: {super: y}` / `crate: z` | ``…: `a.super`: `super` cannot be an accessor name`` | compile error |
| 31 | Root leaf `of` | `of: x` | ``…: `of`: a root-level key cannot be named `of` (it is `Strings::of`)`` | compile error; a nested `a.of` is fine |
| 32 | Const name clash | `a: {b_c: x}`, `a_b: {c: y}` | ``i18n!: keys `a.b_c` and `a_b.c` both map to `keys::A_B_C` `` | compile error |
| 33 | Reserved family not a map | `cmd: x` | ``…: `cmd` is reserved for command localization and must be a map`` (likewise `cmdword`, `ctxcmd`, `noctis`) | compile error |
| 34 | Reserved family leaves | `cmd: {setup: {description: "Set {x} up"}}`, `cmdword: {setup: kurulum}`, `ctxcmd: {show: {name: Show}}` | no accessor, no `keys::` const, no placeholder check; present in `catalog().get(..)`; `placeholders("cmd.setup.description") == None` | none |
| 35 | Reserved segment with `-` | `cmd: {set-up: {description: x}}` | accepted (MessageKey grammar) | none |
| 36 | Framework override | `noctis: {expired: "Panel expired"}` in `tr.yaml` only | no accessor or const; App in `tr` shows "Panel expired"; `de` shows the Builtin text | none |
| 37 | Unknown framework key | `noctis: {wizard: {next: x}}` | ``…: `noctis.wizard.next` is not a framework key; `noctis.` is reserved: use your own namespace`` | compile error |
| 38 | Framework placeholder mismatch | `noctis: {rate_limited: "wait {secs}"}` | ``…: `noctis.rate_limited` must use exactly the placeholders `{retry}`; found `{secs}` `` | compile error |
| 39 | Named placeholders, repeated | `greet: "Hi {name}, {name}! You have {n}"` | `fn greet(&self, name: impl Display, n: impl Display) -> String`; `greet("Ada", 3) == "Hi Ada, Ada! You have 3"` | none |
| 40 | Positional | `p: "{1} of {0}"` | `fn p(&self, arg0, arg1)`; `p("a", "b") == "b of a"` | none |
| 41 | Mixed | `m: "{0} {name}"` | ``…: `m` mixes positional and named placeholders`` | compile error |
| 42 | Positional gap / leading zero / overflow | `"{0} {2}"` / `"{01}"` / `"{99999999999999999999}"` | ``…: `k`: positional placeholders must be `{0}`..`{n}` without gaps; missing `{1}` `` / ``…: `{01}` is not a canonical index`` / the gap error | compile error |
| 43 | Bad named placeholder | `"{Name}"` / `"{_x}"` / `"{1a}"` / `"{self}"` | ``…: `k`: placeholder `{Name}` must be lowercase snake_case or an index`` / `` `{self}` cannot be a parameter `` | compile error |
| 44 | Not placeholders | `"{} {a b} {x"` / `"{😀}"` | literal text, no parameters, returns `&'static str` | none |
| 45 | Doubled braces | `"{{n}}"` | one parameter `n`; `"{" + n + "}"` (format semantics) | none |
| 46 | Unicode values | `"çğ😀 {n}"` | preserved byte for byte | none |
| 47 | Empty string value | `a: ""` | `a() == ""` | none |
| 48 | Translation placeholder mismatch | default `"{n} left"`, tr `"{count} kaldı"` | ``…: `D/tr.yaml`: `k` must use the placeholders of the default locale `{n}`; missing `{n}`, extra `{count}` `` | compile error |
| 49 | Translation with the same set in another order | default `"{a} {b}"`, tr `"{b} {a}"` | accepted; params follow the default order | none |
| 50 | Orphan content key | `tr.yaml` has `x.y` that the default lacks | ``…: `D/tr.yaml`: `x.y` is not in the default locale `en-US.yaml` `` | compile error |
| 51 | Partial locale (LOC-04) | tr lacks `a`, `b` | one `deprecated` warning with ``[LOC-04] `D/tr.yaml` lacks 2 key(s) of the default locale `en-US.yaml`: `a`, `b`; they fall back at runtime``; `Strings::of(Tr).a()` = the en-US value | warning; `#![deny(deprecated)]` makes it an error |
| 52 | LOC-04 with more than 10 missing | 13 missing | lists the first 10 sorted keys, then `, and 3 more` | warning |
| 53 | Sibling row resolution | files `en-US`, `es-ES` (lacks `a`), `es-419` (has `a`); default `en-US` | `of(EnGb)` gives the en-US row; `of(Es419).a()` gives the es-419 value; `of(EsEs).a()` gives the es-419 value (sibling) | none |
| 54 | No file, no sibling | `of(PtBr)` | default row | none |
| 55 | Every locale | each of `Locale::ALL` | `of` never panics; returns a row | none |
| 56 | Missing key in code | `s.shop.missing()` / `keys::SHOP_MISSING` | rustc `E0599`/`E0425` | compile error (by construction) |
| 57 | Wrong arity / type | `greet("a")` / `greet(NoDisplay, 1)` | rustc `E0061` / `E0277` | compile error (by construction) |
| 58 | Catalog lookups | catalog from row 1 | `origin() == "<pkg>/locales"`, `default_locale() == EnUs`, `locales()` = `[EnUs, Tr]` (in `Locale::ALL` order), `get(De, …) == None`, `keys()` sorted and deduplicated | none |
| 59 | `Catalog` `Translate` | locale `EnGb`, key only in en-US / an unknown key | the en-US text with args substituted / `None` | none |
| 60 | `AppStrings` order | catalog has `shop.buy`; no catalog has `noctis.expired` | catalog text; `Builtin` text for `noctis.expired`; `None` for an unknown key | none |
| 61 | Duplicate key across catalogs | A and B both define `common.ok` | `BuildViolation::DuplicateLocaleKey { key: "common.ok", first: A.origin, second: B.origin }` | `build()` `Err`, collected with the other violations |
| 62 | Same catalog twice | `.locales(c).locales(c)` | deduplicated, no violation | none |
| 63 | No catalogs | App without `.locales` | behaves like `Builtin` (existing tests unchanged) | none |
| 64 | `defaults` key path | `#[button("b", defaults(style = Primary, label = keys::SAVE))]` | `DEFAULTS.label == Some(DefaultText::Key(SAVE))`; the unresolved build serializes `"label": "shop.save"`; via the App the Responder sends the translated label | none |
| 65 | Path that is not a `MessageKey` | `const L: &str = "x"; label = L` | rustc `E0308` at `L` | compile error |
| 66 | Literal over the limit | `label = "<81 chars>"` | the existing `[CV2-08]` error (likewise `[CV2-09]` at 151 chars and `[MOD-01]` at 46) | compile error |
| 67 | Resolved key text over the limit | `label = keys::LONG`, value 81 chars | Strict: `ResponderError::Invalid` with path `…label` and nothing sent; Lenient: truncated with `…` | runtime typed error |
| 68 | Select / modal paths | `placeholder = keys::P`, `title = keys::T` | `DefaultText::Key` and resolution like row 64; a modal title resolves in the User Locale first | none |
| 69 | Other non-literal `defaults` | `emoji = keys::E`, `label = concat!("a")` | the existing `` `defaults` values are literals `` error | compile error |
| 70 | Modal input attribute path | `#[text_input(keys::L)]` | `` [MOD-04] the Label text is a locale key path: input attribute texts are string literals; localize them per render with `InputOverrides` `` | compile error |

</intent-contract>

## Code Map

- `Cargo.toml` -- `[workspace.dependencies] yaml-rust2 = "0.13"`. Change it to `{ version = "0.13", default-features = false }` (drops `encoding_rs`). Verified: crates.io max 0.13.0, published 2026-09-11, MSRV 1.85, MIT OR Apache-2.0, deps `arraydeque` and `hashlink` 0.12 (hashbrown 0.17, already in the lock).
- `crates/noctis-macros/Cargo.toml`, `src/lib.rs` (`#[proc_macro]` entries at :111-160 show the pattern), `src/common.rs:19` `combine` -- add `i18n` here.
- `crates/noctis-macros/src/defaults.rs:174-215` (`is_path`, `text_rule`, `key_path_error`, `text`), plus its tests `key_paths_are_planned` (~:679) and `button_errors` (~:750) -- lift paths here.
- `crates/noctis-macros/src/modal.rs:32` (`LOCALIZED`), `:202-240` (`text_arg`, `localized`), `:1728` (title path) -- the title path is lifted, the input-attribute paths stay errors with the new text.
- `crates/noctis-ui/src/route.rs:85-145` (`ButtonDefaults`, `SelectDefaults`, `ModalDefaults`), `:168` `build_button`; `src/component/select.rs:158` (`d.placeholder.map(Text::borrowed)`); `src/modal/builder.rs:110` (`Text::borrowed(X::DEFAULTS.title)`); `src/overrides.rs:35` doc example; `src/text.rs` (`Text`, `Text::borrowed`, resolution docs) -- the `DefaultText` switch.
- `crates/noctis-i18n/src/lib.rs` -- `Builtin`, `format` (single-pass, every occurrence), `keys::ALL`; add `catalog.rs` and `AppStrings` here.
- `crates/noctis-rules/src/key.rs` (`MessageKey`, `Translate`), `src/locale.rs` (`Locale::ALL`, `sibling`, `ParseLocaleError::suggestions`) -- reuse these, and add `FRAMEWORK_KEYS` and `placeholder_names` to `key.rs`, re-exported from the crate root.
- `crates/noctis-core/src/app.rs:1255` `AppBuilder` fields; `:2154` `strings: Arc::new(noctis_i18n::Builtin)`; `:410` test-only `set_strings`; `src/error.rs:140` `BuildViolation` (`&'static str` fields like `DuplicateState`); `src/notice.rs` and `responder.rs:1330` consume `strings`.
- `crates/noctis/src/lib.rs` -- `__private` at :2026; feature docs at :560; the "until locale files arrive (Epic 11)" text at :478, :486 and :2486.
- `crates/noctis/tests/ui.rs` -- the featureless suite and `harness` gating; add the `ui_i18n` suite here.
- `crates/noctis/tests/ui/cv2/CV2-08_defaults_label_path.*`, `CV2-09_defaults_placeholder_path.*`, `ui/mod/MOD-01_defaults_title_path.*` -- delete. Other diagnosed fixtures keep these rules covered (`CV2-08_defaults_label_81`, `CV2-09_defaults_placeholder_151`, `MOD-01_title_too_long`).
- `crates/noctis/tests/ui/mod/MOD-04_text_input_label_path.stderr` -- re-bless with the new message.
- `crates/noctis/tests/types/defaults_unknown_key.rs:25-28` -- remove the `LABEL` path case and re-bless.
- `xtask/src/rules_catalog/mod.rs` (fixture layout: `ui-<feature>/<area>/<RULE-ID>_<slug>.rs`, only `.rs`/`.stderr` in suites, registration check `compile_fail("tests/ui-i18n/**/*.rs")`); `xtask/src/rules_catalog/catalog.rs:575-585` (test lists of by-construction and DocOnly ids taken from the real docs); `xtask/rules-pending.toml:16-19` (the LOC-01 entry for 11.1).
- `docs/discord-rules.md` rows LOC-01 (:121), LOC-04 (:124), CV2-08 (:196), CV2-09 (:197), MOD-01 (:233), MOD-04 (:236).
- `examples/project-template/src/strings.rs`, `locales/{en-US,tr}.yaml`, `src/lib.rs` (`.locales(strings::catalog())` already there), `src/modules/{profile,setup,dashboard}/mod.rs` (`defaults(..)` literals: `common.refresh`, `profile.edit_bio`, `profile.bio_modal.title`, `setup.next`, `setup.channel_placeholder`, `setup.role_placeholder`, `setup.cancel`, `setup.finish`, `dashboard.tab.overview`, `common.next`, `common.enable`, plus any others found by grepping `defaults(`), and the comments at `profile/mod.rs:69,93`. The template is tracking-mode (not compiled by the gates).
- `examples/project-template/.claude/skills/noctis/references/i18n.md` (§2 "[planned shape]", §6 overrides "[planned, Epic 11]", §7), `components-v2.md:374`, `modals.md:80`, `routes-payloads-refs.md:55`, `responder.md:716`, `SKILL.md:23` (API hash), `examples/project-template/AGENTS.md:55`.
- `.github/workflows-disabled/ci.yml` -- no change: the `ui-i18n` suite runs inside the existing `cargo hack --each-feature` job.

## Tasks & Acceptance

**Execution:**

- [ ] `Cargo.toml` -- make `yaml-rust2` `default-features = false`. Then `crates/noctis-macros/Cargo.toml` adds `yaml-rust2.workspace = true`, and `crates/noctis/Cargo.toml` adds `noctis-i18n.workspace = true`. -- the macro's YAML parser and the facade's path for generated code.
- [ ] `crates/noctis-rules/src/key.rs` (+ `lib.rs` re-exports) -- add hidden `FRAMEWORK_KEYS` and `placeholder_names`, with unit tests. -- shared, layer-legal data and grammar for macro checks (macros cannot depend on `noctis-i18n`).
- [ ] `crates/noctis-i18n/src/catalog.rs`, `src/lib.rs` -- add `Catalog` (hidden `__new`, the public methods, `Translate`), `AppStrings`, `DuplicateKey` and tests for rows 58-62. Add a test that `FRAMEWORK_KEYS` equals `keys::ALL` plus each `Builtin` template's placeholder names, and a test that `placeholder_names` agrees with `format`. Update the crate docs. -- the runtime half.
- [ ] `crates/noctis-macros/src/i18n/{mod.rs,args.rs,load.rs,model.rs,emit.rs}` (+ `lib.rs` `#[proc_macro] pub fn i18n`) -- argument parsing; file listing and reading (I/O only in `load.rs`); a pure `model::compile(files: &[(String /*file name*/, String /*contents*/)], default: Locale, dir: &str) -> Result<Model, Vec<String>>` that builds the key tree, runs every check and resolves rows; and `emit` producing the tokens described in Boundaries. Unit tests cover every compile-error and success row of the Matrix through `compile` with inline YAML strings, and token assertions (no `CMD_`/`NOCTIS_` consts, raw idents, the LOC-04 note text). -- the macro itself, testable without the filesystem.
- [ ] `crates/noctis-ui/src/route.rs`, `component/select.rs`, `modal/builder.rs`, `overrides.rs`, `text.rs`, `lib.rs` -- add `DefaultText` with `From<DefaultText> for Text`, switch the three defaults fields, and fix every constructor (grep `ButtonDefaults {`, `SelectDefaults {`, `ModalDefaults {` workspace-wide, including `examples/ui-recipes`). -- the key paths need a const-friendly `Copy` type.
- [ ] `crates/noctis-macros/src/defaults.rs`, `modal.rs` -- lower path texts to `DefaultText::Key` (keeping the span) and literals to `DefaultText::Literal`; set the MOD-04 attribute-path message of row 70; drop every "[planned, Epic 11]"; update the unit tests. -- maintainer decisions §9.
- [ ] `crates/noctis-core/src/app.rs`, `src/error.rs` (+ tests) -- add `AppBuilder::locales`, the `catalogs` field, `AppStrings` at build and `BuildViolation::DuplicateLocaleKey`. Unit tests build an App with hand-written `Catalog::__new` data and check: a notice override in `tr` (row 36), `Text::Key` resolution in a component render (row 64), the duplicate violation (row 61), deduplication (row 62), and the unchanged `Builtin` path (row 63). -- wiring.
- [ ] `crates/noctis/src/lib.rs` -- add `__private::i18n`, the feature-gated `i18n!` re-export and the `i18n` module with doctests, and fix the "until Epic 11" texts. -- facade surface.
- [ ] `crates/noctis/tests/fixtures/i18n/` (YAML case dirs: `basic/` with `en-US`, `tr`, `en-GB`, `es-ES`, `es-419`; `partial/`; `bare_stem/` holding `en-US.yaml` and `en.yaml`; one dir per further trybuild case) and `crates/noctis/tests/i18n.rs` (`#![cfg(feature = "i18n")]`) -- a runtime test of rows 1, 39, 40, 45-47, 51 (inside a module under `#[expect(deprecated, reason = "LOC-04 fallback under test")]`), 53-55 and 58, using `dir = "tests/fixtures/i18n/<case>"`. -- end-to-end macro test.
- [ ] `crates/noctis/tests/i18n_app.rs` (`#![cfg(all(feature = "i18n", feature = "testkit"))]`) -- a `TestApp` with `.locales(catalog())` and a Route button `label = keys::X`. A simulated click with user locale `tr` records an `update` whose label is the `tr` text (row 64). -- outermost surface.
- [ ] `crates/noctis/tests/ui-i18n/loc/` with `LOC-01_bare_language_stem.rs`, `LOC-01_default_not_a_code.rs`, `LOC-04_missing_default_key.rs`, `LOC-04_wrong_arg_count.rs`, `LOC-04_wrong_arg_type.rs` and `LOC-04_partial_locale_warning.rs` (`#![deny(deprecated)]`), each with a blessed `.stderr`. They use `noctis::i18n!(dir = "../../fixtures/i18n/<case>", __dir_from_source_file)`, and each LOC-04 offending line carries `// [LOC-04]` after the code. Add `crates/noctis/tests/types/defaults_key_path_not_message_key.rs` + `.stderr` (row 65). Update `crates/noctis/tests/ui.rs`:
  - add `#[cfg(all(feature = "i18n", not(any(<every other optional feature>))))] #[test] fn ui_i18n()` running `compile_fail("tests/ui-i18n/**/*.rs")`, with the same toolchain and dev-profile assertions;
  - gate `harness` on either suite;
  - update the module docs.

  Delete the three `*_path` fixtures listed in the Code Map, re-bless `MOD-04_text_input_label_path.stderr` and `types/defaults_unknown_key.stderr`, and bless with `TRYBUILD=overwrite cargo test --release -p noctis --test ui --features i18n` (and the featureless run). -- AC fixtures.
- [ ] `docs/discord-rules.md`, `xtask/rules-pending.toml`, `xtask/src/rules_catalog/catalog.rs` -- update the LOC-01 note (`i18n!` file stems and `default`, with the fixtures named), switch LOC-04 to the by-construction cell above, update the CV2-08, CV2-09 and MOD-01 notes (a path is a locale key resolved and validated at run time), and change MOD-04's note to "literal only; per-render `InputOverrides`". Remove the LOC-01 pending entry, and move LOC-04 from the expected DocOnly list to the by-construction list in the catalog test. -- the catalog matches the behavior.
- [ ] `examples/project-template/src/strings.rs`, `locales/*.yaml`, `src/modules/**` -- fix the doc comment (real items, return types, `s.setup.progress(n, total, title)`, `AppBuilder::locales`, `Module::locales` [planned, Story 11.2]); convert every `defaults(..)` key literal to `keys::…` with `use crate::strings::keys;`. At each `BioForm` build site, localize the input labels and placeholder through `BioFormOverrides`/`InputOverrides` with `keys::PROFILE_BIO_MODAL_{BIO,BIO_HINT,PRONOUNS}` (check the generated field names against the derive). Reword the `[planned, Epic 11]` comments (Pronouns: "Choices labels are literal text [planned]"). Make sure `tr.yaml` has every content key of `en-US.yaml` with equal placeholder sets. -- AC: template reconciled.
- [ ] Usage Skill (`references/i18n.md` §2, §6 and §7; `components-v2.md`, `modals.md`, `routes-payloads-refs.md`, `responder.md`, `SKILL.md`) and `examples/project-template/AGENTS.md` -- document:
  - `i18n!` (args, file naming, placeholder grammar, positional `{0}`, key-segment rules, raw identifiers, `of`);
  - the generated items and their return types;
  - LOC-04 as a `deprecated` warning silenced by `#![allow(deprecated)]`;
  - `catalog()`, `Catalog`'s methods, `noctis::i18n::format`, `AppBuilder::locales`, `BuildViolation::DuplicateLocaleKey`;
  - `noctis.*` overrides, now live;
  - `defaults(..)` paths as keys;
  - the interim `Strings::of(ctx.response_locale().parse().unwrap_or(Locale::EnUs))` until Story 11.3 returns `Locale`.

  Remove every stale "[planned, Epic 11]". Then run `cargo run --release -p xtask -- api-snapshot --bless`, set the `API hash` line to the printed hash, and run `skill-check`. -- AC: skill and API gates.
- [ ] `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- log implementation-time `(decision)` lines. -- traceability.

**Acceptance Criteria:**

- Given `locales/en-US.yaml` and `tr.yaml` and `noctis::i18n!()` with the `i18n` feature, when the crate compiles, then `Strings::of`, the nested accessors, `keys::*` and `catalog()` exist as specified, every read file has an `include_bytes!` (so editing it rebuilds the crate), and `cmd.`/`cmdword.`/`ctxcmd.`/`noctis.` keys have no accessors and no consts.
- Given the `tests/ui-i18n/loc/` fixtures in the `--features i18n` run, when the UI tests run, then the missing default key, the wrong argument count, the wrong argument type, `en.yaml` (help naming `en-GB.yaml` and `en-US.yaml`), `default = "en"` and the denied LOC-04 warning all fail to compile with their blessed `.stderr`, and `rules-catalog` counts LOC-01 as diagnosed and LOC-04 as by-construction.
- Given a key missing in `tr.yaml`, when the crate compiles, then exactly one `[LOC-04]` `deprecated` warning names the file and the keys, and at run time `Strings::of(Locale::Tr)` and `catalog()`'s `Translate` return the default value.
- Given a value with a repeated placeholder, when it is formatted through its accessor or through the App's `Translate`, then every occurrence is replaced.
- Given an App built with `.locales(catalog())` and a Route button whose `defaults(label = keys::X)`, when a `tr` user's interaction is answered through the testkit, then the sent label is the `tr` text, and a `noctis.expired` override in the files replaces the built-in notice.
- Given two catalogs that define one key, when `build()` runs, then it fails with `BuildViolation::DuplicateLocaleKey` naming the key and both origins, before any connection.
- Given the finished story, when the gates run, then the Usage Skill documents every new public item, the template's `strings.rs`, locales and `defaults(..)` keys are reconciled, the API hash matches, and `skill-check` passes.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why a hidden `__dir_from_source_file`:** trybuild compiles fixtures in `<target>/tests/trybuild/noctis`, so `CARGO_MANIFEST_DIR` there is not `crates/noctis`. The fixture YAML cannot sit in the suite dir either, because `rules-catalog` rejects files other than `.rs`/`.stderr`. A file-relative escape hatch is deterministic, and `Span::local_file` is stable on the MSRV. Integration tests and doctests use the public crate-root form.
- **Why LOC-04 changes tier:** every compile-time check needs a fixture whose id matches its area directory (`ui-i18n/loc/`). The missing-key and wrong-argument errors are the default-locale side of LOC-04's fallback rule and exist *by construction* (rustc's own errors). The non-default side is a warning implemented with the `deprecated`-lint technique that the `proc-macro-warning` crate uses on stable. If the call-site span ever stops surfacing the warning, the fixture turns red.
- **Why merge in 11.1:** `AppBuilder::locales` has to take a catalog for key paths to resolve at all. Accepting several catalogs with per-key duplicate detection gives Story 11.2 a ready engine: it only adds `Module::locales` and names Modules as origins.
- **Why `&'static str` / `String` returns:** locale data stays resident and nothing is copied for plain text (maintainer decisions §5). Both returns convert `Into<Text>`.
- **Why `Strings::of(Locale)`:** `ctx.*_locale()` still return `&str` until Story 11.3 changes them. The template keeps the planned `Strings::of(ctx.response_locale())`, and the skill shows the interim parse.
- **Generated shape (golden):**

  ```rust
  pub struct Strings { pub setup: __noctis_i18n::N3, __row: &'static [&'static str; 42] }
  impl Strings { pub fn of(locale: Locale) -> Self { /* match on all 32 codes -> &ROWS[i] */ } }
  impl __noctis_i18n::N3 {
      /// `en-US`: "Step {n} of {total}: {title}"
      pub fn progress(&self, n: impl Display, total: impl Display, title: impl Display) -> String {
          format(self.__row[17], &[("n", &n), ("total", &total), ("title", &title)])
      }
  }
  ```

## Verification

**Commands:**

- `cargo fmt --all` -- expected: no diff.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean.
- `cargo test --release --workspace --all-features` -- expected: all pass (macro unit tests, i18n and core tests, `tests/i18n.rs`, `tests/i18n_app.rs`, doctests).
- `cargo hack test --release --workspace --each-feature` -- expected: all pass, including `ui` (featureless) and `ui_i18n` (`--features i18n`).
- `cargo run --release -p xtask -- deps-check` -- expected: ok (only the facade → `noctis-i18n` edge is new, and the facade may depend on any crate).
- `cargo run --release -p xtask -- rules-catalog` -- expected: ok; LOC-01 diagnosed, LOC-04 by-construction, no LOC-01 pending entry.
- `cargo run --release -p xtask -- api-snapshot --bless` then `cargo run --release -p xtask -- api-snapshot` -- expected: matches. Install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if they are missing.
- `cargo run --release -p xtask -- skill-check` -- expected: ok.
- `cargo deny check` -- expected: ok.
- `cargo +1.91 check --release --workspace` -- expected: ok.
- `test ! -d target/debug` -- expected: no debug profile.
