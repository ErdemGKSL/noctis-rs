---
title: 'Story 1.5: Option, context-menu and command metadata rules'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: '01bce9bdef7bd7f77102efa0cf526b9e533f639b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-rules` has no shared functions for option lists, choices, context-menu names or command metadata, so `derive(Options)` (Story 6.2), the metadata and context-menu macros (Story 6.3) and the App-build merge (Story 6.4) would each re-implement OPT-01..13, CMD-06, CTX-01 and META-01..03 and could disagree (FR-10, FR-11, FR-12, AD-3). Story 1.4 also left the CMD-05 options measurer to this story.

**Approach:** Extend `noctis_rules::command` with option descriptor types plus `validate_options` and `measure_options`, `validate_context_menu_name` (CTX-01), metadata types plus `validate_meta` (META-03), and per-base metadata conflict checks inside the existing `validate_tree` (META-01, META-02, CMD-07), with `validate_merge` also running META-03 and CTX-01. By-construction rules (OPT-02, OPT-14, CTX-02, CTX-03, META-05) are shown by closed types and `compile_fail` doctests. The facade already re-exports `noctis::rules::command`; the Usage Skill and the template `AGENTS.md` document the new items.

## Boundaries & Constraints

**Always:**
- `noctis-rules` stays sync, no I/O, no `async`, no `unsafe`, and its only `[dependencies]` entry stays `regex`. No new crates anywhere; `Cargo.lock` unchanged.
- Every new public function takes `&Limits` (AD-3, even where it reads no field) and reads only `command_name_len`, `command_description_len`, `command_options`, `option_length`, `option_choices`, `choice_name_len`, `choice_value_len`, `option_file_types`, `context_menu_name_len`. Nothing in `src/` calls `Limits::default()` outside `#[cfg(test)]` and the existing `Default` impl.
- Every text length is `len::discord_len`. Errors are the existing `Violation`. `Result<_, Vec<Violation>>` never carries an empty `Err`; violation order is deterministic as specified below. Every unit test name cites a rule id in snake case (`opt_06_…`, `meta_01_…`, `ctx_01_…`).
- Existing Story 1.4 behavior and its tests stay unchanged except where this spec extends `CommandDecl`, `ContextMenuDecl`, `validate_tree` and `validate_merge`.
- The public API change updates the Usage Skill and the template `AGENTS.md` in the same commit. No `RuleId` is added (count stays 125).

**Never:**
- No LOC-02 checks of localized option names (only counted by `measure_options`; Story 11.4), no LOC-03, no OPT-12 code (Story 6.7: `Partial<T>`), no META-04 (Publish, Story 7.x), no INT-07. No proc-macro, `BuildError`, Registry, twilight types or Publish code. No new CI gate; no edits to `.github/workflows-disabled/`, `xtask/`, `_bmad/` or the template's `src/`.
- No `thiserror`, `serde`, `bitflags`, `once_cell`, `build.rs` or prelude. No duplicate-choice, duplicate-`file_types` or `channel_types`-value checks (no catalog row).

## I/O & Edge-Case Matrix

Notation: `L` = `Limits::default()`. `I53` = 9007199254740991 (2^53−1), `N53` = 9007199254740992 (2^53). `o(n, K)` = `OptionDecl::new(n, OptionType::K, "d")`. `P` = the option path: its name, or `options[i]` (0-based input index) when the name is empty. "Err[X, Y]" = exactly these rule ids in this order. Actual `T(x)` = `Actual::Text(x)`, `C(n)` = `Actual::Count(n)`. Type names print as Discord spells them (`STRING`, `INTEGER`, `BOOLEAN`, `USER`, `CHANNEL`, `ROLE`, `MENTIONABLE`, `NUMBER`, `ATTACHMENT`). Numbers print with Rust `Display` (`1.0` → `1`, NaN → `NaN`, ∞ → `inf`, −∞ → `-inf`).

**`validate_options(&[OptionDecl], &L) -> Result<(), Vec<Violation>>`**

| Scenario | Input | Expected |
|---|---|---|
| empty | `&[]` | `Ok` |
| AC valid | `o("member",User).required(true)`, `o("days",Integer).min_value(Int(1)).max_value(Int(30))`, `o("reason",String).min_length(3).max_length(200)`, `o("log_to",Channel).channel_types(&[0,5])`, `o("severity",String).choices(&[Choice::new("Low",Str("low"))])`, `o("rule",String).autocomplete(true)`, `o("evidence",Attachment).file_types(&["image",".pdf"])` | `Ok` |
| CMD-13 | 25 options / 26 / 2 with `command_options = 1` | ok / first violation CMD-13 path `options`, `AtMost(25)`, `C(26)` / `AtMost(1)` `C(2)` |
| names | `"süre"`, `"a-b_c"`, `"設定"` ok; `"Days"`; `"it's"`; 33×`a`; `""` | ok; CMD-03 path `Days` help ``use `days` ``; CMD-01 path `it's` (note and help as `validate_name`); CMD-02 `C(33)`; CMD-02 `C(0)` path `options[0]` |
| CMD-04 | description `""`; 100×`x`; 101×`x`; 50×`😀`; 51×`😀`; `.description_localized(&["", 101×x])` | CMD-04 `Between{1,100}` `C(0)` path `P`, note `in the default description`; ok; `C(101)`; ok; `C(102)`; two CMD-04 with notes `in localized description #0`, `#1` |
| OPT-03 bounds | INTEGER `min_value(Int(-I53))`, `max_value(Int(I53))`; `Int(I53+1)`; `Int(-I53-1)`; `Int(i64::MIN)` | ok; OPT-03 path `P.max_value` (or `.min_value`), `Describe("between -9007199254740991 and 9007199254740991")`, `T("9007199254740992")`; `T("-9007199254740992")`; `T("-9223372036854775808")` |
| OPT-04 bounds | NUMBER `Float(N53 as f64)`, `Float(-N53 as f64)`, `Int(N53)`, `Float(-0.0)`, `Float(0.5)`; `Float(9007199254740994.0)`; `Int(N53+1)`; `Float(NAN)`; `Float(INFINITY)`; `Float(NEG_INFINITY)` | ok; OPT-04 `Describe("a finite number between -9007199254740992 and 9007199254740992")`, `T("9007199254740994")`; `T("9007199254740993")`; `T("NaN")`; `T("inf")`; `T("-inf")` |
| OPT-05 misuse | STRING `min_value(Int(1))`; BOOLEAN with both bounds | OPT-05 path `P.min_value`, `Describe("min_value / max_value only on INTEGER or NUMBER options")`, `T("STRING")`; two (min, then max), no ordering check |
| OPT-05 integer bound | INTEGER `min_value(Float(1.0))` | OPT-05 path `P.min_value`, `Describe("an integer bound on an INTEGER option")`, `T("1")` |
| OPT-05 order | INTEGER min `Int(5)` max `Int(5)`; min 6 max 5; NUMBER min `Float(1.5)` max `Int(1)`; NUMBER min `Int(1)` max `Float(1.0)`; INTEGER min `Int(I53+1)` max `Int(0)` | ok; OPT-05 path `P`, `Describe("min_value ≤ max_value")`, `T("6 > 5")`; `T("1.5 > 1")`; ok; only OPT-03 (ordering checked only when both bounds passed their own checks) |
| OPT-06 | STRING `min_length` 0 / 6000 / 6001; `max_length` 1 / 6000 / 0 / 6001; min 5 max 5; min 10 max 5; min 7000 max 6500; `option_length = 100` and `max_length(101)`; INTEGER `min_length(1)` | ok / ok / OPT-06 path `P.min_length` `Between{0,6000}` `C(6001)`; ok / ok / path `P.max_length` `Between{1,6000}` `C(0)` / `C(6001)`; ok; OPT-06 path `P` `Describe("min_length ≤ max_length")` `T("10 > 5")`; two range violations, no ordering one; `Between{1,100}` `C(101)`; OPT-06 path `P.min_length` `Describe("min_length / max_length only on STRING options")` `T("INTEGER")` |
| OPT-07 | CHANNEL `&[0,5]`; STRING `&[0]`; STRING `&[]` | ok; OPT-07 path `P.channel_types` `Describe("channel_types only on CHANNEL options")` `T("STRING")`; ok (empty = unset) |
| OPT-08 | STRING 25 choices; 26; `option_choices = 2` with 3; BOOLEAN with `[Choice::new("Yes", Int(1))]` | ok; OPT-08 path `P.choices` `AtMost(25)` `C(26)`; `AtMost(2)` `C(3)`; OPT-08 `Describe("choices only on STRING, INTEGER or NUMBER options")` `T("BOOLEAN")` (choice names still checked, values not) |
| OPT-09 name | choice name `""`; 100×`a`; 101×`a`; 50×`😀`; 51×`😀`; `.name_localized(&["", 101×a])` | OPT-09 path `P.choices[0]` `Between{1,100}` `C(0)` note `in the choice name`; ok; `C(101)`; ok; `C(102)`; two with notes `in localized choice name #0`, `#1` |
| OPT-09 value | STRING `Str("")`; `Str(100×a)`; `Str(101×a)`; `Str(51×😀)` | ok; ok; OPT-09 `AtMost(100)` `C(101)` note `in the choice value`; `C(102)` |
| OPT-09 type | STRING `Int(1)`; INTEGER `Str("a")`; INTEGER `Float(1.0)`; NUMBER `Int(1)`; NUMBER `Str("a")` | OPT-09 `Describe("a choice value of the option's type")` `T("integer")` note `option type STRING`; `T("string")` note `option type INTEGER`; `T("number")`; ok; `T("string")` note `option type NUMBER` |
| choice ranges | INTEGER `Int(I53+1)`; NUMBER `Float(NAN)`; INTEGER min 1 max 10 with choices 1, 10, 0, 11; only `min_value(Int(1))` with choice 0 | OPT-03 path `P.choices[0]`; OPT-04; two OPT-05 path `P.choices[2]`/`[3]` `Describe("a choice value within min_value and max_value")` `T("0")`/`T("11")`; one OPT-05 |
| OPT-10 | STRING with 1 choice + `autocomplete(true)`; autocomplete without choices | OPT-10 path `P` `Describe("choices or autocomplete, not both")` `T(name)`; ok |
| OPT-11 | autocomplete on STRING/INTEGER/NUMBER; on BOOLEAN, USER, CHANNEL, ROLE, MENTIONABLE, ATTACHMENT | ok; OPT-11 path `P.autocomplete` `Describe("autocomplete only on STRING, INTEGER or NUMBER options")` `T(type)` |
| combo | BOOLEAN + 1 choice + autocomplete | Err[OPT-08, OPT-10, OPT-11] |
| OPT-13 | ATTACHMENT `["image","video","audio",".pdf",".PNG",".7z"]`; 10 entries; 11; `option_file_types = 1` with 2; entries `"Image"`, `"pdf"`, `"."`, `".tar.gz"`, `". pdf"`, `""`, `".pdf "`, `".pdf\u{301}"`, `"*"`; STRING `["image"]` | ok; ok; OPT-13 path `P.file_types` `AtMost(10)` `C(11)`; `AtMost(1)` `C(2)`; one OPT-13 each, path `P.file_types[k]`, ``Describe("`image`, `video`, `audio` or a dot-prefixed extension")``, `T(entry)`; OPT-13 path `P.file_types` `Describe("file_types only on ATTACHMENT options")` `T("STRING")` (entries still checked) |
| OPT-01 | `[a req, b opt, c req, d req]`; `[opt, opt]`; `[req, req]` | Err[OPT-01, OPT-01] paths `c`, `d`, `Describe("required options before optional ones")`, `T("c")`/`T("d")`, note ``after optional `b` ``; ok; ok |
| CMD-06 | names `[a, b, a, a, b]`; `["a","A"]`; `["",""]` | Err[CMD-06, CMD-06]: path `a` `Describe("unique option names")` `T("a")` note `declared at positions 0, 2, 3`; then `b` `1, 4`; only CMD-03 for `A`; CMD-02 ×2 then CMD-06 path `""` |
| all at once | 26 options: #0 `o("Bad",String).min_value(Int(1))`, #1 `o("b",String)` (optional), #2 `o("c",String).required(true)`, #3 `o("c",String).required(true)`, #4..#25 optional with distinct valid names | one `Err`: CMD-13, CMD-03 (`Bad`), OPT-05 (`Bad.min_value`), OPT-01 (`c`), OPT-01 (`c`), CMD-06 (`c`, positions 2, 3) |

Order: CMD-13; then per option in input order: name rules (`validate_name` order), CMD-04 (default, then localized), `min_value` then `max_value` (OPT-05 misuse, else OPT-05 integer bound, else OPT-03/OPT-04 range), OPT-05 ordering, `min_length` then `max_length` (OPT-06 misuse, else range), OPT-06 ordering, OPT-07, OPT-08 (type, then count), per choice (OPT-09 name, localized names, then — only for STRING/INTEGER/NUMBER — OPT-09 type, else OPT-09 value length / OPT-03 / OPT-04, then OPT-05 against each valid bound), OPT-10, OPT-11, OPT-13 (type, count, entries); then OPT-01; then CMD-06 (first-occurrence order).

**`measure_options(&[OptionDecl], &L) -> usize`** (CMD-05 share; saturating sum)

| Input | Expected |
|---|---|
| `&[]` | `0` |
| `o("days",Integer)` | `5` (4 + 1) |
| plus `.name_localized(&["tage","günlerin"])` | `9` (longest name 8 + 1) |
| `o("s",String)` with choices `("Low",Str("low"))`, `("x",Int(-12))`, `("y",Float(1.5))`, `("z",Float(2.0))`, `Choice::new("a",Str("b")).name_localized(&["😀😀"])` | `2 + 6 + 4 + 4 + 2 + 5 = 23` (choice = longest name + value; numbers by their `Display` length) |
| description with `description_localized(&[100×x])` | counts 100 for the description |

**`validate_context_menu_name(name, &L) -> Vec<Violation>`** (path `""`)

| Input | Expected |
|---|---|
| `"Show Profile"`, `"show profile"`, `"A"`, 32×`a`, `"プロフィールを表示"`, `"Report 🚩"`, `"It's"` | `[]` |
| 33×`a`; `""`; 16×`𠀀`; 17×`𠀀`; 11×`a` with `context_menu_name_len = 10` | CTX-01 `Between{1,32}` `C(33)`; `C(0)`; `[]`; `C(34)`; `Between{1,10}` `C(11)` |

**`validate_meta(&CommandMeta, Scope, &L) -> Vec<Violation>`** (META-03)

| Input | Expected |
|---|---|
| `Scope::Global` with every field set | `[]` |
| `Guild(42)` + `contexts(&[Guild])` | META-03 path `contexts`, `Describe("only on global commands")`, `T("[Guild]")`, note `scope: guild 42` |
| `Guild(42)` + `integration_types(&[UserInstall])`; both set; `contexts(&[])`; `contexts(&[PrivateChannel, Guild, Guild])` | path `integration_types` `T("[UserInstall]")`; [contexts, integration_types]; `T("[]")`; `T("[Guild, PrivateChannel]")` (sorted by Discord value, deduplicated) |
| `Guild(42)` + only `default_member_permissions(8)` and `nsfw(true)` | `[]` |

**`validate_tree` metadata checks** (`h(src,path)` = `CommandDecl::handler(src,path).description(LocalizedText::new("d"))`, `m` = `CommandDecl::meta`; `.metadata(CommandMeta::new().…)`)

| Scenario | Input | Expected |
|---|---|---|
| agree | `h("m1","config set")` and `h("m2","config get")` both `default_member_permissions(8)` | `Ok` |
| differ | 8 vs 32 | Err[META-01] path `config`, `Describe("one default_member_permissions per base command")`, `T("config")`, notes ``"`config set` sets `8` (from `m1`)"``, ``"`config get` sets `32` (from `m2`)"``, no help |
| set vs unset | 8 vs unset | META-01, second note ``"`config get` leaves it unset (from `m2`)"``, help ``set it on every command under `config`, or once on `config` with a metadata declaration`` |
| explicit base | `m("m0","config").metadata(perm 8)` + `h set` unset + `h get` perm 8; then `h get` perm 32 instead; two explicit 8 / 32 | `Ok`; META-01 notes `` `config` sets `8` (from `m0`) ``, `` `config get` sets `32` (from `m2`) ``, no help; META-01 |
| contexts sets | `[Guild, BotDm]` vs `[BotDm, Guild, Guild]`; `[Guild]` vs `[Guild, BotDm]`; `Some(&[])` vs unset | `Ok`; META-02 `Describe("one contexts value per base command")`, values `` `[Guild]` ``, `` `[Guild, BotDm]` ``; META-02 (`` sets `[]` `` / leaves it unset) |
| integration types | `[GuildInstall]` vs `[UserInstall]` | META-02 `Describe("one integration_types value per base command")` |
| nsfw | `true` vs `false`; `false` vs unset | CMD-07 `Describe("one nsfw value per base command")` (both cases) |
| misplaced | `m("m1","config set").metadata(nsfw(true))` with a handler at `config set`; same meta decl with default metadata and a description | Err[META-02] path `config set`, `Describe("metadata only on the base command")`, `T("config set")`, note ``declared by `m1` ``, help ``declare it on `config` ``; `Ok` |
| bases independent | `h config` perm 8, `h ping` perm 32; single `h("m","ping")` with all fields | `Ok`; `Ok` |
| all fields | every field conflicting on one base | Err[META-01, META-02 (contexts), META-02 (integration types), CMD-07] |
| ignored decls | a CMD-10-dropped decl, an orphan meta decl, an empty path | never take part in metadata checks |
| order | two bases both conflicting plus a REG-04 description conflict | REG-04 description conflicts first, then per base in first-appearance order: misplaced META-02 (input order), then META-01, META-02 contexts, META-02 integration types, CMD-07 |

**`validate_merge` additions**

| Scenario | Input | Expected |
|---|---|---|
| scopes separate | `h("m1","config")` perm 8 global, `h("m2","config")` perm 32 in `Guild(1)` | `Ok` |
| same scope | both global | Err[META-01], last note `scope: global` |
| META-03 command | `h("m1","ping").scope(Guild(42))` + `contexts(&[Guild])`; source `""` | Err[META-03] path `ping.contexts`, notes ``declared by `m1` ``, `scope: guild 42`; first note `declared by (unnamed)` |
| META-03 menu | `ContextMenuDecl::new("m1",User,"Show Profile").scope(Guild(42)).metadata(integration_types(&[GuildInstall]))` | Err[META-03] path `Show Profile.integration_types` |
| CTX-01 | menu named 33×`a`; `""`; `"Show Profile"` | CTX-01 path = name, `Between{1,32}`, `C(33)`, notes ``declared by `m1` ``, `scope: global`; `C(0)` path `""`; ok |
| order | per scope | `validate_tree` violations, META-03 for commands (input order), META-03 for menus, CTX-01 (input order), counts, REG-03 |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/command/mod.rs` (edit): module docs (rule list l.1-2, Trees section l.37-56: extend violation order and add Options / Context menus / Metadata sections); `CommandDecl` l.124-202 gains `pub metadata: CommandMeta<'a>` (default in `handler`, so `meta` inherits) and `#[must_use] const fn metadata(self, CommandMeta<'a>)`; `ContextMenuDecl` l.213-242 gains the same field and setter; `from_suffix` / `source_name` l.244-260 (reuse for notes). Add `mod option; mod meta;` and re-exports.
- `crates/noctis-rules/src/command/name.rs` (edit): add `validate_context_menu_name` next to `validate_name` (reused by `validate_options` for option names).
- `crates/noctis-rules/src/command/tree.rs` (edit): `validate_tree` l.253-264 — after `check_descriptions`, run the metadata pass over the decls attached to nodes (the `valid` indices minus orphans; `Tree::build` l.301-415 knows both). Update its doc l.220-252.
- `crates/noctis-rules/src/command/merge.rs` (edit): `validate_merge` l.44-130 — after the tree block (l.67-69) add META-03 (commands, then menus) and CTX-01; doc l.16-43.
- `crates/noctis-rules/src/len.rs` (reuse `discord_len`); `src/limits.rs` (read-only; fields l.80-97); `src/rule.rs` (read-only: `Violation`, `Bound::Describe(&'static str)`, `Actual`, ids `OPT_01..OPT_14`, `CTX_*`, `META_*`, `CMD_06`, `CMD_07` already exist).
- `crates/noctis-rules/tests/command.rs` (pattern to copy for the new integration test); `tests/catalog.rs` (unchanged, 125); `tests/purity.rs` (read-only; forbids I/O tokens, allows only `regex`).
- `crates/noctis/src/lib.rs` (edit): `pub mod rules` doc list l.35-38 and one more doctest; the `pub use noctis_rules::command;` already exposes everything.
- `docs/discord-rules.md` (edit): "Unconfirmed values" table (l.543-556).
- Skill: `examples/project-template/.claude/skills/noctis/references/commands.md` (§1 metadata paragraph l.55-60, §3 Options l.176-245, §5 l.271-285, §6 l.287-294), `limits-cheatsheet.md` (Rules types l.100-104, Commands/Options tables l.127-170), `troubleshooting.md` (compile-error table l.10-35). `examples/project-template/AGENTS.md` Conventions l.37. Template `src/` uses none of these items.
- Previous story (1.4, done): types and order conventions above; its rejected-findings list (LOC-02, CTX-01, META conflicts deferred to 1.5) is resolved here except LOC-02.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/command/meta.rs` (new) -- `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)] pub enum InteractionContext { Guild, BotDm, PrivateChannel }` and `pub enum IntegrationType { GuildInstall, UserInstall }`, each with `const fn value(self) -> u8` (0,1,2 / 0,1) and `const fn name(self) -> &'static str` (variant name); `#[non_exhaustive] #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct CommandMeta<'a> { pub default_member_permissions: Option<u64>, pub contexts: Option<&'a [InteractionContext]>, pub integration_types: Option<&'a [IntegrationType]>, pub nsfw: Option<bool> }` with `const fn new()` and by-value `#[must_use] const fn` setters of the same names; `pub fn validate_meta(meta: &CommandMeta<'_>, scope: Scope, limits: &Limits) -> Vec<Violation>`; `pub(crate)` helpers for set rendering/equality and the per-base conflict pass used by `tree.rs`. A `compile_fail` doctest `CommandMeta::new().dm_permission(true); // [META-05]`. Unit tests for every `validate_meta` and tree-metadata row.
- `crates/noctis-rules/src/command/option.rs` (new) -- `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum OptionType { String, Integer, Boolean, User, Channel, Role, Mentionable, Number, Attachment }` with `pub const ALL: [OptionType; 9]`, `const fn value(self) -> u8` (3..=11 per OPT-02) and `const fn name(self) -> &'static str`; `#[derive(Clone, Copy, Debug, PartialEq)] pub enum NumericValue { Int(i64), Float(f64) }`; `pub enum ChoiceValue<'a> { Str(&'a str), Int(i64), Float(f64) }` (same derives); `#[non_exhaustive] #[derive(Clone, Copy, Debug, PartialEq)] pub struct Choice<'a> { pub name: LocalizedText<'a>, pub value: ChoiceValue<'a> }` with `const fn new(name, value)` and `name_localized(&[..])`; `#[non_exhaustive] #[derive(Clone, Copy, Debug, PartialEq)] pub struct OptionDecl<'a> { pub name, pub name_localized: &'a [&'a str], pub description: LocalizedText<'a>, pub kind: OptionType, pub required: bool, pub min_value: Option<NumericValue>, pub max_value: Option<NumericValue>, pub min_length: Option<usize>, pub max_length: Option<usize>, pub channel_types: &'a [u8], pub choices: &'a [Choice<'a>], pub autocomplete: bool, pub file_types: &'a [&'a str] }` with `const fn new(name, kind, description: &'a str)` (optional, everything unset) and by-value `#[must_use] const fn` setters `name_localized`, `description_localized`, `required(bool)`, `min_value`, `max_value`, `min_length`, `max_length`, `channel_types`, `choices`, `autocomplete(bool)`, `file_types`; `pub fn validate_options(options: &[OptionDecl<'_>], limits: &Limits) -> Result<(), Vec<Violation>>` and `pub fn measure_options(options: &[OptionDecl<'_>], limits: &Limits) -> usize` per the matrix. A `compile_fail` doctest `let _ = OptionType::SubCommand; // [OPT-14]`. Unit tests for every option row and an `opt_02_` test of `value()`; proptests: OPT-06 (random `min_length`/`max_length` in `0..7000`: violation set matches the rule) and OPT-03 (random `i64` bound: violation iff outside ±I53).
- `crates/noctis-rules/src/command/name.rs` -- `pub fn validate_context_menu_name(name: &str, limits: &Limits) -> Vec<Violation>`; unit tests for every CTX-01 row.
- `crates/noctis-rules/src/command/mod.rs` -- new fields/setters, re-exports (`Choice`, `ChoiceValue`, `CommandMeta`, `IntegrationType`, `InteractionContext`, `NumericValue`, `OptionDecl`, `OptionType`, `measure_options`, `validate_context_menu_name`, `validate_meta`, `validate_options`), module docs; `compile_fail` doctests `ContextMenuDecl::new("m", MenuKind::User, "X").description("d"); // [CTX-02]` and `let _ = MenuKind::PrimaryEntryPoint; // [CTX-03]` (each failing only on the tagged line).
- `crates/noctis-rules/src/command/tree.rs` -- metadata pass and docs; unit tests for every tree-metadata row (or in `meta.rs`, through `validate_tree`).
- `crates/noctis-rules/src/command/merge.rs` -- META-03 and CTX-01; unit tests for every merge row.
- `crates/noctis-rules/tests/options.rs` (new) -- the three epics ACs through the public API: one option list rejected with several rule ids each naming the option in `path`; CTX-01 accepts `"Show Profile"` and rejects 33 chars; `validate_merge` reports META-01 and META-03 at once.
- `crates/noctis/src/lib.rs` -- doc list mentions options, context-menu and metadata rules; doctest: `validate_options(&[o("a", Integer).required(true)...])` shows OPT-01 for a required option after an optional one.
- `docs/discord-rules.md` -- Unconfirmed table rows: `| OPT-13 | Allowed extension syntax is not specified. | A dot followed by one or more ASCII letters or digits (`.pdf`, `.PNG`, `.7z`); `.tar.gz` and bare `pdf` are rejected. |` and `| META-02 | Whether a leaf that leaves a field unset conflicts with one that sets it. | Without an explicit base declaration every command under a base must agree, unset included; with one, unset leaves inherit it and set ones must match. |`.
- `examples/project-template/.claude/skills/noctis/references/commands.md` -- add "### Rule functions (options)" in §3, CTX-01 function in §5, and "### Metadata rules" in §1/§6 documenting every new public item, the violation order, path format (`P`, `P.min_value`, `P.choices[j]`, `P.file_types[k]`), the META unset semantics and `measure_options` → `with_options(opts.len(), measure_options(&opts, &limits))`.
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` -- Rules types row pointing to the new functions; CMD-07 row "`nsfw` consistent per base (unset counts)"; OPT-13 row adds "`image`/`video`/`audio` or `.ext`".
- `examples/project-template/.claude/skills/noctis/references/troubleshooting.md` -- rows for `[OPT-05]`, `[OPT-06]`, `[OPT-08]`, `[OPT-09]`, `[OPT-13]`, `[META-01]`/`[META-02]`/`[CMD-07]` conflicts (declare once on the base), `[META-03]`, `[CTX-01]`, quoting real `Violation` text.
- `examples/project-template/AGENTS.md` -- extend the convention line: dynamically declared options go through `noctis::rules::command::validate_options` and `measure_options` before registering.

**Acceptance Criteria:**
- Given `noctis-rules`, when `cargo test --release -p noctis-rules` runs, then every matrix row passes as a unit, property, doctest or integration test whose name cites its rule id, the four `compile_fail` doctests fail to compile on their tagged lines, and `cargo tree -p noctis-rules -e normal --depth 1` lists only `regex`.
- Given a bot crate depending only on `noctis`, when it calls `noctis::rules::command::{validate_options, validate_context_menu_name, validate_meta}`, then it compiles and the facade doctest shows OPT-01.
- Given the Usage Skill, when grepped, then each of `validate_options`, `measure_options`, `validate_context_menu_name`, `validate_meta`, `OptionDecl`, `OptionType`, `Choice`, `ChoiceValue`, `NumericValue`, `CommandMeta`, `InteractionContext`, `IntegrationType` and `metadata(` appears in `references/`.
- Given `cargo run --release -p xtask -- deps-check`, when it runs, then it exits 0.

## Spec Change Log

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 31 findings — high 0, medium 2, low 12, false 15, maybe-false 2
- findings:
  - `[low]` `[reject]` `channel_types` values are never validated (blind) — out of scope by intent: Never forbids `channel_types`-value checks (no catalog row); Design Notes leave typing to the Stories 6.2/6.3 macros.
  - `[maybe-false]` `[reject]` STRING choice values are not checked against `min_length`/`max_length` (blind) — no catalog row; settling it needs Discord's behavior for a choice outside the length bounds (a user picks a choice and types nothing). If true it is only low.
  - `[low]` `[reject]` Duplicate choices and duplicate `file_types` entries pass (blind) — the intent's Never excludes them explicitly ("no catalog row").
  - `[low]` `[reject]` Float `Display` length overcounts CMD-05 for extreme magnitudes (blind) — the spec sets "numbers by their `Display` length". The overcount is conservative and needs both a choice like `1e20`/`1e-300` and a command near 4000 characters, which is unlikely.
  - `[false]` `[reject]` Unused `&Limits` in `validate_meta` / `measure_options` (blind) — the intent requires `&Limits` on every new public function "even where it reads no field" (AD-3).
  - `[false]` `[reject]` Inconsistent return types across the new functions (blind) — the intent fixes each signature; `validate_context_menu_name` mirrors the existing `validate_name -> Vec<Violation>`.
  - `[low]` `[patch]` Unnamed options get inconsistent paths and actuals (blind) — `validate_options` docs say every path starts with the name or `options[i]`, but CMD-06 uses the raw name (path `""`, as the matrix requires), and OPT-01/OPT-10 put the empty raw name in `Actual::Text` (option.rs `check_required_order`, l.466). Fix: correct the doc and use `label()` for those actuals.
  - `[false]` `[reject]` META-03 is reported once per declaration (blind) — the Design Notes and the merge matrix say "META-03 runs per declaration … (input order)".
  - `[low]` `[reject]` META-03 path is `.contexts` for an empty command path (blind) — real (merge.rs `meta_03`), but an empty path is already rejected by `validate_tree`, nobody meets it in everyday use, and the fix adds a branch.
  - `[false]` `[reject]` `render_set` sorts by declaration order instead of the Discord value (blind) — the variants are declared in value order (`Guild`=0, `BotDm`=1, `PrivateChannel`=2, meta.rs l.10-17), so derived `Ord` equals value order and no output is wrong. The concern is about a future change only.
  - `[maybe-false]` `[reject]` CTX-01 empty path for an empty name; whitespace-only names pass (blind) — the empty-name path `""` is what the intent matrix specifies. Settling the whitespace part needs Discord's response to a `"   "` menu name. If true it is only low.
  - `[low]` `[reject]` Localized option names are not validated (blind) — LOC-02 is explicitly excluded by the intent (Story 11.4).
  - `[false]` `[reject]` `docs/discord-rules.md` enforcement entries were not updated (blind) — the rows name enforcement phases and macros (e.g. CTX-01 l.132, META-01 l.135), which are still correct. No limit was added or changed, and the two required ambiguity rows are present.
  - `[low]` `[reject]` `compile_fail` doctests do not pin the error code (blind) — each doctest is one valid import plus the tagged line, so an unrelated failure is unlikely. Pinning codes is ignored on stable, and trybuild adds machinery the spec did not ask for.
  - `[medium]` `[patch]` Explicit-base inheritance is tested only for `default_member_permissions` (blind) — same root cause as the vgap per-field gap below. Action: add a `meta_02_explicit_is_per_field` test.
  - `[low]` `[reject]` Float choice of tiny/huge magnitude overcounts CMD-05 (edge) — grouped with the Float `Display` finding above; same reason.
  - `[low]` `[reject]` META-03 for CMD-10-dropped or empty-path declarations (edge) — META-03 is true for a dropped declaration (it is per declaration). The `.contexts` path part is grouped with the empty-path finding above; same reason.
  - `[false]` `[reject]` `option_length = 0` gives an unsatisfiable `Between{1,0}` (edge) — a limit of 0 allows no `max_length`, and reporting every one loudly is correct behavior for that configuration.
  - `[medium]` `[patch]` (vgap, pre-verified) Per-field "explicit base declaration" in `check_base` is pinned only for permissions — action: add a meta.rs unit test: base meta decl with `perm(8)` + `nsfw(true)` handler + unset handler yields exactly `[CMD_07]` with the unset note and help, plus the converse (base sets `nsfw(true)`, perm set/unset split still yields META-01).
  - `[low]` `[patch]` (vgap, pre-verified) Choice OPT-05 checks against only valid bounds are untested — action: extend `opt_05_choice_ranges` with `min_value(Int(I53+1))` and `min_value(Float(5.0))` on INTEGER, each with choice `0`, yielding only the bound's own violation.
  - `[low]` `[patch]` (vgap other) CMD-06 path / OPT-01 actual for empty names contradict the `validate_options` doc — grouped with the unnamed-options finding; same fix.
  - `[false]` `[reject]` "Derive and App build reject identically" is not exercised (intent) — the intent's Problem names Stories 6.2/6.3/6.4 as the later callers. The facade doc has described the intended callers in the present tense since Story 1.4.
  - `[false]` `[reject]` `nsfw` conflicts carry CMD-07 instead of META-01/02 (intent) — settled by the spec and the catalog CMD-07 row ("nsfw consistent per base").
  - `[false]` `[reject]` Set versus unset counts as differing (intent) — settled by the intent matrix ("set vs unset" row) and logged as an ambiguity in `docs/discord-rules.md` (FR-9).
  - `[false]` `[reject]` Conflicts are checked per scope (intent) — settled by the merge matrix "scopes separate" row; Discord registers per scope.
  - `[false]` `[reject]` META-03 also covers context menus and is absent from `validate_tree` (intent) — settled by the merge matrix ("META-03 menu") and Design Notes (macros call `validate_meta` with the attribute's scope).
  - `[false]` `[reject]` OPT-12 has no surface (intent) — the intent's Never assigns it to Story 6.7 (`Partial<T>`), which it depends on.
  - `[false]` `[reject]` OPT-07 values are not typed (intent) — the intent excludes value checks, and Design Notes leave typing to the 6.2/6.3 macros.
  - `[false]` `[reject]` Not every violation names the offending option (intent) — the CMD-13 `options`, `options[i]` and CTX-01 `""` paths are what the intent matrix specifies.
  - `[low]` `[patch]` `tests/options.rs` header claims OPT-01..13 coverage (intent) — it exercises only OPT-01/05/06/13, CTX-01, META-01, META-03. Action: make the header list the rule ids it exercises.
  - `[false]` `[reject]` Template `src/` not changed (intent) — the template code uses none of the new items, and the intent forbids edits to the template's `src/`; the skill and `AGENTS.md` were updated.

## Design Notes

**Decisions (logged to the memlog):**
- All new rules live in `noctis_rules::command` (options are command options); no twilight types: closed local enums `OptionType`, `InteractionContext`, `IntegrationType`; permissions are a raw `u64` bit set and `channel_types` raw Discord channel type values (typed by the macros of Stories 6.2/6.3).
- `validate_options` checks one options array in isolation, including CMD-13 (the derive counts fields); `validate_tree` keeps taking the caller's `with_options(count, measure_options(..))`. A caller that runs both on one oversized list gets CMD-13 twice (paths `options` and the leaf); accepted.
- Metadata: permissions conflicts are META-01, contexts / integration types and misplaced metadata META-02, nsfw CMD-07 (catalog CMD-07 row and cheatsheet). Without an explicit base declaration (a `Meta` decl at the 1-word path that sets the field) unset is a value and must agree (FR-9: no silent override); with one, unset leaves inherit. Lists compare as sets.
- META-03 runs per declaration through `validate_meta` (macros call it with the attribute's scope; `validate_merge` per decl). CTX-01 checks only the default name (localized names: LOC-02, Story 11.4). Localized option names are only measured; localized descriptions (CMD-04) and choice names (OPT-09) are checked, matching Story 1.4's description handling.
- Numbers: INTEGER bounds and choices must be `Int` within ±(2^53−1); NUMBER accepts `Int` or finite `Float` within ±2^53; comparisons use `i64` when both are `Int`, else `f64` (lossless after the range check; scope any `clippy::cast_precision_loss` with `#[expect(.., reason)]`). Choice value strings may be empty (catalog gives only a maximum).
- By-construction rules get `compile_fail` doctests tagged `// [ID]` (OPT-14, CTX-02, CTX-03, META-05); OPT-02 is the closed `OptionType` with a `value()` test; OPT-12 waits for Story 6.7.

## Verification

**Commands** (repo root; CI is paused, these local gates are the whole check):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (unit, property, integration, catalog, purity tests and doctests).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo tree -p noctis-rules -e normal --depth 1` -- expected: `noctis-rules` and `regex v1.13.x` only.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.

## Auto Run Result

- **Summary:** Added option, context-menu and command-metadata rules to `noctis_rules::command`: `validate_options` (OPT-01..11, OPT-13, CMD-13 per options array, closed `OptionType`), `measure_options` for CMD-05, `validate_context_menu_name` (CTX-01), `validate_meta` (META-01..03, CMD-07 nsfw, per-field base inheritance, set comparison of contexts/integration types), merge integration, and `compile_fail` doctests for by-construction rules (OPT-14, CTX-02, CTX-03, META-05). No twilight types; every new public function takes `&Limits`.
- **Files changed:**
  - `crates/noctis-rules/src/command/option.rs`: new option/choice/number-bound validation and option measurement.
  - `crates/noctis-rules/src/command/meta.rs`: new metadata model and META/CMD-07 checks.
  - `crates/noctis-rules/src/command/merge.rs`, `tree.rs`, `name.rs`, `mod.rs`, `src/lib.rs`: META-03 per declaration in merge, CTX-01, re-exports.
  - `crates/noctis-rules/tests/options.rs`: integration tests; header now lists the rule ids it exercises.
  - `crates/noctis/src/lib.rs`: facade re-exports and docs.
  - `docs/discord-rules.md`: two ambiguity rows (set vs unset metadata, FR-9).
  - `examples/project-template/AGENTS.md` and the noctis skill (`commands.md`, `limits-cheatsheet.md`, `troubleshooting.md`): new API usage.
- **Review findings:** 31 findings (high 0, medium 2, low 12, false 15, maybe-false 2).
  - Patched (2 medium, 4 low): per-field explicit-base inheritance tests for `nsfw` and permissions (blind + vgap, grouped); OPT-05 choice checks against only valid bounds; unnamed options use `label()` in OPT-01/OPT-10 actuals and the `validate_options` doc is corrected (blind + vgap, grouped); `tests/options.rs` header fixed.
  - Deferred: none.
  - Rejected: `channel_types` values not validated (out of scope, macros 6.2/6.3); STRING choice vs min/max_length (maybe-false, no catalog row); duplicate choices/`file_types` (excluded by Never); Float `Display` overcount for CMD-05 (spec rule, conservative; twice); unused `&Limits` (AD-3 required); inconsistent return types (signatures fixed by intent); META-03 per declaration (by design); META-03 `.contexts` path for empty command path (already rejected by `validate_tree`; twice); `render_set` ordering (declaration order equals value order); CTX-01 empty/whitespace name (maybe-false, matrix specifies `""`); localized option names (LOC-02, Story 11.4); discord-rules enforcement entries (still correct); `compile_fail` codes not pinned (stable ignores them); `option_length = 0` (correct loud behavior); derive/App parity not exercised (Stories 6.2-6.4); nsfw as CMD-07 (catalog); set vs unset differs (FR-9); per-scope checks (matrix); META-03 for menus outside `validate_tree` (Design Notes); OPT-12 no surface (Story 6.7); OPT-07 untyped (macros); violation paths for CMD-13/CTX-01 (matrix); template `src/` unchanged (intent forbids).
- **Follow-up review recommended:** false. Patched by verdict: high 0, medium 2, low 4. The orchestrator runs follow-up passes only after a patched high, and none was patched; the two medium patches are test additions verified by the green gates.
- **Verification:** local gates via `gates.sh` all green (`cargo fmt --check`, `cargo clippy --release --all-targets --all-features -D warnings`, `cargo test --release --workspace`, `cargo hack --each-feature`, `xtask deps-check`, `cargo tree` for noctis-rules, `cargo deny check`, MSRV 1.91 check, no `target/debug`). CI is paused by the maintainer, so it did not run.
- **Residual risks:** behavior that only Discord can settle (STRING choice outside length bounds, whitespace-only menu names) is not checked; CMD-05 can overcount for extreme float choices; a caller running both `validate_options` and `validate_tree` on one oversized list gets CMD-13 twice; `channel_types`/OPT-07 typing and OPT-12 wait for Stories 6.2/6.3/6.7.
