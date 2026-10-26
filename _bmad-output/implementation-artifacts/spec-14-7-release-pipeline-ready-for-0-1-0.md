---
title: 'Story 14.7: Release pipeline ready for 0.1.0'
type: 'chore'
created: '2026-10-26'
status: 'done'
baseline_revision: 'da152d80f3fec24fad4ac9fb82c9fa93911b45f7'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/guides/cluster.md'
warnings: ['oversized']
deferred:
  - summary: >-
      After 0.1.0, release-plz release-pr may auto-bump changed crates (patch bump, no version_group) on every push instead of waiting for the hand bump the runbook describes, possibly breaking the shared-version invariant.
    evidence: |-
      Unverified: release-plz 0.4.1 compares packages with their crates.io versions, which cannot be exercised before the first publish. Settle it by running `release-plz update` on a copy after 0.1.0 is published (or against a local registry holding 0.1.0) with a sub-crate change and no version bump; if it bumps only some crates or opens a PR without a hand bump, add `version_group` to every published package (and to release-check R2) and fix the "Later releases" runbook section.
    location: >-
      release-plz.toml; docs/guides/release.md (How a release works, Later releases)
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** FR-48/NFR-7/NFR-8/AD-28 require a release pipeline that publishes all 13 `noctis*` crates at the shared version in layer order from a protected `release` environment, a token-less `cargo publish --dry-run` in CI, a final feature matrix with `cargo-semver-checks` on `noctis`, a 0.1.0 changelog entry and a maintainer runbook. Today `release-plz.toml` and `.github/workflows-disabled/release.yml` are stubs, and `cargo publish --workspace --dry-run` fails (`noctis-core` dev-depends on an unpublished `noctis` with a version; same for `noctis-testkit` and `noctis-voice`).

**Approach:** Make the three cyclic dev-dependencies path-only, configure release-plz 0.4.1 (`release_always = false`, one hand-curated `CHANGELOG.md` stamped by the release PR), write `release.yml` (token-free `release-pr` job; `release` job in environment `release` with a crates.io rate-limit retry), add CI jobs/gates (publish dry run, semver, default-graph, all-features tests, new `xtask release-check`), write the 0.1.0 changelog entry and `docs/guides/release.md`.

## Boundaries & Constraints

**Always:**
- Every cargo command uses `--release` where cargo accepts it (`cargo publish`/`cargo tree` have no such flag); never set `CARGO_INCREMENTAL`; never run `cargo hack`.
- Workflows stay in `.github/workflows-disabled/` (CI paused by the maintainer); every new CI gate is mirrored in `tools/story-loop/gates.sh` and AGENTS.md "Running and verifying".
- The crates.io token appears only in the `release` job, which is bound to `environment: release`; `release-pr`, `ci.yml` and every script run without it.
- Scripts: `#!/usr/bin/env bash`, `set -euo pipefail`, errors to stderr prefixed with the script name, as `.github/scripts/check-build-config.sh`.
- `release-check` follows the xtask conventions: `[--root <path>]` via `root_arg`, problems as stderr lines `release-check: <subject>: <message>` sorted and deduplicated, exit 1; usage error exit 2; pure checks over a plain model (unit-testable without cargo), I/O at the edge.
- Never put the maintainer's email in any file, header or User-Agent.

**Never:**
- No public API change (no `api-snapshot --bless`, no skill/template change). No new xtask dependency (`cargo_metadata`, `toml` exist). No `deny.toml` change.
- No release-plz-generated commit lists (the repo's commits are not conventional); no git tags or GitHub releases for sub-crates.
- Do not publish, create tokens, change repository settings or move workflows to `.github/workflows/` (Stories 14.8/14.9, maintainer).
- No `## [0.1.0]` heading committed in `CHANGELOG.md` (it would stop the release PR; see Design Notes D2).

## I/O & Edge-Case Matrix

**`xtask release-check [--root <path>]`.** Model: `cargo metadata --no-deps --manifest-path <root>/Cargo.toml`; W = `[workspace.package] version`, U = `[workspace.package] repository` of `<root>/Cargo.toml`; P = members whose `publish` is not `Some([])`; Q = other members. A dependency is *path-only* when its `req` is `*` (VersionReq::STAR). Success: `release-check: ok (<n> crates at <W>; publish order: <a>, <b>, …)` on stderr, exit 0, order = Kahn topological order of P over normal+build edges inside P (optional deps count), ties alphabetical.

| # | Scenario | Input / State | Expected | Error handling |
|---|---|---|---|---|
| K1 | real repo after this story | 13 crates, `xtask`, 2 examples | ok line `13 crates at 0.1.0; publish order: noctis-ports, noctis-rules, …, noctis`; every crate after its deps | exit 0 |
| K2 | no W | `[workspace.package]` lacks `version`, or root `Cargo.toml` missing | `Cargo.toml: no [workspace.package] version` | exit 1 |
| K3 | metadata fails | invalid manifest | `Cargo.toml: cargo metadata failed: <first error line>` | exit 1, no other checks |
| C1 | publishable non-noctis member | `music-bot` without `publish = false` | `music-bot: published but not a noctis crate (set publish = false)` | exit 1 |
| C2 | facade not published | `noctis` absent from P | `noctis: the facade crate is not published` | exit 1 |
| C3 | version skew | `noctis-ui` version `0.1.1`, W `0.1.0` | `noctis-ui: version 0.1.1 is not the workspace version 0.1.0 (use version.workspace = true)` | exit 1 |
| C4 | missing metadata | `description`/`license`/`repository` None or blank | `<name>: missing description` (per field) | exit 1 |
| C5 | foreign repository | repository ≠ U (trailing `/` ignored) | `<name>: repository <r> is not the workspace repository <U>` | exit 1 |
| D1 | path-only published edge | normal/build dep P→P with `req *` | `<from> -> <to> (normal): path-only dependency (cargo publish drops it; use <to>.workspace = true)` (`build` likewise) | exit 1 |
| D2 | published crate needs Q | normal/build dep P→Q | `<from> -> <to> (normal): depends on an unpublished member` | exit 1 |
| D3 | cyclic dev-dep keeps version | dev dep P→P where `to` reaches `from` over normal/build edges, req ≠ `*` (today's `noctis-core -> noctis`) | `<from> -> <to> (dev): cyclic dev-dependency keeps a version (make it path-only so cargo publish strips it)` | exit 1 |
| D4 | dev-dep on Q keeps version | dev dep P→Q, req ≠ `*` | `<from> -> <to> (dev): dev-dependency on an unpublished member keeps a version` | exit 1 |
| D5 | allowed edges | acyclic versioned dev dep (`noctis-voice -> noctis-testkit`), path-only cyclic dev dep, Q→anything, external deps | ok | — |
| R1 | config missing / invalid | no `release-plz.toml`; TOML syntax error | `release-plz.toml: missing`; `release-plz.toml: <parser message, first line>` | exit 1, R2-R6 skipped |
| R2 | required settings | each key of the table in Design Notes absent or different | `release-plz.toml: <table>.<key> must be <expected> (found <value or missing>)`, table = `workspace`, `package noctis` or `changelog` | exit 1 |
| R3 | changelog_include drift | `package noctis` `changelog_include` set ≠ P∖{noctis} | `release-plz.toml: package noctis changelog_include lacks <name>` / `… names <name>, which is not a published crate` | exit 1 |
| R4 | Q not skipped | member in Q without `[[package]]` `release = false` | `release-plz.toml: <name> is publish = false but has no release = false` | exit 1 |
| R5 | P skipped | member in P with `release = false` or `publish = false` in its entry | `release-plz.toml: <name> is a published crate but release-plz skips it` | exit 1 |
| R6 | bad entries | `[[package]]` without `name`; unknown name; duplicate name | `release-plz.toml: [[package]] without name` / `[[package]] <name> is not a workspace member` / `duplicate [[package]] <name>` | exit 1 |
| R7 | body renders commits or no heading | `[changelog] body` lacks `## [{{ version }}]` or contains `commits` | `release-plz.toml: changelog.body must render only a "## [{{ version }}]" heading` | exit 1 |
| H1 | changelog header | `CHANGELOG.md` missing; first line ≠ `# Changelog`; no line `## [Unreleased]` | `CHANGELOG.md: missing` / `CHANGELOG.md: must start with "# Changelog"` / `CHANGELOG.md: no "## [Unreleased]" heading` | exit 1 |
| H2 | release heading above Unreleased | a `## ` line before `## [Unreleased]` (lines inside ```` ``` ```` fences ignored) | `CHANGELOG.md:<line>: "## [Unreleased]" must be the first "## " heading` | exit 1 |
| H3 | unicode / CRLF | `# Changelog\r\n`, non-ASCII text | CR stripped before comparing; ok | — |
| E1 | args | `release-check --root`, `release-check x` | usage message | exit 2 |
| E2 | duplicate problem | same problem twice | reported once | — |

**`.github/scripts/release-retry.sh <attempts> <wait-seconds> -- <command…>`** (crates.io allows 5 new crates, then 1 per 10 min; 13 are new).

| # | Scenario | Expected | Exit |
|---|---|---|---|
| T1 | command succeeds first try | output passed through, no retry | 0 |
| T2 | fails, output matches `/429\|too many requests\|too many new crates/i`, attempts left | stderr `release-retry: crates.io rate limit (attempt <i>/<n>); retrying in <w>s`, sleep, rerun | final command's |
| T3 | rate-limited on every attempt | after the last: `release-retry: still rate limited after <n> attempts` | last command status |
| T4 | fails without rate-limit text | no retry | command's status |
| T5 | succeeds after k rate-limited tries | stops retrying | 0 |
| T6 | `attempts` not an integer ≥ 1 (`0`, `-1`, `x`, empty) or `wait` not an integer ≥ 0 (`-1`, `x`, empty; `0` is valid), missing `--`, empty command | `release-retry: usage: release-retry.sh <attempts> <wait-seconds> -- <command...>` | 2 |
| T7 | command not found | treated as T4 (shell status 127) | 127 |

**`.github/scripts/semver-check.sh [cargo-semver-checks args…]`**

| # | Scenario | Expected | Exit |
|---|---|---|---|
| S1 | no args, `https://index.crates.io/no/ct/noctis` → 404 | `semver-check: noctis is not on crates.io yet (first release); nothing to compare` | 0 |
| S2 | no args, index 200 | runs `cargo semver-checks check-release -p noctis --all-features` | its status |
| S3 | index other status / network error (`000`) | `semver-check: cannot query the crates.io index (HTTP <code>)` | 1 |
| S4 | args given (e.g. `--baseline-rev HEAD`) | no probe; `cargo semver-checks check-release -p noctis --all-features <args>` | its status |
| S5 | S2/S4 and `cargo semver-checks --version` fails | `semver-check: cargo-semver-checks is not installed` | 1 |

**`.github/scripts/check-default-graph.sh`** — `cargo tree -p noctis -e normal --prefix none --locked` (default features).

| # | Scenario | Expected | Exit |
|---|---|---|---|
| G1 | default graph clean | `check-default-graph: ok` | 0 |
| G2 | forbidden crate in default graph (`noctis-image`, `noctis-voice`, `noctis-prefix`, `noctis-testkit`, `songbird`, any `takumi*`) | one line per name `check-default-graph: <name> is in noctis's default dependency graph` | 1 |
| G3 | stale list guard | `--features image` graph lacks `noctis-image` or any `takumi*` | `check-default-graph: --features image does not pull noctis-image and takumi; the forbidden list is stale` | 1 |
| G4 | empty/odd output | default graph has no `noctis-core` line | `check-default-graph: cargo tree output has no noctis-core` | 1 |
| G5 | cargo tree fails | its stderr passes through | its status |

**Release behaviour (release-plz 0.4.1, verified at planning on a copy).**

| # | Scenario | Expected |
|---|---|---|
| P1 | `release-plz update` on the committed tree | only `CHANGELOG.md` changes: `## [0.1.0](https://github.com/ErdemGKSL/noctis-rs/releases/tag/v0.1.0) - <date>` plus one blank line inserted right after `## [Unreleased]` + blank line; versions stay 0.1.0 |
| P2 | push to `main` that is not a release-PR merge | `release` job runs, release-plz releases nothing (`release_always = false`) |
| P3 | `cargo publish --workspace --dry-run --locked`, no token | 13 crates packaged and verified in dependency order; `xtask`, `music-bot`, `ui-recipes` skipped; exit 0 |

</intent-contract>

## Code Map

- `crates/noctis-core/Cargo.toml:45` `noctis = { workspace = true, features = ["testkit"] }`, `crates/noctis-testkit/Cargo.toml:37` `noctis.workspace = true`, `crates/noctis-voice/Cargo.toml:41` `noctis = { workspace = true, features = ["testkit", "voice"] }` -- the three cyclic dev-deps → `noctis = { path = "../noctis", … }` keeping features (D3; planning run: dry run passes only after this). `noctis-voice -> noctis-testkit` stays versioned.
- `crates/noctis/Cargo.toml` -- facade manifest (features l.13-49, docs.rs metadata at end); add `keywords = ["discord", "bot", "twilight", "framework", "components-v2"]`, `categories = ["api-bindings", "asynchronous"]`.
- `Cargo.toml` -- `[workspace.package] version = "0.1.0"`, `repository = "https://github.com/ErdemGKSL/noctis-rs"`; members incl. `examples/music-bot`, `examples/ui-recipes`, `xtask` (all `publish = false`).
- `release-plz.toml` -- stub (xtask `release = false` only); rewrite per Design Notes.
- `.github/workflows-disabled/release.yml` -- stub; rewrite. `.github/workflows-disabled/ci.yml` -- `all-features` job l.109, `deps-check` job l.143 with inline songbird step l.153 (replace by the script), job style (checkout@v7, `rustup toolchain install`, `Swatinem/rust-cache@v2`, `taiki-e/install-action@v2`).
- `.github/scripts/check-build-config.sh` -- script style to copy.
- `tools/story-loop/gates.sh` -- `run` helper; xtask loop l.42; `deny` l.38.
- `xtask/src/main.rs` -- module list, top docs, `USAGE`, dispatch arms (copy the `docs-check` arm), `root_arg`.
- `xtask/src/docs_check/mod.rs` -- `Problem`/`Problems` shape to copy; `xtask/src/deps_check.rs` -- `MetadataCommand` usage, `DependencyKind`.
- `xtask/tests/docs_check.rs` -- fixture-root CLI harness (`Root`, `CARGO_BIN_EXE_xtask`) to mirror.
- `CHANGELOG.md` -- `## [Unreleased]` with `### Usage Skill (v1)`, `### Rules catalog (v1 audit)`, `### Performance budgets (PRD Q3)`, `### Documentation`, `### Changed`, `### Migration notes` (l.128).
- `README.md` `## Guides` table l.224-231; `docs/guides/README.md` index -- `docs-check` G2/G3 require both to link `release.md`.
- `crates/noctis/src/lib.rs` l.579-620 -- the documented feature list (`image`, `image-gif`, `image-emoji`, `default-font`, `i18n`, `prefix`, `voice`, `testkit`) = the matrix.
- Continuity (14.6 done): `docs-check` checks every `docs/**` Markdown; migration notes live under `[Unreleased]` and 14.7 folds them into 0.1.0; workflows remain disabled.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/Cargo.toml`, `crates/noctis-testkit/Cargo.toml`, `crates/noctis-voice/Cargo.toml` -- make the cyclic `noctis` dev-deps path-only with a comment `# Path-only: cargo publish strips it (noctis depends on this crate).`.
- `crates/noctis/Cargo.toml` -- add `keywords`, `categories` (Code Map).
- `release-plz.toml` -- write the golden config (Design Notes) with comments.
- `xtask/src/release_check.rs` (new; split into `release_check/{mod,model,rules}.rs` if > ~500 lines) -- implement the K/C/D/R/H/E rows; model structs built from metadata + TOML; unit tests for each row with hand-built models, the topological order (ties, optional edges) and the cycle reachability.
- `xtask/src/main.rs` -- `mod release_check;`, `release-check [--root <path>]` arm, top docs and `USAGE` entry.
- `xtask/tests/release_check.rs` (new) -- CLI tests over generated fixture workspaces (`[workspace]` with `noctis`, `noctis-a`, `xtask` crates, each `src/lib.rs`, plus `release-plz.toml` and `CHANGELOG.md`): exact ok line; one test each for K2, C3, D1, D3, R1, R2, R4, R6, H1, H2, E1 (exact stderr lines, exit codes); `real_repo_passes` asserting the K1 prefix `release-check: ok (13 crates at 0.1.0; publish order: ` and suffix `, noctis)`.
- `.github/scripts/release-retry.sh` (new) and `.github/scripts/release-retry-test.sh` (new; stub commands via counter files, wait `0`; covers T1-T7; prints `release-retry-test: ok`).
- `.github/scripts/semver-check.sh` (new) -- S1-S5; `curl -sS -o /dev/null -w '%{http_code}'`, no custom User-Agent.
- `.github/scripts/check-default-graph.sh` (new) -- G1-G5.
- `.github/workflows-disabled/release.yml` -- rewrite (Design Notes).
- `.github/workflows-disabled/ci.yml` -- `all-features` job → name `Build and test all features`, add `cargo test --release --workspace --all-features`; `deps-check` job: replace the inline songbird step with `bash .github/scripts/check-default-graph.sh`; new jobs `publish-dry-run` (`cargo publish --workspace --dry-run --locked`, comment: no token), `semver` (install-action `cargo-semver-checks@0.51`, `bash .github/scripts/semver-check.sh`), `release-check` (`cargo run --release -p xtask -- release-check` and `bash .github/scripts/release-retry-test.sh`).
- `tools/story-loop/gates.sh` -- `run default-graph bash .github/scripts/check-default-graph.sh`, `run release-retry bash .github/scripts/release-retry-test.sh`, `run publish-dry-run cargo publish --workspace --dry-run --locked --allow-dirty`; add `release-check` to the xtask loop. (semver stays CI-only per AD-28.)
- `AGENTS.md` -- add `cargo run --release -p xtask -- release-check`, the two scripts and `cargo publish --workspace --dry-run --locked --allow-dirty` to "Running and verifying".
- `CHANGELOG.md` -- turn `[Unreleased]` into the 0.1.0 entry (D2): an opening paragraph ("The first release, published as 0.1.0: …; the release PR adds the version heading"), a new `### Added` summary of the whole framework by area (facade and its 8 features; gateway and sharding/Cluster; interactions and handlers; Components V2 UI kit; image Templates; i18n; ports and Adapters; testkit and conformance suites; prefix commands; voice; rules catalog; project template and Usage Skill), the existing subsections kept, a `### Release` subsection (release-plz, `release.yml`, dry run, semver, `release-check`, path-only dev-deps), and `### Migration notes` last. No `## [0.1.0]` heading.
- `docs/guides/release.md` (new) -- `# Releasing Noctis`: `## How a release works`; `## Before the first release (Story 14.8)` (numbered: move `.github/workflows-disabled/*.yml` to `.github/workflows/` and make CI green; Settings → Actions → General → Workflow permissions "Read and write" + "Allow GitHub Actions to create and approve pull requests"; crates.io token with scopes `publish-new` + `publish-update`, crate patterns `noctis` and `noctis-*`, an expiry; Settings → Environments → `release`, deployment branches = `main`, secret `CARGO_REGISTRY_TOKEN`, no required reviewers (they would gate every push); optional branch ruleset requiring the `ci.yml` checks on pull requests with the maintainer on the bypass list; next push opens the `chore: release` PR — check its diff is the P1 heading); `## Publishing 0.1.0 (Story 14.9)` (review, merge with a merge commit, watch the `release` job: 5 crates then 1 per 10 min, automatic retries ≈ 90 min, re-run failed jobs if it still stops — published crates are skipped; check crates.io, docs.rs `noctis` all-features build, tag `v0.1.0` and GitHub release; then the template switches to `noctis = "0.1"`); `## Later releases` (write under `[Unreleased]`, breaking change → minor bump before 1.0, semver-checks in CI and in the release PR, `noctis::image::unstable` excluded from semver by AD-28 but still reported); `## Troubleshooting` (no PR opened, 429, verify failure, partial publish, expired token, docs.rs failure); `## Local checks`.
- `README.md` (`## Guides` row) and `docs/guides/README.md` (one line) -- link `docs/guides/release.md`.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- D1-D8 are logged at planning; append only new implementation decisions.

**Acceptance Criteria:**
- Given the repo, when `cargo publish --workspace --dry-run --locked --allow-dirty` runs without `CARGO_REGISTRY_TOKEN`, then it exits 0 having verified all 13 `noctis*` crates in dependency order and skipped `xtask`, `music-bot`, `ui-recipes`.
- Given `release-plz.toml` and `release.yml`, when inspected, then `release-pr` runs on push to `main` without the crates.io token, `release` runs `release-plz release` only in `environment: release` with `CARGO_REGISTRY_TOKEN`, and `cargo run --release -p xtask -- release-check` prints the K1 ok line.
- Given a copy of the repo committed as a fresh git repository, when release-plz 0.4.1 `update` runs, then the only change is the P1 heading in `CHANGELOG.md`.
- Given `ci.yml`, when read, then it has the each-feature hack job, an all-features build and test job, the `check-default-graph.sh` step, `publish-dry-run`, `semver` and `release-check` jobs, and gates.sh/AGENTS.md mirror the local ones.
- Given `CHANGELOG.md` and `docs/guides/release.md`, when `docs-check` runs, then it passes; the changelog's `[Unreleased]` section is the 0.1.0 entry with `### Migration notes`, and the runbook covers every step of Stories 14.8 and 14.9.
- Given the repo, when `skill-check`, `api-snapshot`, `deps-check` and `rules-catalog` run, then they pass unchanged.

## Spec Change Log

## Review Triage Log

### 2026-10-25 — Review pass
- verdicts: 32 findings — high 0, medium 7, low 13, false 9, maybe-false 3
- findings:
  - `maybe-false` `defer` blind: release-plz release-pr auto-bumps changed crates after 0.1.0 (no `version_group`), contradicting the runbook's hand-bump model and the shared version — needs a release-plz run against a registry holding 0.1.0; deferred as medium (unverified).
  - `low` `patch` blind: a release PR opened with `GITHUB_TOKEN` triggers no `ci.yml` run, so the optional ruleset (runbook setup step 5) leaves it without required checks — GitHub does not start workflows for GITHUB_TOKEN events; runbook gets a note that the release PR is merged through the bypass.
  - `medium` `patch` blind: `release-retry.sh` treats any output containing `429` as a rate limit — `grep -qiE '429|…'` matches e.g. `src/lib.rs:429:5`, retrying a real failure 12 times 11 minutes apart; tighten the pattern and add a negative self-test case.
  - `low` `patch` blind: `real_repo_passes` hard-codes `0.1.0`, so the first hand bump breaks the test suite — derive the version from the root `[workspace.package] version` in the test.
  - `maybe-false` `reject` blind: the `publish-dry-run` gate leaves a second build tree on disk — where cargo's verify build lands and its size were not measurable (target was cleaned; full gates passed with 6.4 GB free); if true only low.
  - `medium` `patch` blind: `check-default-graph.sh` staleness guard covers only `image` — voice/prefix/testkit/songbird entries can go stale and pass vacuously; extend the guard to every forbidden entry.
  - `low` `reject` blind: `cargo tree` inspects only the host target and normal edges — the spec fixes the command (`-e normal`, no `--target`); the fix edits the spec, and a forbidden crate via a cfg(windows) or build dependency is implausible.
  - `low` `patch` blind: the CHANGELOG 0.1.0 `### Release` text names `.github/workflows-disabled/release.yml`, wrong once 14.8 moves the workflows — name `release.yml` without the disabled path.
  - `false` `reject` blind: unreadable root manifest reports "no [workspace.package] version" — spec K2 mandates exactly that message for a missing root `Cargo.toml`, and an unparsable manifest fails `cargo metadata` first (K3, no other checks).
  - `low` `reject` blind: `path_only` accepts `version = "*"` and `published` treats `publish = ["other-registry"]` as crates.io — spec model defines both rules; the inputs are contrived for this workspace and the fix adds raw-manifest parsing and registry guards.
  - `low` `reject` blind: R7 is a substring match on `commits` — spec R7 defines exactly this check; comparing against the golden body would edit the spec, and a body printing commits without the word is unlikely.
  - `low` `patch` blind: runbook says release-plz publishes in "the order release-check prints" — release-plz computes its own order; reword as "a valid dependency order, such as the one release-check prints".
  - `false` `reject` blind: missing CLI coverage for R3/R5/R7/C1/C2/C4/C5/D2/D4/H3 — the spec's task list names the CLI test set explicitly and assigns the other rows to unit tests, which exist.
  - `low` `patch` blind: `semver-check.sh` curl probe has no timeout — a stalled connection hangs the CI job; add `--max-time 30`.
  - `medium` `patch` edge: `429` regex false positive in `release-retry.sh` — grouped with the blind 429 finding.
  - `low` `reject` edge: attempts/wait digit strings beyond 64-bit wrap in bash arithmetic — only a hand-mistyped CI argument could reach it; fix adds a length bound.
  - `low` `reject` edge: K3 reports a leading `warning:` line of cargo metadata stderr instead of the error — rarely reached (invalid manifest), still fails loudly with the full stderr a rerun shows; fix adds a branch.
  - `low` `reject` edge: `publish = ["some-other-registry"]` counted as published — grouped with the blind path_only/published finding.
  - `medium` `patch` edge: forbidden voice/prefix/testkit/songbird names never proven live — grouped with the blind staleness-guard finding.
  - `low` `reject` edge: `~~~` fences and trailing whitespace on `## [Unreleased]` give false H1/H2 problems — spec H2 names only backtick fences; the CHANGELOG uses neither, a false report fails loudly, and the fix adds branches.
  - `maybe-false` `defer` edge: release PR auto-bumps after 0.1.0 independent of CHANGELOG headings — grouped with the blind release-plz finding (deferred).
  - `medium` `patch` vgap: check-default-graph.sh forbidden loop is never shown to fail — grouped with the staleness-guard entry; the extended guard runs the loop over the `image,voice,prefix,testkit` graph and requires every entry to hit.
  - `medium` `patch` vgap: release-check branches untested (no workspace repository, non-string body, missing/non-array changelog_include, non-table `[[package]]`, cycle reporting in `check`) — add the listed unit tests.
  - `medium` `patch` vgap (other): bare `429` substring in `release-retry.sh` — grouped with the blind 429 finding.
  - `false` `reject` intent: CHANGELOG has no `## [0.1.0]` heading — Design Notes D2 and the Never list deliberately settle this (a committed heading stops the release PR).
  - `false` `reject` intent: no semver comparison before the first publish — S1/D6 settle it; Verification ran `--baseline-rev HEAD`.
  - `false` `reject` intent: CI publish-dry-run never executed — CI is paused by the maintainer; the identical command runs in the local gate.
  - `false` `reject` intent: topological order instead of AD-2 layers — a dependency order is layer-compatible because deps-check enforces layered edges.
  - `false` `reject` intent: release PR behaviour asserted, not exercised — Verification P1 ran release-plz 0.4.1 `update` (the release-pr computation) on a committed copy; publishing needs the maintainer token (14.9).
  - `false` `reject` intent: examples excluded besides xtask — they are `publish = false`; consistent with the intent.
  - `low` `patch` intent: runbook omits 14.9's "README roadmap marks Milestones 1-4 done" and the copied-out template `cargo build --release` check — add both to "Publishing 0.1.0" step 5 (spec AC: runbook covers every step of 14.8 and 14.9).
  - `false` `reject` intent: all-features tests not exercised by any gate — `tools/story-loop/gates.sh` runs `cargo test --release --workspace --all-features`.

## Design Notes

**Decisions (logged in the memlog at planning).** D1 release-plz 0.4.1 with `release_always = false`: `release-pr` uses `release-plz/action@v0.5.146` (`version: 0.4.1`, `command: release-pr`, only `GITHUB_TOKEN`); `release` installs the CLI with `taiki-e/install-action@v2` (`tool: release-plz@0.4.1`), runs `release-plz/git-config@59144859caf016f8b817a2ac9b051578729173c4`, then `bash .github/scripts/release-retry.sh 12 660 -- release-plz release --git-token "$GITHUB_TOKEN" --forge github` (crates.io: burst 5 new crates, then 1 per 10 min; `timeout-minutes: 180`; retried runs skip published crates). D2 one hand-curated root `CHANGELOG.md`; release-plz writes it only for `noctis`, with a commits-free body that stamps the version heading under `## [Unreleased]`; a committed `## [0.1.0]` heading makes release-plz 0.4.1 treat the changelog as current (`changelog.rs` "already contains this version"), so no PR would open. D3 cyclic `noctis` dev-deps are path-only (cargo strips them on publish). D4 only `noctis` gets a tag (`v{{ version }}`) and GitHub release. D5 examples and xtask are never released. D6 semver: CI `semver` job via script (skip before the first publish) plus release-plz `semver_check` on `noctis`; CI-only (AD-28). D7 new `xtask release-check` guards these invariants offline. D8 workflows stay disabled; re-enabling is runbook step 1 of 14.8. License texts are not copied into each crate (the `license` field is enough for crates.io).

**Golden `release-plz.toml` (R2 table = every key below):**
```toml
[workspace]
release_always = false
changelog_update = false
semver_check = false
git_tag_enable = false
git_release_enable = false

[changelog]
trim = false
body = """
## [{{ version }}]{%- if release_link -%}({{ release_link }}){% endif %} - {{ timestamp | date(format="%Y-%m-%d") }}
"""

[[package]]
name = "noctis"
changelog_update = true
changelog_path = "CHANGELOG.md"
changelog_include = ["noctis-codec", "noctis-core", "noctis-gateway", "noctis-i18n", "noctis-image", "noctis-macros", "noctis-ports", "noctis-prefix", "noctis-rules", "noctis-testkit", "noctis-ui", "noctis-voice"]
semver_check = true
git_tag_enable = true
git_tag_name = "v{{ version }}"
git_release_enable = true
```
plus `[[package]] name = "xtask" | "music-bot" | "ui-recipes"` with `release = false`. `changelog.trim` and `changelog.body` presence are R2/R7 checks.

**`release.yml` shape:** `on: push: branches: [main]` and `workflow_dispatch`; workflow `permissions: contents: read`; both jobs `if: github.repository == 'ErdemGKSL/noctis-rs'`, checkout `actions/checkout@v7` with `fetch-depth: 0`, `persist-credentials: false`, then `rustup toolchain install`. `release-pr`: `permissions: contents: write, pull-requests: write`, `concurrency: {group: release-pr-${{ github.ref }}, cancel-in-progress: false}`. `release`: `environment: release`, `permissions: contents: write, pull-requests: read`, `concurrency: {group: release, cancel-in-progress: false}`, env `GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}`, `CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}`.

## Verification

**Commands:**
- `cargo test --release -p xtask` -- expected: unit and CLI tests pass, incl. `real_repo_passes`
- `cargo run --release -p xtask -- release-check` -- expected: K1 ok line, exit 0
- `bash .github/scripts/release-retry-test.sh` -- expected: `release-retry-test: ok`
- `bash .github/scripts/check-default-graph.sh` -- expected: `check-default-graph: ok`
- `bash .github/scripts/semver-check.sh` -- expected: S1 line, exit 0; then download `https://github.com/obi1kenobi/cargo-semver-checks/releases/download/v0.51.0/cargo-semver-checks-x86_64-unknown-linux-gnu.tar.gz` into the scratchpad, put it on `PATH`, run `bash .github/scripts/semver-check.sh --baseline-rev HEAD` -- expected: exit 0 (no API change)
- `/home/claude/lanes/d/noctis-rs/tools/story-loop/bg.sh cargo publish --workspace --dry-run --locked --allow-dirty` -- expected: exit 0, 13 crates verified
- P1: download `https://github.com/release-plz/release-plz/releases/download/release-plz-v0.4.1/release-plz-x86_64-unknown-linux-gnu.tar.gz` into the scratchpad; copy the tree (`git ls-files -co --exclude-standard`) to a scratch dir, `git init -b main`, commit, `git remote add origin https://github.com/ErdemGKSL/noctis-rs`, run `release-plz update` there -- expected: `git diff` shows only the P1 lines in `CHANGELOG.md`; delete the copy
- `for f in image image-gif image-emoji default-font i18n testkit prefix voice; do cargo check --release -p noctis --all-targets --features $f || break; done` -- expected: all pass (local stand-in for the hack matrix)
- `cargo run --release -p xtask -- docs-check`, `-- skill-check`, `-- api-snapshot`, `-- deps-check`, `-- rules-catalog` -- expected: ok
- `bash /home/claude/lanes/d/noctis-rs/tools/story-loop/gates.sh` via `bg.sh` -- expected: `ALL-OK` (~7 GB free at planning; if the disk fills, delete `target/debug/`)

## Auto Run Result

- **Summary:** release pipeline ready for 0.1.0 (CI still paused): release-plz 0.4.1 config (only `noctis` tagged/released, examples and xtask never released), `release.yml` (release-pr + rate-limit-retrying `release` job), CI semver and default-graph jobs, hand-curated root `CHANGELOG.md`, new offline `xtask release-check` guarding the invariants, `docs/guides/release.md` runbook for 14.8/14.9; path-only cyclic dev-deps.
- **Files changed:** `.github/scripts/{check-default-graph,release-retry,release-retry-test,semver-check}.sh` (CI helpers); `.github/workflows-disabled/{ci,release}.yml` (jobs); `release-plz.toml` (golden config); `CHANGELOG.md`; `docs/guides/release.md` + guides index; `xtask/src/release_check/*`, `xtask/src/main.rs`, `xtask/tests/release_check.rs` (release-check); crate `Cargo.toml`s (path-only dev-deps, metadata); `tools/story-loop/gates.sh` (release-check + publish dry-run gates); AGENTS.md, README.md, memlog, sprint-status.
- **Review findings:** 32 (high 0, medium 7, low 13, false 9, maybe-false 3). Patched: 429 false-positive regex in `release-retry.sh` (+negative self-test), staleness guard extended to every forbidden default-graph entry, release-check unit tests for untested branches, version derived from workspace in `real_repo_passes`, `--max-time 30` on semver curl probe, CHANGELOG release path wording, runbook order wording, GITHUB_TOKEN release-PR/ruleset note, 14.9 runbook steps (patched by verdict: medium 4 unique entries / 7 incl. grouped, low 6).
- **Deferred:** release-plz may auto-bump changed crates after 0.1.0 without a `version_group`, contradicting the hand-bump model (needs a registry holding 0.1.0 to verify).
- **Rejected:** host-target-only `cargo tree` (spec-fixed command); exotic `version="*"`/alt-registry inputs; R7 substring check (spec-defined); K2/K3 message edge cases; bash digit overflow on CI args; `~~~` fences; publish dry-run second build tree (unmeasurable, disk ok); intent findings settled by D2/D6/S1/D8, AD-2 layering, paused CI, and gates.sh all-features tests.
- **Follow-up review recommended: true** -- two or more medium entries patched on a first pass; unverified risk: the rewritten 429 rate-limit pattern in `release-retry.sh` and the extended default-graph staleness guard have only been exercised locally, never against a real crates.io 429 or CI run.
- **Verification:** `tools/story-loop/gates.sh` all green locally (fmt, clippy, workspace tests incl. all-features, deps-check, tw-grammar, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV, release-check, publish dry-run); `release-retry-test.sh`, `check-default-graph.sh`, `semver-check.sh --baseline-rev HEAD` ok. CI paused by maintainer.
- **Residual risks:** deferred release-plz auto-bump behaviour after 0.1.0; workflows never executed on GitHub until 14.8 re-enables them; real publish needs the maintainer token (14.9).
