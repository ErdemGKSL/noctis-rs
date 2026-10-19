---
title: 'Story 1.3: noctis-rules foundations: Limits, length counting, rule ids and locales'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: '687b2f8f43c7824b37d0db90edffc89784ed57fe'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-rules` is an empty skeleton, so there is no single source of truth for Discord limits, text length, rule ids or locale codes. Stories 1.4 to 1.6, the macros (Epics 4 and 6) and the runtime validator (Story 4.3) all need these foundations, and without them a macro and a validator could disagree about a limit (AD-3, FR-24, FR-79, NFR-10, AD-16, LOC-01, CV2-04).

**Approach:** Add the foundation types to `noctis-rules`, with no dependencies: `RuleId` (a closed set of the 124 catalog ids), `Violation`, `Limits`/`LimitMode`, `len::{discord_len, truncate}`, `Locale` (the 32 codes, with `sibling()`), `MessageKey` and `trait Translate`. Re-export them from the facade as `noctis::rules::*`. Add the missing catalog row CV2-17 (media URL length), and update the Usage Skill and the template's `AGENTS.md`.

## Boundaries & Constraints

**Always:**
- `noctis-rules` is sync, does no I/O and has no `async`. In this story it has **no** `[dependencies]` (`regex` is the only crate allowed later, and Story 1.4 adds it). Its only dev-dependency is `proptest.workspace = true` (workspace `1.12`, which resolves to 1.12.0). Error types implement `Display` and `std::error::Error` by hand (no `thiserror`).
- `discord_len` is the only length function for user text. `truncate` uses it to measure.
- The rule functions that compare against a limit (Stories 1.4 to 1.6) take `&Limits`. `discord_len` and `truncate` are counting primitives with an explicit `max`, so they take no `&Limits` (decision). Nothing in `src/` outside `#[cfg(test)]` calls `Limits::default()`, apart from the `Default` impl itself.
- Every Limits field documents its rule id, and every rule id exists in `docs/discord-rules.md`. A new limit adds its catalog row in the same commit (AGENTS.md).
- Public items carry rustdoc (the workspace warns on `missing_docs`, CI uses `-D warnings`, and the facade denies it). Every cargo command uses `--release`.
- Facade: `crates/noctis` gains `noctis-rules.workspace = true` (a normal dependency, allowed by deps-check's facade rule) and a `pub mod rules` that re-exports the public items.

**Never:**
- Don't add a rule *check* function (CMD-01 regex, component counts and so on); those belong to Stories 1.4 to 1.6.
- Don't add `regex`, `thiserror`, `serde` or `unicode-segmentation`. Don't add a `prelude`, a `build.rs` or `unsafe`.
- Don't add a base-language type, a `Default` for `Locale`, or case-insensitive or trimmed locale parsing.
- Don't edit `_bmad/`, `.github/workflows-disabled/` (no new gate), `xtask/` or the template's `src/`.

## I/O & Edge-Case Matrix

"units" means `discord_len` units. `L` is a `Limits`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| len empty | `discord_len("")` | `0` | none |
| len ASCII | `"abc"` | `3` | none |
| len Latin-1/Turkish | `"ğüş"` | `3` | none |
| len CJK (BMP) | `"日本語"` | `3` | none |
| len BMP edge | `"\u{FFFF}"` / `"\u{10000}"` / `"\u{10FFFF}"` | `1` / `2` / `2` | none |
| len astral emoji | `"😀"` | `2` (1 scalar, 2 UTF-16) | none |
| len skin tone | `"👍🏽"` | `4` | none |
| len ZWJ family | `"👨‍👩‍👧"` (5 scalars) | `8` | none |
| len flag | `"🇹🇷"` | `4` | none |
| len combining | `"e\u{301}"` | `2` | none |
| len NUL inside | `"a\0b"` | `3` | none |
| len at CV2-04 limit | 4000 × `a`; 2000 × `😀`; 2001 × `😀` | `4000`; `4000`; `4002` | none |
| len property | any `String` s (proptest) | `== max(chars().count(), encode_utf16().count())`, `== encode_utf16().count()`, `>= chars`, `<= 2*chars`, additive over concatenation | none |
| truncate fits | `truncate("abc", 3)`, `("", 0)`, `("", 5)` | the input unchanged, `Cow::Borrowed` | none |
| truncate ASCII | `("abcd", 3)` | `"ab…"` (3 units) | none |
| truncate max 1 | `("ab", 1)` | `"…"` | none |
| truncate max 0 | `("a", 0)` | `""` | none |
| truncate no split | `("ab🎉", 3)`; `("🎉🎉", 2)`; `("🎉🎉", 3)` | `"ab…"`; `"…"`; `"🎉…"` | none |
| truncate combining | `("e\u{301}x", 2)` | `"e…"` (cuts between scalars; not grapheme-aware, as documented) | none |
| truncate property | any s, any max in 0..=64 | `discord_len(out) <= max`; if `discord_len(s) <= max` then `out == s`; else `out` = a prefix p of s + `…`, and `p` is the longest such prefix, with `discord_len(p) + 1 <= max` (or `out == ""` when max == 0) | none |
| RuleId set | `RuleId::ALL` | 124 ids, unique, in catalog row order, each `^[A-Z][A-Z0-9]{1,3}-[0-9]{2}$`; equal to the `\| **ID**` rows of `docs/discord-rules.md` | none |
| RuleId consts | `RuleId::CV2_03`, `RuleId::META_05` | `as_str()` → `"CV2-03"`, `"META-05"`; `Display` prints the same; `area()` → `"CV2"`, `"META"`; `number()` → `3`, `5` | none |
| RuleId lookup ok | `lookup("CMD-01")`, `lookup("CV2-17")` | `Some(RuleId::CMD_01)`, `Some(RuleId::CV2_17)` | none |
| RuleId lookup miss | `lookup("cmd-01")`, `("CMD-1")`, `("CMD-08")` (gap), `("CMD-01 ")`, `(" CMD-01")`, `("")`, `("XYZ-01")` | `None` | none; never panics |
| structural | `is_structural()` over `ALL` | true for exactly CV2-03, CV2-05, CV2-06, CID-01, CID-02 | none |
| Limits defaults | `Limits::default()` | every field equals the Design Notes table; `mode == LimitMode::Strict`; 57 limit fields | none |
| Limits field rules | the crate-internal field table | 57 unique field names, each rule id in `RuleId::ALL` | none |
| strict never truncates | `L::default().truncates(r)` for every `r` in ALL | `false` | none |
| lenient structural | `mode = Lenient`, `truncates(CV2_03 / CV2_05 / CV2_06 / CID_01 / CID_02)` | `false` (one test per id, its name citing the id, for example `lenient_never_applies_to_cv2_03`) | none |
| lenient text | `mode = Lenient`, `truncates(CV2_04)`, `truncates(MSG_01)` | `true` | none |
| lowered override | `let mut l = Limits::default(); l.components = 30;` | `l.components == 30`, other fields stay at their defaults, `l != Limits::default()` | none |
| Violation display | `Violation::new(CV2_03, "components", Bound::AtMost(40), Actual::Count(41))` | `[CV2-03] components: limit at most 40, actual 41` | none |
| Violation empty path | `Violation::new(CMD_02, "", Bound::Between{min:1,max:32}, Actual::Count(0))`, `Actual::Count(0)` | `[CMD-02] limit between 1 and 32, actual 0` | none |
| Violation other bounds | `AtLeast(2)`, `Exactly(1)`, `Describe("lowercase only")`; `Actual::Text("Ab\n")` | `at least 2`, `exactly 1`, `lowercase only`; actual printed as `` `Ab\n` `` (escape_debug inside backticks) | implements `std::error::Error` |
| Locale all | `Locale::ALL` | 32 variants in LOC-01 order; each `as_str()` round-trips through `FromStr`; `Display == as_str()` | none |
| Locale exact parse | `"en-US"`, `"es-419"`, `"no"`, `"tr"`, `"zh-TW"` | `EnUs`, `Es419`, `No`, `Tr`, `ZhTw` | none |
| Locale bare language | `"en"` | `Err`, suggestions `[EnGb, EnUs]`, Display `` [LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US` `` | typed `ParseLocaleError` |
| Locale wrong case/sep/space | `"en-us"`, `"EN-US"`, `"en_US"`, `"en-US "`, `" tr"` | `Err`; suggestions `[EnGb, EnUs]` for the first four; `[]` for `" tr"` (language part `" tr"`) | typed error, no trimming |
| Locale other region | `"es-MX"`, `"pt-PT"`, `"zh"`, `"sv"` | `Err`; suggestions `[EsEs, Es419]`, `[PtBr]`, `[ZhCn, ZhTw]`, `[SvSe]` | typed error |
| Locale unknown | `"xx"`, `""`, `"-US"`, `"türkçe"`, `"nb"` | `Err`, no suggestions, Display without the `; matching codes` part | typed error, never panics |
| sibling pairs | `EnUs`, `EnGb`, `EsEs`, `Es419` | `Some(EnGb)`, `Some(EnUs)`, `Some(Es419)`, `Some(EsEs)` | none |
| sibling none | the other 28 variants | `None`; `sibling` is an involution where defined | none |
| MessageKey valid | `new("setup.step.channel")`, `"a"`, `"A-b_9"`, `"noctis.error.generic"` | constructed; `as_str()` returns the input; usable in a `const` item | none |
| MessageKey invalid | `""`, `".a"`, `"a."`, `"a..b"`, `"a b"`, `"ğ"`, `"a/b"` | panics (documented `# Panics`); in a `const` item that is a compile error | `#[should_panic]` tests |
| MessageKey framework | `is_framework()` on `"noctis.expired"`, `"noctis"`, `"noctisx.a"`, `"a.noctis"` | `true`, `true`, `false`, `false` | none |
| Translate object | a test impl used as `&dyn Translate` | object-safe; returns `Some` with `{name}` replaced in every occurrence, `None` for an unknown key | none |
| purity | `crates/noctis-rules/src/**/*.rs` and `Cargo.toml` | no `std::fs`, `std::net`, `std::io`, `std::process`, `std::thread`, `std::env`, `async fn`, `async move`, `async {`, `.await`, `tokio` or `unsafe` in the code lines of `src` (lines whose trimmed start is `//` are skipped, so docs may say "no async"); `[dependencies]` names ⊆ {`regex`} (empty now); no `[build-dependencies]`, no `[target.` tables, no `build.rs` | test fails, naming the file and token |

</intent-contract>

## Code Map

- `crates/noctis-rules/Cargo.toml` (edit): currently `[lints] workspace = true` with no dependencies. Add `[dev-dependencies] proptest.workspace = true`.
- `crates/noctis-rules/src/lib.rs` (edit): currently only the `//!` doc. Declare the modules `pub mod len; mod limits; mod locale; mod message; mod rule;` and `pub use` their public items at the root.
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs` (edit): no deps yet, `#![deny(missing_docs)]`.
- `Cargo.toml` (read-only): `noctis-rules = { path, version = "0.1.0" }` and `proptest = "1.12"` are already in `[workspace.dependencies]`. `clippy.toml` allows unwrap/expect in tests.
- `xtask/src/deps_check.rs` (read-only): the facade may depend on any of the 12 crates. After this story the ok line reads `1 internal edges`.
- `docs/discord-rules.md` (edit): §9 table (CV2-16 is the last row, l.202) and "Unconfirmed values" (l.539+). Rule rows are exactly the lines that start with `| **<ID>**` (123 today).
- Skill: `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md`. Its "Limits value" section (l.9-18) uses a struct literal that `#[non_exhaustive]` breaks, and the CV2 table has an id-less `— | media URL ≤ 2048 [U]` row. Also `testing.md:85` and `app-and-sharding.md:41` (`Locale::EnglishUs`), `troubleshooting.md:71` (`LimitError`), `i18n.md` §1 and §2, and `SKILL.md` l.72-74 (the list of imported modules).
- `examples/project-template/AGENTS.md` (edit): the managed block's "Conventions" section. Template `src/` doesn't name `Limits`, `LimitMode` or a `Locale` variant (verified by grep), so there is no code to reconcile.
- Spine AD-3 (l.145-168) and AD-16 (l.590-617): the binding rules. `.memlog.md` of the spine folder: append the decisions.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/rule.rs`: Add `RuleId(&'static str)` with a private field, deriving `Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord`. Generate the 124 `pub const <AREA>_<nn>: RuleId` items (the name is the id with `-` changed to `_`) and `pub const ALL: &[RuleId]` (catalog order) from one `macro_rules!` table. Add `as_str`, `area`, `number -> u8`, `lookup(&str) -> Option<RuleId>` (exact match), `is_structural`, and `Display`. Add `#[non_exhaustive] pub enum Bound { AtMost(usize), AtLeast(usize), Between { min: usize, max: usize }, Exactly(usize), Describe(&'static str) }` and `#[non_exhaustive] pub enum Actual { Count(usize), Text(String) }`, both with `Display`. Add `#[non_exhaustive] pub struct Violation { pub rule, pub path: String, pub limit: Bound, pub actual: Actual }` with `new(rule, path: impl Into<String>, limit, actual)`, the matrix `Display` format, and `Error`. Unit tests cover the matrix rows RuleId, structural and Violation.
- `crates/noctis-rules/src/limits.rs`: Add `LimitMode { #[default] Strict, Lenient }`. `Strict` is documented as failing before send. `Lenient` is documented as truncating text fields with `…` via `len::truncate` and warning, never for CV2-03, CV2-05, CV2-06, CID-01 or CID-02, and not affecting autocomplete, which INT-07 always truncates to 25. Add `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)] pub struct Limits { pub mode: LimitMode, pub <57 usize fields> }`, declared through one `macro_rules!` table of (field, default, rule id, doc). The table generates the fields with rustdoc that starts with the rule id, the `Default` impl and `pub(crate) const FIELDS: &[(&str, RuleId)]`. Add `pub fn truncates(&self, rule: RuleId) -> bool { mode == Lenient && !rule.is_structural() }`. The type docs state that macros check against the defaults, that lowering a field makes the runtime re-check static trees, and that raising one never relaxes a compile-time check (AD-3). They show the `let mut l = Limits::default(); l.mode = LimitMode::Lenient;` pattern. Unit tests cover the Limits rows of the matrix: assert every default by field access, and test each structural id by name.
- `crates/noctis-rules/src/len.rs`: Add `pub fn discord_len(text: &str) -> usize` (the larger of the scalar count and the UTF-16 unit count; the doc notes that this equals the UTF-16 count) and `pub fn truncate(text: &str, max: usize) -> Cow<'_, str>` (matrix semantics; `…` is U+2026, one unit; cuts on scalar boundaries and is not grapheme-aware). Add unit tests plus `proptest!` property tests for every len/truncate row.
- `crates/noctis-rules/src/locale.rs`: Add `Locale` with the 32 variants `Id, Da, De, EnGb, EnUs, EsEs, Es419, Fr, Hr, It, Lt, Hu, Nl, No, Pl, PtBr, Ro, Fi, SvSe, Vi, Tr, Cs, El, Bg, Ru, Uk, Hi, Th, ZhCn, Ja, ZhTw, Ko` in that order. It derives `Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord` and is not `#[non_exhaustive]`, with a rustdoc table of variant, code and language. Add `pub const ALL: [Locale; 32]`, `const fn as_str`, `const fn sibling`, `Display`, and `FromStr<Err = ParseLocaleError>`. `ParseLocaleError` has private `input: String` and `suggestions: Vec<Locale>`, the getters `input() -> &str` and `suggestions() -> &[Locale]`, the matrix `Display`, and `Error`. Suggestions are the codes whose language part (before `-`) ASCII-case-insensitively equals the input's part before its first `-` or `_`, in `ALL` order; an empty part gets none. Unit tests cover every Locale and sibling row.
- `crates/noctis-rules/src/message.rs`: Add `MessageKey(&'static str)`, deriving `Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord`. Add `pub const fn new` (it validates as in the matrix: one or more `.`-separated non-empty segments of ASCII `[A-Za-z0-9_-]`, and otherwise panics with a literal message), `as_str`, `is_framework` (its first segment is `noctis`, which is the reserved namespace) and `Display`. Add `pub trait Translate { fn translate(&self, locale: Locale, key: MessageKey, args: &[(&str, &dyn fmt::Display)]) -> Option<String>; }`. Its docs say implementors apply sibling then default-locale fallback (AD-16, FR-43), replace every `{name}`, and return `None` only when no fallback has the key, and that `noctis-i18n` is the implementation. Unit tests cover the MessageKey and Translate rows.
- `crates/noctis-rules/src/lib.rs`: The modules and re-exports listed above. Keep the "internal; use `noctis`" crate doc and list the contents.
- `crates/noctis-rules/tests/catalog.rs`: Read `concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/discord-rules.md")`. Collect, in order, the id between `| **` and the next `**` of each line that starts with `| **`, and assert the list equals `RuleId::ALL` (as strings). On a mismatch, print the ids that are missing or extra.
- `crates/noctis-rules/tests/purity.rs`: The matrix purity row. Walk `src/` recursively with `std::fs`, and scan `Cargo.toml` line by line by section header.
- `docs/discord-rules.md`: After CV2-16, add `| **CV2-17** Media URL length | Media Gallery item and Thumbnail media `url` ≤ **2048** characters **[U]** (2048 is documented only for Component Embeds) | research `discord-components-v2.md` §Media | Compile-time (literal) / Runtime (dynamic) | Measured with `discord_len`; the default sits in `Limits` (PRD Q4). |`. Add an Unconfirmed-values row for CV2-17. Add one sentence under "Rule IDs": "`noctis_rules::RuleId` holds exactly these ids (a test compares it with this file), and every `Limits` field names its rule id."
- `crates/noctis/Cargo.toml`, `crates/noctis/src/lib.rs`: Add the dependency, and `pub mod rules` with a module doc. It re-exports `Limits, LimitMode, RuleId, Violation, Bound, Actual, Locale, ParseLocaleError, MessageKey, Translate` and `noctis_rules::len::{discord_len, truncate}`. Add a module doctest that uses `Limits::default()` with a mutated `mode`, `"en-US".parse::<Locale>()`, `Locale::EnUs.sibling()` and `discord_len("😀") == 2`.
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md`: Rewrite "Limits value" so it is imported from `noctis::rules` and uses the mutation pattern. Add a table of every `Limits` field with its rule id and default, plus strict/lenient semantics and `truncates`. Add a "Rules types" section covering `Locale` (variants and codes, parse rules, `sibling`), `RuleId`, `Violation`/`Bound`/`Actual`, `discord_len`/`truncate`, `MessageKey` and `Translate`. Replace the id-less media URL row with `CV2-17`.
- The other skill files: change `Locale::EnglishUs` to `Locale::EnUs` (`testing.md`, `app-and-sharding.md`). In `troubleshooting.md`, change `LimitError {…}` to a `Violation { rule, path, limit, actual }` (prints `[RULE-ID] path: limit …, actual …`). In `i18n.md` §1, add that `Locale` variants are code-named (`Locale::EnUs`, `Locale::Es419`), that a bare `en` does not parse, and that `MessageKey` and `Translate` are in `noctis::rules`. In `SKILL.md` l.72-74, add `noctis::rules` to the module list.
- `examples/project-template/AGENTS.md`: In the "Conventions" section of the managed block, add: "Limits and Locales come from `noctis::rules` (`Locale::EnUs` = `en-US`); override limits on the App builder with `Limits`, never with hard-coded numbers."
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md`: Append the Design Notes decisions as `(decision) Story 1.3 planning: …` lines (the planner writes these).

**Acceptance Criteria:**
- Given `noctis-rules`, when `cargo test --release -p noctis-rules` runs, then the unit, property, catalog and purity tests pass, and `cargo tree -p noctis-rules -e normal --depth 1` (cargo tree builds nothing, so it takes no `--release`) lists no dependency.
- Given a bot crate depending only on `noctis`, when it writes `use noctis::rules::{Limits, LimitMode, Locale};`, then it compiles and `Limits::default().mode == LimitMode::Strict` (the facade doctest).
- Given `cargo run --release -p xtask -- deps-check`, when it runs after the change, then it exits 0 with `1 internal edges`.
- Given the catalog gains CV2-17, when `tests/catalog.rs` runs, then `RuleId::ALL` (124 ids) matches it. Removing a row from the doc makes the test fail and name the id (check this manually, then revert).
- Given the Usage Skill, when grepped, then `EnglishUs`, `LimitError` and the struct-literal `Limits {` form no longer appear, and every facade `rules` item is named in `limits-cheatsheet.md`.

## Spec Change Log

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 31 findings — high 0, medium 9, low 9, false 13, maybe-false 0
- findings:
  - `[medium]` `[patch]` (blind) `Limits::truncates` returns `true` in Lenient for count, byte and pattern rules (CV2-10, CV2-07, MSG-04, ATT-01, REG-01, CMD-03) — verified: `truncates` is `Lenient && !is_structural()` (limits.rs:143) while its doc says "whether text over the limit of `rule` is truncated"; group G1 (rule-level predicate over-claims). Patch: narrow the `truncates`/`Lenient`/cheatsheet docs to "mode permits truncation for this rule; only display-text length fields are truncated, everything else fails".
  - `[medium]` `[patch]` (blind) INT-07 is documented as "always truncated, in every mode" but `truncates(INT_07)` is `false` in Strict — verified (limits.rs:17, :95, :143; `strict_never_truncates`); the matrix pins `false` for every id, so group G2 patches the docs to say `truncates` does not govern INT-07.
  - `[false]` `[reject]` (blind) raised overrides are accepted without clamping or validation — raising a field above today's Discord value is the designed escape hatch (FR-24: "a Discord limit change needs no Noctis release"); clamping would defeat it, and a `0` override fails loudly, which is the caller's explicit choice.
  - `[medium]` `[patch]` (blind) `defaults_match_the_catalog` compares against a copy in the same test and labels by `FIELDS[i]` order; the cheatsheet's 57-row table is unchecked — verified (limits.rs:151-218, no test reads the cheatsheet); group G3 with the verification-gap finding. Patch: unit test that parses the cheatsheet `Limits` table against `FIELDS` and `Limits::default()`.
  - `[low]` `[patch]` (blind) purity test skips `[dependencies.foo]` table headers (and substring scan misses grouped imports) — `[dependencies.` part verified at purity.rs `manifest_has_only_allowed_dependencies` (only `section == "[dependencies]"` is inspected); group G4, direct one-condition fix. Grouped-import part is triaged in the edge/vgap rows below.
  - `[medium]` `[patch]` (blind) `Translate` has no `Send + Sync` supertraits — verified (message.rs:99); widgets take `&dyn Translate` inside async handlers on multi-threaded tokio, so a borrow held across `.await` needs `Sync`, and adding it later is a breaking change; group G5, one-line supertrait plus the cheatsheet row.
  - `[false]` `[reject]` (blind) `Translate` fallback chain disagrees with `i18n.md` (Guild Locale step) — the trait handles key fallback (locale → sibling → default, AD-16), while `i18n.md` §1/§7 describe locale resolution (User → Guild → default, FR-43) done by the caller/`LocaleResolver` before `translate` is called; the implementor (`noctis-i18n`) knows its compiled default.
  - `[false]` `[reject]` (blind) `MessageKey` has no fallible/owned constructor for runtime keys — AD-16 generates a `MessageKey` const per key at compile time; no design path builds keys at runtime, and `new` documents `# Panics` as the spec decided.
  - `[false]` `[reject]` (blind) `Locale` is not `#[non_exhaustive]` and `ALL` is `[Locale; 32]` — deliberately settled: AD-16 and LOC-01 define a closed 32-code enum, and the spec's Design Notes log the decision.
  - `[false]` `[reject]` (blind) locale suggestions inconsistent for whitespace and `nb` — the matrix pins exactly this behavior (`" tr"` → no suggestions, `"nb"` → none, no trimming), and the Never list forbids trimmed parsing.
  - `[low]` `[reject]` (blind) `Actual::Text` embeds the whole offending text in `Display` — real but unlikely: length rules report `Actual::Count`; `Text` is for pattern rules on short names, and the fix adds truncation logic to `Display`.
  - `[false]` `[reject]` (blind) no field-to-rule reverse mapping, `FIELDS` is `pub(crate)` — the promise "every field names its rule id" is met by each field's rustdoc and the cheatsheet table; no current caller needs the reverse lookup, and the validator's `Violation.path` names the field.
  - `[low]` `[reject]` (blind) duplicated id-shape helpers, non-const `area`, alphabetical `Ord` — `has_id_shape` is test-only and integration tests cannot share it without new public surface; nothing sorts violations today.
  - `[false]` `[reject]` (blind) `ALLOWED_DEPENDENCIES` pre-approves unused `regex` — the matrix purity row prescribes `[dependencies]` ⊆ {`regex`} with Story 1.4 adding it.
  - `[medium]` `[patch]` (edge) `truncates()` true for count/byte rules ATT-01/03, CV2-07/10, REG-01 — same as G1 (limits.rs:143); docs patch above.
  - `[medium]` `[patch]` (edge) Lenient on `link_url_len`, `media_url_len`, `choice_value_len`, `select_option_value_len` would truncate URLs/values that share ids with label fields — verified: CV2-08/CV2-09/OPT-09 cover both text and URL/value fields, so a rule-level predicate cannot exclude them; group G1, the docs patch states URLs, values and ids are never truncated.
  - `[medium]` `[patch]` (edge) `truncates(INT_07)` false in Strict despite "always truncates" — same as G2.
  - `[false]` `[reject]` (edge) runtime raise above Discord's value passes payloads Discord rejects — same refutation as the blind raised-overrides row (FR-24 intends raising).
  - `[low]` `[patch]` (edge) `ParseLocaleError` `Display` prints `input` unescaped (newlines/backticks garble the message) — verified (locale.rs:157), unlike `Actual::Text`; group G6, direct `escape_debug()` fix.
  - `[low]` `[reject]` (edge) grouped import `use std::{fs, io}` evades the purity token scan — real, but nobody adds I/O to this crate by accident in that form, and a `std::{` token would false-flag legitimate grouped std imports, so the fix adds parsing complexity.
  - `[low]` `[patch]` (edge) `[dependencies.foo]` header passes the manifest check — same as G4.
  - `[medium]` `[patch]` (vgap) `Limits` defaults and field rule ids are pinned only against in-test copies, not the cheatsheet bot authors read — pre-verified gap (no test reads `limits-cheatsheet.md`); group G3.
  - `[low]` `[patch]` (vgap) `[dependencies.tokio]` dotted table skips the dependency check — same as G4.
  - `[low]` `[reject]` (vgap) grouped `use std::{fs, io};` not flagged — same reasoning as the edge grouped-import row.
  - `[medium]` `[patch]` (vgap) `truncates(INT_07)` false in Strict while docs say INT-07 always truncates — same as G2.
  - `[false]` `[reject]` (intent D1) `regex` not a dependency yet — the spec reads "depends only on" as a ceiling, consistent with Story 1.4 adding `regex`; deliberately and correctly settled.
  - `[false]` `[reject]` (intent D2) not every catalog numeric is a `Limits` field (MSG-02, CID-03, minima) — the Design Notes log the exclusions (minima, codec widths, documentation-only MSG-02), consistent with AD-3 "configurable maxima".
  - `[false]` `[reject]` (intent D3) Lenient truncation not wired end to end — the epics AC asks only that Lenient is documented; truncation is Story 4.3's validator (epics l.1219).
  - `[low]` `[patch]` (intent D4) facade `rules` module doc says Limits are "configurable on the App builder" with no `[planned]` marker — verified (crates/noctis/src/lib.rs:25); no App builder hook exists yet; group G7, direct doc correction.
  - `[false]` `[reject]` (intent D5) template code reconciled only by grep — template `src/` names none of the types (re-verified), so there is no code to reconcile.
  - `[false]` `[reject]` (intent D6) CV2-17 added beyond the named ids — required by the AGENTS.md rule that a new limit lands with its catalog row; deliberately and correctly settled.

## Design Notes

**Limits fields** (all `usize`; byte sizes fit 32-bit `usize`): `command_name_len` 32 CMD-02 · `command_description_len` 100 CMD-04 · `command_size` 8000 CMD-05 · `command_path_words` 3 CMD-10 · `command_options` 25 CMD-13 · `pattern_expansions` 25 CMD-15 · `context_menu_name_len` 32 CTX-01 · `chat_input_commands` 100 REG-01 · `user_commands` 15 REG-01 · `message_commands` 15 REG-01 · `entry_point_commands` 1 REG-01 · `option_length` 6000 OPT-06 · `option_choices` 25 OPT-08 · `choice_name_len` 100 OPT-09 · `choice_value_len` 100 OPT-09 · `option_file_types` 10 OPT-13 · `autocomplete_choices` 25 INT-07 · `user_install_followups` 5 INT-11 · `custom_id_len` 100 CID-01 · `components` 40 CV2-03 · `text_total_len` 4000 CV2-04 · `row_buttons` 5 CV2-06 · `section_texts` 3 CV2-07 · `button_label_len` 80 CV2-08 · `link_url_len` 512 CV2-08 · `select_placeholder_len` 150 CV2-09 · `select_options` 25 CV2-09 · `select_values` 25 CV2-09 · `select_option_label_len` 100 CV2-09 · `select_option_value_len` 100 CV2-09 · `select_option_description_len` 100 CV2-09 · `gallery_items` 10 CV2-10 · `gallery_description_len` 1024 CV2-10 · `thumbnail_description_len` 1024 CV2-11 · `legacy_action_rows` 5 CV2-15 · `media_url_len` 2048 CV2-17 · `modal_title_len` 45 MOD-01 · `modal_components` 5 MOD-02 · `label_len` 45 MOD-04 · `label_description_len` 100 MOD-04 · `text_input_len` 4000 MOD-05 · `text_input_placeholder_len` 100 MOD-05 · `file_upload_values` 10 MOD-06 · `file_upload_file_types` 10 MOD-06 · `radio_options` 10 MOD-07 · `checkbox_group_options` 10 MOD-07 · `checkbox_group_values` 10 MOD-07 · `content_len` 2000 MSG-01 · `mention_users` 100 MSG-04 · `mention_roles` 100 MSG-04 · `nonce_len` 25 MSG-06 · `sticker_ids` 3 MSG-06 · `attachment_bytes` 20971520 ATT-01 · `request_bytes` 26214400 ATT-02 · `attachments` 10 ATT-03 · `attachment_description_len` 1024 ATT-06 · `webhook_name_len` 80 WH-04. REG-01 counts also serve REG-02, and `custom_id_len` also serves MOD-08.

**Deliberately not in `Limits`** (logged decision): the fields hold maxima only, and fixed minima (0, 1, 2) belong to each rule's shape in Stories 1.4 to 1.6. Also left out: protocol constants and value domains (CV2-01 flag, OPT-02/CTX-03 type codes, OPT-03/04 JSON number ranges, CV2-13 spacing/RGB, CV2-14 id width); timings (INT-01/02, owned by the core auto-defer config); rate limits, quotas and gateway values enforced by twilight or reported by Discord (RL-*, GW-*, REG-05, INT-13); codec layout widths (CID-03/05, AD-5 constants in `noctis-codec`); documentation-only rows (MSG-02, REG-06).

**Other decisions:** `RuleId` is a newtype with generated consts, not a 124-variant enum: the consts read like the catalog, and rules never need exhaustive matches. `Limits` is `#[non_exhaustive]` with pub fields, so it grows without breaking callers, and the skill shows mutation instead of a struct literal. `Locale` variants are named after their codes (`EnUs`, `Es419`), mirroring the file stems, and it is not `#[non_exhaustive]` (LOC-01 is a closed set). `Violation` is a struct, as the epics name it, not a `<Area>Error` enum. `MessageKey::new` validates in a `const fn`, so a bad key in an `i18n!`-generated const fails at compile time. CV2-17 is a new catalog id for the [U] media URL default (spine open question Q1/Q4).

**Story 1.7 note:** the `RuleId` const table cites every id, so `rules-catalog` must count citations in tests and fixtures only, never in `src/rule.rs`. The structural-id tests here cite CV2-03 and the other structural ids for the Lenient policy only, not for their Runtime rows.

```rust
let mut limits = noctis::rules::Limits::default();
limits.mode = noctis::rules::LimitMode::Lenient;
assert!(!limits.truncates(noctis::rules::RuleId::CV2_03));
assert!(limits.truncates(noctis::rules::RuleId::CV2_04));
```

## Verification

**Commands** (repo root):
- `cargo fmt --all --check`: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: exit 0.
- `cargo test --release --workspace`: exit 0, including the noctis-rules unit, property, catalog and purity tests and the facade doctest.
- `cargo hack test --release --workspace --each-feature`: exit 0.
- `cargo run --release -p xtask -- deps-check`: exit 0, `1 internal edges`.
- `cargo tree -p noctis-rules -e normal --depth 1`: only `noctis-rules` itself.
- `cargo deny check`: exit 0 (proptest's tree is MIT/Apache).
- `cargo +1.91 check --release --workspace`: exit 0.
- `test ! -d target/debug`: true.
- `grep -rn "EnglishUs\|LimitError\|Limits {" examples/project-template/.claude/skills/noctis`: no matches.

CI is paused (workflows live in `.github/workflows-disabled/`), so the local gates above are the whole check, and this story adds no gate.

**Manual checks:**
- Delete the CV2-17 row temporarily: `cargo test --release -p noctis-rules --test catalog` fails and names `CV2-17`. Revert with `git checkout docs/discord-rules.md`.

## Auto Run Result

- **Summary:** `noctis-rules` foundations: `RuleId` (124 catalog ids incl. new CV2-17), `Bound`/`Actual`/`Violation`, `Limits` (57 `#[non_exhaustive]` maxima from one macro table + `LimitMode`), `discord_len`/`truncate`, `Locale` (32 Discord codes, `FromStr` with `ParseLocaleError`), `MessageKey` and `Translate: Send + Sync`; facade `noctis::rules` re-exports; Usage Skill limits cheatsheet rewritten.
- **Files changed:** `crates/noctis-rules/src/{lib,rule,limits,len,locale,message}.rs`, `crates/noctis-rules/tests/{catalog,purity}.rs`, `crates/noctis-rules/Cargo.toml`, `crates/noctis/{Cargo.toml,src/lib.rs}`, `docs/discord-rules.md` (CV2-17), template Usage Skill references and `AGENTS.md`.
- **Review findings:** 31 (medium 9, low 9, false 13). Patched 14 rows in 7 fixes (4 medium: Lenient/truncates docs scope, INT-07 note, cheatsheet-vs-FIELDS test, `Translate: Send + Sync`; 3 low: `[dependencies.x]` purity check, `escape_debug` in parse error, facade doc). Rejected 17 (see triage log). Deferred 0.
- **Follow-up review recommendation:** true by the rule (4 medium patched); named risk: the new cheatsheet-parsing test and the `Send + Sync` supertrait were added in the patch round without a second review. Orchestrator policy runs follow-up passes only after a patched high, so none is run.
- **Verification:** all local gates green (fmt, clippy `-D warnings`, workspace tests, `cargo hack --each-feature`, MSRV 1.91, `cargo deny`, build-config script, deps-check `1 internal edges`), no `target/debug/`.
- **Residual risks:** `FIELDS` is test-only until Story 1.7 consumes it.
