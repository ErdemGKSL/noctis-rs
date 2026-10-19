---
title: 'Story 1.8: Public API snapshot and Usage Skill gate'
type: 'feature'
created: '2026-10-19'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Nothing stops the facade's public API from drifting away from the Noctis Usage Skill (`examples/project-template/.claude/skills/noctis/`), whose `API hash:` line still reads `pending`, and nothing lints the Project Template for UI Doctrine violations (FR-49, FR-50, UX-DR1..9, NFR-8; AD-17, AD-27, AD-28).

**Approach:** Add `cargo run --release -p xtask -- api-snapshot [--bless]`, which records the facade's public API with `cargo public-api` 0.52.0 on `nightly-2026-10-15` into the committed `xtask/public-api.txt` (inlining items the facade re-exports from internal crates) and fails when it is stale; and `cargo run --release -p xtask -- skill-check [--root <path>]`, which fails when a snapshot item is not mentioned in the skill, when the skill's `API hash` differs from the snapshot's blake3 hash, when the Project Template breaks UD-1/UD-3/UD-7, or when the pending component-entry check (Story 4.1) is due. Replace the skill's `pending` hash with the real one.

## Boundaries & Constraints

**Always:**
- Every cargo command uses `--release`, except `cargo public-api` itself (it has no such flag); xtask runs it with `CARGO_TARGET_DIR=<root>/target/public-api`, so `test ! -d target/debug` holds after all gates. `.cargo/config.toml` is never edited.
- Tool pins are xtask constants: `NIGHTLY = "nightly-2026-10-15"`, `CARGO_PUBLIC_API = "0.52.0"`, `RUSTDOC_FORMAT_VERSION = 61` (verified: this nightly emits rustdoc JSON format 61; `cargo-public-api 0.52.0` installed with `--locked` reads it). Invocation: `cargo +nightly-2026-10-15 public-api --manifest-path <root>/Cargo.toml -p <pkg> --all-features --omit blanket-impls,auto-trait-impls --color never`, env `CARGO_TARGET_DIR=<root>/target/public-api`, with `RUSTUP_TOOLCHAIN`, `CARGO`, `RUSTC`, `RUSTDOC` removed from the child env. The run leaves `<root>/target/public-api/doc/<crate_name>.json`.
- Output style copies `rules-catalog`: one line per problem on stderr, `<task>: <subject>: <message>`, sorted and deduplicated, exit 1; usage errors exit 2 with usage; `eprintln!` only (clippy `print_stdout` is denied). Problems are collected, never fail-fast, except tool/IO failures of `api-snapshot`, which stop it.
- New xtask deps from `[workspace.dependencies]`: `serde_json` (resolves 1.0.154), `blake3` (1.8, resolves 1.8.7; new to `Cargo.lock`), `proc-macro2` with `features = ["span-locations"]` (resolves 1.0.107). xtask keeps no internal-crate dependency. `cargo deny check` must pass with the new lock entries (blake3, arrayvec, cc, cfg-if, constant_time_eq, cpufeatures); no `deny.toml` edit.
- The snapshot is the only input of `skill-check` about the API; `skill-check` needs neither nightly nor `cargo public-api`.
- No public API change in `crates/`; the template's Rust code stays as is (it already passes the lint).

**Never:**
- No `syn`, no rustdoc-JSON typed structs (read `serde_json::Value`), no network calls of our own, no edits to `crates/` or `docs/discord-rules.md`, no re-enabled CI (only keep `.github/workflows-disabled/ci.yml` in sync).
- `api-snapshot --bless` never edits `SKILL.md`; the hash is updated by hand together with the skill content (it prints the new hash).
- The component-entry check is not implemented beyond its pending registration and its due-trigger (Story 4.1 implements it).

## I/O & Edge-Case Matrix

Rows L1-L16: snapshot line grammar (`xtask/src/public_api.rs`, shared). "Owner" = path that decides inclusion in an expansion; "item" = path the skill must mention. Paths drop every `<…>` group (angle-depth aware) and a leading `r#` per segment.

| Scenario | Input line | Classification | Error |
|---|---|---|---|
| L1 module | `pub mod noctis::rules` | item `noctis::rules`, name `rules` | — |
| L2 attrs | `#[non_exhaustive] pub struct noctis::rules::Limits`; two attrs `#[repr(u8)] #[non_exhaustive] pub enum …` | item `…::Limits`; every leading `#[…]` (bracket-balanced) skipped | — |
| L3 field | `pub noctis::rules::Limits::button_label_len: usize` | item, name `button_label_len` (stops at `:` not part of `::`) | — |
| L4 variant | `pub noctis::rules::Actual::Count(usize)`; struct-variant field `pub …::DeclKind::Handler::options: usize` | items `…::Actual::Count`, `…::Handler::options` | — |
| L5 generic path | `pub const fn noctis::rules::command::CommandDecl<'a>::with_options(self, usize, usize) -> Self` | item `noctis::rules::command::CommandDecl::with_options` | — |
| L6 keywords | `pub const`, `pub fn`, `pub const fn`, `pub unsafe fn`, `pub async fn`, `pub extern "C" fn`, `pub static`, `pub static mut`, `pub type`, `pub trait`, `pub auto trait`, `pub union`, `pub use`, `pub macro` | keywords skipped, next token is the path | — |
| L7 macro | `pub macro noctis::rgb!` | item `noctis::rgb`, name `rgb` | — |
| L8 tuple struct / alias | `pub struct noctis::rules::MessageKey(_)`; `pub type noctis::X = u8` | items `…::MessageKey`, `noctis::X` | — |
| L9 inherent impl | `impl<'a> noctis::rules::command::Choice<'a>` then `pub const fn noctis::rules::command::Choice<'a>::new(…)` | header: not an item, owner `…::Choice`; member is an item | — |
| L10 trait impl | `impl core::fmt::Display for noctis::rules::Violation` then `pub fn noctis::rules::Violation::fmt(…)`, `pub type noctis::rules::Locale::Err = …` under `impl core::str::FromStr for noctis::rules::Locale` | header owner = self type; members (parent == self path) are not items | — |
| L11 ` for ` detection | `impl<T> core::convert::From<T> for noctis::X where T: for<'a> Fn(&'a str)`; `impl<'a> noctis::X<'a>` | trait impl = first ` for ` at depth 0 of `<>()[]`; self type ends at depth-0 ` where ` | — |
| L12 ref / foreign self | `impl … for &'a noctis::X`; `impl … for alloc::sync::Arc<T>` | self path strips `&`, lifetimes, `mut `: `noctis::X`, `alloc::sync::Arc` | — |
| L13 context ends | inherent/trait member lines, then `pub fn noctis::rules::attachment::validate_file_size(…)` (parent ≠ self) | the free fn is an item; impl context closed | — |
| L14 trait methods | `pub trait noctis::rules::Translate: core::marker::Send + core::marker::Sync` then `pub fn noctis::rules::Translate::translate(…)` | both items (no impl context) | — |
| L15 unicode / odd text | `pub const noctis::X::Y: &str` with `é` elsewhere on the line | parsed; only ASCII path chars matter | — |
| L16 garbage | empty line, `foo`, `pub`, `impl` alone, unbalanced `#[` | — | `xtask/public-api.txt:<n>: cannot parse line` (skill-check) / `api-snapshot: cannot parse cargo public-api line: <line>` |

Rows A1-A12: snapshot assembly (`api-snapshot`, pure core `assemble` + `reexports_from_rustdoc_json`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| A1 real repo | facade lines (19) + `noctis-rules` lines | every `pub use noctis::rules::<X>` line whose source is internal is replaced in place by the `noctis-rules` lines owned by the source (original order), with every mapped source path rewritten to its facade path; no `noctis_` path left; equals the committed snapshot | — |
| A2 item re-export | `pub use noctis_rules::Limits` → `noctis::rules::Limits` | block = struct line, fields, impl headers whose self is `Limits`, their members | — |
| A3 module re-export | `pub use noctis_rules::command` | block = `pub mod noctis::rules::command` and everything owned under `noctis_rules::command::` | — |
| A4 renamed / nested | `pub use noctis_rules::len::discord_len` at `noctis::rules` | line `pub fn noctis::rules::discord_len(&str) -> usize`; `noctis_rules::len` itself not included | — |
| A5 glob | JSON `is_glob: true`, source `noctis_rules::command` in module `noctis::x` | mapping `noctis_rules::command` → `noctis::x`; the glob's own `pub use` line is replaced the same way | — |
| A6 boundary-safe rewrite | mapping `noctis_rules::Limits`; text `noctis_rules::LimitsExt`, `noctis_rules::Limits::truncates`, `&noctis_rules::Limits)` | only whole-path matches (prev char not `[A-Za-z0-9_:]`, next not `[A-Za-z0-9_]`) rewritten; longest source first | — |
| A7 recursion | `noctis-core` output contains `pub use noctis_core::prelude::View` with source `noctis_ui::View` | expanded through `noctis-ui` first, then rewritten into the outer crate's paths; each internal crate is run once | — |
| A8 external re-export | facade `pub use noctis::prelude::Id` with source `twilight_model::id::Id` | line kept verbatim (an item named `Id`) | — |
| A9 leak | an assembled line still contains `noctis_<x>::` | — | `api-snapshot: <line>: names noctis_<x>::…, which the facade does not re-export` |
| A10 empty expansion | source matches no line of its crate (hidden or missing) | — | `api-snapshot: noctis::<path>: re-export of <source> matches no public item of <pkg>` |
| A11 JSON | missing file, invalid JSON, `format_version` ≠ 61, missing `root`/`index` | — | `api-snapshot: target/public-api/doc/<crate>.json: …` (format: `rustdoc JSON format <v>, expected 61; the pinned nightly is nightly-2026-10-15`) |
| A12 unmapped use | a `pub use` line of an internal source with no JSON mapping | — | `api-snapshot: pub use <path>: no re-export found in rustdoc JSON` |

Rows S1-S7: `api-snapshot` CLI.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| S1 fresh | committed snapshot equals assembly | `api-snapshot: ok (<n> lines, hash <64 hex>)`, exit 0 | — |
| S2 stale | differs | `-<line>` for lines only in the committed file, `+<line>` for lines only in the new one (each in order), then `api-snapshot: xtask/public-api.txt is stale; run cargo run --release -p xtask -- api-snapshot --bless, then update the Usage Skill and its API hash`, exit 1 | — |
| S3 missing file | no `xtask/public-api.txt` | treated as stale with every line `+` | exit 1 |
| S4 bless | `--bless` | writes the assembly (LF, one trailing newline, nothing else), `api-snapshot: wrote xtask/public-api.txt (<n> lines, hash <h>)`, exit 0 | write error → exit 1 |
| S5 tool missing / wrong | `cargo +nightly-2026-10-15 public-api --version` fails or prints ≠ `cargo-public-api 0.52.0` | — | `api-snapshot: cargo-public-api 0.52.0 on nightly-2026-10-15 is required (found: <trimmed output or error>); install: rustup toolchain install nightly-2026-10-15 --profile minimal && cargo install cargo-public-api --version 0.52.0 --locked`, exit 1 |
| S6 tool failure | a `public-api` run exits ≠ 0 | its stderr passed through, `api-snapshot: cargo public-api -p <pkg> failed (<status>)`, exit 1 | — |
| S7 bad args | `--bless x`, `--root`, unknown flag | — | message + usage, exit 2 |

Rows M1-M9: mention check (`skill-check`; text = all `*.md` under `examples/project-template/.claude/skills/noctis/`, recursive, sorted).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| M1 mentioned | item name appears as a whole identifier token (maximal run of `[A-Za-z0-9_]`) anywhere in the text | ok | — |
| M2 not mentioned | `validate_tree` absent; or only `validate_trees` / `xvalidate_tree` present | — | `skill-check: noctis::rules::command::validate_tree: public item not mentioned in the Usage Skill` |
| M3 rule-id const | `RuleId::REG_06` (name `^[A-Z][A-Z0-9]{1,3}_[0-9]{2}$`, parent ends `::RuleId`): text has `REG_06` or `REG-06` bounded by non-`[A-Za-z0-9_-]` | ok; `REG-061`, `XREG-06` do not count | M2 message |
| M4 trait-impl members | `fmt`, `clone`, `eq`, `Err` under trait impls | not required | — |
| M5 duplicates | same item path on two lines | one requirement, one error | — |
| M6 case | item `Limits`, text only `limits` | not mentioned (case-sensitive) | M2 |
| M7 unicode text | text with `ʼ`, `…`, `😀` around `discord_len` | identifier boundaries are ASCII-only; non-ASCII chars are separators | — |
| M8 skill missing / bad | no skill dir, no `*.md`, non-UTF-8 file | — | `skill-check: <path>: cannot read …` / `not UTF-8` |
| M9 snapshot missing / bad | no `xtask/public-api.txt`; L16 line | — | `skill-check: xtask/public-api.txt: cannot read …; run cargo run --release -p xtask -- api-snapshot --bless` / L16 error |

Rows H1-H6: `API hash` (blake3 of the snapshot file bytes, 64 lowercase hex).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| H1 match | exactly one `SKILL.md` line containing `**API hash:**`, first code span after it = the hash | ok | — |
| H2 stale | code span is another valid hash | — | `skill-check: SKILL.md: API hash <old> does not match xtask/public-api.txt (<new>); update the skill for the API change, then the hash` |
| H3 pending / malformed | `pending`, 63 hex, uppercase hex, `blake3:<hex>`, no code span | — | `skill-check: SKILL.md: API hash must be the snapshot's 64-hex blake3 hash (<new>)` |
| H4 none / many | no such line; two such lines | — | `skill-check: SKILL.md: expected exactly one **API hash:** line, found <n>` |
| H5 CRLF snapshot | snapshot bytes contain `\r` | — | `skill-check: xtask/public-api.txt: must use LF line endings` |
| H6 real repo | committed snapshot + updated `SKILL.md` | ok | — |

Rows P1-P7: `xtask/skill-pending.toml` (`[[check]] { name, story }`, `#[serde(deny_unknown_fields)]`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| P1 pending, not due | `name = "component-entries"`, `story = "4.1"`; snapshot has no builder item | skipped, counted as pending | — |
| P2 due | pending and the snapshot has a `pub struct`/`pub enum` item outside `noctis::rules::` whose name is one of `Container, Section, TextDisplay, Thumbnail, MediaGallery, File, Separator, ActionRow, RouteButton, LinkButton, PremiumButton, StringSelect, UserSelect, RoleSelect, MentionableSelect, ChannelSelect, Label, TextInput, FileUpload, RadioGroup, CheckboxGroup, Checkbox` | — | `skill-check: skill-pending.toml: component-entries: the snapshot exports component builder <path>; implement the component-entry check and remove this entry (Story 4.1)` |
| P3 not pending | entry absent | — | `skill-check: skill-pending.toml: component-entries must stay pending until Story 4.1 implements it` |
| P4 unknown name | `name = "other"` | — | `skill-check: skill-pending.toml: unknown check other` |
| P5 story format | `"4"`, `"4.1.1"`, `"0.1"`, `"4.0"`, `""` | — | `skill-check: skill-pending.toml: component-entries: story must be <epic>.<story>` (reuse `rules_catalog` `is_story`) |
| P6 duplicate / unknown key / invalid TOML | name twice; `note = ""`; syntax error | — | `skill-check: skill-pending.toml: duplicate entry component-entries` / `<toml message>`; other checks still run |
| P7 missing file | no file | — | `skill-check: xtask/skill-pending.toml: cannot read …` |

Rows D1-D18: UI Doctrine lint over `examples/project-template/{src,tests}/**/*.rs`, lexed with `proc_macro2::TokenStream::from_str` (comments dropped, doc comments and string/char literals are `Literal`s and never match). Interactive handler = a `fn` item preceded (possibly among other attributes) by an attribute whose path's last segment is `command`, `user_command`, `message_command` or `handler`; its scope = the tokens from `fn` through its body group. Event handler = last segment `event`. Ident words: strip `r#`, split at `_` and camel-case boundaries, lowercase.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| D1 real template | the 12 current files | no problem | — |
| D2 embed anywhere | idents `Embed`, `EmbedBuilder`, `embeds`, `add_embed` | — | `skill-check: <path>:<line>: [UD-1] embeds are not an interactive output; build a View` |
| D3 embed near-miss | `embedded`, `"embed"` literal, `// embed`, `/// embeds` | not flagged | — |
| D4 LegacyMessage in handler | `LegacyMessage` in an interactive handler's signature or body | — | `[UD-1] LegacyMessage in an interactive handler; return a View` |
| D5 LegacyMessage elsewhere | in an `#[event]` fn or a plain fn | not flagged | — |
| D6 content in handler | inside an interactive handler's body: a `.content(` method call, or `content` preceded by `{` or `,` and followed by a lone `:` (struct-literal field) | — | `[UD-1] content as handler output; return a View` |
| D7 content read | `msg.content` (no call), `content::x` path, `let content: String`, a `content: &str` parameter, `content` outside handlers | not flagged | — |
| D8 response types anywhere | idents `InteractionResponseType`, `InteractionResponse`, `InteractionResponseData`, `InteractionCallbackType`, `CallbackType`, `ResponseType` | — | `[UD-3] manual response-type selection; return a View and let Noctis pick the response` |
| D9 responder calls | `.defer(`, `.reply(`, `.update(`, `.edit_original(` inside an interactive handler | — | `[UD-3] manual response through the Responder; return a View` |
| D10 non-modal responder | handler param type whose last segment is `Responder`, `CommandResponder`, `ComponentResponder` or `ModalResponder` and whose generic args lack `OpensModal` (always for `ModalResponder`) | — | `[UD-3] take a Responder only to open a modal (<Type><OpensModal>)` |
| D11 modal responder ok | `responder: ComponentResponder<OpensModal>` + `responder.modal(m)` (template `edit_bio`) | not flagged | — |
| D12 collector | any ident with a word `collector`/`collectors` (`ComponentCollector`, `create_collector`) | — | `[UD-7] collectors are not allowed; one #[handler] per Route` |
| D13 collect ok | `.collect()`, `collect::<Vec<_>>` | not flagged | — |
| D14 await loops | idents `wait_for`, `wait_for_component`, `wait_for_interaction`, `await_component`, `await_interaction`, `next_component`, `next_interaction`, `component_stream`, `interaction_stream` | — | `[UD-7] await-component loop; state goes in Payloads, Refs or ports` |
| D15 inline listener | method call `.on_<x>(` or a method whose words contain `listen`/`listener` with a top-level `|`, `||` or `move` in its argument group; any ident with a word `listener`/`listeners` | — | `[UD-7] inline listener; register a #[handler] for a Route` |
| D16 listener near-miss | `.on_error(handler_fn)` (no closure), `listen_addr` field (`listen` word but not a call with closure) | not flagged | — |
| D17 per-message task | `spawn` / `spawn_local` ident inside an interactive or event handler | — | `[UD-7] task spawned per interaction or event; use a Ref TTL or a port` |
| D18 lex error / nesting | unbalanced delimiters; handler inside `mod x { … }` or `impl` | lex error → `skill-check: <path>: cannot lex: <msg>`; nested handlers found | — |

Rows X1-X4: `skill-check` CLI.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| X1 real repo | no args | `skill-check: ok (<N> public items mentioned, 12 template files linted, 1 pending check)`, exit 0 | — |
| X2 `--root <dir>` | generated fixture tree | checks that tree | missing inputs → exit 1 with messages |
| X3 bad args | `--root` without value, extra arg, unknown flag | — | message + usage, exit 2 |
| X4 many problems | M, H, P and D problems together | all reported, sorted, deduplicated, exit 1 | — |

</intent-contract>

## Code Map

- `xtask/src/main.rs` -- dispatch (`rules-catalog` arm and `rules_catalog_root` to copy for `skill-check --root`), `USAGE`, module docs l.1-10 (drop "`api-snapshot` and the other gates arrive in later stories"; list both tasks), `USAGE_ERROR = 2`.
- `xtask/src/rules_catalog/mod.rs` -- `run(root) -> ExitCode`, problem set/sorting/`eprintln!` style to copy; `rules_catalog/pending.rs` l.59 `is_story` (make `pub(crate)` and reuse for P5).
- `xtask/tests/rules_catalog.rs` -- CLI test style: fixture repos under `env!("CARGO_TARGET_TMPDIR")`, `assert_exit(out, code, needles)` (l.167), `#![allow(clippy::expect_used, …)]`.
- `xtask/Cargo.toml` -- add `serde_json.workspace = true`, `blake3.workspace = true`, `proc-macro2 = { workspace = true, features = ["span-locations"] }`.
- `crates/noctis/src/lib.rs` l.114-122 -- the facade's only re-exports today (`pub use noctis_rules::…` in `pub mod rules`); read-only.
- Verified tool behaviour (planning run on this repo): `cargo public-api -p noctis` prints only 19 lines (`pub mod noctis`, `pub mod noctis::rules`, 17 `pub use noctis::rules::…`) because rustdoc JSON never inlines another crate's items, even with `#[doc(inline)]`; `-p noctis-rules` with the two omits prints 1161 lines; `target/public-api/doc/noctis.json` has `format_version` 61, `root`, and `index[id].inner.use = { source, name, id, is_glob }` under `index[id].inner.module.items`; the assembled snapshot has 1161 lines and no `noctis_` leak; `cargo +nightly-2026-10-15` overrides an inherited `RUSTUP_TOOLCHAIN=1.97.0`; plain `cargo public-api` on stable panics.
- Current skill gap under M1-M3: only `noctis::rules::RuleId::REG_06` (catalog REG-06, documentation-only) is unmentioned.
- `examples/project-template/.claude/skills/noctis/SKILL.md` l.22-26 -- the `Status: v0` note and `API hash: pending` line.
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` l.159-163 -- REG rows; l.121 documents `RuleId` consts as "the id with `-` → `_`" (basis for M3).
- `examples/project-template/src/**`, `tests/**` -- 12 files, all lex with proc-macro2; only Responder use is `ComponentResponder<OpensModal>` in `src/modules/profile.rs` l.126 (D11).
- `.github/workflows-disabled/ci.yml` -- copy the `rules-catalog` job for `skill-check`; `api-snapshot` job adds nightly + tool install steps.
- `AGENTS.md` "Running and verifying" l.~25 and "Known pitfalls"; Policy line on public API changes.
- Previous story (1.7, done): rules-catalog patterns (pending TOML, CLI tests over generated repos, sorted problem output); its `rules-pending.toml` `[[check]]` list stays untouched (a new check there would fail P10 of 1.7).

## Tasks & Acceptance

**Execution:**
- `xtask/Cargo.toml` -- add the three deps -- JSON, hash, lexer with line numbers.
- `xtask/src/public_api.rs` -- module docs (line grammar, item vs owner, impl context, hash); `parse(text) -> Result<Vec<Line>, Vec<(usize, String)>>` implementing L1-L16; `items(&[Line])` (non-header, non-trait-member lines → item paths); `hash(bytes) -> String` (blake3 lowercase hex); unit tests for every L row.
- `xtask/src/api_snapshot.rs` -- `run(bless: bool) -> ExitCode` (S1-S6); tool check, per-crate runs (facade first, then every internal crate a mapping reaches, each once), `reexports_from_rustdoc_json(&str, crate_name)` (A5, A11), pure `assemble` (A1-A10, A12) and `diff`; unit tests for A2-A12 and the diff on synthetic lines/JSON.
- `xtask/src/skill_check/{mod.rs,mention.rs,hash.rs,pending.rs,doctrine.rs}` -- `run(root) -> ExitCode`; M1-M9, H1-H5, P1-P7, D1-D18; module docs state each rule and that the component-entry check is Story 4.1's; unit tests for word splitting, identifier matching, M3, every D row on inline sources.
- `xtask/src/main.rs` -- `api-snapshot [--bless]` and `skill-check [--root <path>]` arms, usage text, docs (S7, X3).
- `xtask/public-api.txt` -- generated by `api-snapshot --bless`, committed (1161 lines at planning time: the 2 facade `pub mod` lines plus every `noctis-rules` line except `pub mod noctis_rules` and `pub mod noctis_rules::len`; a different count means the assembly deviates from A1-A4).
- `xtask/skill-pending.toml` -- header comment (purpose, schema, "Story 4.1 removes this entry when it implements the check") and the `component-entries` / `4.1` entry.
- `xtask/tests/skill_check.rs` -- CLI tests over generated roots (snapshot, `skill-pending.toml`, skill dir, template `src/`) for each M, H, P, D (one combined bad template), X row, plus `real_repo_passes` (no `--root`, exit 0, stderr starts `skill-check: ok (`).
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` -- add `| REG-06 | ≤ 100 permission overwrites per command per guild; needs a Bearer token (out of scope in v1) | — |` after REG-05 -- closes the only mention gap.
- `examples/project-template/.claude/skills/noctis/SKILL.md` -- replace the hash lines with `> **API hash:** \`<hash>\` (blake3 of noctis-rs \`xtask/public-api.txt\`; updated in the same commit as every public API change, together with this skill).` -- H6 and the AC wording.
- `AGENTS.md` -- add `api-snapshot` and `skill-check` to the local gates; Policy: a public API change also re-blesses `xtask/public-api.txt` and updates the skill's `API hash`; Known pitfalls: a new container lacks `nightly-2026-10-15` and `cargo-public-api` 0.52.0, install them with the S5 commands.
- `.github/workflows-disabled/ci.yml` -- jobs `api-snapshot` ("API snapshot": `rustup toolchain install nightly-2026-10-15 --profile minimal`, `cargo install cargo-public-api --version 0.52.0 --locked`, `cargo run --release -p xtask -- api-snapshot`) and `skill-check` ("Skill check": `cargo run --release -p xtask -- skill-check`), same setup steps as `rules-catalog`.

**Acceptance Criteria:**
- Given the repo, when `cargo run --release -p xtask -- api-snapshot` runs, then it exits 0 with `api-snapshot: ok (1161 lines, hash <h>)`, and `<h>` equals the code span in `SKILL.md`'s `API hash` line.
- Given a copy of the repo with one new `pub fn` in `noctis-rules`'s `component` module, when `api-snapshot` runs there, then it exits 1 printing the `+` line and the stale message; after `--bless`, `skill-check` there exits 1 naming the stale hash and, unless the skill mentions the fn, the unmentioned item.
- Given the repo, when `cargo run --release -p xtask -- skill-check` runs, then it exits 0 with `skill-check: ok (… public items mentioned, 12 template files linted, 1 pending check)`.
- Given `SKILL.md`, when read, then the `API hash` line holds the 64-hex hash and says it is updated with every public API change (no "pending", no "once published").
- Given the gates, when they finish, then `target/debug` does not exist and `target/public-api/doc/noctis.json` exists.
- Given `.github/workflows-disabled/ci.yml`, when read, then `api-snapshot` and `skill-check` jobs exist.

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions (logged to the memlog):** cargo public-api cannot inline cross-crate re-exports, so the snapshot inlines internal crates' APIs through the facade's rustdoc-JSON `use` items (the only way to make `noctis::rules::*` signature changes visible); `--all-features` and omitting blanket/auto-trait impls (derived impls stay, so a derive change changes the hash); snapshot at `xtask/public-api.txt` beside `rules-pending.toml`; hash = blake3 (Stack crate) of the file; "mentioned" = the item's last segment as a whole identifier anywhere in the skill's Markdown, trait-impl members excluded, rule-id consts also matched by their catalog id; doctrine lint is token-level (proc-macro2), scoped to `#[command]`/`#[user_command]`/`#[message_command]`/`#[handler]` fns for UD-1 `LegacyMessage`/`content` and UD-3 responder rules, file-wide for embeds, response types, collectors, await loops and listeners; component-entry check pending in its own `xtask/skill-pending.toml`, due when a component-builder type appears outside `noctis::rules`.

Weakness accepted: name-only mention matching lets a common member name (`new`) pass through any mention; its parent type must still be mentioned, and the hash forces a deliberate skill touch on every API change.

## Verification

**Commands** (repo root; CI is paused, these local gates are the whole check; prerequisites: `rustup toolchain install nightly-2026-10-15 --profile minimal` and `cargo install cargo-public-api --version 0.52.0 --locked`):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (incl. xtask `skill_check` tests and unit tests).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, `api-snapshot: ok (1161 lines, hash …)`.
- `cargo run --release -p xtask -- skill-check` -- expected: exit 0, `skill-check: ok (…)`.
- `cargo run --release -p xtask -- rules-catalog` and `-- deps-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0.
- `test ! -d target/debug && test -f target/public-api/doc/noctis.json` -- expected: true.
