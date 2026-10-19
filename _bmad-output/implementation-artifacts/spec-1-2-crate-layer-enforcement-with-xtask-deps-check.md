---
title: 'Story 1.2: Crate layer enforcement with xtask deps-check'
type: 'feature'
created: '2026-10-19'
status: 'in-progress'
baseline_revision: '88b429c92cadafc030db8dccb54a12123e360379'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Nothing stops a crate from adding a dependency edge the spine's Invariants graph forbids, so the hexagonal layering (AD-1, AD-2) can erode silently as later stories add edges (Story 1.2, Epic 1; AD-28 CI gate).

**Approach:** Add an `xtask deps-check` task that reads the workspace with `cargo_metadata` (no build), checks every workspace dependency edge against an allow-list encoded from the Invariants graph (including feature-gated edges), prints every violation at once naming the edge and the rule id, and exits non-zero; run it as a blocking `ci.yml` job.

## Boundaries & Constraints

**Always:**
- The allow-list is exactly the spine's Invariants graph (spine l.58-88), listed in Design Notes; the facade `noctis` may depend on any internal crate except `noctis-testkit`, which it reaches only through its `testkit` feature.
- "Internal crate" = a workspace member whose name is `noctis` or starts with `noctis-`. Edges are matched by the dependency's package `name` (ignore `rename`), for every `target` (target-specific deps count like plain ones).
- Every violation line has the form `deps-check: <from> -> <to> (<kind>): <reason> [<rule-id>]` (`<kind>` = `normal`, `build` or `dev`; crate-level rules omit `-> <to>`), all on stderr; all violations are reported in one run, sorted, deterministic.
- Exit codes: 0 pass (stderr `deps-check: ok (<n> internal crates, <m> internal edges)`, where `<m>` counts distinct (from, to) pairs between internal crates), 1 violations or `cargo metadata` failure, 2 usage error.
- Only `cargo_metadata` (workspace dep `0.23`, resolves to 0.23.1, MSRV 1.86, MIT) is added; it goes in `xtask/[dependencies]` via `cargo_metadata.workspace = true`. Tests use no extra crates.
- All cargo commands use `--release`; output uses `eprintln!` (workspace lint `print_stdout` warns).

**Never:**
- No internal edges are added to any crate in this story (later stories add them; deps-check guards them).
- No build of the inspected workspace: metadata only (`MetadataCommand::new().no_deps()`).
- Do not touch `examples/project-template/` (no public API change), `docs/discord-rules.md` (not a Discord rule) or `_bmad/`.
- Rule ids must not use the catalog's `<AREA>-<nn>` shape (they would collide with `rules-catalog` in Story 1.7); use the kebab ids below.

## I/O & Edge-Case Matrix

| Scenario | Fixture state | Expected | Rule id |
|---|---|---|---|
| Clean | 12 crates, no edges | exit 0, `ok` line | - |
| Full graph | every drawn edge; image edges optional behind `render = ["dep:noctis-core", "dep:noctis-ports", "dep:noctis-ui"]`; facade `testkit = ["dep:noctis-testkit"]` with optional dep; facade depends on all other crates; macros -> image plain | exit 0 | - |
| Undrawn edge | `noctis-ui` -> `noctis-core` normal | exit 1, line names `noctis-ui -> noctis-core (normal)` | `undrawn-edge` |
| Testkit to image | `noctis-testkit` -> `noctis-image` | exit 1 | `undrawn-edge` |
| Build dep | `noctis-rules` build-dep -> `noctis-codec` | exit 1 | `undrawn-edge` |
| Depends on facade | `noctis-core` -> `noctis` normal | exit 1 | `depends-on-facade` |
| Facade dev-dep allowed | `noctis-core` dev-dep -> `noctis` (any features) | exit 0 | - |
| Dev-dep to drawn ungated edge | `noctis-ui` dev-dep -> `noctis-codec` | exit 0 | - |
| Dev-dep elsewhere | `noctis-core` dev-dep -> `noctis-testkit` | exit 1 | `undrawn-edge` |
| Image edge not optional | `noctis-image` -> `noctis-core` non-optional | exit 1 | `ungated-edge` |
| Image edge implicit feature | optional, no `dep:` use (implicit feature `noctis-core`) | exit 1, names feature `noctis-core` | `ungated-edge` |
| Image edge other feature | optional, `extra = ["dep:noctis-ui"]` with `extra` not implying `render` | exit 1, names `extra` | `ungated-edge` |
| Feature implies render | `all = ["render"]`, `render = ["dep:noctis-core"]` | exit 0 | - |
| Testkit non-optional | `noctis` -> `noctis-testkit` non-optional | exit 1 | `ungated-edge` |
| Testkit via other feature | `image = ["dep:noctis-testkit"]` | exit 1, names `image` | `ungated-edge` |
| Facade dev-dep testkit | `noctis` dev-dep -> `noctis-testkit` | exit 1 | `ungated-edge` |
| Macros turns render on | `noctis-macros` -> `noctis-image` with `features = ["render"]`, or a macros feature listing `noctis-image/render` | exit 1 | `render-on` |
| Image default render | `noctis-image` `default = ["render"]` | exit 1 | `render-on` |
| Unknown crate | extra member `crates/noctis-foo` | exit 1 | `unknown-crate` |
| Missing crate | fixture lacks `noctis-prefix` | exit 1 | `missing-crate` |
| Non-internal member | member `xtask` -> `noctis-core` (any kind) | exit 1 | `bot-dependency` |
| Non-internal member uses facade | member `bot` -> `noctis` | exit 0 | - |
| Several violations | two forbidden edges | exit 1, both lines | both |
| Usage | `deps-check --bogus` or `--manifest-path` without value | exit 2, usage on stderr | - |
| Metadata failure | `--manifest-path` to a missing file | exit 1, `deps-check: cargo metadata failed: ...` | - |

</intent-contract>

## Code Map

- `xtask/src/main.rs` (edit) -- task dispatch; currently `None | "help"` -> usage exit 0, other -> `unknown task` exit 2. Add `deps-check [--manifest-path <path>]`; update `USAGE` and the `//!` doc (no longer "no tasks yet").
- `xtask/src/deps_check.rs` (new) -- model, allow-list, checks, report.
- `xtask/Cargo.toml` (edit) -- add `[dependencies] cargo_metadata.workspace = true`.
- `xtask/tests/deps_check.rs` (new) -- CLI integration tests over generated fixture workspaces.
- `Cargo.toml` (read-only) -- `[workspace.dependencies] cargo_metadata = "0.23"` already present; members list (12 crates + `xtask`).
- `Cargo.lock` (edit) -- regenerated with cargo_metadata's tree.
- `.github/workflows/ci.yml` (edit) -- add job `deps-check` (same setup steps as other jobs).
- `AGENTS.md` (edit) -- line 23 verification bullet.
- `crates/noctis-image/Cargo.toml` (read-only) -- `default = []`, `render = []`; `crates/noctis/Cargo.toml` (read-only) -- empty features incl. `testkit`.
- Spine `ARCHITECTURE-SPINE.md` l.36-55 (layers), l.58-96 (Invariants graph and prose) -- source of the allow-list.
- Verified: `cargo metadata --no-deps` lists implicit optional-dep features (`"b": ["dep:b"]`), dependency `kind` (`null`/`"dev"`/`"build"`), `optional`, `uses_default_features`, `features`, `target`, and writes no lockfile for path-only workspaces.

## Tasks & Acceptance

**Execution:**
- `xtask/Cargo.toml` -- add `cargo_metadata.workspace = true`; run `cargo update -p cargo_metadata` (or any `--release` build) so `Cargo.lock` pins 0.23.1.
- `xtask/src/deps_check.rs` -- (1) Model: `Workspace { crates: Vec<Member> }`, `Member { name, features: BTreeMap<String, Vec<String>>, deps: Vec<Dep> }`, `Dep { name, kind: Kind, optional, uses_default_features, features }`, `enum Kind { Normal, Build, Dev }`; `Workspace::from_metadata(&Metadata)` keeps only workspace members and only deps whose `name` is a workspace member. (2) `enum Rule { UndrawnEdge, DependsOnFacade, UngatedEdge, RenderOn, UnknownCrate, MissingCrate, BotDependency }` with `fn id(self) -> &'static str` returning the kebab ids; `Violation { rule, from, to: Option<String>, kind: Option<Kind>, reason: String }` implementing `Display` in the line format above, `Ord` for sorting. (3) `const GRAPH: &[(&str, &str)]` (21 ungated edges) and `const GATED: &[(&str, &str, &str)]` (from, to, feature) from Design Notes. (4) `fn check(&Workspace) -> Vec<Violation>` applying the rules in Design Notes, with a feature-closure helper. (5) `pub fn run(manifest_path: &Path) -> ExitCode`. Unit tests: GRAPH+GATED is acyclic; every name in them is one of the 12 crates; closure handles `dep:x`, `x/f`, `x?/f` (does not activate) and transitive features.
- `xtask/src/main.rs` -- parse `deps-check` with optional `--manifest-path <path>`; default manifest = `Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml")` so it works from any cwd; bad args -> usage, exit 2.
- `xtask/tests/deps_check.rs` -- helper writes a fixture workspace under `env!("CARGO_TARGET_TMPDIR")/<test-name>/` (remove first): root `Cargo.toml` with its own `[workspace] resolver = "3"`, the 12 crates (each `edition = "2024"`, `version = "0.1.0"`, empty `src/lib.rs`, `noctis-macros` need not be proc-macro) plus per-crate extra TOML and optional extra members; runs `env!("CARGO_BIN_EXE_xtask")` with `deps-check --manifest-path <fixture>/Cargo.toml`; asserts exit code and stderr substrings (edge and rule id). One test per I/O matrix row.
- `.github/workflows/ci.yml` -- job `deps-check` (name `Deps check`): checkout, `rustup toolchain install`, `Swatinem/rust-cache@v2`, `cargo run --release -p xtask -- deps-check`.
- `AGENTS.md` -- line 23: add `cargo run --release -p xtask -- deps-check` to the local gate list, and replace "plus `cargo deny check` and the MSRV check" with "plus `cargo deny check` (cargo-deny 0.20) and the MSRV check `cargo +1.91 check --release --workspace`" (closes Story 1.1's deferred finding).

**Acceptance Criteria:**
- Given the real workspace, when `cargo run --release -p xtask -- deps-check` runs from the repo root and from `crates/noctis`, then it exits 0 and prints the `ok` line.
- Given a temporary edit adding `noctis-core = { path = "../noctis-core" }` to `crates/noctis-ui/Cargo.toml`, when deps-check runs, then it exits 1 and stderr contains `noctis-ui -> noctis-core (normal)` and `[undrawn-edge]`; reverting restores exit 0.
- Given every I/O matrix row as a fixture, when `cargo test --release -p xtask` runs, then each test passes.
- Given `ci.yml`, when it runs on a push to `main` or a pull request, then job `deps-check` runs without `continue-on-error` and the run concludes `success`.

## Spec Change Log

## Review Triage Log

## Design Notes

Allow-list (from -> to). Ungated: `noctis-prefix -> noctis-core`; `noctis-testkit -> noctis-core, noctis-gateway, noctis-ports`; `noctis-core -> noctis-gateway, noctis-i18n, noctis-ui, noctis-ports, noctis-codec, noctis-rules`; `noctis-i18n -> noctis-ports, noctis-rules`; `noctis-gateway -> noctis-ports`; `noctis-ui -> noctis-codec, noctis-rules`; `noctis-codec -> noctis-rules`; `noctis-macros -> noctis-rules, noctis-image` (21). Gated: `noctis-image -> noctis-core | noctis-ports | noctis-ui` by `render`; `noctis -> noctis-testkit` by `testkit`.

Rules, for each internal crate `A` and internal dep `B`:
- `A == noctis`: any `B` except `noctis-testkit` is allowed (any kind).
- `B == noctis`: `dev` allowed; `normal`/`build` -> `depends-on-facade`.
- `(A, B)` gated by feature `G`: dep must be `normal` and `optional`, else `ungated-edge`; for every feature `F` of `A` whose closure activates `B` and does not contain `G` -> `ungated-edge` naming `F`.
- `(A, B)` in GRAPH: allowed for every kind.
- Otherwise `undrawn-edge` (covers `testkit -> image`, dev-deps on `noctis-testkit`, build deps).
- `render-on`: `noctis-macros`' dep on `noctis-image` lists `render`, or uses default features while `noctis-image`'s `default` closure contains `render`; any `noctis-macros` feature whose closure contains `noctis-image/render` or `noctis-image?/render`; `noctis-image`'s own `default` closure contains `render`.
- Non-internal members (`xtask`, future `examples/<feature>`): may depend on `noctis` only (AD-2: bot code uses the facade); any other internal dep -> `bot-dependency`.
- A member named `noctis`/`noctis-*` outside the 12 -> `unknown-crate` (add it to the spine graph and GRAPH first); one of the 12 absent -> `missing-crate`.

Feature closure of `F`: walk values; `"dep:x"` activates `x`; `"x/f"` activates dep `x` (when `x` is a dep) and is recorded as `x/f`; `"x?/f"` is recorded but does not activate `x`; a bare name is a feature, walked recursively (implicit optional-dep features appear in metadata as `x = ["dep:x"]`, so they need no special case). Track visited features.

Decisions (log in the architecture `.memlog.md`): kebab rule ids; dev-deps of internal crates may target `noctis` or any ungated drawn edge, nothing else (so tests reach testkit via `noctis` + `testkit`); non-graph members follow AD-2; deps-check fails on unknown/missing `noctis*` members so graph and workspace stay in sync.

## Verification

**Commands** (repo root):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `cargo test --release --workspace` -- exit 0, includes `xtask` unit and integration tests.
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo run --release -p xtask -- deps-check` -- exit 0, `ok` line.
- `cargo +1.91 check --release --workspace` -- exit 0.
- `cargo deny check` -- exit 0 (cargo_metadata tree is MIT/Apache/Unicode-3.0 only).
- `test ! -d target/debug` -- true.
- After the main session pushes: `gh run list --branch main --workflow ci.yml --limit 1` -- conclusion `success`, job `Deps check` present.

**Manual checks:**
- The mutation AC above (ui -> core), then revert with `git checkout crates/noctis-ui/Cargo.toml`.
