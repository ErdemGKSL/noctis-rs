---
title: 'Story 1.2: Crate layer enforcement with xtask deps-check'
type: 'feature'
created: '2026-10-19'
status: 'done'
baseline_revision: '88b429c92cadafc030db8dccb54a12123e360379'
review_loop_iteration: 2
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
deferred:
  - summary: >-
      deps-check never inspects examples/project-template, the one real bot crate, so AD-2 (bot code depends only on `noctis`) and the spine's "bot crates enable `testkit` in [dev-dependencies] only" are not checked on it.
    evidence: |-
      Root Cargo.toml has `exclude = ["examples/project-template"]` and the template is its own workspace, so `cargo metadata` on the noctis-rs workspace never sees it. Running deps-check on the template as-is would also misfire: its package name `noctis-bot-template` matches `is_internal` (`noctis-*`), so it would be reported as `unknown-crate` plus 12 `missing-crate`, and its path dep on `noctis` is dropped because it is not a member of the template's workspace. Story 1.2's intent scopes the check to the workspace, so this needs a bot-manifest mode (and a CI step) in a later story, e.g. when the template starts compiling (Story 14.2).
    location: >-
      xtask/src/deps_check.rs:196 (Workspace::from_metadata); Cargo.toml:19 (exclude)
    severity: medium
  - summary: >-
      The AGENTS.md local-gate line chains its commands with a doubled "and", which makes the list hard to read.
    evidence: |-
      AGENTS.md line 23 reads "... `cargo test --release --workspace` and `cargo hack test --release --workspace --each-feature` (cargo-hack 0.6) and `cargo run --release -p xtask -- deps-check`; ...". Fix: comma-separate the list, keeping only the last "and". Deferred because the fix edits an agent-context file.
    location: >-
      AGENTS.md:23
    severity: low
  - summary: >-
      AGENTS.md lists the deps-check gate but gives no guidance for a failing run: that a crate or edge change edits the spine's Invariants graph and CRATES/GRAPH/GATED in xtask/src/deps_check.rs together.
    evidence: |-
      The `unknown-crate` reason names the constants, but the `undrawn-edge` reason only says "edge is not drawn in the spine's Invariants graph", and AGENTS.md has no paired-file line like the one it states for docs/discord-rules.md and noctis-rules. Fix: one line under "Where things are" or "Running and verifying". Deferred because the fix edits an agent-context file.
    location: >-
      AGENTS.md:23
    severity: low
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
- `xtask/src/deps_check.rs` -- (1) Model: `Workspace { crates: Vec<Member> }`, `Member { name, features: BTreeMap<String, Vec<String>>, deps: Vec<Dep> }`, `Dep { name, key, kind: Kind, optional, uses_default_features, features }` (`name` = package name for edge matching; `key` = the rename-aware feature-syntax key, i.e. `rename` if set, else `name`, used to match `dep:k`, `k/f`, `k?/f` in feature closures), `enum Kind { Normal, Build, Dev }`, `Member::package_for(key)`; `Workspace::from_metadata(&Metadata)` keeps only workspace members, and keeps every dep whose `name` is a workspace member OR `is_internal` (`noctis` / `noctis-*`), so a registry or out-of-tree `noctis-*` dep is checked like any edge (e.g. `noctis-ui` -> `noctis-core = "0.1"` is `undrawn-edge`). (2) `enum Rule { UndrawnEdge, DependsOnFacade, UngatedEdge, RenderOn, UnknownCrate, MissingCrate, BotDependency }` with `fn id(self) -> &'static str` returning the kebab ids; `Violation { from: String, edge: Option<(String, Kind)>, rule: Rule, reason: String }` (`edge` = the target and kind, `None` for crate-level rules) implementing `Display` in the line format above, derived `Ord` in that field order (from, edge, rule, reason), collected in a `BTreeSet` so the report is sorted and deduplicated (one line per distinct violation even when the same edge is declared under several targets). (3) `const CRATES` (the 12 names), `const GRAPH: &[(&str, &str)]` (18 ungated edges) and `const GATED: &[(&str, &str, &str)]` (from, to, feature) from Design Notes. (4) `fn check(&Workspace) -> Vec<Violation>` applying the rules in Design Notes (split into `check_edge`, `check_gate_features`, `check_render` and the non-internal-member check, including the bot `testkit` rule and the facade `default` rule), with a feature-closure helper and a facade helper `enables_testkit(feature)` (the analog of `check_render`'s `enables_render`, defined exactly as in Design Notes "Facade feature enables testkit"), used by both the facade `default` rule and the bot rule; `check_edge` keeps its order (gate first, then the facade rule, then GRAPH, then `depends-on-facade`, then `undrawn-edge`), with the facade rule allowing only targets in CRATES. The `unknown-crate` reason names the constants to update: "add it to the spine's Invariants graph and to CRATES/GRAPH/GATED in xtask/src/deps_check.rs first". The module `//!` doc states that the `noctis-macros -> noctis-image` label's "tw module only" half is a source-level property deps-check does not check; only "render feature off" is enforced (`render-on`). (5) `pub fn run(manifest_path: &Path) -> ExitCode`. Unit tests: GRAPH has 18 and GATED 4 edges; GRAPH+GATED is acyclic (Kahn); every name in them is one of the 12 crates; closure handles `dep:x`, `x/f`, `x?/f` (does not activate) and transitive features; `Display` line format.
- `xtask/src/main.rs` -- parse `deps-check` with optional `--manifest-path <path>`; default manifest = `Path::new(env!("CARGO_MANIFEST_DIR")).join("../Cargo.toml")` so it works from any cwd; `help`, `--help` and `-h` (and no task) print usage, exit 0; bad args -> usage, exit 2; only the two-token `--manifest-path <path>` form is accepted.
- `xtask/tests/deps_check.rs` -- helper writes a fixture workspace under `env!("CARGO_TARGET_TMPDIR")/<test-name>/` (remove first): root `Cargo.toml` with its own `[workspace] resolver = "3"`, the 12 crates (each `edition = "2024"`, `version = "0.1.0"`, empty `src/lib.rs`, `noctis-macros` need not be proc-macro) plus per-crate extra TOML and optional extra members; runs `env!("CARGO_BIN_EXE_xtask")` with `deps-check --manifest-path <fixture>/Cargo.toml`; asserts exit code and stderr substrings (edge and rule id). One test per I/O matrix row, plus these rows (they extend the matrix, which lives in the intent contract):
  - Bot testkit in normal deps: `bot` -> `noctis` normal with `features = ["testkit"]` -> exit 1, `[bot-dependency]`, reason names `testkit`.
  - Bot feature enables testkit: `bot` feature `t = ["noctis/testkit"]` with a normal dep on `noctis` -> exit 1, `[bot-dependency]` naming `t`.
  - Bot dev-dep testkit: `bot` dev-dep -> `noctis` with `features = ["testkit"]` -> exit 0.
  - Renamed gated dep: `noctis-image` with `core = { package = "noctis-core", path = "../noctis-core", optional = true }` and `extra = ["dep:core"]` -> exit 1, `noctis-image -> noctis-core (normal): feature `extra``, `[ungated-edge]`.
  - Renamed image dep in macros: `img = { package = "noctis-image", path = ... }`, feature `f = ["img/render"]` -> `[render-on]`.
  - Weak `noctis-image?/render` in a `noctis-macros` feature -> `[render-on]`.
  - `noctis-macros` using `noctis-image` default features while image `default = ["render"]` -> the macros edge line with `[render-on]` (besides image's own `render-on` line).
  - Undrawn edge declared only under `[target.'cfg(unix)'.dependencies]` -> `[undrawn-edge]`.
  - Report order: fixture without `noctis-prefix` plus an undrawn `noctis-codec` edge; the `noctis-codec ->` line precedes `deps-check: noctis-prefix:`.
  - Report dedup: `noctis-ui -> noctis-core` under both `[dependencies]` and `[target.'cfg(unix)'.dependencies]`; the line `noctis-ui -> noctis-core (normal)` appears exactly once.
  - Out-of-workspace internal dep: `noctis-ui` -> a path dep to a `noctis-core` package outside the fixture workspace (any form `cargo metadata --no-deps` resolves offline) -> exit 1, `[undrawn-edge]`.
  - Review loop 2 rows. "Testkit facade" below = the facade with an optional normal dep on `noctis-testkit`, `testkit = ["dep:noctis-testkit"]` and `full = ["testkit"]`, plus member `bot` (non-internal):
    - Bot normal dep `features = ["full"]` -> exit 1, `bot -> noctis (normal)`, `[bot-dependency]`, reason names `full`.
    - Bot build-dep `features = ["full"]` -> exit 1, `bot -> noctis (build)`, `[bot-dependency]`.
    - Bot normal dep plus bot feature `t = ["noctis/full"]` -> exit 1, `[bot-dependency]`, reason names `t` and `full`.
    - Bot optional normal dep plus bot feature `t = ["noctis?/full"]` -> exit 1, `[bot-dependency]` naming `t`.
    - Renamed: bot normal dep `nx = { package = "noctis", path = ..., optional = true }` plus `t = ["nx/full"]` -> exit 1, `[bot-dependency]` naming `t`.
    - Transitive bot feature: `a = ["b"]`, `b = ["noctis/full"]` -> exit 1, `[bot-dependency]` lines for both `a` and `b`.
    - Bot dev-dep `features = ["full"]` and a separate normal dep on `noctis` without features -> exit 0.
    - Variant facade with `full = ["dep:noctis-testkit"]` instead of `["testkit"]`: the facade gets its `ungated-edge` line naming `full`, and a bot normal dep `features = ["full"]` also gets `[bot-dependency]` (the helper counts activation of the `noctis-testkit` package too).
    - Facade `default = ["testkit"]` (no bot) -> exit 1, `deps-check: noctis: default features enable `testkit` ... [ungated-edge]` (crate-level line, no `->`).
    - Facade `default = ["full"]` plus a bot normal dep with default features -> exit 1, both the facade crate-level `[ungated-edge]` line and `bot -> noctis (normal)` `[bot-dependency]` naming default features; the same bot dep with `default-features = false` -> only the facade line.
    - Facade -> out-of-workspace `noctis-foo` path crate -> exit 1, `noctis -> noctis-foo (normal)`, `[undrawn-edge]`.
    - Facade -> member `bot` (normal) -> exit 1, `noctis -> bot (normal)`, `[undrawn-edge]`.
    - Facade `[build-dependencies]` and `[dev-dependencies]` on `noctis-rules` -> exit 0.
    - `noctis-core -> bot` (normal) with member `bot` -> exit 1, `noctis-core -> bot (normal)`, `[undrawn-edge]` (pins the `names.contains` half of the `from_metadata` filter).
- `.github/workflows/ci.yml` -- job `deps-check` (name `Deps check`): checkout, `rustup toolchain install`, `Swatinem/rust-cache@v2`, `cargo run --release -p xtask -- deps-check`.
- `AGENTS.md` -- line 23: add `cargo run --release -p xtask -- deps-check` to the local gate list, and replace "plus `cargo deny check` and the MSRV check" with "plus `cargo deny check` (cargo-deny 0.20) and the MSRV check `cargo +1.91 check --release --workspace`" (closes Story 1.1's deferred finding).

**Acceptance Criteria:**
- Given the real workspace, when `cargo run --release -p xtask -- deps-check` runs from the repo root and from `crates/noctis`, then it exits 0 and prints the `ok` line.
- Given a temporary edit adding `noctis-core = { path = "../noctis-core" }` to `crates/noctis-ui/Cargo.toml`, when deps-check runs, then it exits 1 and stderr contains `noctis-ui -> noctis-core (normal)` and `[undrawn-edge]`; reverting restores exit 0.
- Given every I/O matrix row as a fixture, when `cargo test --release -p xtask` runs, then each test passes.
- Given `ci.yml`, when it runs on a push to `main` or a pull request, then job `deps-check` runs without `continue-on-error` and the run concludes `success`.

## Spec Change Log

### 2026-10-19 — Review loop 1 (bad_spec)
- **Trigger:** `[medium]` blind "bot crates may enable `testkit` only in `[dev-dependencies]`; nothing enforces it" and `[medium]` intent D3 (same root cause): a non-internal member with `noctis = { ..., features = ["testkit"] }` in `[dependencies]`, or a member feature listing `noctis/testkit`, passed deps-check. Design Notes encoded the spine graph but dropped the spine prose sentence that deps-check must enforce.
- **Amended (outside the intent contract):** Design Notes non-internal-member rule (bot `testkit` only via dev-deps, rename-aware, reuses `bot-dependency`); Design Notes edge count 21 -> 18; `unknown-crate` reason; Tasks for `deps_check.rs` (`Dep.key`, `from_metadata` keeps `is_internal` deps, `BTreeSet` sort+dedup, CRATES constant, module doc on "tw module only"), `main.rs` (`help`/`--help`/`-h` exit 0), and `tests/deps_check.rs` (three bot-testkit rows plus the folded-in patch fixtures: renamed deps, weak `?/render`, image default render via macros, target-specific edge, report order and dedup, out-of-workspace internal dep). The I/O matrix is inside the intent contract and was not edited; the new rows live in Tasks.
- **Known-bad state avoided:** a bot member enabling `noctis/testkit` from normal/build deps (directly or via its own feature, renamed or not) exits 0; also the patch-level gaps: out-of-tree `noctis-*` deps silently dropped, `--help` exiting 2, unknown-crate advice omitting CRATES, unpinned rename/order/dedup behavior.
- **KEEP (must survive re-derivation):**
  - The structure of `xtask/src/deps_check.rs`: `CRATES`/`GRAPH` (18)/`GATED` (4) constants, `Kind`, `Dep` with separate `name` and rename-aware `key`, `Member::package_for` and `closure` (`dep:`, `x/f`, weak `x?/f`, transitive, visited set), `Violation` with derived `Ord` over (from, edge, rule, reason) and the `Display` line format, `BTreeSet` collection, `check_edge`, `check_gate_features`, `check_render`, `run` with exit codes 0/1 and the `ok` line.
  - Rule ids exactly: `undrawn-edge`, `depends-on-facade`, `ungated-edge`, `render-on`, `unknown-crate`, `missing-crate`, `bot-dependency`.
  - Gated edges must be optional normal deps (a dev-dep on a gated edge stays `ungated-edge`); dev-deps on ungated drawn edges and on `noctis` pass.
  - `main.rs` parsing: two-token `--manifest-path <path>` only, usage errors exit 2, default manifest from `CARGO_MANIFEST_DIR`, all output via `eprintln!`.
  - The integration harness in `xtask/tests/deps_check.rs` (fixture builder under `CARGO_TARGET_TMPDIR`, `with`/`without`, `full_graph`, `assert_exit`) and all 26 first-pass tests (one per matrix row).
  - Unit tests: edge counts, names valid, Kahn acyclicity, closure, line format.
  - `ci.yml` `deps-check` job, the `AGENTS.md` gate line, `xtask/Cargo.toml` + `Cargo.lock` changes as specified.

### 2026-10-19 — Review loop 2 (bad_spec)
- **Trigger:** `[medium]` blind/edge/vgap/intent D2 "bot-side `testkit` check matches only the literal `testkit` and ignores the facade's feature closure" and "nothing forbids the facade's `default` from enabling `testkit`" (one root cause: Design Notes specified a literal match and no facade `default` rule); `[low]` blind "facade any-crate rule never checks the target is one of the 12 graph crates"; `[low]` blind missing fixtures for facade build/dev deps on a graph crate and bot default features; folded-in `[low]` vgap patch "`names.contains` half of the `from_metadata` filter is unpinned".
- **Amended (outside the intent contract):** Design Notes: facade rule restricted to `B` in CRATES (else `undrawn-edge`); new "Facade feature enables testkit" definition (closure contains `testkit` or activates the `noctis-testkit` package, rename-aware); new facade `default` crate-level `ungated-edge` rule; bot rule rewritten to resolve every requested facade feature from all three sources (dep `features`, `default` when `uses_default_features`, `k/f` / `k?/f` pairs from the member's own feature closures), normal and build kinds, every target, dev-only keys ignored. Tasks: `Violation` shape updated to the shipped `{ from, edge: Option<(String, Kind)>, rule, reason }` with `Ord` in that order; `enables_testkit` helper; `check_edge` order stated; 14 new fixture rows ("Review loop 2 rows"). `review_loop_iteration` 1 -> 2.
- **Known-bad state avoided:** an umbrella facade feature (`full = ["testkit"]`), a bot feature `noctis/full` or `noctis?/full`, a renamed or transitive bot feature, or a facade `default` that reaches testkit lets a bot enable `noctis-testkit` outside `[dev-dependencies]` with exit 0; the facade depending on a non-graph member (`xtask`, `bot`) or an out-of-tree `noctis-*` package passes; the `names.contains` filter half can be dropped with all tests green.
- **KEEP (must survive re-derivation):**
  - Base to rebuild from: the xtask code at commit `6848277` (current HEAD `ec2e244` carries the same code; HEAD only adds triage). Restore it with `git show 6848277:<path>` for `xtask/src/deps_check.rs`, `xtask/src/main.rs`, `xtask/tests/deps_check.rs`, `xtask/Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, `AGENTS.md`, then apply only the amendments above.
  - Everything in the loop-1 KEEP list.
  - `from_metadata` filter `names.contains(..) || is_internal(..)` with the rename-aware `key`.
  - `check()` structure: missing-crate, non-internal branch, unknown-crate with the CRATES/GRAPH/GATED reason, then per-edge, gate-feature and render checks.
  - `check_edge` order: gate first, then allowed (facade rule now CRATES-limited, then GRAPH), then `depends-on-facade`, then `undrawn-edge`.
  - `check_render` with `enables_render` and its `default` handling (template for `enables_testkit`).
  - The module `//!` doc on "tw module only".
  - `main.rs` as it is (`help`/`--help`/`-h` exit 0, two-token `--manifest-path`, exit 2 on usage).
  - Every existing integration test, including the bot-testkit, rename, weak `?/render`, target, order, dedup and out-of-workspace fixtures, and `help_flags_exit_0`.
  - Exit codes 0/1/2 as the intent contract fixes them; a metadata failure stays exit 1.
  - AGENTS.md: only the specified line 23 content; the doubled "and" stays deferred.

## Review Triage Log

### 2026-10-19 — Review pass
- verdicts: 26 findings — high 0, medium 6, low 12, false 8, maybe-false 0
- findings:
  - `[medium]` `[bad_spec]` (blind) Bot crates may enable the facade's `testkit` only in `[dev-dependencies]`; nothing enforces it — verified: in `check()` a non-internal member's dep on `noctis` passes whatever its kind and features, so `bot` with `noctis = { path = ..., features = ["testkit"] }` in `[dependencies]` (or a bot feature listing `noctis/testkit`) exits 0. The spine prose ("which bot crates enable in `[dev-dependencies]` only ... `xtask deps-check` enforces this list") selects the reading; Design Notes omitted it. Amendment made (Spec Change Log, review loop 1): rule added to Design Notes' non-internal-member bullet; the three new fixture rows added to Tasks (the I/O matrix is in the intent contract and was left unchanged).
  - `[medium]` `[defer]` (blind) Bot-crate rules never run on the real bot crate `examples/project-template` — verified: it is in the root `exclude` and is its own workspace; running deps-check on it today would misfire (`noctis-bot-template` matches `noctis-*`). Intent scopes the check to the workspace; deferred to a bot-manifest mode in a later story.
  - `[low]` `[reject]` (blind) Allow-list is not compared with the spine's mermaid block — real but narrow: adding a spine edge without updating GRAPH fails loudly (the new edge is `undrawn-edge`), adding a crate fails as `unknown-crate`; only an edge removed from the spine and left in GRAPH drifts silently, which is rare, and the fix is a mermaid parser in tests.
  - `[false]` `[reject]` (blind) Gated edges cannot be dev-deps and the message gives advice that cannot be followed — the rejection is settled by Design Notes ("gated: dep must be `normal` and `optional`") and matrix row "Facade dev-dep testkit"; the message tells the author to make it an optional normal dependency behind the feature, which is followable (tests then run with that feature).
  - `[low]` `[patch]` (blind) The "tw module only" half of the macros -> image label is neither enforced nor documented as out of reach — verified: module doc and GRAPH comment are silent; a source-level property metadata cannot see. Fix: one doc line saying deps-check checks only the "render feature off" half. Grouped with intent D2.
  - `[medium]` `[patch]` (blind) Feature-closure branches untested: renamed dep, weak `noctis-image?/render` in macros, macros using image's default features when image `default` enables `render`, target-specific undrawn edge — verified by grep of `xtask/tests/deps_check.rs` (no `package =`, no `?/`, no `[target.`). Fix: fixtures per branch. Grouped with vgap "renamed dependencies".
  - `[low]` `[patch]` (blind) `unknown-crate` reason says "add it to the spine and to GRAPH first" but a new crate must go into CRATES (the check consults CRATES) — verified at `deps_check.rs:344`. Fix: name CRATES (and GRAPH/GATED) in the reason. The duplicated CRATES list in the test file is not a silent drift (a missing fixture crate fails as `missing-crate`), so no change there.
  - `[low]` `[reject]` (blind) `--manifest-path=<path>` is rejected — it fails loudly with exit 2 and a usage message showing the supported two-token form (the form the spec and USAGE define); accepting it adds a branch for a rare input.
  - `[low]` `[reject]` (blind) Default manifest path is fixed at compile time and un-normalised — the spec prescribes `env!("CARGO_MANIFEST_DIR")`; xtask is always built and run in place via `cargo run -p xtask`, so a copied binary is not an everyday case, and runtime discovery adds code.
  - `[false]` `[reject]` (blind) Success line goes to stderr — deliberate: Boundaries mandate all output on stderr because the workspace lint `print_stdout` warns; CI gates on the exit code.
  - `[low]` `[reject]` (blind) Spec does not record delivered coverage — the fix is editing this build's spec (Finalize writes `## Auto Run Result`).
  - `[low]` `[patch]` (edge) A dep on a `noctis-*` crate that is not a workspace member (registry or out-of-tree path) is dropped and passes — verified: `from_metadata` keeps only deps whose name is a member (`deps_check.rs:207`), so `noctis-ui` with `noctis-core = "0.1"` exits 0. Fix: also keep deps whose name `is_internal`, so they are checked like any edge.
  - `[low]` `[reject]` (edge) `--manifest-path=<path>` single-token form rejected — same claim as the blind finding above; loud usage error for an unsupported form.
  - `[low]` `[patch]` (edge) `--help` / `-h` are reported as `unknown task` with exit 2 — verified at `main.rs:28`, only `help` matches. Fix: match `help | --help | -h`.
  - `[low]` `[reject]` (edge) Spec says GRAPH holds 21 ungated edges; code has 18 — the Design Notes list sums to 18 and the code is right; the only fix is editing this build's spec.
  - `[false]` `[reject]` (edge) Violation struct shape and sort key differ from the spec — the spec requires "sorted, deterministic", not a rule-first order; no test or consumer depends on rule-first order, and `from`, `edge`, `rule` ordering is deterministic.
  - `[low]` `[patch]` (vgap, pre-verified) Sort and dedup of the report are not observable by any test — filed evidence: replacing the BTreeSet by a Vec keeps every test green. Fix: a fixture where emission order differs from sorted order (`missing-crate` for `noctis-prefix` plus a `noctis-codec` edge) and a duplicated edge (`[dependencies]` plus `[target.'cfg(unix)'.dependencies]`) printed once.
  - `[medium]` `[patch]` (vgap, pre-verified) Renamed dependencies are never exercised, so the `key` mapping is unpinned and a regression lets a renamed optional dep bypass `ungated-edge` / `render-on` — filed evidence: `key: d.name.clone()` keeps all 26 tests green. Fix: renamed-dep fixtures. Grouped with blind "closure branches untested".
  - `[false]` `[reject]` (intent D1) The AC pass case on the real workspace is vacuous; evidence lives on fixtures — the spec deliberately settles this: Never adds no internal edges in this story, the full-graph fixture covers the drawn graph, and the real run plus the manual ui -> core mutation are the spec's own acceptance checks.
  - `[low]` `[patch]` (intent D2) "tw module only" is not enforced — grouped with the blind finding above; source-level, not reachable from metadata; documented instead.
  - `[medium]` `[bad_spec]` (intent D3) The bot-side half of "testkit feature, dev-dependencies only" is not enforced — grouped with the blind testkit finding; the diff's facade-side reading is correct per spine prose, the bot-side rule is missing from Design Notes.
  - `[false]` `[reject]` (intent D4) Reachability is checked by direct edge, not a resolved graph — no other path exists: GRAPH has no edge into `noctis-testkit` and every undrawn edge fails, so any second path fails separately; cross-crate `render` unification through macros is ruled out by the spine itself (resolver 3 keeps proc-macro deps' features apart).
  - `[false]` `[reject]` (intent D5) Rule ids are tool-local rather than spine references — the intent-contract deliberately mandates kebab ids (Never: no `<AREA>-<nn>` shape) and the reasons name the broken rule.
  - `[medium]` `[defer]` (intent D6) Scope ends at the cargo workspace; the template is never checked — grouped with the blind template finding.
  - `[false]` `[reject]` (intent D7) Scope additions beyond the ACs — each is an explicit spec matrix row or Design Notes decision derived from the spine prose; deliberately settled.
  - `[false]` `[reject]` (intent D8) "Runs as a gate" met only at the workflow file — the job has no `continue-on-error`; checking the pushed run is the spec's Verification step after the main session pushes, outside the diff by design.

### 2026-10-19 — Review pass
- verdicts: 28 findings — high 0, medium 8, low 13, false 7, maybe-false 0
- findings:
  - `[medium]` `[bad_spec]` (blind) Bot-side `testkit` check matches only the literal `testkit` and ignores the facade's own feature closure — verified: `check_non_internal` tests `dep.features.iter().any(|f| f == TESTKIT_FEATURE)` and the exact pair `(key, "testkit")`, never `ws.member(FACADE).closure(..)`; with facade `full = ["testkit"]`, a bot normal dep with `features = ["full"]`, or a bot feature `t = ["noctis/full"]`, exits 0 (contrast `check_render`'s `enables_render`). Root cause: Design Notes specify the literal match. Amendment: Design Notes bot rule resolves every requested facade feature (dep `features`, `default` when `uses_default_features`, `k/f` / `k?/f` from the bot's own closures) through the facade's closure; new Tasks fixture rows.
  - `[medium]` `[bad_spec]` (blind) Nothing forbids the facade's `default` from enabling `testkit` — verified: `check_gate_features` accepts `default = ["testkit"]` (closure contains the gate) and there is no facade counterpart to image's `default`-closure `render-on` check, so every bot's normal `noctis` dep reaches `noctis-testkit` with exit 0. Same root cause (Design Notes omit it). Amendment: crate-level `ungated-edge` on `noctis` when its `default` closure contains `testkit`, plus fixture.
  - `[low]` `[bad_spec]` (blind) Facade any-crate rule never checks the target is one of the 12 graph crates — verified: `check_edge` returns `None` for every `from == FACADE` (bar the testkit gate), and `from_metadata` keeps out-of-tree `noctis-*` deps and non-internal members, so `noctis -> noctis-foo` (outside path) or `noctis -> xtask` passes. Root cause: Design Notes "any `B`" wording. Amendment: facade rule allows only `B` in CRATES (except testkit), otherwise `undrawn-edge`; fixture.
  - `[low]` `[reject]` (blind) `check_render`'s feature loop prints `(normal)` for a build/dev-dep and leaves macros dev-dep render undecided — real but a macros build-dep on `noctis-image` is not an everyday setup; the line still names the edge and rule, and the fix adds a kind lookup across possibly several deps with the same key.
  - `[low]` `[reject]` (blind) `deps-check --help` / `-h` is a usage error (exit 2) — loud failure that still prints the full usage; supporting it changes the parser's return type for a rare input.
  - `[false]` `[reject]` (blind) Tool failure and violations share exit code 1 — the intent contract's Boundaries mandate "1 violations or `cargo metadata` failure"; deliberately settled.
  - `[low]` `[reject]` (blind) Tasks bullet still describes `Violation { rule, from, to, kind, reason }` — the fix is editing this build's spec (harmonised in the bad_spec amendment anyway, since Tasks is amended).
  - `[low]` `[reject]` (blind) Deferred item and pass-1 triage rows cite stale line numbers — the fix is editing this build's spec.
  - `[low]` `[defer]` (blind) AGENTS.md gate line has a doubled "and" — verified at AGENTS.md:23; the fix edits an agent-context file, deferred.
  - `[false]` `[reject]` (blind) `path = {:?}` in `out_of_workspace_internal_dep_fails` breaks on non-ASCII paths — `str`'s `Debug` keeps printable Unicode such as "ö" unescaped and writes `\\` for backslashes, both valid in a TOML basic string; only control characters would differ, which a path under `CARGO_TARGET_TMPDIR` does not contain.
  - `[low]` `[bad_spec]` (blind) No fixture for a facade build/dev dep on a graph crate nor for a bot relying on facade default features — verified by grep; the default-features half is the same defect as the testkit-closure finding. Amendment: both fixtures added to Tasks.
  - `[medium]` `[bad_spec]` (edge) Bot normal dep requests a facade feature whose closure contains `testkit` — same as the blind testkit-closure finding.
  - `[medium]` `[bad_spec]` (edge) Facade `default` reaching `testkit` plus a default-features bot dep passes — same as the blind facade-default finding.
  - `[medium]` `[bad_spec]` (edge) Bot feature `noctis/full` (only literal `k/testkit` matched) passes — same as the blind testkit-closure finding.
  - `[low]` `[reject]` (edge) `check_render` prints `(normal)` for a macros build-dep on image — same claim as the blind kind finding; rare setup, line still names edge and rule.
  - `[low]` `[reject]` (edge) `std::env::args` panics on a non-UTF-8 argument — real, but a loud failure (exit 101) on an input nobody types into a repo tool; the fix switches the parser to `OsString`.
  - `[low]` `[reject]` (edge) `deps-check --help` / `-h` exits 2 — same claim as the blind finding; usage is printed.
  - `[low]` `[patch]` (vgap, pre-verified) The `names.contains` half of the `from_metadata` filter is unpinned: no fixture has a graph crate depending on a non-internal member — filed evidence: dropping it keeps every test green. Fix (folded into the bad_spec amendment as a Tasks row since code is re-derived): `noctis-core -> bot` fixture asserting exit 1, `noctis-core -> bot (normal)`, `[undrawn-edge]`.
  - `[medium]` `[bad_spec]` (vgap other) Bot `testkit` rule ignores facade closures (feature `full`, bot feature `noctis/full`, facade `default`) — same as the blind testkit-closure and facade-default findings.
  - `[false]` `[reject]` (intent D1) AC pass case on the real workspace is vacuous — carried: the spec deliberately settles this (no internal edges in this story; full-graph fixture plus the manual mutation AC).
  - `[medium]` `[bad_spec]` (intent D2) Testkit reachable through the facade's own feature graph — same as the blind testkit-closure and facade-default findings.
  - `[medium]` `[defer]` (intent D3) Bot rules never run on `examples/project-template` — carried: already deferred (frontmatter `deferred`, first item); not deferred again.
  - `[low]` `[patch]` (intent D4) "tw module only" half not enforced — carried: the pass-1 patch (module doc line) is in place at `deps_check.rs` module doc; not patched again.
  - `[false]` `[reject]` (intent D5) `depends-on-facade` covers only internal crates, narrowing "any crate" — the intent contract's matrix row "Non-internal member uses facade: `bot` -> `noctis`, exit 0" and AD-2 settle it; the spine's "No crate depends on `noctis`" cannot include bots, which AD-2 requires to depend on it.
  - `[false]` `[reject]` (intent D6) Rule ids are tool-local — carried: the intent contract mandates kebab ids.
  - `[low]` `[reject]` (intent D7) Allow-list hand-copied, not compared with the spine — carried: rare silent drift only for removed spine edges; fix is a mermaid parser.
  - `[false]` `[reject]` (intent D8) CI gate shown only at the workflow file — carried: no `continue-on-error`; the pushed run is the spec's Verification step.
  - `[false]` `[reject]` (intent D9) Scope additions beyond the ACs — carried: each is a spec matrix row or Design Notes decision.

### 2026-10-19 — Review pass
- verdicts: 25 findings — high 0, medium 1, low 14, false 10, maybe-false 0
- findings:
  - `[false]` `[reject]` (blind) `xtask` gets the bot rule, which blocks later xtask tasks (`rules-catalog`) that might want `noctis-codec` — the intent contract's matrix row "Non-internal member: member `xtask` -> `noctis-core` (any kind), exit 1, `bot-dependency`" mandates exactly this; no current task needs the edge, and a later story that does must amend the spine and intent first.
  - `[low]` `[reject]` (blind) Any `noctis-*` package, including a third-party registry crate, counts as internal — real (`is_internal` is prefix-only and `from_metadata` keeps such deps), but the inspected workspace is noctis-rs itself, whose graph crates and tooling members have no reason to depend on community `noctis-*` crates; telling sources apart adds branches over `source`/`path`.
  - `[low]` `[reject]` (blind) Integration tests assert substrings only, never the absence of extra lines — real test weakness (`assert_exit` uses `contains`), but every pass fixture already fails on any spurious line (`assert_ok` needs exit 0), and switching ~60 failing fixtures to exact line sets is a broad rework, not a direct correction.
  - `[false]` `[reject]` (blind) Rule logic has no in-memory unit tests — Tasks deliberately prescribe CLI tests over generated fixture workspaces (one per matrix/Tasks row), which pin the `cargo metadata` mapping the finding itself wants kept; no wrong outcome is named beyond speed and style.
  - `[low]` `[patch]` (blind) `full_graph`'s doc says "Every drawn edge" but it also gives the facade edges to all 10 non-testkit crates (32 edges) — verified at `xtask/tests/deps_check.rs:133`; the fixture itself follows the intent's "Full graph" row ("facade depends on all other crates"), and a subset of it passes a fortiori (rules are per edge), so only the comment misleads. Fix: correct the doc comment.
  - `[low]` `[reject]` (blind) Reasons print `k/f` for a weak `k?/f` request — real (`Member::closure` strips `?` before recording `dep_features`), but the line still names the member feature, edge and rule, weak requests are rare, and the fix threads a weak flag through the closure tuple and three reason sites.
  - `[low]` `[patch]` (blind) `main.rs` branch `[flag, _, extra, ..]` ("unexpected argument: <extra>") is untested — verified: `usage_errors_exit_2` covers only `--bogus` and a bare `--manifest-path`. Fix: add `deps-check --manifest-path a b` -> exit 2 with `unexpected argument: b`. The default-manifest half is already exercised by the CI `deps-check` job, which runs without `--manifest-path`; no test added for it.
  - `[low]` `[patch]` (blind) Test module doc claims "one per row of Story 1.2's I/O matrix" though about 35 tests come from Tasks rows — verified at `xtask/tests/deps_check.rs:1-2`. Fix: doc says the tests cover the I/O matrix rows and the extra rows listed in the spec's Tasks.
  - `[low]` `[defer]` (blind) AGENTS.md lists the deps-check gate but says nothing about pairing the spine graph with `CRATES`/`GRAPH`/`GATED` when deps-check fails — real for `undrawn-edge` (its reason does not name the constants; `unknown-crate`'s does); the fix edits an agent-context file, so deferred. The doubled-"and" half is carried (deferred in pass 2, frontmatter `deferred` second item) and not deferred again.
  - `[low]` `[reject]` (blind) Spec uses short commit hashes, unnumbered triage-pass titles and stale line numbers — the fix is editing this build's spec.
  - `[false]` `[reject]` (blind) Facade `default` reuses `ungated-edge` as a crate-level line instead of an edge line or new id — deliberately settled: Design Notes "Facade `default`" mandates a crate-level `ungated-edge` on `noctis` firing in addition to `check_gate_features`, the Tasks row pins the no-`->` form, and the rule-id list is fixed (KEEP).
  - `[false]` `[reject]` (edge) A dep on a registry/out-of-tree package with a CRATES name (e.g. facade -> `noctis-core = "0.1"`) passes as a drawn edge — no layering breach occurs: the edge is the drawn one, and a crates.io `noctis-core` is this project's own published crate (the workspace publishes `noctis-*`), so the layer order deps-check guards is unchanged.
  - `[low]` `[reject]` (edge) A bot depending on a registry `noctis` is judged against the workspace facade's feature table — real (`check_non_internal` always uses `ws.member(FACADE)`), but non-internal workspace members (`xtask`, `examples/<feature>`) use the in-tree facade; a registry facade dep there is not an everyday setup, and `testkit` itself is still caught by name; the fix adds a source check.
  - `[low]` `[reject]` (edge) `check_render`'s feature loop prints `(normal)` for a macros build/dev-dep on image — carried: pass-2 row (same claim, `deps_check.rs:549` still hardcodes `Kind::Normal`); rare setup, line names the edge and rule.
  - `[false]` `[reject]` (edge) An unknown `noctis-*` member's own edges are skipped (`continue` after `unknown-crate`) — a crate outside the graph has no allow-list entry, so none of its edges can be judged until the step the reason prescribes (add it to the spine and CRATES/GRAPH/GATED) is done; edges into it from graph crates are still reported (`undrawn-edge`). The skip is the specified `check()` structure (KEEP).
  - `[low]` `[reject]` (edge) `--manifest-path ""` or `--manifest-path --bogus` exits 1 as a metadata failure instead of 2 — real but loud: the message `cargo metadata failed: ...` names the bad path; the spec's usage rule covers only the missing-value form; the fix adds a guard for an input nobody types.
  - `[low]` `[patch]` (vgap, pre-verified) The `*key == dep.key` filter on bot `k/f` requests (source (c)) is unpinned — filed evidence: dropping the key comparison keeps all 60 tests green, and a bot with `t = ["helper/full"]` then fails falsely. Fix: add `bot_feature_requesting_other_dep_feature_passes` (testkit facade with `full = ["testkit"]`, member `helper` with `full = []`, `bot` with normal deps on `noctis` and `helper` and `t = ["helper/full"]`, `assert_ok`).
  - `[false]` `[reject]` (intent D1) AC1 pass case on the real workspace is vacuous — carried: the spec deliberately settles this (no internal edges in this story; full-graph fixture plus the manual mutation AC).
  - `[false]` `[reject]` (intent D2) `depends-on-facade` covers only internal crates, narrowing "any crate" — carried: the intent matrix row "Non-internal member uses facade, exit 0" and AD-2 settle it.
  - `[medium]` `[defer]` (intent D3) Bot rules never run on `examples/project-template` — carried: already deferred (frontmatter `deferred`, first item); not deferred again.
  - `[low]` `[patch]` (intent D4) "tw module only" half not enforced — carried: the pass-1 patch (module doc lines 9-12 of `deps_check.rs`) is in place; not patched again.
  - `[false]` `[reject]` (intent D5) Reachability by feature closure, not a resolved graph — carried: pass-1 refutation (every other edge into `noctis-testkit` is undrawn and fails on its own; resolver 3 keeps proc-macro features apart).
  - `[false]` `[reject]` (intent D6) CI gate shown only at the workflow file — carried: no `continue-on-error`; the pushed run is the spec's Verification step.
  - `[low]` `[reject]` (intent D7) Allow-list hand-copied, not compared with the spine — carried: rare silent drift only for removed spine edges; fix is a mermaid parser.
  - `[false]` `[reject]` (intent D8) Scope additions beyond the ACs, notably the facade limited to CRATES targets against AC1's "any crate" — the additions are carried (pass-1/2: spec matrix rows or Design Notes decisions); the facade limit is settled by the intent contract's Always bullet ("the facade `noctis` may depend on any internal crate", internal = a workspace member named `noctis`/`noctis-*`), which excludes `xtask`, `bot` and out-of-tree packages.

## Design Notes

Allow-list (from -> to). Ungated: `noctis-prefix -> noctis-core`; `noctis-testkit -> noctis-core, noctis-gateway, noctis-ports`; `noctis-core -> noctis-gateway, noctis-i18n, noctis-ui, noctis-ports, noctis-codec, noctis-rules`; `noctis-i18n -> noctis-ports, noctis-rules`; `noctis-gateway -> noctis-ports`; `noctis-ui -> noctis-codec, noctis-rules`; `noctis-codec -> noctis-rules`; `noctis-macros -> noctis-rules, noctis-image` (18; the facade's drawn ungated edges are covered by its any-crate rule). Gated: `noctis-image -> noctis-core | noctis-ports | noctis-ui` by `render`; `noctis -> noctis-testkit` by `testkit`.

Rules, for each internal crate `A` and internal dep `B`:
- `A == noctis`: any `B` in CRATES except `noctis-testkit` (which follows the gated rule) is allowed for every kind; any other `B` (a non-graph workspace member such as `xtask` or `bot`, or an out-of-tree or registry `noctis-*` package kept by `from_metadata`) -> `undrawn-edge`.
- `B == noctis`: `dev` allowed; `normal`/`build` -> `depends-on-facade`.
- `(A, B)` gated by feature `G`: dep must be `normal` and `optional`, else `ungated-edge`; for every feature `F` of `A` whose closure activates `B` and does not contain `G` -> `ungated-edge` naming `F`.
- `(A, B)` in GRAPH: allowed for every kind.
- Otherwise `undrawn-edge` (covers `testkit -> image`, dev-deps on `noctis-testkit`, build deps).
- `render-on`: `noctis-macros`' dep on `noctis-image` lists `render`, or uses default features while `noctis-image`'s `default` closure contains `render`; any `noctis-macros` feature whose closure contains `noctis-image/render` or `noctis-image?/render`; `noctis-image`'s own `default` closure contains `render`.
- Facade feature enables testkit: a facade feature name `f` *enables testkit* when `f == "testkit"`, or the facade member's closure of `f` contains feature `testkit`, or activates a dependency whose package is `noctis-testkit` (via `dep:k` or `k/x`, rename-aware). A name that is not a facade feature (including `default` when the facade declares none) has an empty closure and enables nothing unless it is `testkit`. This is the only test for "reaches testkit"; it is applied to every facade feature name a bot requests, never a literal string match.
- Facade `default`: when `enables_testkit("default")` -> crate-level `ungated-edge` on `noctis` (reason: "default features enable `testkit`; it is for bot `[dev-dependencies]` only"). This is the facade counterpart of image's `default` `render-on` rule; it fires in addition to any gate-feature line (`check_gate_features` alone accepts `default = ["testkit"]` because that closure contains the gate).
- Non-internal members (`xtask`, future `examples/<feature>`): may depend on `noctis` only (AD-2: bot code uses the facade); any other internal dep (including `noctis-testkit` directly, any kind) -> `bot-dependency`. Spine: "Only the facade's `testkit` feature, which bot crates enable in `[dev-dependencies]` only, reaches `noctis-testkit`". For each of the member's dependencies on `noctis` of kind `normal` or `build` (every target; key `k` = its rename-aware key), report `bot-dependency` on that edge for each of these that enables testkit: (a) each name in the dependency's `features` list; (b) `default`, when `uses_default_features` is true; (c) each facade feature `f` from a pair `(k, f)` (from `k/f` or weak `k?/f`) in the closure of any of the member's own features, `default` included. The reason names the facade feature and its source (the `features` list, default features, or the member feature from (c)) and says `testkit` is for `[dev-dependencies]` only. Pairs in (c) are matched against the key of each normal/build dep; a key that names only a `dev` dependency is ignored. `dev` dependencies on `noctis` may enable anything. These three sources are every way Cargo lets a member turn on facade features for a non-dev dep (a `dep:k` entry turns on the dep with its own `features`/default, already covered by (a)/(b)); a non-internal member reaching testkit through another non-internal member is caught on that other member's own deps. No new rule id.
- A member named `noctis`/`noctis-*` outside the 12 -> `unknown-crate` (reason: add it to the spine's Invariants graph and to CRATES/GRAPH/GATED first); one of the 12 absent -> `missing-crate`.

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

## Auto Run Result

- **Summary:** `cargo run --release -p xtask -- deps-check [--manifest-path <p>]` reads workspace metadata (`cargo_metadata` 0.23.1, `--no-deps`) and checks every internal dependency edge against the spine's layer graph (18 drawn edges, gated edges behind their features, facade any-crate rule), facade `default`/testkit closure rules, and the bot rule (testkit only from dev-dependencies, rename-aware, through every feature path). Kebab rule ids: `undrawn-edge`, `depends-on-facade`, `ungated-edge`, `render-on`, `unknown-crate`, `missing-crate`, `bot-dependency`. Exit 0/1/2. Blocking CI job `Deps check`.
- **Files changed:** `xtask/src/deps_check.rs` (new), `xtask/src/main.rs`, `xtask/tests/deps_check.rs` (new, 55 fixture tests), `xtask/Cargo.toml`, `Cargo.lock`, `.github/workflows/ci.yml`, `AGENTS.md`.
- **Review findings:** three passes. Pass 1: 26 findings → bad_spec (bot testkit via normal deps). Pass 2: 28 findings → bad_spec (testkit reachable through facade feature closures/default). Pass 3: 25 findings (medium 1, low 14, false 10) → 4 low patches in tests/docs, 1 new defer; 18 rejected (see triage log). Deferred total 3 (template not checked, AGENTS.md wording ×2).
- **Follow-up review recommendation:** false (pass 3 patched only low entries).
- **Verification:** fmt, clippy `-D warnings`, workspace tests, `cargo hack --each-feature`, `cargo +1.91 check`, `cargo deny check`, build-config script, deps-check on the real tree (`ok (12 internal crates, 0 internal edges)`), mutation check (`noctis-ui -> noctis-core` → exit 1 `[undrawn-edge]`), no `target/debug/`.
- **Residual risks:** the "tw module only" half of the macros→image rule is documented as unchecked; `examples/project-template` not checked by deps-check (deferred).
