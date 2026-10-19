# Epic 1 Context: Project foundation and Discord rules as code

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Lay down the Cargo workspace for Noctis, a Rust Discord bot framework on twilight, exactly as the architecture's structural seed describes, with every CI quality gate in place from the start. Then put every Discord limit and rule into one tested, I/O-free crate, `noctis-rules`, that proc macros and runtime validators both call, so the two can never disagree. CI checks that the rules catalog (`docs/discord-rules.md`) matches the tests, and it catches Usage Skill and Project Template drift from the first public item. Every later epic builds on this crate layout, its gates and its rule functions.

## Stories

- Story 1.1: Workspace skeleton and baseline CI
- Story 1.2: Crate layer enforcement with `xtask deps-check`
- Story 1.3: `noctis-rules` foundations: Limits, length counting, rule ids and locales
- Story 1.4: Command naming and tree rules
- Story 1.5: Option, context-menu and command metadata rules
- Story 1.6: Component, modal, message and attachment rules
- Story 1.7: trybuild harness and rules-catalog cross-check
- Story 1.8: Public API snapshot and Usage Skill gate

## Requirements & Constraints

- Bot code depends only on the `noctis` facade. Capabilities are additive cargo features (`image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `prefix`, `testkit`). Default features build without the image stack. Every documented feature combination builds in CI.
- Toolchain: edition 2024, MSRV 1.91 (own CI job), development toolchain pinned to stable 1.97.0. Dependency versions are pinned in `[workspace.dependencies]` from the architecture Stack table. `Cargo.lock` is committed.
- Quality gates: every crate passes fmt, clippy with `-D warnings`, tests and doctests. `missing_docs` is denied on the facade.
- Every enforced Discord rule has a catalog row with a limit, a source and an Enforcement Point. Static input is always checked at compile time. A runtime-only check for static input is a defect.
- Each rule is enforced at the earliest point its input is known. Compile-time rules have compile-fail tests whose error names the rule, the limit and the offending value. Cross-Module conflicts fail at App build and report every violation at once.
- All numeric limits sit in one configurable `Limits` value. Lenient mode truncates text with `…` but never applies to the structural rules CV2-03, CV2-05, CV2-06, CID-01 and CID-02.
- The Noctis Usage Skill documents every public item. A public API change without a skill update fails CI. The Project Template follows the UI Doctrine.
- Release builds only: every cargo command uses `--release`. The repo `.cargo/config.toml` makes `[profile.dev]` release-equivalent so trybuild's runs are covered, and it must never be weakened.

## Technical Decisions

- **Workspace:** a virtual workspace with `resolver = "3"`, members `crates/{noctis, noctis-rules, noctis-codec, noctis-ui, noctis-ports, noctis-gateway, noctis-core, noctis-macros, noctis-i18n, noctis-image, noctis-prefix, noctis-testkit}` plus `xtask`, and `examples/project-template` excluded (it keeps its own empty `[workspace]`). The shared version and `license = "MIT OR Apache-2.0"` are inherited by every crate, and the lints are shared. Every sub-crate's docs say "internal; use `noctis`". `noctis-macros` is a proc-macro crate. `noctis-image` has a `render` feature that is off by default, so only its takumi-free `tw` module builds by default.
- **Layering:** only the edges drawn in the architecture's Invariants graph are allowed, and every edge points to a lower layer. A labelled edge exists only behind its feature: `noctis-image` reaches core, ports and ui only as optional dependencies enabled by `render`. There is no `testkit → image` edge. No crate depends on `noctis` except through `[dev-dependencies]`. `noctis-testkit` is reachable only through the facade's `testkit` feature. `xtask deps-check` enforces all of this.
- **Rules crate:** sync, no I/O, no async, and it depends only on small pure crates (`regex` with Unicode tables). Rule ids are `<AREA>-<nn>`, taken from the catalog, and they appear in the code, in error messages and in tests. The violation type carries the rule id, path, limit and actual value. Every measure, pack or validate function takes `&Limits` and never reads `Limits::default()` internally; only App builder defaults, macros and tests call it. `discord_len`, the larger of the Unicode scalar count and the UTF-16 unit count, is the only length function for user text. Proc macros call rule functions directly and never copy constants. A rule must not rely on a `const` assertion in generic code.
- **Locale:** a closed enum of exactly the 32 Discord codes. A bare language such as `en` does not parse. `sibling()` pairs only `en-US` ↔ `en-GB` and `es-ES` ↔ `es-419`. `MessageKey` and `Translate` (object-safe, `Send + Sync`) also live in `noctis-rules`.
- **Command patterns:** `expand` runs left to right in declaration order, with a default cap of 25 expansions checked before materializing; malformed patterns fail with CMD-16. `validate_tree` and `validate_merge` (per scope, REG-01..04) are shared by the `#[command]` macro and the App-build merge, and report every violation at once. `Violation` carries notes and help so macros render rule output without recomputing it.
- **Rules crate types:** `noctis-rules` uses its own closed enums (option types, interaction contexts, integration types) and raw `u64` permissions / `u8` channel types, never twilight types. Its only dependency is `regex`; error types implement `Display`/`Error` by hand. `Limits` is `#[non_exhaustive]` and callers mutate `Limits::default()`. Metadata conflicts per command base are checked inside `validate_tree`; context-menu and metadata rules join `validate_merge`.
- **Enforcement ladder:** compile-time checks must fail under `cargo check`, using proc-macro errors, trait bounds with `#[diagnostic::on_unimplemented(message = "[<RULE-ID>] …")]`, free `const _` items, or construction. The compile error format is `[<RULE-ID>] …` with `= note:` and `= help:` lines.
- **Tests:** trybuild suites are `compile_fail` only. Fixtures live at `tests/ui/<area>/<RULE-ID>_<slug>.rs`, and fixtures that need a feature live under `tests/ui-<feature>/`. For a rule Noctis diagnoses, the fixture's `.stderr` contains `[<RULE-ID>]`. For a by-construction rule, the fixture or `compile_fail` doctest source carries `// [<RULE-ID>]` on the offending line. Boundary passes are ordinary integration tests in `tests/boundary/`. App-build and runtime rules get unit tests that cite the rule id. A toolchain bump re-blesses `.stderr` files in the same commit.
- **CI (`ci.yml`; workflows are paused in `.github/workflows-disabled/` until Epic 14, so stories are done when the same gates pass locally, but stories still keep the job definitions in sync):** fmt, clippy, tests, `cargo hack test --release --workspace --each-feature`, an all-features build, `cargo deny`, the MSRV check, a non-blocking template build (tracking mode until Story 14.2), and the xtask gates `deps-check`, `rules-catalog`, `api-snapshot` (cargo public-api 0.52 on `nightly-2026-10-15`, the only nightly use) and `skill-check`. `main.yml` and `release.yml` start as stubs.
- **Supply chain:** `deny.toml` holds the exact license allowlist, the crate-scoped exceptions for `xxhash-rust` and `jpeg-encoder`, `[graph] targets` limited to non-wasm tier-1 targets, and a ban on `aws-lc-rs`/`aws-lc-sys`. A new license needs the maintainer's approval.
- **xtask tooling:** `pulldown-cmark` 0.13 parses the catalog and `toml` 1.1 reads `xtask/rules-pending.toml`. A pending entry names the story that will add its test, and an entry that already has a test fails, so the list only shrinks. It must be empty by Story 14.4.
- **Public API changes:** every story that changes the public API updates the template code, its `AGENTS.md` and the Usage Skill (`examples/project-template/.claude/skills/noctis/`) in the same commit. Story 1.8 replaces the skill's `API hash: pending` with the current hash.

## UX & Interaction Patterns

- `skill-check` lints the Project Template for UI Doctrine violations: embeds or `content` as the primary output of an interactive handler, manual response-type selection, and collectors, await-component loops or inline per-message listeners (UD-1, UD-3, UD-7). Components V2 is for anything interactive, and each Route has exactly one handler.

## Cross-Story Dependencies

- Story 1.1 is the base for every other story. Stories 1.4 to 1.6 build on the `Limits`, `RuleId`, violation and `discord_len` foundations from Story 1.3.
- Story 1.7's catalog check covers rule ids whose tests land in later epics through `rules-pending.toml`. Its `custom_id` constants check stays pending until Story 2.2.
- Story 1.8's component-entry check stays pending until Story 4.1. The rule functions from Stories 1.4 to 1.6 are later called by the macros in Epics 4 and 6, the runtime validator in Story 4.3 and the App-build merge in Story 6.4.
- The template build stays non-blocking until Story 14.2.
