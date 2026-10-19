---
title: 'Story 1.7: trybuild harness and rules-catalog cross-check'
type: 'feature'
created: '2026-10-19'
status: 'in-progress'
baseline_revision: 'b3e6d993d2dab8bccdaed7689e8208f1a8e8fbd4'
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

**Problem:** Nothing proves that every rule row of `docs/discord-rules.md` has a test at its enforcement point, and the facade has no trybuild harness, so the catalog and the code can silently diverge (FR-75, FR-79, NFR-10, AD-4).

**Approach:** Add the facade's trybuild `compile_fail` harness (with a dev-profile proof and a toolchain guard), a `tests/boundary/` pass suite, and `cargo run --release -p xtask -- rules-catalog`, which parses the catalog's Enforcement column, collects fixtures, `compile_fail` doctests and rule-citing unit tests, honours `xtask/rules-pending.toml`, checks the `custom_id` constants table (pending for Story 2.2), and reports every problem at once.

## Boundaries & Constraints

**Always:**
- Every cargo command uses `--release`; trybuild's own `cargo check` runs are covered by `.cargo/config.toml`, which is never edited. `test ! -d target/debug` holds after all gates.
- trybuild suites are `compile_fail` only (no `.pass(`). Fixtures live only in `crates/noctis/tests/ui/<area>/<RULE-ID>_<slug>.rs` and `crates/noctis/tests/ui-<feature>/<area>/<RULE-ID>_<slug>.rs`, `<area>` = the id's area lowercased (`CV2-05` → `cv2`), slug `[a-z0-9]+(_[a-z0-9]+)*`, `.stderr` beside each `.rs`.
- Evidence kinds per row (derived, never hand-listed): `diagnosed`, `by-construction`, `unit`. Classification rules exactly as in the matrix; on the real catalog it must yield by-construction = exactly CV2-01, CV2-02, CV2-12, CV2-14, CID-05, CID-07, INT-03, INT-04, INT-06, INT-08, INT-09, INT-10, MSG-03, MSG-05, MSG-07, WH-01, WH-03, META-05, OPT-12, OPT-14, CTX-02, CTX-03, ATT-05, ATT-07, GW-07 (Story 14.4's list); documentation-only = CMD-14, LOC-04, REG-06, MSG-02; aliases = LOC-05 → CMD-05, MOD-08 → CID-01.
- Output: one line per problem on stderr, `rules-catalog: <subject>: <message>` (subject = rule id, `custom-id constant <NAME>`, a repo-relative path, or `rules-pending.toml`), sorted and deduplicated, exit 1; success prints `rules-catalog: ok (<R> rules: <C> covered, <P> pending, <D> documentation-only, <A> aliases)` on stderr, exit 0; usage errors exit 2. Use `eprintln!` (clippy `print_stdout` is denied).
- xtask depends on no internal crate (deps-check: xtask may depend only on `noctis`; it needs none). New xtask deps come from `[workspace.dependencies]`: `pulldown-cmark` (0.13, resolves 0.13.4), `toml` (1.1, resolves 1.1.8), `serde` (derive). New `noctis` dev-deps: `trybuild` (1.0, resolves 1.0.122), `serde_json`. All licenses are already allowed by `deny.toml`.
- No public API change: `examples/project-template/` and the Usage Skill stay untouched.

**Never:**
- No new rule rows, rule functions or `Limits` fields; no edits to `crates/noctis-rules/`.
- No trybuild `pass` cases, no debug builds, no `syn`-based parsing (text scanning suffices), no dependency of xtask on `noctis-rules`.
- Do not re-enable CI (`.github/workflows-disabled/` stays disabled); only keep `ci.yml` there in sync.
- Do not validate story ids against `epics.md` (format check only); do not require the aliasing id to be cited by the target's tests.

## I/O & Edge-Case Matrix

Rows R1-R14: Enforcement-cell classification (`classify`, after pulldown-cmark renders the cell; code spans are kept wrapped in backticks and are opaque to the splitter).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| R1 diagnosed | `Compile-time` / `Compile-time (literal)` | {diagnosed} | — |
| R2 by construction | `Compile-time (by construction)`, `(types, by construction)`, `(by construction: Renderer output enum)` | {by-construction} | — |
| R3 `;` clause | `Compile-time (`#[command]`/`#[handler]` check; typestate by construction) / Runtime (…)` (INT-05) | {diagnosed, unit}: only text before the first `;` decides | — |
| R4 multi tier | `Compile-time (literal) / App-build (loaded locales)` | {diagnosed, unit} | — |
| R5 unit heads | `App-build`, `Runtime`, `Startup (cluster config) / Runtime`, `App-build or Runtime (localized)`, `Runtime (by construction)` | {unit} | — |
| R6 nested / code parens | CMD-12 `Compile-time (one pattern, e.g. `config (set)?`) / App-build (…)`; GW-01 code span with `/` | split only at ` / ` at paren depth 0 outside code | — |
| R7 escaped pipes | CMD-16, META-01 rows with `\|` in code spans | row keeps 5 cells; Enforcement read correctly | — |
| R8 doc-only | `(permitted; no check)`, `(out of scope)`, `(no embed API in v1; documented)` | documentation-only, no requirement | — |
| R9 alias | `see CMD-05`, `as CID-01` | alias; no requirement | target unknown, alias or doc-only → `<ID>: alias target <T> …` error |
| R10 unknown head | `Linktime`, `compile-time` (lowercase) | — | `<ID>: cannot parse Enforcement "<cell>"` |
| R11 empty / malformed | empty cell, `Compile-time / ` (empty tier), unbalanced `(`, `Compile-time (x) y` (text after `)`) | — | same parse error |
| R12 duplicate kinds | `Compile-time (literal) / Compile-time (by construction)` | {diagnosed, by-construction} | — |
| R13 parenthesis then tier | `(x) / Runtime` | not doc-only (cell is not one group) → parse error | parse error |
| R14 unicode | cells with `ʼ`, `≤`, `…` | parsed unchanged | — |

Rows C1-C6: catalog table parsing.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| C1 real catalog | `docs/discord-rules.md` | 125 rule rows in order; tier-table rows (`**Compile-time**`) skipped because the bold text is not `^[A-Z][A-Z0-9]{1,3}-[0-9]{2}$` | — |
| C2 missing file | no catalog at root | — | `docs/discord-rules.md: cannot read …` |
| C3 no Enforcement column | rule row in a table whose header lacks `Enforcement` | — | `<ID>: table has no Enforcement column` |
| C4 duplicate id | two `**CMD-01**` rows | — | `<ID>: duplicate catalog row` |
| C5 short row | rule row with fewer cells than the header | — | `<ID>: row has no Enforcement cell` |
| C6 zero rows | catalog without rule rows | — | `docs/discord-rules.md: no rule rows found` |

Rows F1-F14: fixtures (`crates/noctis/tests/ui*`), suite registration (`crates/noctis/tests/ui.rs`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| F1 diagnosed ok | `ui/cv2/CV2-05_x.rs` + `.stderr` containing `[CV2-05]` | diagnosed evidence for CV2-05 | — |
| F2 diagnosed, id missing in stderr | same, `.stderr` lacks `[CV2-05]`, row has no by-construction tier | — | `<path>: .stderr does not contain [CV2-05]` |
| F3 by-construction ok | `ui/msg/MSG-03_all_with_ids.rs` with `let _ = MentionIds::All(&[1_u64]); // [MSG-03]` | by-construction evidence | — |
| F4 tag alone on a line | `// [MSG-03]` with only whitespace before it | not evidence | `<path>: no line carries // [MSG-03] after code` |
| F5 wrong tag id | source tags `// [MSG-04]` in an `MSG-03_*` fixture | not evidence for MSG-03 | F4 error |
| F6 bad name | `ui/msg/msg-03_x.rs`, `MSG-03.rs`, `MSG-03_.rs`, `MSG-03_Bad.rs`, `ui/MSG-03_x.rs` (no area), `ui/a/b/MSG-03_x.rs` (too deep) | — | `<path>: fixture must be <suite>/<area>/<RULE-ID>_<slug>.rs` |
| F7 area mismatch | `ui/cmd/MSG-03_x.rs` | — | `<path>: area directory must be msg` |
| F8 unknown id | `ui/zzz/ZZZ-01_x.rs` | — | `<path>: ZZZ-01 is not a catalog rule` |
| F9 no compile tier | `ui/int/INT-01_x.rs` (Runtime only), or alias/doc-only id | — | `<path>: INT-01 has no Compile-time tier` (aliases add `; use <target>`) |
| F10 missing / orphan stderr | `.rs` without `.stderr`; `.stderr` without `.rs`; any other file (e.g. `.txt`) | — | `<path>: missing .stderr` / `orphan .stderr` / `unexpected file` |
| F11 feature suite | `ui-image/…` dir | validated like `ui/`; `image` must be a `[features]` key of `crates/noctis/Cargo.toml` and `ui.rs` must contain `compile_fail("tests/ui-image/**/*.rs")` | `<path>: unknown facade feature …` / `crates/noctis/tests/ui.rs: no compile_fail suite for tests/ui-image` |
| F12 registration | `ui.rs` lacks `compile_fail("tests/ui/**/*.rs")`, contains `.pass(`, omits `feature = "<f>"` for a facade feature other than `default`, or is missing | — | `crates/noctis/tests/ui.rs: …` one line per problem |
| F13 stray suites | `crates/noctis-macros/tests/ui/…` exists | — | `<path>: trybuild fixtures belong in crates/noctis/tests/ui` |
| F14 empty suite | `crates/noctis/tests/ui/` absent or empty | no fixture evidence; no error by itself | — |

Rows D1-D7: `compile_fail` doctests (`///` and `//!` blocks in `crates/*/src/**/*.rs`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| D1 tagged | ```` ```compile_fail ```` block with `let _ = Spacing::Medium; // [CV2-13]` | by-construction evidence for CV2-13 (counts only where the row has that kind) | — |
| D2 info strings | ```` ```compile_fail,E0599 ````, ```` ```rust,compile_fail ````, `~~~compile_fail` | counted | — |
| D3 ignored / other | ```` ```compile_fail,ignore ````, ```` ```rust ````, ```` ```text ```` | not counted | — |
| D4 tag outside fence or alone | tag in prose, or `// [CV2-13]` with no code before it | not counted | — |
| D5 block ends | a non-doc line closes an open fence | later lines not counted | — |
| D6 unknown id tag | `// [ZZZ-99]` in a compile_fail block or fixture | — | `<path>:<line>: ZZZ-99 is not a catalog rule` |
| D7 hidden line | `# let _ = x; // [CV2-13]` | counted | — |

Rows U1-U8: unit tests (`crates/*/src/**/*.rs`, `crates/*/tests/**/*.rs`, excluding `crates/noctis/tests/ui*/`; `xtask/` and `examples/` are never scanned).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| U1 cited | `#[test]` then `fn cv2_03_forty_one_fails()` | unit evidence for CV2-03 | — |
| U2 token positions | names `cv2_03`, `x_cv2_03`, `x_cv2_03_y`, `meta_01_and_meta_02_conflict` | cite every embedded id | — |
| U3 near misses | `cv2_031_x`, `xcv2_03`, `cv2_3_x`, `CV2_03` (uppercase) | not cited | — |
| U4 attributes between | `#[test]` / `#[should_panic]` / `/// doc` / `fn …` within 10 lines | counted | — |
| U5 ignored | `#[ignore]` between `#[test]` and `fn` | not counted | — |
| U6 async / proptest | `#[tokio::test]`, `#[tokio::test(flavor = "…")]`, `#[test]` inside `proptest! { … }` | counted | — |
| U7 in doc comment | `/// #[test]` | not counted | — |
| U8 non-UTF-8 file | invalid UTF-8 `.rs` | — | `<path>: not UTF-8` |

Rows P1-P11: `xtask/rules-pending.toml`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| P1 pending ok | `[[rule]] id = "CV2-03" evidence = "diagnosed" story = "4.4"`, no fixture | requirement treated as pending | — |
| P2 satisfied pending | same entry and a fixture whose `.stderr` has `[CV2-03]` | — | `CV2-03: pending entry (diagnosed, Story 4.4) already has a test; remove it from xtask/rules-pending.toml` |
| P3 missing, not pending | requirement unmet, no entry | — | `CV2-03: Compile-time row has no fixture whose .stderr contains [CV2-03]` / `…Compile-time (by construction) row has no fixture or compile_fail doctest carrying // [CV2-01]` / `…App-build or Runtime row has no unit test citing cv2_03` |
| P4 unknown id | `id = "ZZZ-01"` | — | `rules-pending.toml: ZZZ-01 is not a catalog rule` |
| P5 kind not required | `id = "CMD-14"` (doc-only), `id = "LOC-05"` (alias), or `INT-01` with `evidence = "diagnosed"` | — | `rules-pending.toml: INT-01 does not require diagnosed evidence` |
| P6 bad evidence / key | `evidence = "runtime"`, extra key `note = ""`, unknown table | — | `rules-pending.toml: <toml/serde message>` |
| P7 story format | `story = "4"`, `"4.4.1"`, `"0.3"`, `"4.0"`, `"v4.4"`, `""` | — | `rules-pending.toml: <ID>: story must be <epic>.<story>` (`^[1-9][0-9]*\.[1-9][0-9]*$`) |
| P8 duplicate | same (id, evidence) twice; or `[[check]]` name twice | — | `rules-pending.toml: duplicate entry …` |
| P9 empty / missing | file with only comments | no pending; every unmet requirement fails | missing file → `xtask/rules-pending.toml: cannot read …` |
| P10 unknown check | `[[check]] name = "other"` | — | `rules-pending.toml: unknown check other` |
| P11 invalid TOML | syntax error | — | `rules-pending.toml: <parse message>`; other checks still run with no pending entries |

Rows K1-K7: `custom_id` constants (table headed `Constant (`noctis-codec`)` | `Value`; `pub const NAME: <ty> = <int>;` in `crates/noctis-codec/src/**/*.rs`, `_` separators allowed).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| K1 pending, none defined | `[[check]] name = "custom-id-constants" story = "2.2"`, codec empty | comparison skipped | — |
| K2 pending, one defined | pending and `pub const REF_LEN: usize = 11;` in codec | — | `custom-id constant REF_LEN: noctis-codec defines it; remove the custom-id-constants pending entry` |
| K3 not pending, all equal | 8 consts matching | ok | — |
| K4 mismatch | `pub const REF_LEN: usize = 12;` | — | `custom-id constant REF_LEN: catalog says 11, noctis-codec says 12` |
| K5 missing / doubled | const absent, or defined twice | — | `custom-id constant TAG_LEN: not defined in noctis-codec` / `defined 2 times` |
| K6 table broken | table absent, empty, non-integer value, duplicate name, name not `[A-Z][A-Z0-9_]*` | — | `docs/discord-rules.md: custom_id constants table …` (always checked, pending or not) |
| K7 not `pub` | `const REF_LEN` or `pub(crate) const` | not a definition | K5 when not pending |

Rows X1-X4: CLI.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| X1 default root | `rules-catalog` | root = `CARGO_MANIFEST_DIR/..`; real repo exits 0 | — |
| X2 `--root <dir>` | fixture repo | checks that tree | missing inputs → exit 1 with messages |
| X3 bad args | `--root` without value, extra argument, unknown flag | — | message + usage, exit 2 |
| X4 many problems | several failures | all reported, sorted, deduped, exit 1 | — |

Rows T1-T5: facade harness (`crates/noctis/tests/ui.rs`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| T1 default features | `cargo test --release -p noctis` | `ui` runs trybuild on `tests/ui/**/*.rs`; MSG-03 fixture matches its blessed `.stderr` | trybuild panics on mismatch |
| T2 any optional feature on | `cargo hack … --each-feature` with e.g. `image` | featureless `ui` test is compiled out | — |
| T3 dev profile proof | after the suite | every `compiler-artifact` profile in the re-run is `opt_level "3"`, `debuginfo 0`, `debug_assertions false`, `overflow_checks false`, and at least one artifact is seen | panic naming the offending package and profile |
| T4 toolchain guard | `rustc -V` differs from `rust-toolchain.toml` `channel` | — | panic: run on the pinned toolchain; a bump re-blesses with `TRYBUILD=overwrite` |
| T5 boundary | `cargo test --release -p noctis --test boundary` | `msg_03_typed_mentions_validate` passes | — |

</intent-contract>

## Code Map

- `xtask/src/main.rs` -- task dispatch (l.24-44), `USAGE` (l.13), module docs (l.1-7, drop "`rules-catalog` … arrive in later stories" for rules-catalog), `deps_check_manifest` arg-parsing style to copy for `--root`; `USAGE_ERROR = 2`.
- `xtask/src/deps_check.rs` -- style to copy: module docs, violation struct with `Display`, `BTreeSet` sort/dedupe, `run(..) -> ExitCode`, `eprintln!` output, summary `deps-check: ok (…)`.
- `xtask/tests/deps_check.rs` -- CLI test style to copy: `env!("CARGO_BIN_EXE_xtask")`, fixture trees under `env!("CARGO_TARGET_TMPDIR")/<test>`, `assert_exit(out, code, needles)`, crate-level `#![allow(clippy::expect_used, reason = …)]`.
- `xtask/Cargo.toml` -- add `pulldown-cmark.workspace = true`, `toml.workspace = true`, `serde.workspace = true`.
- `docs/discord-rules.md` -- rule tables §1-§15 (columns Rule | Limit/value | Source | Enforcement | How); constants table l.305-314; §6 "trybuild compile-fail tests per rule" l.502-538 (tests layout, alias sentence l.536-538).
- `crates/noctis-rules/tests/catalog.rs` -- existing id-shape helper `is_rule_id` (reuse the same regex shape in xtask); keeps passing unchanged.
- `crates/noctis/Cargo.toml` -- `[features]` (default, image, image-gif, image-emoji, default-font, i18n, prefix, testkit); add `[dev-dependencies] trybuild.workspace = true`, `serde_json.workspace = true`.
- `crates/noctis/src/lib.rs` l.119 -- `noctis::rules::message` re-export used by the fixture and boundary test (read-only).
- `crates/noctis-rules/src/message.rs` -- `MentionIds` (l.26), `AllowedMentions::new/users/roles` (l.52-71), `validate_mentions` (l.113); existing `// [MSG-03]` doctest l.10-13 (read-only).
- `.github/workflows-disabled/ci.yml` -- copy the `deps-check` job (l.~108) as `rules-catalog`.
- trybuild 1.0.122 internals relied on (read in its source): project dir `<target>/tests/trybuild/noctis/`, bin `noctis-tests`, cargo env `CARGO_TARGET_DIR=<target>/tests/trybuild`, `--offline`, `--target <host>`, `CARGO_INCREMENTAL=0`; an empty glob prints "no tests enabled" and passes.
- Previous story (1.6, done): rule functions and `// [ID]`-tagged `compile_fail` doctests (CTX-02, CTX-03, CV2-07, CV2-08, CV2-13, META-05, MOD-03, MSG-03, OPT-14); unit/integration tests named with snake ids (`msg_05_default_parses_nothing`). A prototype scan of the current tree with the rules above finds exactly the 111 unmet requirements listed in Design Notes.

## Tasks & Acceptance

**Execution:**
- `xtask/Cargo.toml` -- add the three deps -- parser, pending file, serde derive.
- `xtask/src/rules_catalog.rs` (or `rules_catalog/{mod,catalog,evidence,pending}.rs`) -- `pub fn run(root: &Path) -> ExitCode` implementing every matrix row R/C/F/D/U/P/K/X; module docs describe the classification, evidence kinds, layout and output format; unit tests for `classify` (R1-R14), the tier splitter, the doctest scanner (D1-D7), the test-name matcher (U1-U7), and `real_catalog_classification` (reads `../docs/discord-rules.md`: 125 rows, the 25 by-construction ids, 4 doc-only, 2 aliases).
- `xtask/src/rules_catalog.rs` (pending model) -- `#[serde(deny_unknown_fields)]` structs `Pending { #[serde(default)] rule: Vec<RuleEntry>, #[serde(default)] check: Vec<CheckEntry> }`, `Evidence` enum with `#[serde(rename_all = "kebab-case")]` (`Diagnosed`, `ByConstruction`, `Unit`), shared by the classifier; story ids validated after parsing (P7).
- `xtask/src/main.rs` -- `rules-catalog [--root <path>]` dispatch, usage text, docs -- CLI.
- `xtask/rules-pending.toml` -- header comment (purpose, schema, "the list only shrinks; empty by Story 14.4"), the 111 `[[rule]]` entries of Design Notes in catalog order, and `[[check]] name = "custom-id-constants" story = "2.2"`.
- `xtask/tests/rules_catalog.rs` -- CLI tests over generated repos (minimal catalog with the tier header and a few rows, `crates/noctis/Cargo.toml` with features, `ui.rs`, fixtures, `crates/x/src/lib.rs`, pending file) covering each F, P, K, X row and R9-R11/C2-C6 through the binary, plus `real_repo_passes` (no `--root`, exit 0, stderr starts `rules-catalog: ok (125 rules:`).
- `crates/noctis/Cargo.toml` -- dev-deps `trybuild`, `serde_json`.
- `crates/noctis/tests/ui.rs` -- `//!` docs (layout, blessing command `TRYBUILD=overwrite cargo test --release -p noctis --test ui`, feature suites added by the first story needing them); `#[cfg(not(any(feature = "image", feature = "image-gif", feature = "image-emoji", feature = "default-font", feature = "i18n", feature = "prefix", feature = "testkit")))] #[test] fn ui()` that checks the toolchain (T4), runs `trybuild::TestCases::new().compile_fail("tests/ui/**/*.rs")` in an inner scope (it runs on drop), then calls `assert_trybuild_dev_profile()` (T3: `CARGO` env or `cargo`, `check --message-format=json --offline --target <host from rustc -vV> --bin noctis-tests`, cwd `<target>/tests/trybuild/noctis`, `CARGO_TARGET_DIR=<target>/tests/trybuild`, `CARGO_INCREMENTAL=0`, `<target>` = parent of `env!("CARGO_TARGET_TMPDIR")`; parse each stdout line with `serde_json`).
- `crates/noctis/tests/ui/msg/MSG-03_all_with_ids.rs` + `.stderr` -- `use noctis::rules::message::MentionIds; fn main() { let _ = MentionIds::All(&[1_u64]); // [MSG-03] }` (rustfmt-formatted), stderr blessed on 1.97.0 -- first fixture, exercises the harness.
- `crates/noctis/tests/boundary.rs` + `crates/noctis/tests/boundary/msg.rs` -- `//!` docs ("boundary passes, one module per area"); `mod msg;`; `#[test] fn msg_03_typed_mentions_validate()`: `AllowedMentions::new().users(MentionIds::All).roles(MentionIds::Only(&[1_u64; 100]))` gives no violations with `Limits::default()`.
- `docs/discord-rules.md` §6 -- state the classification rules (first-`;` clause, `Runtime (by construction)` needs a unit test), the fixture naming/area rule, unit-test citation rule (snake id token in the `#[test]` fn name), `xtask/rules-pending.toml`, and replace "which also cite the aliasing id" with "an alias row needs no test of its own".
- `AGENTS.md` -- add `cargo run --release -p xtask -- rules-catalog` to the local gate list in "Running and verifying" -- it is a gate from this story.
- `.github/workflows-disabled/ci.yml` -- add job `rules-catalog` (name "Rules catalog") running `cargo run --release -p xtask -- rules-catalog`, same setup steps as `deps-check`.

**Acceptance Criteria:**
- Given the repo, when `cargo run --release -p xtask -- rules-catalog` runs, then it exits 0 with exactly `rules-catalog: ok (125 rules: 16 covered, 111 pending, 4 documentation-only, 2 aliases)` (covered = requirement-bearing rows with every requirement met and none pending; pending = number of pending entries, spanning 103 rows), both definitions stated in the module docs.
- Given a copy of the repo where any one pending entry is removed, or a fixture/test is added for a pending entry, when `rules-catalog --root <copy>` runs, then it exits 1 naming that id (the `xtask/tests` P2/P3 cases).
- Given default features, when `cargo test --release -p noctis` runs, then the `ui` test runs the MSG-03 fixture against its `.stderr`, proves the dev profile (T3) and the `boundary` test passes; given `cargo hack test --release --workspace --each-feature`, then every run passes and the featureless suite is skipped when an optional feature is on.
- Given the gates, when they finish, then `target/debug` does not exist and `target/tests/trybuild/` exists.
- Given `.github/workflows-disabled/ci.yml`, when read, then a `rules-catalog` job exists.

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions (logged to the memlog):** classification by the first `;` clause (keeps INT-05 diagnosed, matching Story 14.4's 25 by-construction rows); `Runtime (by construction)` rows (LOC-06, INT-12, MSG-08, ATT-04) need unit tests; all seven optional facade features gate the featureless suite (spine: "any optional facade feature"); the constants comparison is implemented now and skipped while pending; the dev-profile proof re-runs cargo in trybuild's own project and target dirs (a flag mismatch only costs a re-check, never a wrong verdict); a `rustc` guard makes toolchain bumps re-bless.

**Pending list (`<story> <evidence>: ids`), 111 entries; story picked per evidence kind from the epics' Implements lines and ACs:**
2.1 by-construction: CID-07 · 2.2 unit: CID-06 · 2.3 by-construction: CID-05 · 2.3 unit: CID-05 · 2.5 diagnosed: CID-01, CID-03 · 4.1 by-construction: CV2-12, CV2-14 · 4.1 diagnosed: CV2-05, CV2-07 · 4.2 by-construction: INT-09, CV2-01, CV2-02, MSG-05, MSG-07 · 4.2 diagnosed: MSG-01 · 4.4 diagnosed: CID-02, CV2-03, CV2-04, CV2-06, CV2-08, CV2-09, CV2-10, CV2-11, CV2-13, CV2-15, CV2-17 · 4.5 diagnosed: MOD-03 · 4.6 diagnosed: MOD-01, MOD-02, MOD-04, MOD-05, MOD-06, MOD-07 · 4.6 unit: MOD-09 · 5.4 unit: GW-01, GW-02, GW-03 · 5.6 unit: GW-05 · 6.1 diagnosed: CMD-01, CMD-02, CMD-03, CMD-04, CMD-05, CMD-10, CMD-11, CMD-12, CMD-13, CMD-15, CMD-16 · 6.2 diagnosed: CMD-06, OPT-01..OPT-11, OPT-13 · 6.3 diagnosed: CMD-07, CTX-01, META-01, META-02, META-03 · 6.5 by-construction: INT-03, INT-04, INT-08, INT-10 · 6.5 unit: LOC-06, INT-12 · 6.6 diagnosed: INT-05 · 6.6 unit: INT-01, INT-02, INT-03, INT-05, INT-11 · 6.7 by-construction: OPT-12, INT-06 · 6.7 unit: OPT-12, INT-06 · 6.9 unit: META-04, REG-05 · 7.1 unit: CID-04 · 9.1 by-construction: ATT-05 · 9.1 diagnosed: ATT-06 · 9.1 unit: MSG-08, ATT-04 · 9.5 by-construction: WH-01, WH-03 · 9.5 diagnosed: WH-02, WH-04 · 9.5 unit: WH-05 · 10.4 by-construction: ATT-07 · 11.1 diagnosed: LOC-01 · 11.4 diagnosed: LOC-02, LOC-03 · 11.4 unit: LOC-02, LOC-03 · 12.4 unit: INT-13, RL-01, RL-02, RL-03, RL-04 · 15.3 by-construction: GW-07 · 15.3 diagnosed: GW-06 · 15.3 unit: GW-04, GW-07.

If the implemented scanner finds a different unmet set than this list, the scanner is wrong unless the difference is explained by a matrix row; fix the scanner, not the list.

## Verification

**Commands** (repo root; CI is paused, these local gates are the whole check):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (incl. `noctis` `ui` and `boundary`, xtask `rules_catalog` tests).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- rules-catalog` -- expected: exit 0, `rules-catalog: ok (125 rules: …)`.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `bash .github/scripts/check-build-config.sh` -- expected: exit 0 (`.cargo/config.toml` untouched).
- `test ! -d target/debug && test -d target/tests/trybuild` -- expected: true.
