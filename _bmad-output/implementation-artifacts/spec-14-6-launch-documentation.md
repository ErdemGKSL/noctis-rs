---
title: 'Story 14.6: Launch documentation'
type: 'chore'
created: '2026-10-25'
status: 'in-progress'
baseline_revision: '27a50578a43c95a6425fb8af2c24186c70572e8f'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/app-and-sharding.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/concept-map.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-74 requires launch documentation: docs.rs docs for every public item with doctested examples, a tescord/dbi (+ serenity/poise) → Noctis concept map, a Cluster deployment guide, changelog migration notes, and a crate README that links every guide, verified by a CI link check. Today `docs/guides/` holds only `voice.md` and an index, three rustdoc links and the skill already point at a missing `docs/guides/cluster.md`, `cargo doc -p noctis` fails under `RUSTDOCFLAGS="-D warnings"` (7 broken/ambiguous intra-doc links with `--all-features`, 3 more feature-gated ones without), and there is no link check and no docs CI job.

**Approach:** Fix the rustdoc warnings and gate rustdoc (`-D warnings`, default and all features) plus doctests in a new CI `docs` job and `gates.sh`; write `docs/guides/concept-map.md` and `docs/guides/cluster.md`; add a README `## Guides` section and CHANGELOG migration notes; add an offline `xtask docs-check` (relative links, anchors, same-repo GitHub URLs, README/index link every guide, no `ignore` doctests).

## Boundaries & Constraints

**Always:**
- Every cargo command uses `--release`; never set `CARGO_INCREMENTAL`; never run `cargo hack`.
- Guides state only facts that match the real API (`xtask/public-api.txt`, the Usage Skill references, the source). Rust snippets in `docs/**/*.md` obey UD-7 (skill-check lints them): no collectors, inline listeners or await loops.
- `docs-check` is offline and deterministic: it never fetches a URL. Problems are printed to stderr as `docs-check: <subject>: <message>`, sorted and deduplicated (same `Problems` shape as `skill_check/mod.rs`), exit 1; success prints `docs-check: ok (<m> Markdown files, <l> links checked, <g> guides linked, <r> Rust files scanned)` to stderr, exit 0; argument errors exit 2 (as the other tasks, `root_arg`).
- `<subject>` is `<repo-relative path with />:<1-based line>` for a located problem, or the bare path for a file-level one.
- Mirror every new gate in `.github/workflows-disabled/ci.yml`, `tools/story-loop/gates.sh` and AGENTS.md "Running and verifying".
- `CHANGELOG.md` keeps its `## [Unreleased]` layout (Story 14.7 turns it into the 0.1.0 entry).

**Never:**
- No public API change (doc text, crate metadata and xtask only), so no `api-snapshot --bless` and no API hash change; if a fix seems to need one, rewrite the doc text instead.
- No new dependency (`pulldown-cmark` 0.13 and `toml` are already xtask deps); no `deny.toml` change.
- No `ignore` doctests; no weakening of `#![deny(missing_docs)]` or any `missing_docs` allow on public items.
- No external-URL fetching, no anchor checking inside non-Markdown targets, no edits under `_bmad/`.
- Do not write `docs/guides/release.md` (Story 14.7).

## I/O & Edge-Case Matrix

`docs-check [--root <path>]` (default root: the repo). Markdown set M = `README.md`, `CHANGELOG.md`, every `*.md` under `docs/` and under `examples/` (recursive; skip directories named `target` or `.git`; `.claude` is included; symlinked directories are not followed). Rust set R = every `*.rs` under `crates/*/src/` (recursive). Guide set G = `docs/guides/*.md` except `docs/guides/README.md` (not recursive). Repo URL U = `[workspace.package] repository` of `<root>/Cargo.toml`, trailing `/` trimmed; same-repo URL = `U/blob/main/<path>` or `U/tree/main/<path>` (`<path>` may carry `#frag`/`?query`).

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| L1 | relative file link | `[x](docs/discord-rules.md)` in README, file exists | ok, counted | — |
| L2 | relative link to missing file | `[x](docs/nope.md)` | problem `README.md:<n>: broken link docs/nope.md: no such file or directory` | exit 1 |
| L3 | relative directory link | `[x](examples/project-template/)` exists | ok | — |
| L4 | path relative to the linking file | `[x](voice.md)` in `docs/guides/README.md` | resolved against `docs/guides/` | — |
| L5 | `..` escaping root | `[x](../../outside.md)` from `README.md` | problem `… link leaves the repository: ../../outside.md` (checked lexically, before any fs access) | exit 1 |
| L6 | root-relative | `[x](/docs/a.md)` | problem `… root-relative link /docs/a.md: use a path relative to this file` | exit 1 |
| L7 | empty target | `[x]()` | problem `… empty link target` | exit 1 |
| L8 | percent-encoded path | `[x](my%20file.md)`, `my file.md` exists | decoded, ok; invalid escape (`%G1`, trailing `%`) kept literally | — |
| L9 | query | `[x](a.md?plain=1)` | query dropped, `a.md` checked | — |
| L10 | image link | `![x](docs/assets/a.png)` | checked like L1/L2 | — |
| L11 | reference-style | `[x][r]` + `[r]: docs/a.md` | checked once per use, line of the use | — |
| L12 | raw HTML attribute | `<img src="docs/assets/noctis-logo-512.png">`, `<a href="x.md">` | `src`/`href` values (double or single quoted) checked like L1-L9 | — |
| L13 | link inside code | `` `[a](b)` `` span, fenced/indented code block, e.g. discord-rules.md:306 regexes | not a link, ignored | — |
| L14 | external URL | `https://discord.com/…#x`, `http://…`, `mailto:`, any other `scheme:` | skipped, not counted | — |
| L15 | same-repo URL | `U/blob/main/docs/guides/cluster.md` | checked as root-relative path `docs/guides/cluster.md` (+ anchor per A-rows) | missing → L2 message with the URL |
| L16 | same-repo, other ref or form | `U/blob/<sha>/…`, `U/issues/1`, bare `U`, different letter case of owner | skipped | — |
| A1 | same-file anchor | `[x](#roadmap)`, heading `## Roadmap` | ok | — |
| A2 | missing anchor | `[x](#nope)` | problem `… broken anchor #nope: no such heading in README.md` | exit 1 |
| A3 | cross-file anchor | `[x](docs/discord-rules.md#9-components-v2-messages)` | slug of `## 9. Components V2 (messages)` matches | missing → A2 message naming the target file |
| A4 | GitHub slug rules | heading `1. The `custom_id` layout and fixed-length Refs` | `1-the-custom_id-layout-and-fixed-length-refs`: rendered text (code-span content kept, emphasis/link markup dropped), lowercase (Unicode), drop every char that is not alphanumeric (Unicode), space, `-` or `_`, spaces → `-` | — |
| A5 | unicode / punctuation heading | `## Çalışma — notları!` | `çalışma--notları` | — |
| A6 | duplicate headings | two `## Setup` | `setup`, `setup-1` | — |
| A7 | fragment case / encoding | `#Roadmap`, `#%C3%A7al` | percent-decoded, compared lowercase | — |
| A8 | empty fragment | `[x](#)`, `a.md#` | ok (top of file) | — |
| A9 | HTML anchors | `<a id="x"></a>` / `name="x"` in target | count as anchors | — |
| A10 | fragment on non-Markdown or dir | `a.rs#L10`, `dir/#x` | only existence checked | — |
| A11 | heading inside code block | ```` ```\n# not a heading\n``` ```` | not an anchor | — |
| A12 | setext / closed ATX heading | `Title\n===`, `## Title ##` | anchor `title` | — |
| G1 | README links every guide | G = {concept-map, cluster, voice} all linked from README (relative or same-repo URL, any fragment) | ok | — |
| G2 | guide not linked from README | `docs/guides/new.md` unlinked | problem `README.md: guide not linked: docs/guides/new.md` | exit 1 |
| G3 | guides index | `docs/guides/README.md` must also link every guide in G | missing → `docs/guides/README.md: guide not linked: docs/guides/new.md` | exit 1 |
| G4 | link only in a code span | `` `docs/guides/x.md` `` | does not count as linking | G2 |
| G5 | G empty / no `docs/guides/` | — | ok, `0 guides linked` | — |
| G6 | `docs/guides/README.md` missing while G non-empty | — | problem `docs/guides/README.md: missing guides index` | exit 1 |
| R1 | Rust same-repo URL | `/// [guide](U/blob/main/docs/guides/cluster.md)` in R | substring scan (URL ends at whitespace or one of `)>"'` `` ` `` `]`), checked as L15/A-rows, subject `<file>:<line>` | missing → L2 message |
| R2 | `ignore` doctest | doc line (`///` or `//!`, trimmed) opening a fence ```` ```ignore ```` or ```` ```rust,ignore ```` (info tokens split on `,` and whitespace, a token equal to `ignore`) | problem `<file>:<n>: doctest marked ignore: make it run, no_run or compile_fail` | exit 1 |
| R3 | other fences | ```` ``` ````, `no_run`, `compile_fail,E0277`, `text`, `should_panic` | ok | — |
| E1 | non-UTF-8 file in M or R | — | problem `<path>: not UTF-8`, others still checked | exit 1 |
| E2 | no `repository` in `<root>/Cargo.toml` or no Cargo.toml | — | problem `Cargo.toml: no [workspace.package] repository`; L15/R1 then skipped | exit 1 |
| E3 | bad args | `docs-check --root`, `docs-check x` | usage message (as `skill-check`) | exit 2 |
| E4 | same problem twice | two identical links on one line | reported once | — |

</intent-contract>

## Code Map

- `crates/noctis/src/lib.rs` -- facade docs (`#![deny(missing_docs)]` at l.621). rustdoc `-D warnings` errors today: l.81 `Modal` (and one more `Modal`), `Payload`, `PayloadEnum`, `Choices` (prelude list ~l.44-48), l.2728 `[`format`]` ambiguous (use `format()` or `format!` as meant), l.2828 `AppBuilder` unresolved in a sub-module doc (use `crate::AppBuilder`); without features: links to `prefix`, `testkit`, `voice` (feature-gated modules: write them as plain code or `#[cfg]`-split the text). l.2260 links the cluster guide.
- `crates/noctis/Cargo.toml` -- `readme = "../../README.md"`; add `[package.metadata.docs.rs] all-features = true`.
- Sub-crate `src/lib.rs` -- every one already says "internal; use `noctis`" (`noctis-image` l.40, `-macros` l.9, `-prefix` l.8, `-testkit` l.5, others l.3, `-voice` l.1); sub-crate `description`s say "Internal; use `noctis`". No change; AC checks it.
- `Cargo.toml` -- `[workspace.lints.rust] missing_docs = "warn"` (+ clippy `-D warnings` = deny for every crate); `repository = "https://github.com/ErdemGKSL/noctis-rs"` (l.29).
- `crates/noctis-ports/src/identify.rs:15`, `crates/noctis-ports/src/memory/identify.rs:26`, skill `references/adapters.md:315,839` -- existing links to `docs/guides/cluster.md` (R1/L15 must pass once it exists).
- Skill `references/app-and-sharding.md` §4 "Clusters (several processes)" (~l.528-583, checklist l.560-575), §2 env table (l.294-310), AppBuilder table (l.105-157), §5 HTTP proxy (l.625+), `GuildNotInRange` (l.791); `references/adapters.md` (shared Adapters, `Namespaced`, IdentifyQueue l.839); `references/concept-map.md` (14.5's table to adapt); `docs/guides/voice.md` (Cluster notes, guide style).
- `docs/guides/README.md` -- index; drop "(written with Story 14.6)", list all guides.
- `README.md` -- "Read the full design" table (l.215-222), `## Roadmap` (l.224); crates.io page too (relative links are rewritten by crates.io against `repository`).
- `CHANGELOG.md` -- `## [Unreleased]` subsections (`### Usage Skill (v1)`, `### Rules catalog (v1 audit)`, `### Performance budgets (PRD Q3)`, `### Changed`).
- `xtask/src/main.rs` -- top docs, `USAGE`, dispatch (`skill-check` arm l.90 pattern, `root_arg` l.139).
- `xtask/src/skill_check/mod.rs` -- `Problem`/`Problems`/`Repo` pattern to copy (not share); `xtask/src/rules_catalog/catalog.rs` -- pulldown-cmark usage (`Parser::new_ext`, `offset_iter`).
- `xtask/tests/skill_check.rs` -- CLI test harness (`root(test)` fixture dir, `Command::new(env!("CARGO_BIN_EXE_xtask"))`) to mirror.
- `.github/workflows-disabled/ci.yml` -- job style (`rustup toolchain install`, `Swatinem/rust-cache@v2`), `skill-check` job l.198; `tools/story-loop/gates.sh` -- `run` helper and the xtask loop.
- Continuity (14.5 done): skill already has `concept-map.md`; `skill-check` lints Rust in `docs/` Markdown (UD-7); decision 14.2 D2 "no `Ctx` trait"; Kv stores ids as `u64`.

## Tasks & Acceptance

**Execution:**
- `crates/noctis/src/lib.rs` (and any other facade file rustdoc flags) -- fix every rustdoc warning so both `cargo doc` commands in Verification pass with `-D warnings`; doc text only.
- `crates/noctis/Cargo.toml` -- add `[package.metadata.docs.rs]` with `all-features = true`.
- `xtask/src/docs_check.rs` (new; split into `docs_check/{mod,markdown,slug}.rs` if > ~400 lines) -- implement the matrix; unit tests for the slug function (A4-A6, A12 strings), link classification (L5-L9, L14-L16), fence detection (R2/R3) and URL scan end chars (R1). Module docs list the checks.
- `xtask/src/main.rs` -- `mod docs_check;`, `docs-check [--root <path>]` arm via `root_arg`, top docs and `USAGE` entry.
- `xtask/tests/docs_check.rs` (new) -- CLI tests over generated roots (fixture writes `Cargo.toml` with `[workspace.package] repository`, `README.md`, `docs/guides/{README,a}.md`, `crates/x/src/lib.rs`): ok summary line exact; one test per problem row L2, L5, L6, L7, A2, A3-missing, G2, G3, G6, R1-missing, R2, E1, E2, E3 asserting the exact stderr line(s) and exit code; one positive test covering L3, L4, L8-L13, L14, L16, A1, A5-A10, R3; plus `real_repo_passes` (root = repo, exit 0).
- `docs/guides/concept-map.md` (new) -- `# Coming from tescord, dbi, serenity or poise`; `## tescord / dbi → Noctis` table including rows Pack → Module, `buildComponent` → typed Payload (kind-derived struct + `.build()`), `locale.user` → User Locale (`ctx.user_locale()`, `Strings::of`), plus the skill's other rows rewritten for humans; `## serenity / poise → Noctis` (short table: `EventHandler` trait, `#[poise::command]`, `ctx.say`, `Context<Data>`, `ComponentInteractionCollector` → Route, framework setup → `App::from_env()`); `## What Noctis does not do` (collectors, inline listeners, widgets → recipes); links to the skill references by relative path.
- `docs/guides/cluster.md` (new) -- `# Cluster deployment`: when to cluster; `## Shard Ranges` (`NOCTIS_SHARD_TOTAL` same everywhere, `NOCTIS_SHARD_RANGE` half-open `start..end`, `.shard_total`/`.shard_range`, build violations, `max_concurrency` alignment warning, `GuildNotInRange`); `## IdentifyQueue` (one shared implementation for the whole Cluster, `MemoryIdentifyQueue` is per process only, `.identify_queue(..)`, conformance suite `noctis::testkit::conformance`); `## HTTP proxy` (`NOCTIS_HTTP_PROXY`/`.http_proxy`, local limiter off, startup warning without it, proxy + HTTP edge compose); `## Shared Adapters` (which ports must be shared vs per process, same `NOCTIS_APP_ID`, `Namespaced` key prefixes, SessionStore resume); `## NOCTIS_PAYLOAD_KEY` (identical on every process, rotation with `NOCTIS_PAYLOAD_KEY_PREVIOUS`, never logged); `## One publisher` (`NOCTIS_PUBLISHER=true`/`.publisher(true)` on exactly one process, or a `--publish` deploy step); `## Voice` (a guild's call lives in the process owning its shard; link `voice.md`); `## Example` (3-process env table or compose snippet); `## Checklist`. Every fact cross-checked against the skill/source.
- `docs/guides/README.md` -- list every guide in G with one line each.
- `README.md` -- add `## Guides` (before `## Roadmap`): table linking `docs/guides/README.md`, `concept-map.md`, `cluster.md`, `voice.md`; also link the concept map from `## Lineage`.
- `CHANGELOG.md` -- under `[Unreleased]` add `### Documentation` (guides, rustdoc gate, `docs-check`, docs.rs metadata) and `### Migration notes` (first release: no Noctis-to-Noctis migration; adopting from tescord/dbi and serenity/poise: the 5-8 biggest differences with links to `docs/guides/concept-map.md`; running several processes: link `docs/guides/cluster.md`; pre-release adopters of the template: Kv-stored ids are `u64`).
- `.github/workflows-disabled/ci.yml` -- new job `docs` ("Docs"): `RUSTDOCFLAGS: -D warnings` env on the two `cargo doc` steps, `cargo test --release -p noctis --all-features --doc`, `cargo run --release -p xtask -- docs-check`.
- `tools/story-loop/gates.sh` -- `run docs env RUSTDOCFLAGS="-D warnings" cargo doc --release -p noctis --no-deps`, `run docs-all env RUSTDOCFLAGS="-D warnings" cargo doc --release -p noctis --all-features --no-deps`; add `docs-check` to the xtask loop.
- `AGENTS.md` -- add the two rustdoc commands and `cargo run --release -p xtask -- docs-check` to "Running and verifying".
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- D1-D5 are logged at planning; append only new implementation decisions.

**Acceptance Criteria:**
- Given the `noctis` crate, when `RUSTDOCFLAGS="-D warnings" cargo doc --release -p noctis --no-deps` runs with and without `--all-features`, then both succeed; `#![deny(missing_docs)]` is still in `crates/noctis/src/lib.rs`, and `cargo test --release -p noctis --all-features --doc` passes.
- Given every `crates/*/src`, when `docs-check` runs, then no doctest is `ignore`, and every sub-crate's crate docs and `description` say it is internal and point to `noctis`.
- Given `docs/guides/`, when listed, then `concept-map.md` (with Pack → Module, `buildComponent` → typed Payload, `locale.user` → User Locale and a serenity/poise section) and `cluster.md` (Shard Ranges, IdentifyQueue, HTTP proxy, shared Adapters, `NOCTIS_PAYLOAD_KEY`, single publisher) exist.
- Given the repository, when `cargo run --release -p xtask -- docs-check` runs, then it exits 0 and README.md and `docs/guides/README.md` link every guide; and given `CHANGELOG.md`, it has a `### Migration notes` section under `[Unreleased]`.
- Given the repository, when `cargo run --release -p xtask -- skill-check` and `api-snapshot` run, then both pass unchanged (no API change; guide Rust snippets pass UD-7).

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions (logged in the memlog):** D1 the CI link check is a new offline `xtask docs-check` built on the existing `pulldown-cmark` dep: relative links, GitHub-slug anchors and same-repo `blob|tree/main` URLs (also in `.rs` docs, where the cluster-guide links live); external URLs are never fetched (deterministic gate, no network in gates). D2 "runnable examples are doctests" is enforced as "no `ignore` fence in any `crates/*/src` doc comment" plus `cargo test --doc` in CI; `no_run` stays allowed for examples needing a token or network. D3 "every public item has rustdoc" = the existing `#![deny(missing_docs)]` on `noctis` plus workspace `missing_docs = "warn"` under clippy `-D warnings` for the sub-crates; rustdoc itself is gated with `RUSTDOCFLAGS=-D warnings` for default and all features, and docs.rs builds with `all-features = true` (docs.rs's build image has a C compiler and cmake; Story 14.9's publish confirms it). D4 the human concept map is a separate `docs/guides/concept-map.md` adapted from the skill's `references/concept-map.md` (agents keep theirs). D5 migration notes go to `[Unreleased]` `### Migration notes`; Story 14.7 folds them into the 0.1.0 entry.

**Slug example:** `## 3. Command Pattern expansion and structural validation` → `3-command-pattern-expansion-and-structural-validation` (already linked in `docs/discord-rules.md:91`).

## Verification

**Commands:**
- `RUSTDOCFLAGS="-D warnings" cargo doc --release -p noctis --no-deps` and the same with `--all-features` -- expected: exit 0
- `cargo test --release -p noctis --all-features --doc` -- expected: all doctests pass
- `cargo test --release -p xtask` -- expected: unit and CLI tests pass, incl. `real_repo_passes`
- `cargo run --release -p xtask -- docs-check` -- expected: `docs-check: ok (…)`, exit 0
- `cargo fmt --all --check`; `/home/claude/lanes/d/noctis-rs/tools/story-loop/bg.sh cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: clean
- `cargo run --release -p xtask -- skill-check`, `-- rules-catalog`, `-- api-snapshot`, `-- deps-check` -- expected: ok, unchanged
- `bash /home/claude/lanes/d/noctis-rs/tools/story-loop/gates.sh` (via `bg.sh`) -- expected: `ALL-OK` (disk had ~4.9 GB free at planning; if it fills, delete `target/debug/`)
