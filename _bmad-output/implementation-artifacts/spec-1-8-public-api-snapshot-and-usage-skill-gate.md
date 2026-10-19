---
title: 'Story 1.8: Public API snapshot and Usage Skill gate'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: 'be651abca41d363ba1a4f81c9b435d1b2954128f'
review_loop_iteration: 0
followup_review_recommended: true
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

### 2026-10-19 — Review pass
- verdicts: 46 findings — high 0, medium 8, low 24, false 14, maybe-false 0
- findings:
  - `[low]` `[reject]` blind: `diff()` is a set diff, so reorder/duplicate-only changes print no `-`/`+` lines — real (duplicate `fmt` lines exist), but adding a type's impl always shows its new header line; pure reorders are rare and the fix adds a branch; `git diff` after `--bless` shows it.
  - `[medium]` `[patch]` blind: `expand()` rewrites every re-export of the same source to the same facade path — verified: `mappings` gets two entries with one source and `rewrite` uses `find`; the spine (l.138) plans `noctis::prelude` re-exports, so this is reachable. Patch: rewrite each block with its own mapping first.
  - `[medium]` `[patch]` blind: `owned_by()` drops impls with a foreign self type — verified: owner = self path, so `impl From<noctis_rules::X> for alloc::string::String` matches no re-export and is silently omitted. Patch: include impl blocks whose header names the source.
  - `[medium]` `[patch]` blind: two globs in one module collapse — verified: `line_path()` is `M::*` for both and `find` returns the first. Patch: match `pub use` lines to re-exports one to one.
  - `[false]` `[reject]` blind: `expand()` has no cycle guard — a re-export cycle needs crates that re-export each other, which needs a cargo dependency cycle; cargo rejects those, so the recursion cannot loop.
  - `[low]` `[reject]` blind: `parse_impl` fails on tuple/slice/array/fn/dyn self types — no such impl exists in the 1161-line snapshot; a tuple/slice one fails loudly with "cannot parse", and `impl dyn noctis::Trait` is rare; the fix adds several branches.
  - `[low]` `[reject]` blind: `skip_attributes` counts brackets inside string literals — needs an attribute printed by cargo public-api with a `[`/`]` in a string; none exists, it fails loudly, and the fix adds string/escape tracking.
  - `[false]` `[reject]` blind: the mention check uses only the last segment, so common names pass — M1 inside `<intent-contract>` settles last-segment token matching, and the Design Notes record this as an accepted weakness.
  - `[low]` `[reject]` blind: UD-7 spawn rule misses `spawn_blocking`, aliases, helpers — D17 names `spawn`/`spawn_local` idents; the lint only covers the noctis-maintained template, which spawns nothing; unlikely to be met.
  - `[low]` `[reject]` blind: UD-1 `content` struct-literal rule misses shorthand `Msg { content }` — real, but template handlers return Views and build no message struct; the fix adds branches.
  - `[low]` `[patch]` blind: `real_repo_passes` hard-codes `12 template files linted` — verified at `xtask/tests/skill_check.rs:171`; every template file added with an API change breaks it. Patch: derive the count or drop it from the assertion.
  - `[low]` `[reject]` blind: problems sort `lib.rs:10` before `lib.rs:9` — cosmetic; every problem is still reported; zero-padding or a separate line key changes the output format copied from rules-catalog.
  - `[low]` `[reject]` blind: `Repo::files` hides unreadable subdirectories and symlinks — real (`unwrap_or_default`, `file_type()` does not follow links), but a git checkout of the template has neither; the fix threads problems through the recursion and needs a symlink policy.
  - `[false]` `[reject]` blind: `--bless` leaves the hash step manual — the `<intent-contract>` Never list says `--bless` never edits `SKILL.md`; deliberate, so the skill is touched by hand.
  - `[false]` `[reject]` blind: the pending trigger misses non-struct/enum builders — P2 inside `<intent-contract>` settles the trigger as `pub struct`/`pub enum` with the listed names.
  - `[low]` `[patch]` blind: the TOML parse error drops its location — verified: `err.message()` has no line/column. Patch: report the error's `Display` output.
  - `[low]` `[reject]` blind: the CI `api-snapshot` job rebuilds cargo-public-api every run — CI is paused (`workflows-disabled`), so no one meets this today; the spec names `cargo install … --locked` for that job.
  - `[low]` `[reject]` blind: an unreadable committed snapshot reads as missing; `api-snapshot` has no `--root` — the read error case is real but needs a permission/IO fault on a committed file, and the fix adds a branch; no `--root` is settled by S7 (`--root` is a bad arg).
  - `[medium]` `[patch]` edge: duplicate re-export at two facade paths (api_snapshot.rs:403-406) — same defect as blind row 2; patched there.
  - `[medium]` `[patch]` edge: two globs in one module share `line_path` (api_snapshot.rs:380) — same defect as blind row 4; patched there.
  - `[low]` `[reject]` edge: an external glob `pub use noctis::x::*` gives an item named `*` that can never be mentioned (mention.rs:379) — real and loud, but the facade has no glob re-export today and the failure shows at once when one is added; the fix adds a branch.
  - `[low]` `[reject]` edge: a snapshot that differs only by order, duplicates or CRLF prints no `-`/`+` lines — same root cause as blind row 1; rejected for the same reason (CRLF in a committed LF file also needs a misconfigured checkout).
  - `[low]` `[reject]` edge: `fs::read` failure other than not-found is reported as stale — same as blind row 18; rejected for the same reason.
  - `[medium]` `[patch]` edge: trait impl of an internal type for a foreign self type is dropped (api_snapshot.rs:411-422) — same defect as blind row 3; patched there.
  - `[low]` `[reject]` edge: `[`/`]` inside a string literal in an attribute — same as blind row 7; rejected for the same reason.
  - `[low]` `[reject]` edge: impl self type that is a tuple/slice/array/dyn/pointer — same as blind row 6; rejected for the same reason.
  - `[false]` `[reject]` edge: non-ASCII identifier characters in a path truncate it — L15 inside `<intent-contract>` settles that only ASCII path chars matter, and noctis's API uses only ASCII identifiers.
  - `[low]` `[reject]` edge: unreadable or symlinked subdirectories are silently skipped (mod.rs:118-126) — same as blind row 13; rejected for the same reason.
  - `[low]` `[reject]` edge: handler generics with parentheses (`fn h<F: Fn(u8)>`) are taken as params — real, but `#[handler]`/`#[command]` fns are not generic over closures in noctis's handler model; the fix adds generic skipping.
  - `[false]` `[reject]` edge: unrelated `.update(`/`.content(` calls inside a handler are flagged — D6/D9 inside `<intent-contract>` settle the token-level rule: any `.content(`/`.update(` call in an interactive handler is flagged.
  - `[low]` `[reject]` edge: turbofish `r.reply::<T>(x)` and shorthand `Msg { content }` are not flagged — the shorthand part is blind row 10's defect; the turbofish form is not used by any Responder API; both fixes add branches.
  - `[low]` `[reject]` edge: closure parameters `|a, content: &str|` inside a handler body trigger a false UD-1 finding — real (preceded by `,`, followed by a lone `:`), but no template handler has such a closure, and the fix adds pipe tracking.
  - `[low]` `[reject]` edge: a listener-named call with a boolean `||` argument is a false UD-7 finding — needs `.on_x(a || b)` in the template; unlikely; the fix adds position logic.
  - `[low]` `[reject]` edge: a Responder reached through a type alias or renamed import is not flagged — deliberate evasion inside the noctis-maintained template is unlikely; the fix needs alias collection.
  - `[low]` `[patch]` edge: `real_repo_passes` hard-codes the template file count (tests/skill_check.rs:171) — same defect as blind row 11; patched there.
  - `[medium]` `[patch]` vgap: the `api-snapshot` CLI outcomes S1-S6 and crate discovery have no automated test — pre-verified gap; inverting the ok branch, accepting a missing file, a missing trailing newline or a dropped `queue.push` keep `cargo test` green. Patch: pure `verdict` fn plus S1-S3/S4 tests, and an injectable crate fetcher with a discovery test.
  - `[medium]` `[patch]` vgap other: the same internal item re-exported at two facade paths gets the same path — same defect as blind row 2; patched there.
  - `[low]` `[reject]` vgap other: a glob `pub use … ::*` line kept in the snapshot can never be mentioned — same defect as edge row 3; rejected for the same reason.
  - `[false]` `[reject]` intent: snapshot freshness is not exercised by any test — the spec deliberately keeps `api-snapshot` a nightly-only gate in `## Verification` and keeps `skill-check` nightly-free; the untested decision logic is the vgap finding above.
  - `[false]` `[reject]` intent: record vs check split into default and `--bless` — the `<intent-contract>` Approach and S1-S4 settle it.
  - `[false]` `[reject]` intent: the snapshot is post-processed, not raw tool output — the `<intent-contract>` Approach and A1-A12 settle inlining internal re-exports, since raw output only lists 19 `pub use` lines.
  - `[false]` `[reject]` intent: "mentioned" is last-segment token presence — settled by M1-M3 and the accepted weakness in the Design Notes.
  - `[false]` `[reject]` intent: the hash rule is a state equality check — settled by H1-H6 inside `<intent-contract>`.
  - `[false]` `[reject]` intent: the CI gate is defined but not live — the `<intent-contract>` Never list says no re-enabled CI, and AGENTS.md keeps CI paused.
  - `[false]` `[reject]` intent: the doctrine lint covers only template `.rs` files and flags some idents file-wide — settled by D1-D18 and the Design Notes.
  - `[false]` `[reject]` intent: the pending check triggers on a name list, not on "builder without a skill entry" — P2 inside `<intent-contract>` settles the due-trigger; the real check is Story 4.1's.

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

## Auto Run Result

- **Summary:** added `xtask api-snapshot`, which records and checks `xtask/public-api.txt` (1161 lines). It is built from cargo-public-api 0.52.0 output, with the internal crates' APIs inlined through the facade's rustdoc-JSON `use` items, `--all-features` on, and blanket/auto-trait impls left out. Also added `xtask skill-check`, which runs four checks: a blake3 hash ties the template's noctis skill to the snapshot; every public item must be mentioned (matched by its last segment); a token-level doctrine lint covers UD-1/UD-3/UD-7 and the other rules in `examples/project-template/`; and a pending component-entry check reads `xtask/skill-pending.toml`. Both run as a CI job in the paused workflow.
- **Files changed:**
  - `xtask/src/api_snapshot.rs`: snapshot generation, re-export inlining, impl ownership, verdict and diff.
  - `xtask/src/public_api.rs`: parses cargo public-api lines and impl headers.
  - `xtask/src/skill_check/{mod,hash,mention,doctrine,pending}.rs`: the skill-check gate and its four checks.
  - `xtask/src/main.rs`: wires up the subcommands. `xtask/Cargo.toml` and `Cargo.lock`: add blake3 and proc-macro2.
  - `xtask/public-api.txt`, `xtask/skill-pending.toml`: the committed snapshot and the pending list.
  - `xtask/tests/skill_check.rs`: integration tests.
  - `examples/project-template/.claude/skills/noctis/SKILL.md` and `references/limits-cheatsheet.md`: add the hash line and the missing mentions.
  - `.github/workflows-disabled/ci.yml`: the api-snapshot/skill-check job. `AGENTS.md`: updated verification notes. `xtask/src/rules_catalog/*`: small shared-helper tweaks.
- **Review findings:** 46 total (high 0, medium 8, low 24, false 14).
  - Patched: 8 medium entries and 3 low entries, which come down to 6 distinct fixes:
    - each re-export of the same source gets its own facade path;
    - impls whose self type is foreign but which name an internal type are kept;
    - two globs in one module are matched to their re-exports one to one;
    - a pure `verdict` fn with S1-S4 tests, plus an injectable crate fetcher with a discovery test;
    - the test no longer hard-codes the template file count;
    - TOML errors keep their location (`Display`).
  - Deferred: none.
  - Rejected: 21 low and 14 false. Each is recorded in the Review Triage Log with its reason. Main groups:
    - The set diff ignores reorders and duplicates.
    - Unusual impl self-type syntax and brackets inside attribute strings are not parsed; both fail loudly.
    - Unreadable or symlinked directories are skipped.
    - Some doctrine-lint gaps (shorthand fields, turbofish, aliases, closures, `spawn_blocking`) do not occur in the template.
    - Re-running cargo-public-api in CI only matters once CI resumes.
    - The rest were settled by the intent contract (M1, P2, D6/D9, L15, H1-H6, the Never list).
- **Follow-up review recommended: true.** This is a first pass that patched 0 high, 8 medium and 3 low entries, and 2 or more patched medium entries call for a follow-up. Named risk: the patched re-export inlining in `api_snapshot.rs` (separate mapping per re-export, foreign-self-type impls, one-to-one glob matching) has not been reviewed independently. A regression there would silently drop or misattribute public items, and the snapshot and hash would not catch it.
- **Verification:** all local gates in `/tmp/claude-0/-home-user/a6a0257c-fd4a-4940-b108-046dbf0fca6f/scratchpad/gates.sh` passed after the patches: fmt, clippy `--release -D warnings`, `cargo test --release`, cargo hack each-feature, `api-snapshot` ok, `skill-check` ok, rules-catalog, deps-check, cargo deny, MSRV 1.91 check, build-config check, and no `target/debug`. CI is paused by the maintainer (`workflows-disabled`), so these local gates are the whole check.
- **Residual risks:**
  - "Mentioned" only matches an item's last segment, so common member names such as `new` pass trivially.
  - The doctrine lint is token-level and can be evaded.
  - `api-snapshot` needs pinned nightly-2026-10-15 and cargo-public-api 0.52.0, and fails when they are missing.
  - The real component-entry check is left to Story 4.1.
  - The CI job has not run because CI is paused.
