---
title: 'Story 1.4: Command naming and tree rules'
type: 'feature'
created: '2026-10-19'
status: 'ready-for-dev'
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

**Problem:** No shared function expands Command Patterns or checks command names and trees, so the `#[command]` macro (Story 6.1) and the App-build merge (Story 6.4) would each re-implement CMD-01..05, CMD-10..15 and REG-01/03/04 and could disagree (FR-7, FR-8, FR-9, FR-77, AD-3, AD-8).

**Approach:** Add `noctis_rules::command` (it depends on `regex` 1.13): `expand` (pattern → ordered Command Paths), `validate_name` (one word), `validate_tree` (one tree set: names, depth, leaf/group, children, descriptions, budget, duplicate paths) and `validate_merge` (per-scope trees, REG-01/02 counts, REG-03 context-menu names). Every function takes `&Limits` and returns every violation at once. `Violation` gains `notes` and `help`. A new catalog row CMD-16 covers pattern syntax. Re-export as `noctis::rules::command` and document it in the Usage Skill.

## Boundaries & Constraints

**Always:**
- `noctis-rules` stays sync, no I/O, no `async`, no `unsafe`. `[dependencies]` gains exactly `regex.workspace = true` (workspace `regex = "1.13"`, default features, resolves to 1.13.1; MIT/Apache, its `memchr`/`aho-corasick` are `Unlicense OR MIT`, accepted by `deny.toml`). Compiled regexes live in `std::sync::LazyLock` statics.
- Every public function takes `&Limits` and reads only its fields (`pattern_expansions`, `command_name_len`, `command_description_len`, `command_size`, `command_path_words`, `command_options`, `chat_input_commands`, `user_commands`, `message_commands`); nothing in `src/` calls `Limits::default()` outside `#[cfg(test)]` and the existing `Default` impl.
- Every length is `len::discord_len`. Error type is the existing `Violation` (hand-written `Display`/`Error`, no `thiserror`). Results are `Result<_, Vec<Violation>>`; an `Err` vector is never empty; violation order is deterministic (rules below).
- Every unit test name cites its rule id in snake case (`cmd_11_…`, `reg_04_…`).
- The public API change updates the Usage Skill and the template `AGENTS.md` in the same commit (AGENTS.md policy). The new rule id lands in `docs/discord-rules.md` and `RuleId` together.

**Never:**
- No option, choice, context-menu-name (CTX-01) or metadata (META-01..03) checks: Story 1.5. No LOC-02 checks of localized names (they only count toward CMD-05). No proc-macro, `BuildError`, Registry or Publish code. No new CI gate, no edits to `.github/workflows-disabled/`, `xtask/`, `_bmad/` or the template's `src/`.
- No `thiserror`, `serde`, `once_cell`, `unicode-*` crates, `build.rs` or prelude.

## I/O & Edge-Case Matrix

`L` = `Limits::default()` unless stated. `h(src, path)` = `CommandDecl::handler(src, path).description(LocalizedText::new("d"))`; `m(src, path)` = `CommandDecl::meta(src, path)`. "Err[X]" means the `Err` vector holds exactly the listed rule ids, in that order. Display of a violation = existing format plus `\n  = note: …` per note and `\n  = help: …`.

**`expand(pattern, &L) -> Result<Vec<String>, Vec<Violation>>`**

| Scenario | Input | Expected | Error handling |
|---|---|---|---|
| AC example | `"config (set|get) (prefix|welcome)"` | `["config set prefix","config set welcome","config get prefix","config get welcome"]` | — |
| optional group | `"config (set|get) (prefix|welcome)?"` | `[… set prefix, … set welcome, "config set", … get prefix, … get welcome, "config get"]` (the absent choice comes after the alternatives; leftmost group varies slowest) | — |
| plain / single alt | `"ping"`; `"a (b)"`; `"(a|b)"` | `["ping"]`; `["a b"]`; `["a","b"]` | — |
| whitespace collapse | `"  ping   pong "`, `"a\tb"`, `"a\u{3000}b"`, `"a\nb"` | `["ping pong"]`, `["a b"]`×3 (Unicode `White_Space` separates; output joins with one ASCII space, trimmed) | — |
| empty | `""`, `"   "` | `[""]` (validate_tree reports CMD-10) | — |
| optional only | `"(a)?"`; `"(a)? b"` | `["a",""]`; `["a b","b"]` | — |
| unicode | `"ayarlar (göster|değiştir)"`, `"設定 (表示|変更)"` | 2 expansions each, words unchanged | — |
| duplicate alt | `"a (b|b)"` | Err[CMD-15]: path `"a (b|b)"`, `Describe("no duplicate expansions")`, actual `Text("a b")`, note `produced 2 times` | one violation per distinct duplicated expansion, first-occurrence order |
| optional dup | `"(a)? (a)?"` | Err[CMD-15] for `"a"` (produced 2 times) | — |
| case differs | `"a (b|B)"` | Ok 2 (CMD-03 is validate_tree's job) | — |
| cap boundary | `"x (a|b|c|d|e) (f|g|h|i|j)"` (25) | Ok 25 | — |
| cap + 1 | 13 alts × `(n|o)` (26); 12 alts optional (13) × `(m)?` (2) = 26 | Err[CMD-15]: path = pattern, `AtMost(25)`, `Count(26)`; duplicates not checked | count computed before materializing |
| custom cap | 26-pattern with `pattern_expansions = 30`; `"(a|b)"` with `= 1`; `"ping"` with `= 0` | Ok 26; Err `AtMost(1)` `Count(2)`; Err `AtMost(0)` `Count(1)` | — |
| saturation | 30 groups of 10 alts | Err[CMD-15] `Count(usize::MAX)` (saturating product), returns instantly, no allocation of expansions | — |
| unclosed | `"a (b"`; `"ğ (x"` | Err[CMD-16] `Describe("unclosed group")`, actual `Text("(")`, note `at byte 2`; second: `at byte 3` (byte offsets) | first syntax error only, scan left to right |
| unmatched `)` | `"a b)"` | CMD-16 `"unmatched `)`"`, `)`, byte 3 | — |
| stray `|` | `"a | b"` | CMD-16 `"`|` outside a group"`, byte 2 | — |
| stray `?` | `"a ?"`, `"a?"`, `"a (b)??"` | CMD-16 `"`?` must directly follow a group"` at bytes 2, 1, 6 | — |
| nested | `"a ((b))"` | CMD-16 `"nested group"`, `(`, byte 3 | — |
| empty alt | `"a ()"`, `"a (b|)"`, `"a (|b)"` | CMD-16 `"empty alternative"`, actual = the `)` or `|` ending it, bytes 3, 5, 3 | — |
| space in group | `"a (b c)"`, `"a ( b)"` | CMD-16 `"whitespace inside a group"`, actual `" "`, bytes 4, 3 | — |
| glued group | `"a(b)"`, `"a (b)c"`, `"a (b)?c"`, `"(a)(b)"` | CMD-16 `"a group must be a whole word"`, actual = glued char: `(`@1, `c`@5, `c`@6, `(`@3 | — |
| property | random patterns of 1–4 tokens (words `[a-z]{1,3}`, groups of 1–3 distinct alts, optional flag) with no duplicate expansions | `Ok.len()` == product of (alts + optional); each item == its `split_whitespace` re-join; order is the odometer order | proptest |

**`validate_name(word, &L) -> Vec<Violation>`** (path `""`; order CMD-02, CMD-01, CMD-03; empty → CMD-02 only)

| Scenario | Input | Expected |
|---|---|---|
| valid | `"ayarlar"`, `"設定"`, `"नमस्ते"`, `"สวัสดี"`, `"a-b_c"`, `"itʼs"` (U+02BC), `"١٢٣"`, `"ß"`, `"ǆ"`, `"x1"` | `[]` |
| CMD-02 bounds | 32×`a`; 33×`a`; `""`; 16×`𠀀` (U+20000, 2 units each); 17×`𠀀`; 11×`a` with `command_name_len = 10` | `[]`; CMD-02 `Between{1,32}` `Count(33)`; CMD-02 `Count(0)`; `[]`; CMD-02 `Count(34)`; CMD-02 `Between{1,10}` `Count(11)` |
| CMD-01 ASCII `'` | `"it's"` | CMD-01 `Describe("letters, numbers, `-`, `_`, `ʼ`, Devanagari or Thai only")`, `Text("it's")`, note ``first invalid character `'` (U+0027)``, help ``use `ʼ` (U+02BC) instead of `'` `` (help only when the word has ASCII `'`) |
| CMD-01 others | `"a!b"`, `"a.b"`, `"🎉"`, `"x\u{A0}y"`, `"e\u{301}"` (combining mark), `"a|b"` | CMD-01, note names the first offending char (`!` U+0021, `.` U+002E, `🎉` U+1F389, U+00A0, U+0301, `|` U+007C), no help |
| CMD-03 | `"Config"`; `"İstanbul"`; `"ΣΑΣ"`; `"ǅ"`; `"Ⅷ"` | CMD-03 `Describe("lowercase")`, `Text(word)`, help ``use `config` ``; `i̇stanbul`; `σας`; `ǆ`; `ⅷ` (help = `str::to_lowercase`) — a char fails when `c.to_lowercase()` is not exactly `[c]` |
| multiple | `"It's"`; 33×`A` | [CMD-01, CMD-03]; [CMD-02, CMD-03] |

**`validate_tree(&[CommandDecl], &L) -> Result<(), Vec<Violation>>`** (paths split on single ASCII space; node = a word prefix of a handler path)

| Scenario | Input | Expected |
|---|---|---|
| empty input | `&[]` | `Ok(())` |
| AC valid | `h` for the 4 expansions of `"config (set|get) (prefix|welcome)"` | `Ok` |
| CMD-14 | `h("","mod ban")`, `h("","mod case show")` | `Ok` |
| CMD-11 AC | `h` for the 6 expansions of the `?` pattern | Err[CMD-11, CMD-11] for `set` then `get`: path `"config set"`, `Describe("a subcommand or a subcommand group, not both")`, `Text("set")`, notes ``"`config set` makes `set` a subcommand"``, ``"`config set prefix` makes `set` a subcommand group"`` (first leaf decl and first child decl, input order) |
| CMD-11 across sources | `h("m1","config set")`, `h("m2","config set prefix")` | Err[CMD-11], notes end with `` (from `m1`) `` / `` (from `m2`) `` (suffix only when source non-empty) |
| CMD-12 | `h("a","config")`, `h("b","config show")` | Err[CMD-12] path `"config"`, `Describe("a base command with subcommands is not invokable")`, `Text("config")`, notes ``"`config` makes `config` invokable (from `a`)"``, ``"`config show` gives `config` subcommands (from `b`)"`` |
| CMD-10 | `"a b c d"`; `""`; `"a b c"` with `command_path_words = 2` | CMD-10 `AtMost(3)` `Count(4)`; `Between{1,3}` `Count(0)`; `AtMost(2)` `Count(3)`; the decl is left out of the tree |
| words checked once per node | the 6 `?` expansions with base `Config` | exactly one CMD-03 for node `"Config"` (plus the CMD-11s) |
| empty word | `h("","config  set")`, `h("","config ")` | CMD-02 `Count(0)` at node path `"config "` (reported once) |
| CMD-04 | handler without description; `""`; 100×`x`; 101×`x`; 50×`😀`; 51×`😀`; default ok + localized `[101×x]`; meta `"config"` with 101-char description | CMD-04 `Between{1,100}` `Count(0)` note `missing description`; `Count(0)` note `in the default description`; ok; `Count(101)` same note; ok; `Count(102)`; note `in localized description #0`; CMD-04 on `"config"`. A non-handler node without description is fine |
| CMD-13 | base with 25 subcommands; 26; group `a g` with 26 subcommands; handler with options 25 / 26; 3 children with `command_options = 2` | ok; CMD-13 `AtMost(25)` `Count(26)` path `"a"`; path `"a g"`; ok / CMD-13 path = leaf; `AtMost(2)` `Count(3)` |
| CMD-05 boundary | `h("","a")` desc 100×`x`, `with_options(0, 7899)` / `7900` | ok (1+100+7899=8000) / CMD-05 `AtMost(8000)` `Count(8001)` path `"a"`, note `longest variant per field` |
| CMD-05 longest | desc `"x"` + localized `[100×y]` and options size 7899 | Count(8000) → ok; with meta `"a"` `name_localized(&["abcdefghij"])` → name counts 10 → `Count(8009)` → CMD-05 |
| CMD-05 sum | `h("","a b")`, `h("","a c")` (desc `"d"`), `command_size = 4` | CMD-05 `Count(5)` (a + b + d + c + d) |
| REG-04 duplicate | `h("m1","a b")`, `h("m2","a b")` | Err[REG-04] path `"a b"`, `Describe("one handler per command path")`, `Text("a b")`, notes `` registered by `m1` ``, `` registered by `m2` `` (every source; `""` prints `registered by (unnamed)`); budget uses the first |
| REG-04 description | `m("m1","a").description("x")`, `m("m2","a").description("y")` + `h("m1","a b")` | Err[REG-04] path `"a"`, `Describe("one description per command or group")`, `Text("a")`, notes `` `m1` declares `x` ``, `` `m2` declares `y` ``; equal values (default and localized) are fine |
| REG-04 orphan | `m("m1","zzz")` with no handler under it | Err[REG-04] `Describe("metadata only for a registered command path")`, `Text("zzz")`, note `` declared by `m1` `` |
| all at once | `"a b c d"`, `"Ab x"`, base with 26 children | every violation (CMD-10, CMD-03, CMD-13) in one `Err` |

Order: per-decl CMD-10 (input order), then REG-04 duplicates and orphans (first-occurrence order), then nodes in first-appearance pre-order: name rules, CMD-04, CMD-11/CMD-12, CMD-13 (children, then options), then CMD-05 per base after its subtree, then REG-04 description conflicts.

**`validate_merge(&[CommandDecl], &[ContextMenuDecl], &L) -> Result<(), Vec<Violation>>`**

| Scenario | Input | Expected |
|---|---|---|
| empty | `&[]`, `&[]` | `Ok` |
| merged base counts once | `h("m1","a b")`, `h("m2","a c")` | `Ok` (1 command) |
| REG-01 chat-input | 100 distinct global bases / 101 | ok / Err[REG-01] path `"chat-input commands"`, `AtMost(100)`, `Count(101)`, note `scope: global` |
| REG-02 guild | 101 bases in `Scope::Guild(42)` | Err[REG-02] note `scope: guild 42` |
| scopes separate | 100 global + 100 in guild 42; `h("","ping")` global and in guild 42 | `Ok` |
| user/message | 15 / 16 `MenuKind::User`; 16 `MenuKind::Message`; `chat_input_commands = 1` with 2 bases | ok / REG-01 path `"user commands"` `Count(16)`; `"message commands"`; REG-01 `AtMost(1)` `Count(2)` |
| meta and empty paths | meta decls; handler `""` | not counted |
| REG-03 | two `User` `"Show Profile"` global (m1, m2) | Err[REG-03] path `"Show Profile"`, `Describe("one context-menu command per name, type and scope")`, `Text("Show Profile")`, notes `` registered by `m1` ``, `` registered by `m2` ``, `scope: global` |
| REG-03 allowed | User + Message same name; global + guild same name; `"show profile"` vs `"Show Profile"` | `Ok` |
| tree rules per scope | `h("m1","config")`, `h("m2","config set")` global | Err[CMD-12] with `scope: global` appended as the last note; same paths in two different scopes → no CMD-12 |
| chat-input same name | `h("m1","ping")`, `h("m2","ping")` | Err[REG-04] (duplicate), never REG-03 |

Order: scopes in first appearance (commands, then menus); per scope: `validate_tree` violations, then counts (chat-input, user, message), then REG-03 in first-occurrence order.

</intent-contract>

## Code Map

- `crates/noctis-rules/Cargo.toml` (edit): add `[dependencies] regex.workspace = true`. Workspace `Cargo.toml:68` already has `regex = "1.13"`; `Cargo.lock` gains `regex 1.13.1`, `regex-automata`, `aho-corasick`, `memchr` (`regex-syntax` is already locked via proptest).
- `crates/noctis-rules/src/lib.rs` (edit): add `pub mod command;` and a Contents bullet.
- `crates/noctis-rules/src/rule.rs` (edit): `rule_ids!` table l.38-41 (add `CMD_16 = "CMD-16"` right after `CMD_15`); `Violation` l.215-250 (add fields); test `all_has_124_unique_well_formed_ids` l.270 (becomes 125).
- `crates/noctis-rules/tests/catalog.rs:48` (edit): `124` → `125`. `tests/purity.rs` (read-only): already allows `regex`, forbids `std::thread`/`std::io`/… (`LazyLock` is `std::sync`, fine).
- `crates/noctis-rules/src/limits.rs` (read-only): fields l.80-90 (`command_*`, `pattern_expansions`, REG-01 counts).
- `crates/noctis-rules/src/len.rs` (reuse): `discord_len`.
- `crates/noctis/src/lib.rs` (edit): `pub mod rules` re-export block.
- `docs/discord-rules.md` (edit): §2 table (CMD-15 row l.95), Compile-time strategy §3 l.400-414.
- Skill (edit): `examples/project-template/.claude/skills/noctis/references/commands.md` (§2 "Other structural errors" table l.100-111, §6 l.228-232), `references/limits-cheatsheet.md` (Rules types table l.100-115 says `124`; Commands table l.127-143), `references/troubleshooting.md` (compile-error table l.20-23). `examples/project-template/AGENTS.md` Conventions l.32-37. Template `src/` names none of these items (grep `Violation\|rules::` finds nothing), so no code reconcile.
- Spine AD-3 (l.145-168), AD-8 (l.327-344); catalog CMD-01..15 (l.78-95), REG-01..04 (l.144-147). Previous story (1.3, done): foundations, `Bound`/`Actual`/`Violation`, purity and catalog tests; its Story 1.7 note (rule-id citations are counted in tests and fixtures, not `src/rule.rs`) still holds.

## Tasks & Acceptance

**Execution:**
- `docs/discord-rules.md` -- after CMD-15 add `| **CMD-16** Command Pattern syntax | A pattern is whitespace-separated words and groups; a group is a whole word `(a|b|…)`, optionally followed by `?`, whose alternatives are non-empty single words; no nesting, no `|` or `?` outside that shape **[N]** | — | Compile-time (literal patterns) / App-build (dynamically built patterns) | `noctis_rules::command::expand` reports the first syntax error with its byte offset. |`; in Compile-time strategy §3 item 1 add "Malformed patterns fail with CMD-16." -- a syntax error needs a rule id (AD-4 `[<RULE-ID>]` format).
- `crates/noctis-rules/src/rule.rs` -- add `CMD_16`; add to `Violation` `pub notes: Vec<String>` and `pub help: Option<String>` (empty/`None` in `new`), `#[must_use] pub fn with_note(self, impl Into<String>) -> Self` and `with_help`; extend `Display` with `\n  = note: {n}` per note then `\n  = help: {h}`; update the 124 test to 125 and add Display tests with notes/help (existing outputs unchanged) -- macros need note/help lines (AD-4) without recomputing.
- `crates/noctis-rules/tests/catalog.rs` -- `124` → `125`.
- `crates/noctis-rules/Cargo.toml` -- add the regex dependency.
- `crates/noctis-rules/src/command/mod.rs` -- module docs (pattern grammar, expansion order, node/tree model, violation order) and public types: `#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct LocalizedText<'a> { pub default: &'a str, pub localized: &'a [&'a str] }` with `const fn new`, `const fn with_localized`, `fn longest(&self) -> usize` (max `discord_len`); `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum Scope { Global, Guild(u64) }` (`Default` = Global); `#[non_exhaustive] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum DeclKind { Handler { options: usize, options_size: usize }, Meta }`; `#[non_exhaustive] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct CommandDecl<'a> { pub source, pub path, pub scope, pub kind, pub description: Option<LocalizedText<'a>>, pub name_localized: &'a [&'a str] }` with `const fn handler(source, path)` (Handler{0,0}), `const fn meta(source, path)`, and by-value `#[must_use] const fn` setters `description`, `name_localized`, `scope`, `with_options(count, size)` (sets `kind = Handler{count,size}`); `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum MenuKind { User, Message }`; `#[non_exhaustive] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct ContextMenuDecl<'a> { pub source, pub scope, pub kind: MenuKind, pub name: &'a str }` with `const fn new(source, kind, name)` and `scope` setter. Re-export the four functions. `options_size` is documented as the caller-computed CMD-05 contribution of the handler's options (names, descriptions, choice names/values, longest variant each; Story 1.5 provides the measurer).
- `crates/noctis-rules/src/command/pattern.rs` -- `pub fn expand(pattern: &str, limits: &Limits) -> Result<Vec<String>, Vec<Violation>>` per the matrix: tokenize (CMD-16), saturating count vs `limits.pattern_expansions` (CMD-15), odometer expansion, duplicate check (CMD-15). Unit + proptest tests for every expand row.
- `crates/noctis-rules/src/command/name.rs` -- `pub fn validate_name(word: &str, limits: &Limits) -> Vec<Violation>`: CMD-02 via `discord_len`, CMD-01 via a `LazyLock<Regex>` of the negated class `[^-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]` (first match = offending char), CMD-03 per char. Unit tests for every name row, plus a test that the catalog regex `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$` agrees with `validate_name` on all valid/CMD-01 rows.
- `crates/noctis-rules/src/command/tree.rs` -- `pub fn validate_tree(decls: &[CommandDecl<'_>], limits: &Limits) -> Result<(), Vec<Violation>>` (ignores `scope`; builds nodes from handler paths, attaches meta; rules and order per the matrix; node name uses the longest of the word and every `name_localized` at that node; node description = first `Some` in input order). Unit tests for every tree row.
- `crates/noctis-rules/src/command/merge.rs` -- `pub fn validate_merge(commands: &[CommandDecl<'_>], menus: &[ContextMenuDecl<'_>], limits: &Limits) -> Result<(), Vec<Violation>>`: group by scope, call `validate_tree` per scope and append the scope note, count distinct first words of non-empty handler paths (REG-01 global, REG-02 guild), count menus, REG-03. Unit tests for every merge row.
- `crates/noctis-rules/tests/command.rs` -- integration test through the public API for the three epics ACs (expand order, CMD-11 Display text including both notes, merge returning several rule ids at once).
- `crates/noctis-rules/src/lib.rs`, `crates/noctis/src/lib.rs` -- `pub mod command`; facade `pub use noctis_rules::command;` inside `pub mod rules`, its doc listing `command`, and a doctest: `expand("config (set|get) (prefix|welcome)", &Limits::default())` yields 4 paths and `validate_tree` on the `?` variant's handlers returns CMD-11.
- `examples/project-template/.claude/skills/noctis/references/commands.md` -- add "### Rule functions (`noctis::rules::command`)" documenting every new public item (types, setters, four functions, violation order, pattern grammar, CMD-16) and a CMD-16 row in "Other structural errors"; state that `#[command]` and `App::build` call these.
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` -- RuleId count 125; `Violation` row gains `notes`, `help`, `with_note`, `with_help` and the note/help lines; Commands table gains `CMD-16 | pattern syntax: whole-word groups, single-word alternatives, `?` only after a group | C / B`; Rules types table points to `commands.md` for `noctis::rules::command`.
- `examples/project-template/.claude/skills/noctis/references/troubleshooting.md` -- add rows `[CMD-16]` (malformed pattern: fix the token at the byte offset) and `[CMD-15]` (duplicate or too many expansions).
- `examples/project-template/AGENTS.md` -- Conventions: "Dynamically built Command Patterns are checked with `noctis::rules::command::{expand, validate_tree}` before registering; never hand-split patterns."
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- the planner appends the Design Notes decisions.

**Acceptance Criteria:**
- Given `noctis-rules`, when `cargo test --release -p noctis-rules` runs, then every matrix row passes as a unit, property or integration test whose name cites its rule id, and `cargo tree -p noctis-rules -e normal --depth 1` lists only `regex`.
- Given a bot crate depending only on `noctis`, when it calls `noctis::rules::command::expand` and `validate_tree`, then it compiles and the facade doctest shows the 4 expansions and a CMD-11 violation.
- Given the catalog gains CMD-16, when `tests/catalog.rs` runs, then `RuleId::ALL` (125 ids) matches it in row order.
- Given `cargo run --release -p xtask -- deps-check`, when it runs, then it exits 0 (`1 internal edges`; regex is external).
- Given the Usage Skill, when grepped, then each of `expand`, `validate_name`, `validate_tree`, `validate_merge`, `CommandDecl`, `DeclKind`, `LocalizedText`, `Scope`, `ContextMenuDecl`, `MenuKind`, `with_note`, `with_help` and `CMD-16` appears in `references/`, and `124` no longer appears in `limits-cheatsheet.md`.

## Spec Change Log

## Review Triage Log

## Design Notes

**Exact `Bound::Describe` strings** (the matrix abbreviates them inside table code spans; these are authoritative): CMD-16: ``unclosed group``, ``unmatched `)` ``, `` `|` outside a group ``, `` `?` must directly follow a group ``, ``nested group``, ``empty alternative``, ``whitespace inside a group``, ``a group must be a whole word``. CMD-15: ``no duplicate expansions``. CMD-01: ``letters, numbers, `-`, `_`, `ʼ`, Devanagari or Thai only``. CMD-03: ``lowercase``. CMD-11: ``a subcommand or a subcommand group, not both``. CMD-12: ``a base command with subcommands is not invokable``. REG-04: ``one handler per command path``, ``one description per command or group``, ``metadata only for a registered command path``. REG-03: ``one context-menu command per name, type and scope``. Notes use backticks around paths, words, sources and values exactly as in the matrix; CMD-01's note is ``first invalid character `<c>` (U+XXXX)`` (uppercase hex, at least 4 digits).

**Decisions (logged to the memlog):**
- Pattern grammar is strict: a group is a whole whitespace-delimited token and alternatives are single words; anything else is CMD-16 (new [N] catalog row). The strict reading matches FR-8's "one alternative per word" and can be relaxed later without breaking patterns.
- Expansion order is an odometer: leftmost group varies slowest, alternatives in declaration order, an optional group's "absent" choice comes last. The count is a saturating product checked against `limits.pattern_expansions` before any expansion is built. `#[command(max_expansions = n)]` (Story 6.1) passes a `Limits` copy with `pattern_expansions = n`, so `expand` keeps the `&Limits` rule.
- `expand` reports syntax errors (first only), else the cap, else duplicates; it never reports name rules. An empty expansion `""` is yielded and validate_tree reports CMD-10 (`Between{1, max}`).
- `validate_tree` splits paths on single ASCII spaces so `"config  set"` has an empty word (CMD-02, FR-7); depth 1 handler with children = CMD-12, depth 2 = CMD-11. Duplicate handler paths, conflicting explicit node descriptions (FR-57) and metadata for unregistered paths are REG-04. A non-handler node may lack a description (App build resolves the FR-57 default before calling); a handler may not (CMD-04).
- CMD-05 per base = Σ nodes (longest name variant + longest description variant, 0 if none) + Σ handler `options_size`; options are measured by the caller (Story 1.5) and passed as a number, which keeps this story free of option types.
- `validate_merge` checks trees per `Scope` and reports guild counts as REG-02 (the catalog's id for guild scope). Entry Point commands are not representable in v1 (CTX-03), so `entry_point_commands` is unused here. Context-menu names compare exactly (byte equality).
- `Violation` gains `notes`/`help` (additive; the struct is `#[non_exhaustive]`) so macros print `= note:`/`= help:` lines from rule output (AD-4). CMD-01 regex is compiled once in a `LazyLock` (std, MSRV 1.91 ok).

```rust
let l = Limits::default();
let paths = expand("config (set|get) (prefix|welcome)?", &l).unwrap();
let decls: Vec<_> = paths.iter()
    .map(|p| CommandDecl::handler("config", p).description(LocalizedText::new("d"))).collect();
let errs = validate_tree(&decls, &l).unwrap_err();
assert_eq!(errs[0].rule, RuleId::CMD_11); // `set`, with both contributing paths as notes
```

## Verification

**Commands** (repo root; CI is paused, these local gates are the whole check, and no gate is added):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0, including noctis-rules unit/property/integration/catalog/purity tests and the facade doctest.
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0, `1 internal edges`.
- `cargo tree -p noctis-rules -e normal --depth 1` -- expected: `noctis-rules` and `regex v1.13.x` only.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.
- `grep -c "124" examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` -- expected: 0.

**Manual checks:**
- Temporarily delete the CMD-16 catalog row: `cargo test --release -p noctis-rules --test catalog` fails naming `CMD-16`; revert with `git checkout docs/discord-rules.md`.
